use alloy_rpc_types::{Block, Transaction};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::testnet_controllers::AngstromTestnetConfig;

pub struct TestnetBlockProvider {
    tx:     broadcast::Sender<(u64, Vec<Transaction>)>,
    config: AngstromTestnetConfig
}

impl TestnetBlockProvider {
    pub fn new(config: AngstromTestnetConfig) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self { tx, config }
    }

    pub fn subscribe_to_new_blocks(&self) -> BroadcastStream<(u64, Vec<Transaction>)> {
        BroadcastStream::new(self.tx.subscribe())
    }

    pub fn broadcast_block(&self, block: Block) {
        let block_num = block.header.number;
        let txs = block.transactions.as_transactions().unwrap().to_vec();

        let _ = self.tx.send((block_num, txs));
    }
}
