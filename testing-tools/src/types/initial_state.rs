use alloy::providers::PendingTransaction;
use alloy_primitives::{Address, Bytes, TxHash};
use angstrom_types::contract_bindings::angstrom::Angstrom::PoolKey;

#[derive(Debug, Clone)]
pub struct InitialTestnetState {
    pub angstrom_addr: Address,
    pub state:         Bytes,
    pub pool_keys:     Vec<PoolKey>
}

impl InitialTestnetState {
    pub fn new(angstrom_addr: Address, state: Bytes, pool_keys: Vec<PoolKey>) -> Self {
        Self { angstrom_addr, state, pool_keys }
    }
}

pub struct PendingDeployedPools {
    pending_txs: Vec<PendingTransaction>,
    pool_keys:   Vec<PoolKey>
}

impl Default for PendingDeployedPools {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingDeployedPools {
    pub fn new() -> Self {
        Self { pending_txs: Vec::new(), pool_keys: Vec::new() }
    }

    pub fn add_pending_tx(&mut self, tx: PendingTransaction) {
        self.pending_txs.push(tx)
    }

    pub fn add_pool_key(&mut self, pool_key: PoolKey) {
        self.pool_keys.push(pool_key)
    }

    pub fn pool_keys(&self) -> &[PoolKey] {
        &self.pool_keys
    }

    pub async fn finalize_pending_txs(&mut self) -> eyre::Result<(Vec<PoolKey>, Vec<TxHash>)> {
        let keys = std::mem::take(&mut self.pool_keys);

        let tx_hashes = futures::future::join_all(std::mem::take(&mut self.pending_txs))
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        Ok((keys, tx_hashes))
    }
}
