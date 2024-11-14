use std::fmt::Display;

use alloy::node_bindings::Anvil;

#[derive(Debug, Clone)]
pub struct AngstromTestnetConfig {
    pub anvil_key:               usize,
    pub intial_node_count:       u64,
    pub initial_rpc_port:        u16,
    pub testnet_block_time_secs: u64,
    pub testnet_kind:            TestnetKind
}

impl AngstromTestnetConfig {
    pub fn new(
        anvil_key: usize,
        intial_node_count: u64,
        initial_rpc_port: u16,
        testnet_block_time_secs: u64,
        testnet_kind: TestnetKind
    ) -> Self {
        Self {
            anvil_key,
            intial_node_count,
            initial_rpc_port,
            testnet_block_time_secs,
            testnet_kind
        }
    }

    pub fn rpc_port_with_node_id(&self, node_id: u64) -> u64 {
        self.initial_rpc_port as u64 + node_id
    }

    pub fn state_machine_config(&self) -> Option<StateMachineConfig> {
        match self.testnet_kind.clone() {
            TestnetKind::StateMachine(c) => Some(c),
            _ => None
        }
    }

    pub fn is_state_machine(&self) -> bool {
        matches!(self.testnet_kind, TestnetKind::StateMachine(..))
    }

    pub fn configure_anvil(&self, id: impl Display) -> Anvil {
        let mut anvil_builder = Anvil::new()
            .chain_id(1)
            .arg("--ipc")
            .arg(format!("/tmp/anvil_{id}.ipc"))
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit");

        if let Some(config) = self.state_machine_config() {
            anvil_builder = anvil_builder.arg("--no-mining");

            if let Some((start_block, fork_url)) = config.fork_config() {
                anvil_builder = anvil_builder
                    .fork(fork_url)
                    .arg("--fork-block-number")
                    .arg(format!("{}", start_block));
            }
        } else {
            anvil_builder = anvil_builder.block_time(self.testnet_block_time_secs);
        }
        anvil_builder
    }

    pub fn fork_block_number(&self) -> Option<u64> {
        self.state_machine_config().and_then(|s| s.start_block)
    }
}

impl Default for AngstromTestnetConfig {
    fn default() -> Self {
        Self {
            anvil_key:               7,
            intial_node_count:       2,
            initial_rpc_port:        4200,
            testnet_block_time_secs: 12,
            testnet_kind:            TestnetKind::new_raw()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum TestnetKind {
    StateMachine(StateMachineConfig),
    #[default]
    Raw
}

impl TestnetKind {
    pub fn new_raw() -> Self {
        Self::Raw
    }

    pub fn new_state_machine(start_block: Option<u64>, fork_url: Option<String>) -> Self {
        Self::StateMachine(StateMachineConfig { start_block, fork_url })
    }
}

#[derive(Debug, Clone, Default)]
pub struct StateMachineConfig {
    pub start_block: Option<u64>,
    pub fork_url:    Option<String>
}

impl StateMachineConfig {
    fn fork_config(&self) -> Option<(u64, String)> {
        self.start_block.zip(self.fork_url.clone())
    }
}
