use reth_network::Direction;
use reth_primitives::PeerId;
use tokio::{sync::mpsc, time::Instant};

use crate::types::message::StromProtocolMessage;
/// Commands that can be sent to the spawned session.
//TODO: Create a subvariant of messages only for bidirectional messages received during an active
// session
#[derive(Debug)]
pub enum SessionCommand {
    /// Disconnect the connection
    Disconnect {
        /// The direction of the session, either `Inbound` or `Outgoing`
        direction: Direction
    },
    /// Sends a message to the peer
    Message(StromProtocolMessage)
}

/// An established session with a remote peer.
#[derive(Debug)]
#[allow(dead_code)]
pub struct StromSessionHandle {
    /// The direction of the session
    pub(crate) direction:           Direction,
    /// The identifier of the remote peer
    pub(crate) remote_id:           PeerId,
    /// The timestamp when the session has been established.
    pub(crate) established:         Instant,
    /// Sender half of the command channel used send commands _to_ the spawned
    /// session
    pub(crate) commands_to_session: mpsc::Sender<SessionCommand>
}