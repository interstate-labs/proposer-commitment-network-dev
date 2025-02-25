use std::ops::{Deref, DerefMut};

use alloy_v092::primitives::{Address, U256};

use crate::{metrics::ApiMetrics, utils::score_cache::ScoreCache};

const GET_SCORE: isize = 4;
const INSERT_SCORE: isize = 4;
const UPDATE_SCORE: isize = -1;

#[derive(Debug, Clone, Copy, Default)]
pub struct AccountState {
    pub transaction_count: u64,
    pub balance: U256,
    pub has_code: bool,
}

#[derive(Debug, Default)]
pub struct AccountStateCache(
    pub ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, Address, AccountState>,
);

impl Deref for AccountStateCache {
    type Target = ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, Address, AccountState>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AccountStateCache {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AccountStateCache {
    pub fn insert(&mut self, address: Address, account_state: AccountState) {
        ApiMetrics::set_account_states(self.len());
        self.0.insert(address, account_state);
    }
}
