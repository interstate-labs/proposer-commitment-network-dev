pub const DEFAULT_BLOCK_GAS_LIMIT: u64 = 30_000_000;

const BASE_MULTIPLIER: f64 = 0.019;
const GAS_SCALAR: f64 = 1.02e-6;

#[derive(Debug)]
pub struct InclusionPricer {
    block_gas_limit: u64,
    base_multiplier: f64,
    gas_scalar: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("Preconfirmed gas {0} exceeds block limit {1}")]
    ExceedsBlockLimit(u64, u64),
    #[error("Insufficient remaining gas: requested {requested}, available {available}")]
    InsufficientGas {
        requested: u64,
        available: u64,
    },
    #[error("Invalid gas limit: Incoming gas ({incoming_gas}) is zero")]
    InvalidGasLimit {
        incoming_gas: u64,
    },

    #[error("Tip {tip} is too low. Minimum required priority fee is {min_priority_fee}")]
    TipTooLow {
        tip: u128,
        min_priority_fee: u128,
    },
}

impl Default for InclusionPricer {
    fn default() -> Self {
        Self::new(DEFAULT_BLOCK_GAS_LIMIT)
    }
}

impl InclusionPricer {
    pub fn new(block_gas_limit: u64) -> Self {
        Self { block_gas_limit, base_multiplier: BASE_MULTIPLIER, gas_scalar: GAS_SCALAR }
    }

    pub fn calculate_min_priority_fee(
        &self,
        incoming_gas: u64,
        preconfirmed_gas: u64,
    ) -> Result<u64, PricingError> {
        validate_fee_inputs(incoming_gas, preconfirmed_gas, self.block_gas_limit)?;
        let remaining_gas = self.block_gas_limit - preconfirmed_gas;
        let after_gas = remaining_gas - incoming_gas;

        let fraction = (self.gas_scalar * (remaining_gas as f64) + 1.0) /
            (self.gas_scalar * (after_gas as f64) + 1.0);

        let block_space_value = self.base_multiplier * fraction.ln();

        let inclusion_tip_wei = (block_space_value * 1e18) as u64;

        Ok(inclusion_tip_wei / incoming_gas)
    }
}

fn validate_fee_inputs(
    incoming_gas: u64,
    preconfirmed_gas: u64,
    gas_limit: u64,
) -> Result<(), PricingError> {
    if preconfirmed_gas >= gas_limit {
        return Err(PricingError::ExceedsBlockLimit(preconfirmed_gas, gas_limit));
    }

    if incoming_gas == 0 {
        return Err(PricingError::InvalidGasLimit { incoming_gas });
    }

    let remaining_gas = gas_limit - preconfirmed_gas;
    if incoming_gas > remaining_gas {
        return Err(PricingError::InsufficientGas {
            requested: incoming_gas,
            available: remaining_gas,
        });
    }
    Ok(())
}