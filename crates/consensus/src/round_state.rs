use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker}
};

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, BlockNumber},
    providers::Provider,
    rpc::types::TransactionRequest,
    transports::Transport
};
use angstrom_metrics::ConsensusMetricsWrapper;
use angstrom_network::{manager::StromConsensusEvent, StromMessage};
use angstrom_types::{
    consensus::{PreProposal, Proposal},
    contract_payloads::angstrom::{AngstromBundle, UniswapAngstromRegistry},
    orders::OrderSet,
    primitive::PeerId,
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use angstrom_utils::timer::async_time_fn;
use eyre::Report;
use futures::{future::BoxFuture, Future, Stream};
use itertools::Itertools;
use matching_engine::MatchingEngineHandle;
use order_pool::order_storage::OrderStorage;
use pade::PadeEncode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::{AngstromValidator, Signer};

#[derive(Error, Debug)]
pub enum RoundStateMachineError {
    #[error("Failed to build proposal: {0}")]
    ProposalBuildError(Report),
    #[error("Transaction submission failed")]
    TransactionError
}

pub struct RoundStateMachine<T, Matching> {
    current_state:     ConsensusState,
    matching_engine:   Matching,
    signer:            Signer,
    round_leader:      PeerId,
    validators:        Vec<AngstromValidator>,
    _order_storage:    Arc<OrderStorage>,
    metrics:           ConsensusMetricsWrapper,
    transition_future: Option<BoxFuture<'static, Result<ConsensusState, RoundStateMachineError>>>,
    waker:             Option<Waker>,
    pool_registry:     UniswapAngstromRegistry,
    uniswap_pools:     SyncedUniswapPools,
    provider:          Arc<Pin<Box<dyn Provider<T>>>>
}

impl<T, Matching> RoundStateMachine<T, Matching>
where
    T: Transport + Clone,
    Matching: MatchingEngineHandle
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        block_height: BlockNumber,
        order_storage: Arc<OrderStorage>,
        signer: Signer,
        round_leader: PeerId,
        validators: Vec<AngstromValidator>,
        metrics: ConsensusMetricsWrapper,
        pool_registry: UniswapAngstromRegistry,
        uniswap_pools: SyncedUniswapPools,
        provider: impl Provider<T> + 'static,
        matching_engine: Matching
    ) -> Self {
        Self {
            current_state: Self::initial_state(block_height),
            round_leader,
            validators,
            _order_storage: order_storage,
            pool_registry,
            uniswap_pools,
            signer,
            metrics,
            transition_future: None,
            waker: None,
            matching_engine,
            provider: Arc::new(Box::pin(provider))
        }
    }

    pub fn my_id(&self) -> PeerId {
        self.signer.my_id
    }

    pub fn is_leader(&self, node: PeerId) -> bool {
        self.round_leader == node
    }

    pub fn i_am_leader(&self) -> bool {
        self.is_leader(self.my_id())
    }

    pub fn has_quorum(&self, voters: usize) -> bool {
        voters > (self.validators.len() * 2) / 3
    }

    pub fn reset_round(&mut self, block: BlockNumber, leader: PeerId) {
        self.round_leader = leader;
        self.current_state = Self::initial_state(block);
        self.transition_future = None;
    }

    pub fn initial_state(block_height: BlockNumber) -> ConsensusState {
        ConsensusState::PreProposalSubmission(PreProposalSubmission {
            block_height,
            ..Default::default()
        })
    }

    pub fn my_pre_proposal(&self, pre_proposals: &HashSet<PreProposal>) -> Option<StromMessage> {
        pre_proposals
            .iter()
            .find(|p| p.source == self.my_id())
            .cloned()
            .map(StromMessage::PrePropose)
    }

    /// Invariants:
    ///  * won't be called with messages with payload from ourselves
    ///  * won't be called with messages for non-current block height
    pub fn on_strom_message(
        &mut self,
        strom_msg: StromConsensusEvent
    ) -> Option<(Option<PeerId>, StromMessage)> {
        let i_am_leader = self.i_am_leader();
        match strom_msg {
            StromConsensusEvent::PreProposal(_, pre_proposal) => {
                // it does not make sense to accumulate pre_proposals here, since the leader
                // already submitted on chain
                if matches!(self.current_state, ConsensusState::Finalization(_))
                    || !pre_proposal.is_valid()
                {
                    return None
                }

                self.current_state
                    .pre_proposals_mut()
                    .insert(pre_proposal.clone());

                // we do not want to allow another node to push us to transition
                // we wait for our grace period of 3 seconds to finish
                if !matches!(self.current_state, ConsensusState::PreProposalAggregation(_)) {
                    return None
                }

                if !i_am_leader {
                    let block_height = self.current_state.block_height();
                    let merged_pre_proposal = Self::generate_our_merged_pre_proposal(
                        block_height,
                        Vec::new(),
                        Vec::new(),
                        self.current_state.pre_proposals(),
                        &self.signer
                    );
                    self.current_state
                        .pre_proposals_mut()
                        .insert(merged_pre_proposal.clone());

                    let pre_proposals = self.current_state.pre_proposals();

                    if self.have_quorum(self.all_searcher_orders(pre_proposals))
                        && self.have_quorum(self.all_limit_orders(pre_proposals))
                    {
                        // send the quorum pre_proposal to the leader
                        return Some((
                            Some(self.round_leader),
                            StromMessage::PrePropose(merged_pre_proposal)
                        ))
                    }

                    return Some((None, StromMessage::PrePropose(merged_pre_proposal)))
                }

                // Leader path
                let pre_proposals = self.current_state.pre_proposals();
                let block_height = self.current_state.block_height();
                if self.have_quorum(self.all_searcher_orders(pre_proposals))
                    && self.have_quorum(self.all_limit_orders(pre_proposals))
                {
                    self.force_transition(ConsensusState::Finalization(Finalization {
                        block_height,
                        proposal: None,
                        pre_proposals: pre_proposals.clone()
                    }));
                    return None
                }
            }
            StromConsensusEvent::Proposal(msg_sender, proposal) => {
                let Proposal {
                    source: proposal_sender, block_height: proposal_block_height, ..
                } = proposal;

                let pre_proposals = self.current_state.pre_proposals();
                if proposal.is_valid() && !i_am_leader {
                    self.force_transition(ConsensusState::Finalization(Finalization {
                        block_height:  proposal_block_height,
                        proposal:      Some(proposal),
                        pre_proposals: pre_proposals.clone()
                    }));
                }

                if i_am_leader {
                    panic!(
                        "Message sender: {}, Proposal block height: {}, Proposal source: {}, \
                         Current state {}, something is terribly wrong",
                        msg_sender,
                        proposal_block_height,
                        proposal_sender,
                        self.current_state.name()
                    );
                }
            }
        }

        None
    }

    fn _generate_bid_aggregation(
        &self,
        block_height: BlockNumber,
        pre_proposals: &HashSet<PreProposal>
    ) -> PreProposalAggregation {
        let OrderSet { limit, searcher } = self._order_storage.get_all_orders();
        let mut pre_proposals = pre_proposals.clone();

        let pre_proposal = Self::generate_our_merged_pre_proposal(
            block_height,
            limit,
            searcher,
            &pre_proposals,
            &self.signer
        );
        pre_proposals.insert(pre_proposal);

        PreProposalAggregation { block_height, pre_proposals }
    }

    fn generate_our_merged_pre_proposal(
        block_height: BlockNumber,
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        searcher: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        pre_proposals: &HashSet<PreProposal>,
        signer: &Signer
    ) -> PreProposal {
        let merged_limit_orders: Vec<_> = pre_proposals
            .iter()
            .flat_map(|p| p.limit.clone())
            .chain(limit)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let merged_searcher_orders: Vec<_> = pre_proposals
            .iter()
            .flat_map(|p| p.searcher.clone())
            .chain(searcher)
            .into_group_map_by(|order| order.pool_id)
            .into_values()
            .filter_map(|group| group.into_iter().max_by_key(|order| order.tob_reward))
            .collect();

        PreProposal::generate_pre_proposal(
            block_height,
            signer.my_id,
            merged_limit_orders,
            merged_searcher_orders,
            &signer.key
        )
    }

    fn all_searcher_orders(
        &self,
        pre_proposals: &HashSet<PreProposal>
    ) -> Vec<OrderWithStorageData<TopOfBlockOrder>> {
        pre_proposals
            .iter()
            .flat_map(|pre_proposal| pre_proposal.searcher.clone())
            .collect()
    }

    fn all_limit_orders(
        &self,
        pre_proposals: &HashSet<PreProposal>
    ) -> Vec<OrderWithStorageData<GroupedVanillaOrder>> {
        pre_proposals
            .iter()
            .flat_map(|pre_proposal| pre_proposal.limit.clone())
            .collect()
    }

    fn have_quorum<O: Hash + Eq + Clone>(&self, orders: Vec<OrderWithStorageData<O>>) -> bool {
        orders.len() == self.filter_quorum_orders(orders).len()
    }

    fn filter_quorum_orders<O: Hash + Eq + Clone>(
        &self,
        input: Vec<OrderWithStorageData<O>>
    ) -> Vec<OrderWithStorageData<O>> {
        input
            .into_iter()
            .fold(HashMap::new(), |mut acc, order| {
                *acc.entry(order).or_insert(0) += 1;
                acc
            })
            .into_iter()
            .filter(|(_, count)| self.has_quorum(*count))
            .map(|(order, _)| order)
            .collect()
    }

    fn force_transition(&mut self, new_state: ConsensusState) {
        self.transition_future = Some(Box::pin(self.build_transition_future(new_state)));

        // wake up the poller
        if let Some(waker) = &self.waker {
            waker.wake_by_ref();
        }
    }

    fn build_transition_future(
        &self,
        mut new_state: ConsensusState
    ) -> impl Future<Output = Result<ConsensusState, RoundStateMachineError>> {
        let signer = self.signer.clone();
        let metrics = self.metrics.clone();
        let pre_proposal_height = self.current_state.block_height();
        let pre_proposals: Vec<PreProposal> =
            self.current_state.pre_proposals().iter().cloned().collect();
        let provider = self.provider.clone();
        let pool_registry = self.pool_registry.clone();
        let uniswap_pools = self.uniswap_pools.clone();
        let matching = self.matching_engine.clone();

        async move {
            if let ConsensusState::Finalization(finalization) = &mut new_state {
                // someone already proposed and we are not a leader
                if finalization.proposal.is_some() {
                    // TODO: use this opportunity to trigger the proposal validation
                    return Ok(new_state)
                }

                let pool_snapshots = uniswap_pools
                    .iter()
                    .filter_map(|(key, pool)| {
                        let (token_a, token_b, snapshot) =
                            pool.read().unwrap().fetch_pool_snapshot().ok()?;
                        let entry = pool_registry.get_ang_entry(key)?;

                        Some((*key, (token_a, token_b, snapshot, entry.store_index as u16)))
                    })
                    .collect::<HashMap<_, _>>();

                let (proposal, timer) = async_time_fn(|| async {
                    match matching
                        .solve_pools(pre_proposals.clone(), pool_snapshots.clone())
                        .await
                    {
                        Ok((solutions, gas_info)) => {
                            let proposal =
                                signer.sign_proposal(pre_proposal_height, pre_proposals, solutions);
                            Ok((proposal, gas_info))
                        }
                        Err(err) => Err(RoundStateMachineError::ProposalBuildError(err))
                    }
                })
                .await;
                metrics.set_proposal_build_time(pre_proposal_height, timer);
                let (proposal, gas_info) = proposal?;

                let bundle =
                    AngstromBundle::from_proposal(&proposal, gas_info, &pool_snapshots).unwrap();

                let tx = TransactionRequest::default()
                    .with_to(Address::default())
                    .with_input(bundle.pade_encode());

                let submitted_tx = provider
                    .send_transaction(tx)
                    .await
                    .map_err(|_| RoundStateMachineError::TransactionError)?;
                let _receipt = submitted_tx
                    .get_receipt()
                    .await
                    .map_err(|_| RoundStateMachineError::TransactionError)?;
            }
            Ok(new_state)
        }
    }
}

impl<T, Matching> Stream for RoundStateMachine<T, Matching>
where
    T: Transport + Clone,
    Matching: MatchingEngineHandle
{
    type Item = Result<ConsensusState, RoundStateMachineError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        this.waker = Some(cx.waker().clone());

        if let Some(future) = &mut this.transition_future {
            return match future.as_mut().poll(cx) {
                Poll::Ready(result) => Poll::Ready(Some(result)),
                Poll::Pending => Poll::Pending
            }
        }

        Poll::Pending
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PreProposalSubmission {
    pub block_height:  BlockNumber,
    // this is used mostly for early messages
    pub pre_proposals: HashSet<PreProposal>
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PreProposalAggregation {
    pub block_height:  BlockNumber,
    pub pre_proposals: HashSet<PreProposal>
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Finalization {
    pub block_height:  BlockNumber,
    pub pre_proposals: HashSet<PreProposal>,
    pub proposal:      Option<Proposal>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConsensusState {
    PreProposalSubmission(PreProposalSubmission),
    PreProposalAggregation(PreProposalAggregation),
    Finalization(Finalization)
}

impl ConsensusState {
    fn name(&self) -> &'static str {
        match self {
            Self::PreProposalSubmission(_) => "PreProposalSubmission",
            Self::PreProposalAggregation(_) => "PreProposalAggregation",
            Self::Finalization(_) => "Finalization"
        }
    }
}

impl ConsensusState {
    pub fn block_height(&self) -> BlockNumber {
        match self {
            Self::PreProposalSubmission(state) => state.block_height,
            Self::PreProposalAggregation(state) => state.block_height,
            Self::Finalization(state) => state.block_height
        }
    }

    pub fn pre_proposals_mut(&mut self) -> &mut HashSet<PreProposal> {
        match self {
            Self::PreProposalSubmission(state) => &mut state.pre_proposals,
            Self::PreProposalAggregation(state) => &mut state.pre_proposals,
            Self::Finalization(state) => &mut state.pre_proposals
        }
    }

    pub fn pre_proposals(&self) -> &HashSet<PreProposal> {
        match self {
            Self::PreProposalSubmission(state) => &state.pre_proposals,
            Self::PreProposalAggregation(state) => &state.pre_proposals,
            Self::Finalization(state) => &state.pre_proposals
        }
    }
}
