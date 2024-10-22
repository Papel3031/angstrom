use std::task::{Context, Poll};

use alloy::{
    network::TransactionResponse,
    primitives::Address,
    sol_types::{SolCall, SolType}
};
use alloy_rpc_types::Transaction;
use angstrom_eth::{
    handle::{EthCommand, EthHandle},
    manager::EthEvent
};
use angstrom_types::sol_bindings::{sol::ContractBundle, testnet::TestnetHub};
use futures::{Future, Stream, StreamExt};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{Instrument, Span};

pub struct AnvilEthDataCleanser<S: Stream<Item = (u64, Vec<Transaction>)>> {
    angstrom_contract:           Address,
    /// our command receiver
    commander:                   ReceiverStream<EthCommand>,
    /// people listening to events
    event_listeners:             Vec<UnboundedSender<EthEvent>>,
    block_subscription:          S,
    block_finalization_lookback: u64
}

impl<S: Stream<Item = (u64, Vec<Transaction>)> + Unpin + Send + 'static> AnvilEthDataCleanser<S> {
    pub async fn spawn<TP: TaskSpawner>(
        tp: TP,
        angstrom_contract: Address,
        tx: Sender<EthCommand>,
        rx: Receiver<EthCommand>,
        block_subscription: S,
        block_finalization_lookback: u64,
        span: Span
    ) -> eyre::Result<EthHandle> {
        let stream = ReceiverStream::new(rx);
        let this = Self {
            commander: stream,
            event_listeners: Vec::new(),
            block_subscription,
            angstrom_contract,
            block_finalization_lookback
        };
        tp.spawn_critical("eth handle", Box::pin(this.instrument(span)));

        let handle = EthHandle::new(tx);

        Ok(handle)
    }

    fn send_events(&mut self, event: EthEvent) {
        self.event_listeners
            .retain(|e| e.send(event.clone()).is_ok());
    }

    fn on_command(&mut self, command: EthCommand) {
        match command {
            EthCommand::SubscribeEthNetworkEvents(tx) => self.event_listeners.push(tx)
        }
    }

    fn on_new_block(&mut self, block: (u64, Vec<Transaction>)) {
        let (bn, txes) = block;

        self.send_events(EthEvent::NewBlock(bn));
        // no underflows today
        if bn > self.block_finalization_lookback {
            self.send_events(EthEvent::FinalizedBlock(bn - self.block_finalization_lookback));
        }

        // find angstrom tx
        let Some(angstrom_tx) = txes
            .into_iter()
            .find(|tx| tx.to == Some(self.angstrom_contract))
        else {
            tracing::info!("No angstrom tx found");
            return
        };
        let input = angstrom_tx.input();

        let Ok(bytes) = TestnetHub::executeCall::abi_decode(input, false) else {
            tracing::warn!("found angstrom contract call thats not a bundle");
            return
        };

        // decode call input to grab orders. Drop function sig
        let Ok(bundle) = ContractBundle::abi_decode(&bytes.data, false) else {
            tracing::error!("failed to decode bundle");
            return
        };

        let hashes = bundle.get_filled_hashes();
        let addresses = bundle.get_addresses_touched();
        tracing::info!("found angstrom tx with orders filled {:#?}", hashes);
        self.send_events(EthEvent::NewBlockTransitions {
            block_number:      block.0,
            filled_orders:     hashes,
            address_changeset: addresses
        });
    }
}

impl<S: Stream<Item = (u64, Vec<Transaction>)> + Unpin + Send + 'static> Future
    for AnvilEthDataCleanser<S>
{
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while let Poll::Ready(Some(block)) = self.block_subscription.poll_next_unpin(cx) {
            tracing::info!("received new block from anvil");
            self.on_new_block(block);
        }
        while let Poll::Ready(Some(cmd)) = self.commander.poll_next_unpin(cx) {
            tracing::info!("received command from chan");
            self.on_command(cmd);
        }

        Poll::Pending
    }
}
