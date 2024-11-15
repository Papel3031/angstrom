use std::{pin::Pin, task::Poll};

use alloy::primitives::Address;
use angstrom_types::pair_with_price::PairsWithPrice;
use angstrom_utils::key_split_threadpool::KeySplitThreadpool;
use futures::{Future, Stream, StreamExt};
use tokio::runtime::Handle;

pub mod db;
pub use db::*;

pub mod token_pricing;
pub use token_pricing::*;

/// Tools that are shared between both order and bundle validation
pub struct SharedTools {
    token_pricing:       TokenPriceGenerator,
    token_price_updater: Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
    thread_pool: KeySplitThreadpool<Address, Pin<Box<dyn Future<Output = ()> + Send>>, Handle>
}

impl SharedTools {
    pub fn new(
        token_pricing: TokenPriceGenerator,
        token_price_updater: Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
        thread_pool: KeySplitThreadpool<Address, Pin<Box<dyn Future<Output = ()> + Send>>, Handle>
    ) -> Self {
        Self { token_price_updater, token_pricing, thread_pool }
    }

    pub fn token_pricing_snapshot(&self) -> TokenPriceGenerator {
        self.token_pricing.clone()
    }
}

impl Future for SharedTools {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        self.thread_pool.try_register_waker(|| cx.waker().clone());
        while let Poll::Ready(Some(_)) = self.thread_pool.poll_next_unpin(cx) {}

        while let Poll::Ready(Some(updates)) = self.token_price_updater.poll_next_unpin(cx) {
            self.token_pricing.apply_update(updates);
        }

        Poll::Pending
    }
}
