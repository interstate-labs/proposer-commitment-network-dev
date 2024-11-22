use std::{
  collections::HashMap,
  pin::Pin,
  task::{Context, Poll},
  time::Duration
};

use alloy::primitives::Sign;
use futures::{future::poll_fn, Future, FutureExt};
use tokio::time::Sleep;

use crate::constraints::SignedConstraints;
pub struct ConstraintState {
  pub blocks: HashMap<u64, Block>,
  pub commitment_deadline: CommitmentDeadline
}

impl ConstraintState {
  pub fn new () -> Self {
    Self {
      blocks: HashMap::new(),
      commitment_deadline: CommitmentDeadline::new(0, Duration::from_millis(100))
    }
  }
  
  pub fn add_constraint(&mut self, slot: u64, signed_constraints: SignedConstraints) {
    if let Some(block) = self.blocks.get_mut(&slot) {
        block.add_constraints(signed_constraints);
    } else {
        let mut block = Block::default();
        block.add_constraints(signed_constraints);
        self.blocks.insert(slot, block);
    }
  }

  pub fn remove_constraints_at_slot( &mut self, slot: u64) -> Option<Block> {
    self.blocks.remove(&slot)
  }
}

#[derive(Debug, Default)]
pub struct  Block {
  pub signed_constraints_list: Vec<SignedConstraints>
}

impl Block {
  pub fn add_constraints ( &mut self, constraints: SignedConstraints) {
    self.signed_constraints_list.push(constraints);
  }
  pub fn remove_constraints( &mut self, slot: u64){
    self.signed_constraints_list.remove(slot.try_into().unwrap());
  }
}


/// The deadline for a which a commitment is considered valid.
#[derive(Debug)]
pub struct CommitmentDeadline {
    slot: u64,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl CommitmentDeadline {
    /// Create a new deadline for a given slot and duration.
    pub fn new(slot: u64, duration: Duration) -> Self {
        let sleep = Some(Box::pin(tokio::time::sleep(duration)));
        Self { slot, sleep }
    }

    /// Poll the deadline until it is reached.
    pub async fn wait(&mut self) -> Option<u64> {
        let slot = poll_fn(|cx| self.poll_unpin(cx)).await;
        self.sleep = None;
        slot
    }
}

/// Poll the deadline until it is reached.
///
/// - If already reached, the future will return `None` immediately.
/// - If not reached, the future will return `Some(slot)` when the deadline is reached.
impl Future for CommitmentDeadline {
    type Output = Option<u64>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(ref mut sleep) = self.sleep else {
            return Poll::Ready(None);
        };

        match sleep.as_mut().poll(cx) {
            Poll::Ready(_) => Poll::Ready(Some(self.slot)),
            Poll::Pending => Poll::Pending,
        }
    }
}
