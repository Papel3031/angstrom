use std::{fmt::Debug, future::Future, pin::Pin};

use angstrom_types::{
    orders::{OrderOrigin, OrderValidationOutcome, PoolOrder},
    rpc::{
        EcRecoveredComposableLimitOrder, EcRecoveredComposableSearcherOrder, EcRecoveredLimitOrder,
        EcRecoveredSearcherOrder
    },
    sol_bindings::{
        grouped_orders::{
            AllOrders, GroupedComposableOrder, GroupedVanillaOrder, OrderWithStorageData
        },
        sol::TopOfBlockOrder
    }
};
use tokio::sync::oneshot::{channel, Sender};

use crate::validator::ValidationRequest;

pub mod order_validator;
pub mod sim;
pub mod state;

use crate::validator::ValidationClient;

pub type ValidationFuture<'a, O> =
    Pin<Box<dyn Future<Output = OrderWithStorageData<O>> + Send + Sync + 'a>>;

pub type ValidationsFuture<'a, O> =
    Pin<Box<dyn Future<Output = Vec<OrderWithStorageData<O>>> + Send + Sync + 'a>>;

pub enum OrderValidationRequest {
    ValidateOrder(Sender<OrderWithStorageData<AllOrders>>, AllOrders, OrderOrigin)
}

/// TODO: not a fan of all the conversions. can def simplify
impl From<OrderValidationRequest> for OrderValidation {
    fn from(value: OrderValidationRequest) -> Self {
        match value {
            OrderValidationRequest::ValidateOrder(tx, order, orign) => match order {
                AllOrders::Partial(p) => {
                    if p.hook_data.is_empty() {
                        OrderValidation::Limit(tx, GroupedVanillaOrder::Partial(p), orign)
                    } else {
                        OrderValidation::LimitComposable(
                            tx,
                            GroupedComposableOrder::Partial(p),
                            orign
                        )
                    }
                }
                AllOrders::KillOrFill(kof) => {
                    if kof.hook_data.is_empty() {
                        OrderValidation::Limit(tx, GroupedVanillaOrder::KillOrFill(kof), orign)
                    } else {
                        OrderValidation::LimitComposable(
                            tx,
                            GroupedComposableOrder::KillOrFill(kof),
                            orign
                        )
                    }
                }
                AllOrders::TOB(tob) => OrderValidation::Searcher(tx, tob, orign)
            }
        }
    }
}

pub enum OrderValidation {
    Limit(Sender<OrderWithStorageData<AllOrders>>, GroupedVanillaOrder, OrderOrigin),
    LimitComposable(Sender<OrderWithStorageData<AllOrders>>, GroupedComposableOrder, OrderOrigin),
    Searcher(Sender<OrderWithStorageData<AllOrders>>, TopOfBlockOrder, OrderOrigin)
}

/// Provides support for validating transaction at any given state of the chain
pub trait OrderValidatorHandle: Send + Sync + Clone + Debug + Unpin + 'static {
    /// The order type of the limit order pool
    type Order: Send + Sync;

    fn validate_order(
        &self,
        origin: OrderOrigin,
        transaction: Self::Order
    ) -> ValidationFuture<Self::Order>;

    /// Validates a batch of orders.
    ///
    /// Must return all outcomes for the given orders in the same order.

    fn validate_orders(
        &self,
        transactions: Vec<(OrderOrigin, Self::Order)>
    ) -> ValidationsFuture<Self::Order> {
        Box::pin(futures_util::future::join_all(
            transactions
                .into_iter()
                .map(|(origin, tx)| self.validate_order(origin, tx))
        ))
    }
}

impl OrderValidatorHandle for ValidationClient {
    type Order = AllOrders;

    fn validate_order(
        &self,
        origin: OrderOrigin,
        transaction: Self::Order
    ) -> ValidationFuture<Self::Order> {
        Box::pin(async move {
            let (tx, rx) = channel();
            // let _ = self
            //     .0
            //     .send(ValidationRequest::Order(OrderValidationRequest::ValidateLimit(
            //         tx,
            //         origin,
            //         transaction
            //     )));

            rx.await.unwrap()
        })
    }
}
