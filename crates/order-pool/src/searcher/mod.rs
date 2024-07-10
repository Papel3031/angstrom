use std::collections::HashMap;

use angstrom_types::{
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, user_types::TopOfBlockOrder}
};
use pending::PendingPool;

use crate::common::SizeTracker;

mod pending;

pub const SEARCHER_POOL_MAX_SIZE: usize = 15;

pub struct SearcherPool {
    /// Holds all non composable searcher order pools
    searcher_orders: HashMap<PoolId, PendingPool>,
    /// The size of the current transactions.
    _size:           SizeTracker
}

impl SearcherPool {
    pub fn new(ids: &[PoolId], max_size: Option<usize>) -> Self {
        Self {
            searcher_orders: HashMap::default(),
            _size:           SizeTracker { max: max_size, current: 0 }
        }
    }

    pub fn add_searcher_order(
        &mut self,
        order: OrderWithStorageData<TopOfBlockOrder>
    ) -> eyre::Result<()> {
        // let size = order.size();
        // if !self._size.has_space(size) {
        //     return Err(SearcherPoolError::MaxSize(order.order))
        // }
        //
        // self.searcher_orders.add_order(order)?;
        Ok(())
    }

    pub fn remove_searcher_order(&mut self, id: &u128) -> Option<TopOfBlockOrder> {
        // self.searcher_orders.remove(id)
        None
    }
}
