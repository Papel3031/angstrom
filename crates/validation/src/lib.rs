#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

pub mod common;
pub mod order;
pub mod validator;

use std::{
    fmt::Debug,
    path::Path,
    sync::{atomic::AtomicU64, Arc}
};

use angstrom_utils::key_split_threadpool::KeySplitThreadpool;
use common::db::BlockStateProviderFactory;
use futures::Stream;
use order::state::{
    config::load_validation_config,
    db_state_utils::{FetchUtils, StateFetchUtils},
    pools::{AngstromPoolsTracker, PoolsTracker}
};
use reth_provider::StateProviderFactory;
use tokio::sync::mpsc::unbounded_channel;
use validator::Validator;

use crate::{
    order::{
        order_validator::OrderValidator, sim::SimValidation,
        state::config::load_data_fetcher_config
    },
    validator::ValidationClient
};

pub const TOKEN_CONFIG_FILE: &str = "./crates/validation/state_config.toml";

pub fn init_validation<DB: Unpin + Clone + 'static + revm::DatabaseRef + Send + Sync>(
    db: DB,
    cache_max_bytes: usize,
    current_block: u64
) -> ValidationClient
where
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    let (validator_tx, validator_rx) = unbounded_channel();
    let config_path = Path::new(TOKEN_CONFIG_FILE);
    let validation_config = load_validation_config(config_path).unwrap();
    let data_fetcher_config = load_data_fetcher_config(config_path).unwrap();
    let current_block = Arc::new(AtomicU64::new(current_block));
    let revm_lru = Arc::new(db);
    let fetch = FetchUtils::new(data_fetcher_config.clone(), revm_lru.clone());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();
        let handle = rt.handle().clone();

        // load storage slot state + pools
        let pools = AngstromPoolsTracker::new(validation_config.clone());
        let thread_pool =
            KeySplitThreadpool::new(handle, validation_config.max_validation_per_user);
        let sim = SimValidation::new(revm_lru.clone());
        let order_validator = OrderValidator::new(sim, current_block, pools, fetch, thread_pool);

        rt.block_on(async { Validator::new(validator_rx, order_validator).await })
    });

    ValidationClient(validator_tx)
}

pub fn init_validation_tests<
    DB: Unpin + Clone + 'static + revm::DatabaseRef + Send + Sync,
    State: StateFetchUtils + Sync + 'static,
    Pool: PoolsTracker + Sync + 'static
>(
    db: DB,
    cache_max_bytes: usize,
    state: State,
    pool: Pool,
    block_number: u64
) -> (ValidationClient, Arc<DB>)
where
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    let (tx, rx) = unbounded_channel();
    let config_path = Path::new(TOKEN_CONFIG_FILE);
    let validation_config = load_validation_config(config_path).unwrap();
    let fetcher_config = load_data_fetcher_config(config_path).unwrap();
    let current_block = Arc::new(AtomicU64::new(block_number));
    let revm_lru = Arc::new(db);
    let task_db = revm_lru.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        let thread_pool =
            KeySplitThreadpool::new(handle, validation_config.max_validation_per_user);
        let sim = SimValidation::new(task_db);
        let order_validator = OrderValidator::new(sim, current_block, pool, state, thread_pool);

        rt.block_on(Validator::new(rx, order_validator))
    });

    (ValidationClient(tx), revm_lru)
}

pub trait BundleValidator: Send + Sync + Clone + Unpin + 'static {}

impl BundleValidator for ValidationClient {}
