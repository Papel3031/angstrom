use std::pin::Pin;

use alloy_rlp::Encodable;
use futures::{
    task::{Context, Poll},
    Stream, StreamExt
};
use reth_eth_wire::multiplex::ProtocolConnection;
use reth_metrics::common::mpsc::MeteredPollSender;
use reth_primitives::{BytesMut, PeerId};
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use super::handle::SessionCommand;
use crate::StromSessionMessage;
#[allow(dead_code)]
pub struct StromSession {
    /// Keeps track of request ids.
    pub(crate) next_id: u64,
    /// The underlying connection.
    pub(crate) conn: ProtocolConnection,
    /// Identifier of the node we're connected to.
    pub(crate) remote_peer_id: PeerId,
    /// Incoming commands from the manager
    pub(crate) commands_rx: ReceiverStream<SessionCommand>,
    /// Sink to send messages to the [`SessionManager`](super::SessionManager).
    pub(crate) to_session_manager: MeteredPollSender<StromSessionMessage>,
    /// A message that needs to be delivered to the session manager
    pub(crate) pending_message_to_session: Option<StromSessionMessage>,

    /// All requests sent to the remote peer we're waiting on a response
    //pub(crate) inflight_requests: FnvHashMap<u64, InflightRequest>,
    /// All requests that were sent by the remote peer and we're waiting on an
    /// internal response
    //pub(crate) received_requests_from_remote: Vec<ReceivedRequest>,
    /// Buffered messages that should be handled and sent to the peer.
    //pub(crate) queued_outgoing: VecDeque<OutgoingMessage>,
    /// The maximum time we wait for a response from a peer.

    /// If an [ActiveSession] does not receive a response at all within this
    /// duration then it is considered a protocol violation and the session
    /// will initiate a drop.
    pub(crate) protocol_breach_request_timeout: Duration,
    /// Used to reserve a slot to guarantee that the termination message is
    /// delivered
    pub(crate) terminate_message: Option<(PollSender<StromSessionMessage>, StromSessionMessage)>
}

impl StromSession {
    pub fn new(
        conn: ProtocolConnection,
        peer_id: PeerId,
        commands_rx: ReceiverStream<SessionCommand>,
        to_session_manager: MeteredPollSender<StromSessionMessage>,
        protocol_breach_request_timeout: Duration
    ) -> Self {
        Self {
            next_id: 0,
            conn,
            remote_peer_id: peer_id,
            commands_rx,
            to_session_manager,
            pending_message_to_session: None,
            protocol_breach_request_timeout,
            terminate_message: None
        }
    }

    /// Report back that this session has been closed.
    fn emit_disconnect(&mut self, cx: &mut Context<'_>) -> Poll<Option<BytesMut>> {
        let msg = StromSessionMessage::Disconnected { peer_id: self.remote_peer_id };

        self.terminate_message = Some((self.to_session_manager.inner().clone(), msg));
        self.poll_terminate_message(cx).expect("message is set")
    }

    /// If a termination message is queued, this will try to send it to the
    /// manager
    fn poll_terminate_message(&mut self, cx: &mut Context<'_>) -> Option<Poll<Option<BytesMut>>> {
        let (mut tx, msg) = self.terminate_message.take()?;
        match tx.poll_reserve(cx) {
            Poll::Pending => {
                self.terminate_message = Some((tx, msg));
                return Some(Poll::Pending)
            }
            Poll::Ready(Ok(())) => {
                let _ = tx.send_item(msg);
            }
            Poll::Ready(Err(_)) => {
                // channel closed
            }
        }
        // terminate the task
        Some(Poll::Ready(None))
    }
}

//TODO: Implement poll functionality with: on_command, on_timeout, on_message
// from wire..

impl Stream for StromSession {
    type Item = BytesMut;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // if the session is terminate we have to send the termination message before we
        // can close
        if let Some(terminate) = this.poll_terminate_message(cx) {
            return terminate
        }

        loop {
            let mut _progress = false;

            // we prioritize incoming commands sent from the session manager
            loop {
                match this.commands_rx.poll_next_unpin(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        // this is only possible when the manager was dropped, in which case we also
                        // terminate this session
                        return Poll::Ready(None)
                    }
                    Poll::Ready(Some(command)) => {
                        _progress = true;
                        match command {
                            SessionCommand::Disconnect { direction: _ } => {
                                return this.emit_disconnect(cx)
                            }
                            SessionCommand::Message(msg) => {
                                let mut bytes = BytesMut::new();
                                msg.encode(&mut bytes);

                                return Poll::Ready(Some(bytes))
                            }
                        }
                    }
                }
            }
        }
    }
}