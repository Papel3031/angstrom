use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::AnvilInstance,
    providers::{ext::AnvilApi, Provider},
    pubsub::PubSubFrontend,
    signers::local::PrivateKeySigner
};
use alloy_primitives::{
    aliases::{I24, U24},
    Address, FixedBytes, U256
};
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::{AngstromInstance, PoolKey},
        mintable_mock_erc_20::MintableMockERC20,
        pool_gate::PoolGate::PoolGateInstance
    },
    matching::SqrtPriceX96
};

use super::{utils::AnvilWalletRpc, AnvilWallet};
use crate::{
    contracts::{
        anvil::SafeDeployPending,
        environment::{
            angstrom::AngstromEnv,
            uniswap::{TestUniswapEnv, UniswapEnv},
            TestAnvilEnvironment
        }
    },
    testnet_controllers::AngstromTestnetConfig,
    types::initial_state::{InitialTestnetState, PendingDeployedPools}
};

const ANVIL_TESTNET_DEPLOYMENT_ENDPOINT: &str = "anvil_temp_deploy";

pub struct AnvilTestnetIntializer {
    provider:      AnvilWallet,
    // uniswap_env:  UniswapEnv<AnvilWallet>,
    angstrom_env:  AngstromEnv<UniswapEnv<AnvilWallet>>,
    angstrom:      AngstromInstance<PubSubFrontend, AnvilWalletRpc>,
    pool_gate:     PoolGateInstance<PubSubFrontend, AnvilWalletRpc>,
    pending_state: PendingDeployedPools,
    _instance:     AnvilInstance
}

impl AnvilTestnetIntializer {
    pub async fn new(config: AngstromTestnetConfig) -> eyre::Result<Self> {
        let anvil = config
            .configure_anvil(ANVIL_TESTNET_DEPLOYMENT_ENDPOINT)
            .try_spawn()?;

        let ipc = alloy::providers::IpcConnect::new(format!(
            "/tmp/anvil_{ANVIL_TESTNET_DEPLOYMENT_ENDPOINT}.ipc"
        ));
        let sk: PrivateKeySigner = anvil.keys()[config.anvil_key].clone().into();
        let controller_address = anvil.addresses()[config.anvil_key];

        tracing::debug!("deploying initialization anvil");

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = alloy::providers::builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_ipc(ipc)
            .await?;

        tracing::info!("connected to initialization anvil");

        let wallet_provider = AnvilWallet::new(rpc, controller_address, sk);

        tracing::debug!("deploying UniV4 enviroment");
        let uniswap_env = UniswapEnv::new(wallet_provider.clone()).await?;
        tracing::info!("deployed UniV4 enviroment");

        tracing::debug!("deploying Angstrom enviroment");
        let angstrom_env = AngstromEnv::new(uniswap_env).await?;
        tracing::info!("deployed Angstrom enviroment");

        let angstrom =
            AngstromInstance::new(angstrom_env.angstrom(), angstrom_env.provider().clone());
        let pool_gate =
            PoolGateInstance::new(angstrom_env.pool_gate(), angstrom_env.provider().clone());

        let pending_state = PendingDeployedPools::new();

        Ok(Self {
            provider: wallet_provider,
            //uniswap_env,
            angstrom_env,
            angstrom,
            pool_gate,
            pending_state,
            _instance: anvil
        })
    }

    /// deploys tokens, a uniV4 pool, angstrom pool
    pub async fn deploy_pool_full(&mut self) -> eyre::Result<()> {
        let nonce = self
            .provider
            .provider
            .get_account(self.provider.controller_address)
            .await?
            .nonce;

        let (first_token_tx, first_token) =
            MintableMockERC20::deploy_builder(self.provider.provider_ref())
                .deploy_pending_creation(nonce, self.provider.controller_address)
                .await?;
        self.pending_state.add_pending_tx(first_token_tx);

        let (second_token_tx, second_token) =
            MintableMockERC20::deploy_builder(self.provider.provider_ref())
                .deploy_pending_creation(nonce + 1, self.provider.controller_address)
                .await?;
        self.pending_state.add_pending_tx(second_token_tx);

        let (currency0, currency1) = if first_token < second_token {
            (first_token, second_token)
        } else {
            (second_token, first_token)
        };

        let fee = U24::ZERO;
        let tick_spacing = 10;
        let pool = PoolKey {
            currency0,
            currency1,
            fee,
            tickSpacing: I24::unchecked_from(tick_spacing),
            hooks: Address::default()
        };
        self.pending_state.add_pool_key(pool.clone());

        let liquidity = 1_000_000_000_000_000_u128;
        let price = SqrtPriceX96::at_tick(100000)?;

        let init_pool = self
            .angstrom
            .initializePool(pool.currency0, pool.currency1, U256::ZERO, *price)
            .nonce(nonce + 2)
            .deploy_pending()
            .await?;
        self.pending_state.add_pending_tx(init_pool);

        let add_liq = self
            .pool_gate
            .addLiquidity(
                pool.currency0,
                pool.currency1,
                I24::unchecked_from(99000),
                I24::unchecked_from(101000),
                U256::from(liquidity),
                FixedBytes::<32>::default()
            )
            .from(self.provider.controller_address)
            .nonce(nonce + 3)
            .deploy_pending()
            .await?;

        self.pending_state.add_pending_tx(add_liq);

        Ok(())
    }

    pub async fn initialize_state(&mut self) -> eyre::Result<InitialTestnetState> {
        let (pool_keys, _) = self.pending_state.finalize_pending_txs().await?;
        let state_bytes = self.provider.provider_ref().anvil_dump_state().await?;
        Ok(InitialTestnetState::new(self.angstrom_env.angstrom(), state_bytes, pool_keys))
    }
}

#[cfg(test)]
mod tests {
    use alloy::providers::Provider;

    use super::*;

    async fn get_block(provider: &AnvilWallet) -> eyre::Result<u64> {
        Ok(provider.provider.get_block_number().await?)
    }

    #[tokio::test]
    async fn test_can_deploy() {
        let config = AngstromTestnetConfig::default();
        let mut initializer = AnvilTestnetIntializer::new(config).await.unwrap();
        initializer.deploy_pool_full().await.unwrap();

        let current_block = get_block(&initializer.provider).await.unwrap();

        let block_mine = initializer.provider.provider.evm_mine(None).await.unwrap();

        println!("BLOCK MINED: {block_mine}");

        assert_eq!(current_block + 1, get_block(&initializer.provider).await.unwrap());

        initializer
            .pending_state
            .finalize_pending_txs()
            .await
            .unwrap();

        assert_eq!(current_block + 1, get_block(&initializer.provider).await.unwrap());
    }
}
