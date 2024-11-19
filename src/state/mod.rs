use std::collections::HashMap;
use alloy::primitives::Sign;

use crate::constraints::SignedConstraints;
pub struct ConstraintState {
  blocks: HashMap<u64, Block>
}

impl ConstraintState {
  pub fn new () -> Self {
    Self {
      blocks: HashMap::new()
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