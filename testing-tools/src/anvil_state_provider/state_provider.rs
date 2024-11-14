use std::future::IntoFuture;

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::{builder, ext::AnvilApi, Provider},
    rpc::types::{anvil::MineOptions, Block},
    signers::local::PrivateKeySigner
};
use alloy_primitives::{Address, BlockNumber, Bytes, B256, U256};
use eyre::bail;
use reth_provider::{
    BlockHashReader, BlockNumReader, CanonStateNotification, CanonStateNotifications,
    CanonStateSubscriptions, ProviderError, ProviderResult
};
use reth_revm::primitives::Bytecode;
use tokio::sync::broadcast;
use validation::common::db::BlockStateProviderFactory;

use super::{rpc_provider::RpcStateProvider, utils::AnvilWalletRpc, AnvilWallet};
use crate::{
    anvil_state_provider::utils::async_to_sync,
    mocks::canon_state::AnvilConsensusCanonStateNotification,
    testnet_controllers::AngstromTestnetConfig
};

#[derive(Debug)]
pub struct AnvilStateProviderWrapper {
    provider:  AnvilStateProvider,
    _instance: AnvilInstance
}
impl AnvilStateProviderWrapper {
    pub async fn spawn_new(config: AngstromTestnetConfig, id: u64) -> eyre::Result<Self> {
        let anvil = config.configure_anvil(id).try_spawn()?;

        let endpoint = format!("/tmp/anvil_{id}.ipc");
        tracing::info!(?endpoint);
        let ipc = alloy::providers::IpcConnect::new(endpoint.to_string());
        let sk: PrivateKeySigner = anvil.keys()[config.anvil_key].clone().into();
        let controller_address = anvil.addresses()[config.anvil_key];

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(ipc)
            .await?;

        tracing::info!("connected to anvil");

        let (tx, _) = broadcast::channel(1000);

        let provider = AnvilStateProvider {
            provider:       AnvilWallet::new(rpc, controller_address, sk),
            canon_state_tx: tx,
            canon_state:    AnvilConsensusCanonStateNotification::new()
        };

        Ok(Self { provider, _instance: anvil })
    }

    pub async fn spawn_new_isolated() -> eyre::Result<Self> {
        let anvil = Anvil::new()
            .block_time(12)
            .chain_id(1)
            .arg("--ipc")
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit")
            .try_spawn()?;

        let endpoint = "/tmp/anvil.ipc".to_string();
        tracing::info!(?endpoint);
        let ipc = alloy::providers::IpcConnect::new(endpoint.to_string());
        let sk: PrivateKeySigner = anvil.keys()[7].clone().into();
        let controller_address = anvil.addresses()[7];

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(ipc)
            .await?;

        tracing::info!("connected to anvil");

        let (tx, _) = broadcast::channel(1000);

        Ok(Self {
            provider:  AnvilStateProvider {
                provider:       AnvilWallet::new(rpc, controller_address, sk),
                canon_state_tx: tx,
                canon_state:    AnvilConsensusCanonStateNotification::new()
            },
            _instance: anvil
        })
    }

    pub fn state_provider(&self) -> AnvilStateProvider {
        self.provider.clone()
    }

    pub fn wallet_provider(&self) -> AnvilWallet {
        self.provider.provider.clone()
    }

    pub fn rpc_provider(&self) -> AnvilWalletRpc {
        self.provider.provider().clone()
    }

    pub async fn execute_and_return_state(&self) -> eyre::Result<(Bytes, Block)> {
        let block = self.mine_block().await?;

        Ok((self.provider.provider().anvil_dump_state().await?, block))
    }

    pub async fn return_state(&self) -> eyre::Result<Bytes> {
        Ok(self.provider.provider().anvil_dump_state().await?)
    }

    pub async fn set_state(&self, state: Bytes) -> eyre::Result<()> {
        self.provider.provider().anvil_load_state(state).await?;

        Ok(())
    }

    pub async fn mine_block(&self) -> eyre::Result<Block> {
        let mined = self
            .provider
            .provider()
            .anvil_mine_detailed(Some(MineOptions::Options { timestamp: None, blocks: Some(1) }))
            .await?
            .first()
            .cloned()
            .unwrap();

        self.provider.update_canon_chain(&mined)?;

        Ok(mined)
    }
}

// impl TestAnvilEnvironment for AnvilStateProviderWrapper {
//     type P = AnvilWalletRpc;
//     type T = PubSubFrontend;

//     fn provider(&self) -> &AnvilWalletRpc {
//         self.provider.provider.provider_ref()
//     }

//     fn controller(&self) -> Address {
//         self.provider.provider.controller_address
//     }
// }

#[derive(Debug, Clone)]
pub struct AnvilStateProvider {
    provider:       AnvilWallet,
    canon_state:    AnvilConsensusCanonStateNotification,
    canon_state_tx: broadcast::Sender<CanonStateNotification>
}

impl AnvilStateProvider {
    pub fn provider(&self) -> AnvilWalletRpc {
        self.provider.provider.clone()
    }

    fn update_canon_chain(&self, new_block: &Block) -> eyre::Result<()> {
        let state = self.canon_state.new_block(new_block);
        if self.canon_state_tx.receiver_count() == 0 {
            tracing::warn!("no canon state rx")
        } else {
            let _ = self
                .canon_state_tx
                .send(CanonStateNotification::Commit { new: state })?;
        }

        Ok(())
    }
}
impl reth_revm::DatabaseRef for AnvilStateProvider {
    type Error = eyre::Error;

    fn basic_ref(
        &self,
        address: Address
    ) -> Result<Option<reth_revm::primitives::AccountInfo>, Self::Error> {
        let acc = async_to_sync(self.provider().get_account(address).latest().into_future())?;
        let code = async_to_sync(self.provider().get_code_at(address).latest().into_future())?;
        let code = Some(Bytecode::new_raw(code));

        Ok(Some(reth_revm::primitives::AccountInfo {
            code_hash: acc.code_hash,
            balance: acc.balance,
            nonce: acc.nonce,
            code
        }))
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let acc = async_to_sync(self.provider().get_storage_at(address, index).into_future())?;
        Ok(acc)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        let acc = async_to_sync(
            self.provider()
                .get_block_by_number(reth_primitives::BlockNumberOrTag::Number(number), false)
                .into_future()
        )?;

        let Some(block) = acc else { bail!("failed to load block") };
        Ok(block.header.hash)
    }

    fn code_by_hash_ref(&self, _: B256) -> Result<reth_revm::primitives::Bytecode, Self::Error> {
        panic!("This should not be called, as the code is already loaded");
    }
}
impl BlockNumReader for AnvilStateProvider {
    fn chain_info(&self) -> ProviderResult<reth_chainspec::ChainInfo> {
        panic!("never used");
    }

    fn block_number(&self, _: alloy_primitives::B256) -> ProviderResult<Option<BlockNumber>> {
        panic!("never used");
    }

    fn convert_number(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        Ok(async_to_sync(self.provider().get_block_number().into_future()).unwrap())
    }

    fn last_block_number(&self) -> ProviderResult<BlockNumber> {
        Ok(async_to_sync(self.provider().get_block_number().into_future()).unwrap())
    }

    fn convert_hash_or_number(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<BlockNumber>> {
        panic!("never used");
    }
}
impl BlockHashReader for AnvilStateProvider {
    fn block_hash(&self, _: BlockNumber) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn convert_block_hash(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn canonical_hashes_range(
        &self,
        _: BlockNumber,
        _: BlockNumber
    ) -> ProviderResult<Vec<alloy_primitives::B256>> {
        panic!("never used");
    }
}

impl BlockStateProviderFactory for AnvilStateProvider {
    type Provider = RpcStateProvider;

    fn state_by_block(&self, block: u64) -> ProviderResult<Self::Provider> {
        Ok(RpcStateProvider::new(block, self.provider()))
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        async_to_sync(self.provider().get_block_number())
            .map_err(|_| ProviderError::BestBlockNotFound)
    }
}

impl CanonStateSubscriptions for AnvilStateProvider {
    fn subscribe_to_canonical_state(&self) -> CanonStateNotifications {
        self.canon_state_tx.subscribe()
    }
}

/*

send bundles and assert the same solution
validation
order pool + matching engine








*/
