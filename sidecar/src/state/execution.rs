use alloy_v092::{
    consensus::{BlobTransactionValidationError, EnvKzgSettings, Transaction},
    eips::eip4844::MAX_BLOBS_PER_BLOCK,
    primitives::{Address, U256},
    transports::TransportError,
};
use ethereum_consensus::deneb::Slot;

use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, error, info, trace, warn};

use crate::{
    builder::BlockTemplate,
    commitment::inclusion::InclusionRequest,
    config::limits::LimitOptions,
    metrics::ApiMetrics,
    utils::{
        score_cache::ScoreCache,
        transactions::{calculate_max_basefee, max_transaction_cost, validate_transaction},
    },
};

use super::{
    account_state::{AccountState, AccountStateCache},
    fetcher::StateFetcher,
    pricing::{self, InclusionPricer},
    signature::SignatureError,
};

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Transaction fee is too low, need {0} gwei to cover the maximum basefee")]
    BaseFeeTooLow(u128),
    #[error("Transaction blob fee is too low, need {0} gwei to cover the maximum blob basefee")]
    BlobBaseFeeTooLow(u128),
    #[error(transparent)]
    BlobValidation(#[from] BlobTransactionValidationError),
    #[error("Invalid max basefee calculation: overflow")]
    MaxBaseFeeCalcOverflow,
    #[error("Transaction nonce too low. Expected {0}, got {1}")]
    NonceTooLow(u64, u64),
    #[error("Transaction nonce too high. Expected {0}, got {1}")]
    NonceTooHigh(u64, u64),
    #[error("Account has code")]
    AccountHasCode,
    #[error("Gas limit too high")]
    GasLimitTooHigh,
    #[error("Transaction input size too high")]
    TransactionSizeTooHigh,
    #[error("Max priority fee per gas is greater than max fee per gas")]
    MaxPriorityFeePerGasTooHigh,
    #[error("Max priority fee per gas {0} is less than min priority fee {1}")]
    MaxPriorityFeePerGasTooLow(u128, u128),
    #[error("Not enough balance to pay for value + maximum fee")]
    InsufficientBalance,
    #[error("Pricing calculation error: {0}")]
    Pricing(#[from] pricing::PricingError),
    #[error("Too many EIP-4844 transactions in target block")]
    Eip4844Limit,
    #[error("Already requested a preconfirmation for slot {0}. Slot must be >= {0}")]
    SlotTooLow(u64),
    #[error("Max commitments reached for slot {0}: {1}")]
    MaxCommitmentsReachedForSlot(u64, usize),
    #[error("Max committed gas reached for slot {0}: {1}")]
    MaxCommittedGasReachedForSlot(u64, u64),
    #[error("Invalid signature")]
    Signature(#[from] SignatureError),
    #[error("Could not recover signer")]
    RecoverSigner,
    #[error("Chain ID mismatch")]
    ChainIdMismatch,
    #[error("Internal error: {0}")]
    Internal(String),
}

impl ValidationError {
    pub fn is_internal(&self) -> bool {
        matches!(self, Self::Internal(_))
    }

    pub const fn to_tag_str(&self) -> &'static str {
        match self {
            Self::BaseFeeTooLow(_) => "base_fee_too_low",
            Self::BlobBaseFeeTooLow(_) => "blob_base_fee_too_low",
            Self::BlobValidation(_) => "blob_validation",
            Self::MaxBaseFeeCalcOverflow => "max_base_fee_calc_overflow",
            Self::NonceTooLow(_, _) => "nonce_too_low",
            Self::NonceTooHigh(_, _) => "nonce_too_high",
            Self::AccountHasCode => "account_has_code",
            Self::GasLimitTooHigh => "gas_limit_too_high",
            Self::TransactionSizeTooHigh => "transaction_size_too_high",
            Self::MaxPriorityFeePerGasTooHigh => "max_priority_fee_per_gas_too_high",
            Self::MaxPriorityFeePerGasTooLow(_, _) => "max_priority_fee_per_gas_too_low",
            Self::InsufficientBalance => "insufficient_balance",
            Self::Pricing(_) => "pricing",
            Self::Eip4844Limit => "eip4844_limit",
            Self::SlotTooLow(_) => "slot_too_low",
            Self::MaxCommitmentsReachedForSlot(_, _) => "max_commitments_reached_for_slot",
            Self::MaxCommittedGasReachedForSlot(_, _) => "max_committed_gas_reached_for_slot",
            Self::Signature(_) => "signature",
            Self::RecoverSigner => "recover_signer",
            Self::ChainIdMismatch => "chain_id_mismatch",
            Self::Internal(_) => "internal",
        }
    }
}

#[derive(Debug)]
pub struct ExecutionState<C> {
    block_number: u64,
    slot: u64,
    basefee: u128,
    blob_basefee: u128,
    account_states: AccountStateCache,
    block_templates: HashMap<Slot, BlockTemplate>,
    chain_id: u64,
    limits: LimitOptions,
    kzg_settings: EnvKzgSettings,
    client: C,
    validation_params: ValidationParams,
    pricing: InclusionPricer,
}

#[derive(Debug)]
pub struct ValidationParams {
    pub block_gas_limit: u64,
    pub max_tx_input_bytes: usize,
    pub max_init_code_byte_size: usize,
}

impl ValidationParams {
    pub fn new(gas_limit: u64) -> Self {
        Self {
            block_gas_limit: gas_limit,
            max_tx_input_bytes: 4 * 32 * 1024,
            max_init_code_byte_size: 2 * 24576,
        }
    }
}

impl<C: StateFetcher> ExecutionState<C> {
    pub async fn new(
        client: C,
        limits: LimitOptions,
        gas_limit: u64,
    ) -> Result<Self, TransportError> {
        let (basefee, blob_basefee, block_number, chain_id) = tokio::try_join!(
            client.get_basefee(None),
            client.get_blob_basefee(None),
            client.get_head(),
            client.get_chain_id()
        )?;

        let num_accounts = limits
            .max_account_states_size
            .get()
            .div_ceil(size_of::<AccountState>() + size_of::<Address>());

        Ok(Self {
            basefee,
            blob_basefee,
            block_number,
            chain_id,
            limits,
            client,
            slot: 0,
            account_states: AccountStateCache(ScoreCache::with_max_len(num_accounts)),
            block_templates: HashMap::new(),
            kzg_settings: EnvKzgSettings::default(),
            validation_params: ValidationParams::new(gas_limit),
            pricing: InclusionPricer::new(gas_limit),
        })
    }

    pub fn basefee(&self) -> u128 {
        self.basefee
    }

    pub async fn verify_el_tx(
        &mut self,
        req: &mut InclusionRequest,
    ) -> Result<(), ValidationError> {
        req.recover_signers()?;

        let target_slot = req.slot;

        // info!("Validating Chain Id");
        if !req.validate_chain_id(self.chain_id) {
            return Err(ValidationError::ChainIdMismatch);
        }

        // info!("Validating Committed gas");
        let preconfirmed_gas = self
            .get_block_template(target_slot)
            .map(|t: &BlockTemplate| t.committed_gas())
            .unwrap_or(0);

        // info!("Validating Transaction Size");
        if preconfirmed_gas + req.gas_limit() >= self.limits.max_committed_gas_per_slot.get() {
            return Err(ValidationError::MaxCommittedGasReachedForSlot(
                self.slot,
                self.limits.max_committed_gas_per_slot.get(),
            ));
        }

        // info!("Validating Transaction Is a Contract Creation and the init code size exceeds the maximum");
        if !req.validate_init_code_limit(self.validation_params.max_init_code_byte_size) {
            return Err(ValidationError::TransactionSizeTooHigh);
        }

        // info!("Validating Gas limit is higher than the maximum block gas limit");
        if req.gas_limit() > self.validation_params.block_gas_limit {
            return Err(ValidationError::GasLimitTooHigh);
        }

        // info!("Validating if the transaction size exceeds the maximum");
        if !req.validate_tx_size_limit(self.validation_params.max_tx_input_bytes) {
            return Err(ValidationError::TransactionSizeTooHigh);
        }

        // info!("Validating max_priority_fee_per_gas is less than max_fee_per_gas");
        if !req.validate_max_priority_fee() {
            return Err(ValidationError::MaxPriorityFeePerGasTooHigh);
        }

        // info!("Validating if the max_fee_per_gas would cover the maximum possible basefee.");
        let slot_diff = target_slot.saturating_sub(self.slot);

        info!("basefee and slot_diff, {:?}, {:?}", self.basefee, slot_diff);
        let max_basefee = calculate_max_basefee(self.basefee, slot_diff)
            .ok_or(ValidationError::MaxBaseFeeCalcOverflow)?;

        debug!(%slot_diff, basefee = self.basefee, %max_basefee, "Validating basefee");

        // info!("Validating the base fee");
        if !req.validate_basefee(max_basefee) {
            return Err(ValidationError::BaseFeeTooLow(max_basefee));
        }

        // info!("Validating max_priority_fee_per_gas is greater than or equal to the calculated min_priority_fee");
        if let Err(err) = req.validate_min_priority_fee(
            &self.pricing,
            preconfirmed_gas,
            self.limits.min_inclusion_profit,
            max_basefee,
        ) {
            return Err(match err {
                pricing::PricingError::TipTooLow {
                    tip,
                    min_priority_fee,
                } => ValidationError::MaxPriorityFeePerGasTooLow(tip, min_priority_fee),
                other => ValidationError::Pricing(other),
            });
        }

        // info!("Validating target slot lower than the current slot");
        if target_slot < self.slot {
            debug!(%target_slot, %self.slot, "Target slot lower than current slot");
            return Err(ValidationError::SlotTooLow(self.slot));
        }

        // info!("Validating  each transaction in the request against the account state, keeping track of the nonce and balance diffs");
        let mut bundle_nonce_diff_map = HashMap::new();
        let mut bundle_balance_diff_map = HashMap::new();
        for tx in &req.txs {
            let sender = tx.sender().expect("Recovered sender");

            let (nonce_diff, balance_diff, highest_slot_for_account) =
                compute_diffs(&self.block_templates, sender);

            if target_slot < highest_slot_for_account {
                debug!(%target_slot, %highest_slot_for_account, "There is a request for a higher slot");
                return Err(ValidationError::SlotTooLow(highest_slot_for_account));
            }

            let account_state = match self.account_states.get(sender).copied() {
                Some(account) => account,
                None => {
                    let account = match self.client.get_account_state(sender, None).await {
                        Ok(account) => account,
                        Err(err) => {
                            return Err(ValidationError::Internal(format!(
                                "Error fetching account state: {:?}",
                                err
                            )))
                        }
                    };

                    self.account_states.insert(*sender, account);
                    account
                }
            };

            debug!(
                ?sender,
                ?account_state,
                ?nonce_diff,
                ?balance_diff,
                "Validating transaction"
            );

            let sender_nonce_diff = bundle_nonce_diff_map.entry(sender).or_insert(0);
            let sender_balance_diff = bundle_balance_diff_map.entry(sender).or_insert(U256::ZERO);

            let account_state_with_diffs = AccountState {
                transaction_count: account_state
                    .transaction_count
                    .saturating_add(nonce_diff)
                    .saturating_add(*sender_nonce_diff),

                balance: account_state
                    .balance
                    .saturating_sub(balance_diff)
                    .saturating_sub(*sender_balance_diff),

                has_code: account_state.has_code,
            };

            validate_transaction(&account_state_with_diffs, tx)?;

            if let Some(transaction) = tx.as_eip4844_with_sidecar() {
                if let Some(template) = self.block_templates.get(&target_slot) {
                    if template.blob_count() >= MAX_BLOBS_PER_BLOCK {
                        return Err(ValidationError::Eip4844Limit);
                    }
                }

                let max_blob_basefee = calculate_max_basefee(self.blob_basefee, slot_diff)
                    .ok_or(ValidationError::MaxBaseFeeCalcOverflow)?;

                let blob_basefee = transaction.max_fee_per_blob_gas().unwrap_or(0);

                debug!(%max_blob_basefee, %blob_basefee, "Validating blob basefee");
                if blob_basefee < max_blob_basefee {
                    return Err(ValidationError::BlobBaseFeeTooLow(max_blob_basefee));
                }

                transaction.validate_blob(self.kzg_settings.get())?;
            }

            *sender_nonce_diff += 1;
            *sender_balance_diff += max_transaction_cost(tx);
        }

        // debug!("before okay!");
        Ok(())
    }

    pub async fn update_head(
        &mut self,
        block_number: Option<u64>,
        slot: u64,
    ) -> Result<(), TransportError> {
        self.slot = slot;

        let accounts = self.account_states.keys().collect::<Vec<_>>();
        let update = self.client.get_state_update(accounts, block_number).await;
        trace!(%slot, ?update, "Applying execution state update");

        for template in self.remove_block_templates_until(slot) {
            debug!(%slot, "Removed block template for slot");
            let hashes = template.transaction_hashes();
            let receipts = self.client.get_receipts_unordered(hashes.as_ref()).await?;

            let mut receipts_len = 0;
            for receipt in receipts.iter().flatten() {
                let tip_per_gas = receipt.effective_gas_price - self.basefee;
                let total_tip = tip_per_gas * receipt.gas_used as u128;

                trace!(hash = %receipt.transaction_hash, total_tip, "Receipt found");

                ApiMetrics::increment_gross_tip_revenue_count(total_tip);
                receipts_len += 1;
            }

            if hashes.len() != receipts_len {
                warn!(
                    %slot,
                    template_hashes = hashes.len(),
                    receipts_found = receipts_len,
                    "mismatch between template transaction hashes and receipts found from client"
                );
                hashes.iter().for_each(|hash| {
                    if !receipts
                        .iter()
                        .flatten()
                        .any(|receipt| receipt.transaction_hash == *hash)
                    {
                        warn!(%hash, "missing receipt for transaction");
                    }
                });
            }
        }

        self.apply_state_update(update?);

        Ok(())
    }

    fn apply_state_update(&mut self, update: StateUpdate) {
        self.block_number = update.block_number;
        self.basefee = update.min_basefee;

        for (address, state) in update.account_states {
            let Some(prev_state) = self.account_states.get_mut(&address) else {
                error!(%address, "Account state requested for update but not found in cache");
                continue;
            };
            *prev_state = state
        }

        self.refresh_templates();
    }

    fn refresh_templates(&mut self) {
        for (address, (account_state, _)) in self.account_states.iter() {
            trace!(%address, ?account_state, "Refreshing templates...");

            let (address, mut expected_account_state) = (*address, *account_state);

            for template in self.block_templates.values_mut() {
                template.retain(address, expected_account_state);

                if let Some((nonce_diff, balance_diff)) = template.get_diff(&address) {
                    expected_account_state.transaction_count += nonce_diff;
                    expected_account_state.balance -= balance_diff;
                }
            }
        }
    }

    pub fn get_block_template(&mut self, slot: u64) -> Option<&BlockTemplate> {
        self.block_templates.get(&slot)
    }

    pub fn remove_block_templates_until(&mut self, slot: u64) -> Vec<BlockTemplate> {
        let mut slots_to_remove = self
            .block_templates
            .keys()
            .filter(|s| **s <= slot)
            .copied()
            .collect::<Vec<_>>();
        slots_to_remove.sort();

        let mut templates = Vec::with_capacity(slots_to_remove.len());
        for s in slots_to_remove {
            if let Some(template) = self.block_templates.remove(&s) {
                templates.push(template);
            }
        }

        templates
    }
}

#[derive(Debug, Clone)]
pub struct StateUpdate {
    pub account_states: HashMap<Address, AccountState>,
    pub min_basefee: u128,
    pub min_blob_basefee: u128,
    pub block_number: u64,
}

fn compute_diffs(
    block_templates: &HashMap<u64, BlockTemplate>,
    sender: &Address,
) -> (u64, U256, u64) {
    block_templates.iter().fold(
        (0, U256::ZERO, 0),
        |(nonce_diff_acc, balance_diff_acc, highest_slot), (slot, block_template)| {
            let (nonce_diff, balance_diff, current_slot) = block_template
                .get_diff(sender)
                .map(|(nonce, balance)| (nonce, balance, *slot))
                .unwrap_or((0, U256::ZERO, 0));
            trace!(?nonce_diff, ?balance_diff, ?slot, ?sender, "found diffs");

            (
                nonce_diff_acc + nonce_diff,
                balance_diff_acc.saturating_add(balance_diff),
                u64::max(highest_slot, current_slot),
            )
        },
    )
}
