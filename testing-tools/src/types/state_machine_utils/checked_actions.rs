use std::{future::Future, pin::Pin};

use angstrom_network::{manager::StromConsensusEvent, StromMessage};
use angstrom_types::{
    consensus::{PreProposal, Proposal},
    sol_bindings::grouped_orders::AllOrders
};
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};

use crate::{
    testnet_controllers::{AngstromTestnet, StateMachineTestnet},
    types::StateMachineCheckedActionHookFn
};

pub trait WithCheckedAction<'a, C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    type FunctionOutput = StateMachineCheckedActionHookFn<'a, C>;

    fn send_pooled_orders(&mut self, orders: Vec<AllOrders>);

    fn send_propose(&mut self, proposal: Proposal);

    fn send_prepropose(&mut self, preproposal: PreProposal);
}

impl<'a, C> WithCheckedAction<'a, C> for StateMachineTestnet<'a, C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    fn send_pooled_orders(&mut self, orders: Vec<AllOrders>) {
        let f = |testnet: &'a mut AngstromTestnet<C>| {
            pin_action(testnet.broadcast_orders_message(
                None,
                StromMessage::PropagatePooledOrders(orders.clone()),
                orders
            ))
        };
        self.add_checked_action("send bundles", f);
    }

    fn send_propose(&mut self, proposal: Proposal) {
        let f = |testnet: &'a mut AngstromTestnet<C>| {
            pin_action(testnet.broadcast_consensus_message(
                Some(0),
                StromMessage::Propose(proposal.clone()),
                StromConsensusEvent::Proposal(testnet.get_peer(0).peer_id(), proposal)
            ))
        };
        self.add_checked_action("send propose", f);
    }

    fn send_prepropose(&mut self, preproposal: PreProposal) {
        let f = |testnet: &'a mut AngstromTestnet<C>| {
            pin_action(testnet.broadcast_consensus_message(
                Some(0),
                StromMessage::PrePropose(preproposal.clone()),
                StromConsensusEvent::PreProposal(testnet.get_peer(0).peer_id(), preproposal)
            ))
        };
        self.add_checked_action("send prepropose", f);
    }
}

fn pin_action<'a, F>(fut: F) -> Pin<Box<dyn Future<Output = eyre::Result<bool>> + Send + Sync + 'a>>
where
    F: Future<Output = eyre::Result<bool>> + Send + Sync + 'a
{
    Box::pin(fut)
}
