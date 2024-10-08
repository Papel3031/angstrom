use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc
    },
    task::{Context, Poll}
};

use angstrom_network::StromNetworkManager;
use futures::FutureExt;
use reth_network::test_utils::Peer;
use reth_provider::BlockReader;
use tracing::{Instrument, Span};

pub(crate) struct TestnetPeerFuture {
    eth_peer_fut:      Pin<Box<dyn Future<Output = ()> + Send>>,
    /// the default ethereum network peer
    strom_network_fut: Pin<Box<dyn Future<Output = ()> + Send>>,
    running:           Arc<AtomicBool>
}

impl TestnetPeerFuture {
    pub(crate) fn new<C: Unpin + BlockReader + 'static>(
        eth_peer: Peer<C>,
        strom_network: StromNetworkManager<C>,
        running: Arc<AtomicBool>,
        span: Span
    ) -> Self {
        Self {
            eth_peer_fut: Box::pin(eth_peer.instrument(span.clone())),
            strom_network_fut: Box::pin(strom_network.instrument(span.clone())),
            running
        }
    }
}

impl Future for TestnetPeerFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if this.running.load(Ordering::Relaxed) {
            if this.eth_peer_fut.poll_unpin(cx).is_ready() {
                return Poll::Ready(())
            }

            if this.strom_network_fut.poll_unpin(cx).is_ready() {
                return Poll::Ready(())
            }
        }

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}