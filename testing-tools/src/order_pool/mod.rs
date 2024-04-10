use std::task::Poll;

use angstrom_eth::manager::EthEvent;
use angstrom_network::{
    pool_manager::{PoolHandle, PoolManager},
    NetworkOrderEvent, StromNetworkEvent, StromNetworkHandle
};
use angstrom_types::rpc::{
    EcRecoveredComposableLimitOrder, EcRecoveredComposableSearcherOrder, EcRecoveredLimitOrder,
    EcRecoveredSearcherOrder
};
use futures::{future::poll_fn, FutureExt};
use order_pool::{OrderPoolInner, PoolConfig};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::mocks::validator::MockValidator;

type DefaultMockPoolManager = PoolManager<
    EcRecoveredLimitOrder,
    EcRecoveredComposableLimitOrder,
    EcRecoveredSearcherOrder,
    EcRecoveredComposableSearcherOrder,
    MockValidator
>;

type DefaultPoolHandle = PoolHandle<
    EcRecoveredLimitOrder,
    EcRecoveredComposableLimitOrder,
    EcRecoveredSearcherOrder,
    EcRecoveredComposableSearcherOrder
>;
/// The Testnet orderpool allows us to test the orderpool functionality in a
/// standalone and an iterop mode. what this means is we can use this for
/// specific unit tests aswell as longer full range tests
pub struct TestnetOrderPool {
    pub pool_manager: DefaultMockPoolManager,
    pub pool_handle:  DefaultPoolHandle
}

impl TestnetOrderPool {
    pub fn new_full_mock(
        validator: MockValidator,
        network_handle: StromNetworkHandle,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>
    ) -> Self {
        let (tx, rx) = unbounded_channel();
        let rx = UnboundedReceiverStream::new(rx);
        let handle = PoolHandle { manager_tx: tx.clone() };
        let inner = OrderPoolInner::new(validator, PoolConfig::default());

        Self {
            pool_manager: PoolManager::new(
                inner,
                network_handle,
                strom_network_events,
                eth_network_events,
                tx,
                rx,
                order_events
            ),
            pool_handle:  handle
        }
    }

    pub fn get_handle(&self) -> &DefaultPoolHandle {
        &self.pool_handle
    }

    pub async fn poll_until<F: FnMut() -> bool>(&mut self, mut finished: F) -> bool {
        poll_fn(|cx| {
            if let Poll::Ready(_) = self.pool_manager.poll_unpin(cx) {
                return Poll::Ready(false)
            }

            if finished() {
                return Poll::Ready(true)
            } else {
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
        .await
    }
}
