use std::{collections::HashMap, pin::Pin, task::Poll};

use futures::stream::Stream;
mod strom_peer;
use angstrom_network::{StromMessage, StromNetworkEvent};
use futures::{stream::StreamExt, FutureExt};
use reth_metrics::common::mpsc::metered_unbounded_channel;
use reth_primitives::*;
use reth_provider::test_utils::NoopProvider;
use secp256k1::SecretKey;
use tokio_stream::wrappers::UnboundedReceiverStream;

use self::strom_peer::StromPeer;

/// the goal of the angstrom testnet is to extend reth's baseline tests
/// as-well as expand appon to allow for composing tests and ensuring full
/// performance
pub struct AngstromTestnet {
    pub peers:               HashMap<PeerId, StromPeer>,
    pub peer_network_events: HashMap<PeerId, UnboundedReceiverStream<StromNetworkEvent>>
}

impl AngstromTestnet {
    pub async fn new(peers: usize, provider: NoopProvider) -> Self {
        let peers = futures::stream::iter(0..peers)
            .map(|_| async move {
                let peer = StromPeer::new(provider.clone()).await;
                let pk = peer.get_node_public_key();
                (pk, peer)
            })
            .buffer_unordered(4)
            .collect::<HashMap<_, _>>()
            .await;

        let peer_network_events = peers
            .iter()
            .map(|(k, p)| (*k, p.sub_network_events()))
            .collect::<HashMap<_, _>>();

        Self { peers, peer_network_events }
    }

    pub fn add_new_peer(&mut self, peer: StromPeer) {
        let pk = peer.get_node_public_key();
        self.peers.insert(pk, peer);
    }

    pub fn peers(&self) -> impl Iterator<Item = (&PeerId, &StromPeer)> + '_ {
        self.peers.iter()
    }

    pub fn peers_mut(&mut self) -> impl Iterator<Item = (&PeerId, &mut StromPeer)> + '_ {
        self.peers.iter_mut()
    }

    /// ensures all peers have eachother on there validator list
    pub async fn connect_all_peers(&mut self) {
        // add all as validators
        let peer_set = self.peers.iter().collect::<Vec<_>>();
        for (pk, peer) in &self.peers {
            for (other, _) in &peer_set {
                if pk == *other {
                    continue
                }
                peer.add_validator(**other);
            }
        }
        // add all peers to each other
        let peers = self.peers.iter().collect::<Vec<_>>();
        for (idx, (_, handle)) in peers.iter().enumerate().take(self.peers.len() - 1) {
            for idx in (idx + 1)..peers.len() {
                let (id, neighbour) = &peers[idx];
                handle.connect_to_peer(**id, neighbour.socket_addr());
            }
        }

        // wait on each peer to add all other peers
        let needed_peers = self.peers.len() - 1;
        let mut peers = self.peers.iter_mut().map(|(_, p)| p).collect::<Vec<_>>();
        let mut chans = self.peer_network_events.values_mut().collect::<Vec<_>>();

        std::future::poll_fn(|cx| {
            let mut all_connected = true;
            for peer in &mut peers {
                if let Poll::Ready(_) = peer.poll_unpin(cx) {
                    tracing::error!("peer failed");
                }
                all_connected &= peer.get_peer_count() == needed_peers
            }

            for chan in &mut chans {
                if let Poll::Ready(Some(msg)) = chan.poll_next_unpin(cx) {
                    tracing::debug!(?msg, "peer got msg");
                }
            }
            if all_connected {
                return Poll::Ready(())
            }

            Poll::Pending
        })
        .await
    }

    pub async fn clear_strom_network_event_chan(&mut self) {
        std::future::poll_fn(|cx| {
            self.peer_network_events.values_mut().for_each(|chan| {
                while !chan.as_ref().is_empty() {
                    let _ = chan.poll_next_unpin(cx);
                }
            });
            Poll::Ready(())
        })
        .await;
    }

    async fn message_test<T: PartialEq + Eq>(
        &mut self,
        mut rx: Pin<Box<dyn Stream<Item = T>>>,
        expected: T,
        mut expected_messages: usize
    ) -> bool {
        std::future::poll_fn(|cx| {
            // make sure to progress our peers so they can receive msgs
            for peer in self.peers.values_mut() {
                if let Poll::Ready(_) = peer.poll_unpin(cx) {
                    tracing::warn!("peer returned early");

                    return Poll::Ready(false)
                }
            }

            // poll the channel and check all messages are equal
            while let Poll::Ready(Some(received_msg)) = rx.poll_next_unpin(cx) {
                if &received_msg != &expected {
                    tracing::warn!("unexpected message");
                    return Poll::Ready(false)
                }
                expected_messages -= 1;
            }

            if expected_messages == 0 {
                return Poll::Ready(true)
            }

            Poll::Pending
        })
        .await
    }

    /// takes a random peer and gets them to broadcast the message. we then
    /// take all other peers and ensure that they received the message.
    pub async fn broadcast_message_orders(&mut self, msg: StromMessage) -> bool {
        // clear all of the channels from any message
        let (tx, rx) = metered_unbounded_channel("testing orders");

        self.peers.iter_mut().for_each(|(_, peer)| {
            peer.manager_mut().install_pool_manager(tx.clone());
        });

        // fetch our sender peer
        let (_, peer) = self.peers.iter_mut().take(1).collect::<Vec<_>>().remove(0);

        // send message to other peers
        peer.handle.broadcast_tx(msg.clone());
        let expected_msg_cnt = self.peers.len() - 1;

        let expected = if let StromMessage::PropagatePooledOrders(o) = msg {
            o
        } else {
            tracing::warn!("broadcast message orders called with a non order message");
            return false
        };

        let rx = Box::pin(rx.map(|msg| match msg {
            angstrom_network::NetworkOrderEvent::IncomingOrders { orders, .. } => orders
        }));

        let res = self.message_test(rx, expected, expected_msg_cnt).await;

        // uninstall channel
        self.peers.iter_mut().for_each(|(_, peer)| {
            peer.manager_mut().remove_pool_manager();
        });

        res
    }

    /// returns the next event that any peer emits
    pub async fn progress_to_next_network_event(&mut self) -> StromNetworkEvent {
        std::future::poll_fn(|cx| {
            for sub in self.peer_network_events.values_mut() {
                if let Poll::Ready(Some(res)) = sub.poll_next_unpin(cx) {
                    return Poll::Ready(res)
                }
            }

            Poll::Pending
        })
        .await
    }
}
