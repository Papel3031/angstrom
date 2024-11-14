use devnet::cli::Cli;
use reth_provider::test_utils::NoopProvider;
use testing_tools::{
    testnet_controllers::AngstromTestnet,
    types::{actions::WithAction, checked_actions::WithCheckedAction, checks::WithCheck}
};
use tracing::{debug, info};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let config = Cli::build_config();

    let mut testnet = AngstromTestnet::spawn_testnet(NoopProvider::default(), config)
        .await?
        .as_state_machine();

    info!("deployed state machine");

    testnet.check_block(4);
    testnet.advance_block();
    testnet.check_block(5);
    testnet.send_pooled_orders(vec![]);
    debug!("added pooled orders to state machine");

    testnet.run().await;

    Ok(())
}
