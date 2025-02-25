use crate::metrics;
use alloy::eips::eip2718::Eip2718Error;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};
use tracing::{error, info};

use super::types::{ConstraintsMessage, ConstraintsWithProofData};

pub(crate) const PER_SLOT_MAX_CONSTRAINTS: usize = 128;

/// A thread-safe cache for storing constraints.
#[derive(Clone, Default, Debug)]
pub struct ConstraintStore {
    cache: Arc<RwLock<HashMap<u64, Vec<ConstraintsWithProofData>>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreConflict {
    #[error("Only one ToB constraint is allowed per slot.")]
    TopOfBlock,
    #[error("Duplicate transactions found within the same slot.")]
    DuplicateTransaction,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Conflict(#[from] StoreConflict),
    #[error(transparent)]
    Decode(#[from] Eip2718Error),
    #[error("Slot {0} has reached its maximum constraint limit.")]
    LimitExceeded(u64),
}

impl ConstraintStore {
    pub fn new() -> Self {
        Self { cache: Default::default() }
    }

    /// Adds constraints for the specified slot to the cache. This function will first check for conflicts
    /// and return an error if any are found. It also decodes transactions for future use.
    pub fn add_constraints(
        &mut self,
        slot: u64,
        constraints: ConstraintsMessage,
    ) -> Result<(), StoreError> {
        if let Some(conflict) = self.check_conflicts(&slot, &constraints) {
            return Err(conflict.into());
        }

        let message_with_data = ConstraintsWithProofData::try_from(constraints)?;

        let mut cache = self.cache.write();
        if let Some(cs) = cache.get_mut(&slot) {
            if cs.len() >= PER_SLOT_MAX_CONSTRAINTS {
                error!("Max constraints per slot reached for slot {}", slot);
                return Err(StoreError::LimitExceeded(slot));
            }

            cs.push(message_with_data);
        } else {
            cache.insert(slot, vec![message_with_data]);
        }

        metrics::CACHE_SIZE_CONSTRAINTS.inc();

        Ok(())
    }

    /// Checks for conflicts with existing constraints in the cache for the specified slot.
    /// Returns a [StoreConflict] if there is a conflict, or None if there are no issues.
    ///
    /// # Possible conflicts
    /// - Multiple ToB constraints per slot
    /// - Duplicates of the same transaction per slot
    pub fn check_conflicts(
        &self,
        slot: &u64,
        constraints: &ConstraintsMessage,
    ) -> Option<StoreConflict> {
        if let Some(saved_constraints) = self.cache.read().get(slot) {
            for saved_constraint in saved_constraints {
                //  Only one ToB constraint per slot is allowed
                // if constraints.top && saved_constraint.message.top {
                //     return Some(StoreConflict::TopOfBlock);
                // }

                // Check for duplicate transactions
                for tx in &constraints.transactions {
                    if saved_constraint.message.transactions.iter().any(|existing| tx == existing) {
                        return Some(StoreConflict::DuplicateTransaction);
                    }
                }
            }
        }

        None
    }

    /// Removes all constraints before the given slot.
    pub fn remove_before_constraints(&self, slot: u64) {
        self.cache.write().retain(|k, _| *k >= slot);
        metrics::CACHE_SIZE_CONSTRAINTS.set(self.total_constraints() as i64);
    }

    /// Gets and removes the constraints for the given slot.
    pub fn remove_constraints(&self, slot: u64) -> Option<Vec<ConstraintsWithProofData>> {
        self.cache.write().remove(&slot).inspect(|c| {
            metrics::CACHE_SIZE_CONSTRAINTS.sub(c.len() as i64);
        })
    }

    fn total_constraints(&self) -> usize {
        self.cache.read().values().map(|v| v.len()).sum()
    }
}
