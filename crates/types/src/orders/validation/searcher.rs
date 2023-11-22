use alloy_primitives::{Address, Bytes, TxHash, B160, U256};

use super::{
    super::{OrderId, OrderOrigin, PooledComposableOrder, PooledOrder},
    ValidatedOrder
};
use crate::{
    primitive::{ComposableOrder, Order, PoolId, PoolKey},
    rpc::{EcRecoveredComposableSearcherOrder, EcRecoveredSearcherOrder}
};

pub trait PooledSearcherOrder: PooledOrder {
    /// The liquidity pool this order trades in
    fn pool(&self) -> u8;
    /// donate value
    fn donate(&self) -> (u128, u128);

    fn volume(&self) -> u128;

    fn gas(&self) -> u128;

    fn donated(&self) -> u128;
}

impl<O> ValidatedOrder<O, SearcherPriorityData>
where
    O: PooledSearcherOrder
{
    pub fn pool_id(&self) -> usize {
        self.pool_id
    }

    pub fn is_bid(&self) -> bool {
        self.is_bid
    }

    pub fn priority_data(&self) -> SearcherPriorityData {
        self.data.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearcherPriorityData {
    pub donated: u128,
    pub volume:  u128,
    pub gas:     u128
}

/// Reverse ordering for arb priority data to sort donated value in descending
/// order
impl PartialOrd for SearcherPriorityData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(
            other
                .donated
                .cmp(&self.donated)
                .then_with(|| other.volume.cmp(&self.volume))
        )
    }
}

impl Ord for SearcherPriorityData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl PooledOrder for EcRecoveredSearcherOrder {
    type ValidationData = ValidatedOrder<Self, SearcherPriorityData>;

    fn is_valid(&self) -> bool {
        todo!()
    }

    fn is_bid(&self) -> bool {
        todo!()
    }

    fn hash(&self) -> TxHash {
        self.signed_order.hash
    }

    fn from(&self) -> Address {
        self.signer
    }

    fn nonce(&self) -> U256 {
        self.order.nonce
    }

    fn amount_in(&self) -> u128 {
        self.signed_order.order.amountIn
    }

    fn amount_out_min(&self) -> u128 {
        self.signed_order.order.amountOutMin
    }

    fn limit_price(&self) -> u128 {
        self.amount_out_min() / self.amount_in()
    }

    fn deadline(&self) -> U256 {
        self.signed_order.order.deadline
    }

    fn size(&self) -> usize {
        unreachable!()
    }

    fn encoded_length(&self) -> usize {
        unreachable!()
    }

    fn chain_id(&self) -> Option<u64> {
        unreachable!()
    }

    fn order_id(&self) -> OrderId {
        todo!()
    }

    fn token_in(&self) -> B160 {
        todo!()
    }
}

impl PooledSearcherOrder for EcRecoveredSearcherOrder {
    fn gas(&self) -> u128 {
        todo!()
    }

    fn pool(&self) -> u8 {
        todo!()
    }

    fn donate(&self) -> (u128, u128) {
        todo!()
    }

    fn volume(&self) -> u128 {
        todo!()
    }

    fn donated(&self) -> u128 {
        todo!()
    }
}

impl PooledOrder for EcRecoveredComposableSearcherOrder {
    type ValidationData = ();

    fn is_valid(&self) -> bool {
        todo!()
    }

    fn is_bid(&self) -> bool {
        todo!()
    }

    fn hash(&self) -> TxHash {
        self.signed_order.hash
    }

    fn from(&self) -> Address {
        self.signer
    }

    fn token_in(&self) -> B160 {
        todo!()
    }

    fn nonce(&self) -> U256 {
        self.order.nonce
    }

    fn amount_in(&self) -> u128 {
        self.signed_order.order.amountIn
    }

    fn amount_out_min(&self) -> u128 {
        self.signed_order.order.amountOutMin
    }

    fn limit_price(&self) -> u128 {
        self.amount_out_min() / self.amount_in()
    }

    fn deadline(&self) -> U256 {
        self.signed_order.order.deadline
    }

    fn size(&self) -> usize {
        unreachable!()
    }

    fn encoded_length(&self) -> usize {
        unreachable!()
    }

    fn chain_id(&self) -> Option<u64> {
        unreachable!()
    }

    fn order_id(&self) -> OrderId {
        todo!()
    }
}

impl PooledSearcherOrder for EcRecoveredComposableSearcherOrder {
    fn gas(&self) -> u128 {
        todo!()
    }

    fn pool(&self) -> u8 {
        todo!()
    }

    fn donate(&self) -> (u128, u128) {
        todo!()
    }

    fn volume(&self) -> u128 {
        todo!()
    }

    fn donated(&self) -> u128 {
        todo!()
    }
}

impl PooledComposableOrder for EcRecoveredComposableSearcherOrder {
    fn pre_hook(&self) -> Option<Bytes> {
        todo!()
    }

    fn post_hook(&self) -> Option<Bytes> {
        todo!()
    }
}
