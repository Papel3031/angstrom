use std::{collections::HashMap, path::Path, time::Duration};

use alloy_primitives::{hex, Address, U256};
use angstrom_types::orders::OrderOrigin;
use futures::future::{select, Either};
use testing_tools::{
    load_reth_db, mocks::eth_events::MockEthEventHandle,
    type_generator::orders::generate_rand_valid_limit_order, validation::TestOrderValidator
};
use validation::order::{state::upkeepers::ANGSTROM_CONTRACT, OrderValidator};

const WETH_ADDRESS: Address = Address::new(hex!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"));
const USDT_ADDRESS: Address = Address::new(hex!("dAC17F958D2ee523a2206206994597C13D831ec7"));

macro_rules! init_tools {
    () => {{
        reth_tracing::init_test_tracing();
        let db_path = Path::new("/home/data/reth/db/");
        let db = load_reth_db(db_path);

        let (handle, eth_events) = MockEthEventHandle::new();
        TestOrderValidator::new(db, Box::pin(eth_events), handle)
    }};
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn test_validation_pass() {
    let mut validator = init_tools!();

    // setup order to validate
    let mut order = generate_rand_valid_limit_order();
    order.order.currencyIn = WETH_ADDRESS;
    order.order.currencyOut = USDT_ADDRESS;
    let nonce = order.order.nonce;

    let address = order.recover_signer().unwrap();
    // overwrite the slots to ensure the balance needed exists
    let weth_approval = validator
        .config
        .approvals
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let approval_slot = weth_approval
        .generate_slot(address, ANGSTROM_CONTRACT)
        .unwrap();

    let weth_balance = validator
        .config
        .balances
        .iter()
        .find(|a| a.token == WETH_ADDRESS)
        .unwrap();

    let balance_slot = weth_balance.generate_slot(address).unwrap();
    let mut state_overrides = HashMap::new();

    let mut weth = HashMap::new();
    weth.insert(balance_slot, U256::from(order.order.amountIn));
    weth.insert(approval_slot, U256::from(order.order.amountIn));

    let mut nonce_map = HashMap::new();
    let slot = validator.generate_nonce_slot(address, nonce.to());
    nonce_map.insert(slot, U256::ZERO);

    state_overrides.insert(WETH_ADDRESS, weth);
    state_overrides.insert(ANGSTROM_CONTRACT, nonce_map);
    validator.revm_lru.set_state_overrides(state_overrides);

    let client = validator.client.clone();
    let out = select(
        client.validate_order(OrderOrigin::External, order.try_into().unwrap()),
        Box::pin(validator.poll_for(Duration::from_millis(100)))
    )
    .await;

    match out {
        Either::Left((i, _)) => {
            assert!(i.is_valid(), "order wasn't valid");
        }
        Either::Right(..) => {
            panic!("timeout hit on validation");
        }
    }
}

// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// #[serial_test::serial]
// async fn test_validation_nonce_failure() {
//     init_tools!();
//     // setup order to validate
//     let mut order = generate_rand_valid_limit_order();
//     order.order.currencyIn = WETH_ADDRESS;
//     order.order.currencyOut = USDT_ADDRESS;
//     let address = order.recover_signer().unwrap();
// }
//
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// #[serial_test::serial]
// async fn test_validation_balance_failure() {
//     init_tools!();
//
//     // setup order to validate
//     let mut order = generate_rand_valid_limit_order();
//     order.order.currencyIn = WETH_ADDRESS;
//     order.order.currencyOut = USDT_ADDRESS;
//     let address = order.recover_signer().unwrap();
// }
//
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// #[serial_test::serial]
// async fn test_validation_approval_failure() {
//     init_tools!();
//
//     // setup order to validate
//     let mut order = generate_rand_valid_limit_order();
//     order.order.currencyIn = WETH_ADDRESS;
//     order.order.currencyOut = USDT_ADDRESS;
//     let address = order.recover_signer().unwrap();
// }
