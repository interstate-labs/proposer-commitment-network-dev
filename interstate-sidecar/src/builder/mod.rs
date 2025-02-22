pub mod constraint;

use alloy_v092::{
    consensus::Transaction,
    primitives::{Address, TxHash, U256},
};
use constraint::SignedConstraints;
use std::collections::HashMap;
use tracing::warn;

use crate::{
    constraints::{Constraint, TransactionExt}, state::account_state::AccountState, utils::transactions::{max_transaction_cost, FullTransaction}
};
#[derive(Debug, Default)]
pub struct BlockTemplate {
    /// The state diffs per address given the list of commitments.
    pub(crate) state_diff: StateDiff,
    /// The signed constraints associated to the block
    pub signed_constraints_list: Vec<SignedConstraints>,
}

impl BlockTemplate {
    pub fn get_diff(&self, address: &Address) -> Option<(u64, U256)> {
        self.state_diff.get_diff(address)
    }

    #[inline]
    pub fn transaction_hashes(&self) -> Vec<TxHash> {
        self.signed_constraints_list
            .iter()
            .flat_map(|sc| sc.message.transactions.iter().map(|c| *c.tx.hash()))
            .collect()
    }

    #[inline]
    pub fn committed_gas(&self) -> u64 {
        self.signed_constraints_list.iter().fold(0, |acc, sc| {
            acc + sc
                .message
                .transactions
                .iter()
                .fold(0, |acc, c| acc + c.tx.gas_limit())
        })
    }

    #[inline]
    pub fn blob_count(&self) -> usize {
        self.signed_constraints_list.iter().fold(0, |mut acc, sc| {
            acc += sc.message.transactions.iter().fold(0, |acc, c| {
                acc + c
                    .tx.as_eip4844()
                    .map(|tx| tx.blob_versioned_hashes.len())
                    .unwrap_or(0)
            });

            acc
        })
    }

    fn remove_constraints_at_index(&mut self, index: usize) {
        let constraints = self.signed_constraints_list.remove(index);

        for constraint in &constraints.message.transactions {
            self.state_diff
                .diffs
                .entry(constraint.sender.expect("recovered sender"))
                .and_modify(|(nonce, balance)| {
                    *nonce = nonce.saturating_sub(1);
                    *balance -= max_transaction_cost(&constraint.tx);
                });
        }
    }

    pub fn retain(&mut self, address: Address, state: AccountState) {
        let mut indexes: Vec<usize> = Vec::new();

        let constraints_with_address: Vec<(usize, Vec<&Constraint>)> = self
            .signed_constraints_list
            .iter()
            .enumerate()
            .map(|(idx, c)| (idx, &c.message.transactions))
            .filter(|(_idx, c)| {
                c.iter()
                    .any(|c| c.sender.expect("recovered sender") == address)
            })
            .map(|(idx, c)| {
                (
                    idx,
                    c.iter()
                        .filter(|c| c.sender.expect("recovered sender") == address)
                        .collect(),
                )
            })
            .collect();

        let (max_total_cost, min_nonce) = constraints_with_address
            .iter()
            .flat_map(|c| c.1.clone())
            .fold((U256::ZERO, u64::MAX), |(total_cost, min_nonce), c| {
                (
                    total_cost + max_transaction_cost(&c.tx),
                    min_nonce.min(c.tx.nonce()),
                )
            });

        if state.balance < max_total_cost || state.transaction_count > min_nonce {
            warn!(
                %address,
                "Removing invalidated constraints for address"
            );
            indexes = constraints_with_address.iter().map(|(i, _)| *i).collect();
        }

        for index in indexes.into_iter().rev() {
            self.remove_constraints_at_index(index);
        }
    }
}

#[derive(Debug, Default)]
pub struct StateDiff {
    pub(crate) diffs: HashMap<Address, (u64, U256)>,
}

impl StateDiff {
    pub fn get_diff(&self, address: &Address) -> Option<(u64, U256)> {
        self.diffs.get(address).copied()
    }
}
