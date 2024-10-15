use std::time::Duration;

use angstrom_network::manager::StromConsensusEvent;
use angstrom_types::consensus::{Commit, PreProposal, Proposal};
use reth_rpc_types::PeerId;
use tokio::{
    sync::{
        mpsc::{unbounded_channel, UnboundedSender},
        Mutex
    },
    time::Interval
};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub struct MockConsensusEventHandle {
    tx:       UnboundedSender<StromConsensusEvent>,
    interval: Mutex<Interval>
}

impl MockConsensusEventHandle {
    pub fn new() -> (Self, UnboundedReceiverStream<StromConsensusEvent>) {
        let (tx, rx) = unbounded_channel();

        // 15 second interval
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        // If we miss, we want to make sure we're aligned with our original interval and
        // skip extra ticks
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        (Self { tx, interval: Mutex::new(interval) }, UnboundedReceiverStream::new(rx))
    }

    pub fn prepropose(&self, peer: PeerId, proposal: PreProposal) {
        self.tx
            .send(StromConsensusEvent::PrePropose(peer, proposal))
            .expect("Failed to send proposal");
    }

    pub fn propose(&self, peer: PeerId, proposal: Proposal) {
        self.tx
            .send(StromConsensusEvent::Propose(peer, proposal))
            .expect("Failed to send proposal");
    }

    pub fn commit(&self, peer: PeerId, commit: Commit) {
        self.tx
            .send(StromConsensusEvent::Commit(peer, commit))
            .expect("Failed to send commit message")
    }

    pub async fn propose_on_next_tick(&self, peer: PeerId, proposal: Proposal) {
        let mut i = self.interval.lock().await;
        i.tick().await;
        self.propose(peer, proposal);
    }
}
