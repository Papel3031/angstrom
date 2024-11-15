pub mod bundle;
pub mod common;
pub mod order;
pub mod validator;

use std::{
    fmt::Debug,
    sync::{atomic::AtomicU64, Arc}
};

use alloy::primitives::Address;
use angstrom_types::{
    contract_payloads::angstrom::{AngstromBundle, AngstromPoolConfigStore},
    pair_with_price::PairsWithPrice
};
use angstrom_utils::key_split_threadpool::KeySplitThreadpool;
use bundle::{BundleResponse, BundleValidator};
use common::{token_pricing, SharedTools};
use futures::StreamExt;
use matching_engine::cfmm::uniswap::pool_manager::SyncedUniswapPools;
use order::state::{db_state_utils::StateFetchUtils, pools::PoolsTracker};
use reth_provider::CanonStateNotificationStream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use validator::Validator;

use crate::{
    common::TokenPriceGenerator,
    order::{
        order_validator::OrderValidator,
        sim::SimValidation,
        state::{db_state_utils::FetchUtils, pools::AngstromPoolsTracker}
    },
    validator::{ValidationClient, ValidationRequest}
};

const MAX_VALIDATION_PER_ADDR: usize = 2;

#[allow(clippy::too_many_arguments)]
pub fn init_validation<
    DB: Unpin + Clone + 'static + reth_provider::BlockNumReader + revm::DatabaseRef + Send + Sync
>(
    db: DB,
    current_block: u64,
    angstrom_address: Option<Address>,
    state_notification: CanonStateNotificationStream,
    uniswap_pools: SyncedUniswapPools,
    price_generator: TokenPriceGenerator,
    pool_store: Arc<AngstromPoolConfigStore>,
    validator_rx: UnboundedReceiver<ValidationRequest>
) where
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    let current_block = Arc::new(AtomicU64::new(current_block));
    let revm_lru = Arc::new(db);
    let fetch = FetchUtils::new(Address::default(), revm_lru.clone());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();

        let handle = rt.handle().clone();
        let pools = AngstromPoolsTracker::new(angstrom_address.unwrap_or_default(), pool_store);
        // load storage slot state + pools
        let thread_pool = KeySplitThreadpool::new(handle, MAX_VALIDATION_PER_ADDR);
        let sim = SimValidation::new(revm_lru.clone(), angstrom_address);

        // load price update stream;
        let update_stream = PairsWithPrice::into_price_update_stream(
            angstrom_address.unwrap_or_default(),
            state_notification
        )
        .boxed();

        let order_validator =
            rt.block_on(OrderValidator::new(sim, current_block, pools, fetch, uniswap_pools));

        let bundle_validator = BundleValidator::new(revm_lru.clone());
        let shared_utils = SharedTools::new(price_generator, update_stream, thread_pool);

        rt.block_on(async {
            Validator::new(validator_rx, order_validator, bundle_validator, shared_utils).await
        })
    });
}

pub fn init_validation_tests<
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Send + Sync,
    State: StateFetchUtils + Sync + 'static,
    Pool: PoolsTracker + Sync + 'static
>(
    db: DB,
    uniswap_pools: SyncedUniswapPools,
    state: State,
    pool: Pool,
    block_number: u64,
    state_notification: CanonStateNotificationStream,
    price_generator: TokenPriceGenerator
) -> (ValidationClient, Arc<DB>)
where
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    let (tx, rx) = unbounded_channel();
    let current_block = Arc::new(AtomicU64::new(block_number));
    let revm_lru = Arc::new(db);
    let task_db = revm_lru.clone();

    // load price update stream;
    let update_stream = PairsWithPrice::into_price_update_stream(
        // TODO: set later.
        Default::default(),
        state_notification
    )
    .boxed();

    let revm = revm_lru.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        let thread_pool = KeySplitThreadpool::new(handle, MAX_VALIDATION_PER_ADDR);

        let sim = SimValidation::new(task_db, None);

        let order_validator =
            rt.block_on(OrderValidator::new(sim, current_block, pool, state, uniswap_pools));

        let bundle_validator = BundleValidator::new(revm_lru.clone());
        let shared_utils = SharedTools::new(price_generator, update_stream, thread_pool);

        rt.block_on(Validator::new(rx, order_validator, bundle_validator, shared_utils))
    });

    (ValidationClient(tx), revm)
}
