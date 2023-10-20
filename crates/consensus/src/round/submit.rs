use std::{
    pin::Pin,
    task::{Context, Poll, Waker}
};

use common::{ConsensusState, PollExt, WAITING_NEXT_BLOCK};
use futures::FutureExt;
use guard_types::on_chain::SimmedBundle;

use super::{completed::CompletedState, RoundAction, RoundStateMessage, StateTransition, Timeout};

/// This state is only reached if this guard is the leader
pub struct SubmitState {
    submit_deadline: Timeout,
    best_bundle:     SimmedBundle,
    current_commits: Vec<()>,
    needed_commits:  usize,
    can_send:        bool
}

impl SubmitState {
    pub fn new() -> Self {
        todo!()
    }

    pub fn on_new_commit(&mut self, commit: ()) {
        // check if contains if
        if self.current_commits.len() == self.needed_commits {
            self.can_send = true;
        }
    }
}

impl StateTransition for SubmitState {
    fn should_transition(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)> {
        self.submit_deadline
            .poll_unpin(cx)
            .filter(|_| self.can_send)
            .map(|_| {
                // submission here
                (RoundAction::Completed(CompletedState), WAITING_NEXT_BLOCK, None)
            })
    }
}
