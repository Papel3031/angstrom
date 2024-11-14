mod anvil_cleanser;
mod rpc_provider;

use alloy_primitives::Address;
pub use rpc_provider::*;
mod state_provider;
pub use anvil_cleanser::*;
pub use state_provider::*;
mod block_provider;
pub mod utils;
pub use block_provider::*;
mod initializer;
use alloy::pubsub::PubSubFrontend;
pub use initializer::*;
use utils::AnvilWalletRpc;

use crate::contracts::environment::TestAnvilEnvironment;

#[derive(Debug, Clone)]
pub struct AnvilWallet {
    provider:                  AnvilWalletRpc,
    pub controller_address:    Address,
    pub controller_secret_key: alloy::signers::local::PrivateKeySigner
}

impl AnvilWallet {
    pub fn new(
        provider: AnvilWalletRpc,
        controller_address: Address,
        controller_secret_key: alloy::signers::local::PrivateKeySigner
    ) -> Self {
        Self { provider, controller_address, controller_secret_key }
    }

    pub fn provider_ref(&self) -> &AnvilWalletRpc {
        &self.provider
    }

    pub fn provider(&self) -> AnvilWalletRpc {
        self.provider.clone()
    }
}

impl TestAnvilEnvironment for AnvilWallet {
    type P = AnvilWalletRpc;
    type T = PubSubFrontend;

    fn provider(&self) -> &AnvilWalletRpc {
        &self.provider
    }

    fn controller(&self) -> Address {
        self.controller_address
    }
}
