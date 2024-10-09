//! extension functionality to sol types
use std::fmt;

use alloy::primitives::{Address, TxHash, U256};
use serde::{Deserialize, Serialize};

use crate::orders::OrderLocation;

pub mod order_ext;
pub mod contract_bundle_ext;
pub mod grouped_orders;
pub mod top_of_block_ext;

/// The capability of all default orders.
pub trait RawPoolOrder: fmt::Debug + Send + Sync + Clone + Unpin + 'static {
    /// defines  
    /// Hash of the order
    fn order_hash(&self) -> TxHash;

    /// The order signer
    fn from(&self) -> Address;

    /// Amount of tokens to sell
    fn amount_in(&self) -> u128;

    /// Min amount of tokens to buy
    fn amount_out_min(&self) -> u128;

    /// Limit Price
    fn limit_price(&self) -> U256;

    /// Order deadline
    fn deadline(&self) -> Option<U256>;
    /// order flash block
    fn flash_block(&self) -> Option<u64>;

    /// the way in which we avoid a respend attack
    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod;

    /// token in
    fn token_in(&self) -> Address;
    /// token out
    fn token_out(&self) -> Address;

    fn is_valid_signature(&self) -> bool;

    fn order_location(&self) -> OrderLocation;
}

/// allows us to flip a order. This is useful for when we need to do gas
/// simulation as we will want to be able to simulate a order completely against
/// itself.
pub trait FlipOrder {
    fn flip_order(&self) -> Self
    where
        Self: Sized;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash, Copy)]
pub enum RespendAvoidanceMethod {
    Nonce(u64),
    Block(u64)
}

impl RespendAvoidanceMethod {
    pub fn get_ord_for_pending_orders(&self) -> u64 {
        let Self::Nonce(n) = self else { return 0 };
        *n
    }
}
