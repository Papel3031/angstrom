use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc
    },
    task::{Context, Poll}
};

use alloy::transports::Transport;
use angstrom_network::StromNetworkManager;
use consensus::ConsensusManager;
use futures::FutureExt;
use parking_lot::Mutex;
use reth_network::test_utils::Peer;
use reth_provider::BlockReader;
use tokio::task::JoinHandle;
use tracing::{span, Level};

pub(crate) struct TestnetStateFutureLock<C, T> {
    eth_peer:              StateLockInner<Peer<C>>,
    strom_network_manager: StateLockInner<StromNetworkManager<C>>,
    strom_consensus:       Option<StateLockInner<ConsensusManager<T>>>
}

impl<C, T> TestnetStateFutureLock<C, T>
where
    C: Unpin + BlockReader + 'static,
    T: Transport + Clone
{
    pub(crate) fn new(
        node_id: u64,
        eth_peer: Peer<C>,
        strom_network_manager: StromNetworkManager<C>,
        consensus: Option<ConsensusManager<T>>
    ) -> Self {
        Self {
            eth_peer:              StateLockInner::new(node_id, eth_peer),
            strom_network_manager: StateLockInner::new(node_id, strom_network_manager),
            strom_consensus:       consensus.map(|c| StateLockInner::new(node_id, c))
        }
    }

    pub(crate) fn strom_network_manager<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&StromNetworkManager<C>) -> R
    {
        self.strom_network_manager.on_inner(f)
    }

    pub(crate) fn strom_network_manager_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut StromNetworkManager<C>) -> R
    {
        self.strom_network_manager.on_inner_mut(f)
    }

    pub(crate) fn eth_peer<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Peer<C>) -> R
    {
        self.eth_peer.on_inner(f)
    }

    pub(crate) fn eth_peer_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Peer<C>) -> R
    {
        self.eth_peer.on_inner_mut(f)
    }

    pub(crate) fn strom_consensus<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ConsensusManager<T>) -> R
    {
        self.strom_consensus
            .as_ref()
            .expect("consensus is not enabled")
            .on_inner(f)
    }

    pub(crate) fn strom_consensus_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ConsensusManager<T>) -> R
    {
        self.strom_consensus
            .as_ref()
            .expect("consensus is not enabled")
            .on_inner_mut(f)
    }

    pub(crate) fn set_network(&self, running: bool) {
        self.eth_peer.lock.store(running, Ordering::Relaxed);
        self.strom_network_manager
            .lock
            .store(running, Ordering::Relaxed);
    }

    pub(crate) fn set_consensus(&self, running: bool, checked: bool) {
        if checked {
            if let Some(c) = self.strom_consensus.as_ref() {
                c.lock.store(running, Ordering::Relaxed)
            }
        } else {
            self.strom_consensus
                .as_ref()
                .expect("consensus is not enabled")
                .lock
                .store(running, Ordering::Relaxed);
        }
    }

    /// false -> off
    /// true -> on
    pub(crate) fn network_state(&self) -> bool {
        self.eth_peer.lock.load(Ordering::Relaxed)
            && self.strom_network_manager.lock.load(Ordering::Relaxed)
    }

    /// false -> off
    /// true -> on
    pub(crate) fn consensus_state(&self) -> bool {
        self.strom_consensus
            .as_ref()
            .map(|c| c.lock.load(Ordering::Relaxed))
            .unwrap_or_default()
    }

    pub(crate) fn poll_fut_to_initialize_network_connections(
        &mut self,
        cx: &mut Context<'_>
    ) -> Poll<()> {
        if self.eth_peer.fut.poll_unpin(cx).map(|_| ()).is_ready()
            || self
                .strom_network_manager
                .fut
                .poll_unpin(cx)
                .map(|_| ())
                .is_ready()
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

struct StateLockInner<T> {
    inner: Arc<Mutex<T>>,
    lock:  Arc<AtomicBool>,
    fut:   JoinHandle<()>
}

impl<T: Unpin + Future + Send + 'static> StateLockInner<T> {
    fn new(node_id: u64, inner_unarced: T) -> Self {
        let lock = Arc::new(AtomicBool::new(false));
        let inner = Arc::new(Mutex::new(inner_unarced));
        let inner_fut = StateLockFut::new(node_id, inner.clone(), lock.clone());

        Self { inner, lock, fut: tokio::spawn(inner_fut) }
    }

    fn on_inner<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R
    {
        f(&self.inner.lock())
    }

    fn on_inner_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R
    {
        f(&mut self.inner.lock())
    }
}

impl<T> Drop for StateLockInner<T> {
    fn drop(&mut self) {
        self.fut.abort();
    }
}

struct StateLockFut<T> {
    node_id: u64,
    inner:   Arc<Mutex<T>>,
    lock:    Arc<AtomicBool>
}

impl<T> StateLockFut<T> {
    fn new(node_id: u64, inner: Arc<Mutex<T>>, lock: Arc<AtomicBool>) -> Self {
        Self { inner, lock, node_id }
    }
}

impl<T> Future for StateLockFut<T>
where
    T: Unpin + Future
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let span = span!(Level::TRACE, "node", id = this.node_id);
        let e = span.enter();

        if this.lock.load(Ordering::Relaxed) && this.inner.lock_arc().poll_unpin(cx).is_ready() {
            return Poll::Ready(())
        }

        drop(e);

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
