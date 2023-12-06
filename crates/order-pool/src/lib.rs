mod common;
mod config;
mod inner;
mod limit;
mod searcher;
mod validator;
use std::{collections::HashMap, sync::Arc};

use alloy_primitives::TxHash;
use config::PoolConfig;
use futures_util::future::BoxFuture;
use guard_types::{
    orders::{
        OrderConversion, OrderOrigin, OrderPriorityData, PoolOrder, PooledComposableOrder,
        PooledLimitOrder, PooledOrder, PooledSearcherOrder, SearcherPriorityData, ToOrder,
        ValidatedOrder
    },
    primitive::PoolId
};
pub use guard_utils::*;
pub use inner::*;

#[derive(Debug)]
pub struct OrderSet<Limit: PoolOrder, Searcher: PoolOrder> {
    pub limit_vanilla:    HashMap<PoolId, BidsAndAsks<Limit>>,
    pub searcher_vanilla: HashMap<PoolId, ValidatedOrder<Searcher, Searcher::ValidationData>>
}

#[derive(Debug)]
pub struct BidsAndAsks<O: PoolOrder> {
    pub bids: Vec<ValidatedOrder<O, O::ValidationData>>,
    pub asks: Vec<ValidatedOrder<O, O::ValidationData>>
}

#[derive(Debug)]
pub struct AllOrders<
    Limit: PoolOrder,
    Searcher: PoolOrder,
    LimitCompose: PoolOrder,
    SearcherCompose: PoolOrder
> {
    pub vanilla:    OrderSet<Limit, Searcher>,
    pub composable: OrderSet<LimitCompose, SearcherCompose>
}

/// The OrderPool Trait is how other processes can interact with the orderpool
/// asyncly. This allows for requesting data and providing data from different
/// threads efficiently.
// #[auto_impl::auto_impl(Arc)]
pub trait OrderPool: Send + Sync + Clone + Unpin + 'static {
    /// The transaction type of the limit order pool
    type LimitOrder: ToOrder;

    /// The transaction type of the searcher order pool
    type SearcherOrder: ToOrder;

    /// The transaction type of the composable limit order pool
    type ComposableLimitOrder: ToOrder;

    /// The transaction type of the composable searcher order pool
    type ComposableSearcherOrder: ToOrder;
    // New order functionality.
    fn new_limit_order(&self, origin: OrderOrigin, order: Self::LimitOrder);
    fn new_searcher_order(&self, origin: OrderOrigin, order: Self::SearcherOrder);
    fn new_composable_limit_order(&self, origin: OrderOrigin, order: Self::ComposableLimitOrder);
    fn new_composable_searcher_order(
        &self,
        origin: OrderOrigin,
        order: Self::ComposableSearcherOrder
    );

    // Queries for fetching all orders. Will be used for quoting
    // and consensus.

    // fetches all vanilla orders
    fn get_all_vanilla_orders(
        &self
    ) -> BoxFuture<
        OrderSet<<Self::LimitOrder as ToOrder>::Order, <Self::SearcherOrder as ToOrder>::Order>
    >;
    // fetches all vanilla orders where for each pool the bids and asks overlap plus
    // a buffer on each side
    // fn get_all_vanilla_orders_intersection(
    //     &self,
    //     buffer: usize
    // ) -> BoxFuture<
    //     OrderSet<<Self::LimitOrder as ToOrder>::Order, <Self::SearcherOrder as
    // ToOrder>::Order> >;

    // fn get_all_composable_orders(
    //     &self
    // ) -> BoxFuture<
    //     OrderSet<
    //         <Self::ComposableLimitOrder as OrderConversion>::Order,
    //         <Self::ComposableSearcherOrder as OrderConversion>::Order
    //     >
    // >;
    //
    // fn get_all_composable_orders_intersection(
    //     &self,
    //     buffer: usize
    // ) -> BoxFuture<
    //     OrderSet<
    //         <Self::ComposableLimitOrder as OrderConversion>::Order,
    //         <Self::ComposableSearcherOrder as OrderConversion>::Order
    //     >
    // >;
    //
    // fn get_all_orders(
    //     &self
    // ) -> BoxFuture<
    //     AllOrders<
    //         <Self::LimitOrder as OrderConversion>::Order,
    //         <Self::SearcherOrder as OrderConversion>::Order,
    //         <Self::ComposableLimitOrder as OrderConversion>::Order,
    //         <Self::ComposableSearcherOrder as OrderConversion>::Order
    //     >
    // >;
    //
    // fn get_all_orders_intersection(
    //     &self,
    //     buffer: usize
    // ) -> BoxFuture<
    //     AllOrders<
    //         <Self::LimitOrder as OrderConversion>::Order,
    //         <Self::SearcherOrder as OrderConversion>::Order,
    //         <Self::ComposableLimitOrder as OrderConversion>::Order,
    //         <Self::ComposableSearcherOrder as OrderConversion>::Order
    //     >
    // >;
}
