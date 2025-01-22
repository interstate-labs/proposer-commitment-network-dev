use std::net::SocketAddr;
use std::time::Duration;

use eyre::{bail, Result};
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing::info;

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use reth_primitives::TxType;

//  Counters ----------------------------------------------------------------
const HTTP_REQUESTS_COUNTER: &str = "http_requests_counter";
const PROPOSED_LOCAL_BLOCKS_COUNTER: &str = "proposed_local_blocks_counter";
const PROPOSED_REMOTE_BLOCKS_COUNTER: &str = "proposed_remote_blocks_counter";
const RECEIVED_COMMITMENTS_COUNTER: &str = "received_commitments_counter";
const APPROVED_COMMITMENTS_COUNTER: &str = "approved_commitments_counter";
const PRECONFIRMED_TRANSACTIONS_COUNTER: &str = "preconfirmed_transactions_counter";
const VALIDATION_ERRORS_COUNTER: &str = "validation_errors_counter";
const GROSS_TIP_REVENUE_COUNTER: &str = "gross_tip_revenue_counter";

//  Gauges ------------------------------------------------------------------
const LATEST_HEAD: &str = "latest_head";

//  Histograms --------------------------------------------------------------
const HTTP_REQUESTS_DURATION_SECONDS: &str = "http_requests_duration_seconds";

/// Metrics for the commitments API.
#[derive(Debug, Clone, Copy)]
pub struct ApiMetrics;

#[allow(missing_docs)]
impl ApiMetrics {
    pub fn describe_all() {
        // Counters
        describe_counter!(HTTP_REQUESTS_COUNTER, "Total number of requests");
        describe_counter!(
            PROPOSED_LOCAL_BLOCKS_COUNTER,
            "Total number of local blocks proposed"
        );
        describe_counter!(
            PROPOSED_REMOTE_BLOCKS_COUNTER,
            "Total number of remote blocks proposed"
        );
        describe_counter!(RECEIVED_COMMITMENTS_COUNTER, "Total number of commitments");
        describe_counter!(
            APPROVED_COMMITMENTS_COUNTER,
            "Total number of commitments approved"
        );
        describe_counter!(
            PRECONFIRMED_TRANSACTIONS_COUNTER,
            "Total number of transactions preconfirmed"
        );
        describe_counter!(
            VALIDATION_ERRORS_COUNTER,
            "Total number of validation errors"
        );
        describe_counter!(
            GROSS_TIP_REVENUE_COUNTER,
            "Total number of gross tip revenue"
        );

        // Gauges
        describe_gauge!(LATEST_HEAD, "Latest slot");

        // Histograms
        describe_histogram!(
            HTTP_REQUESTS_DURATION_SECONDS,
            "Total duration of HTTP requests in seconds"
        );
    }

    /// Counters ----------------------------------------------------------------
    #[allow(dead_code)]
    pub fn increment_http_requests_count(method: String, path: String, status: String) {
        counter!(
            HTTP_REQUESTS_DURATION_SECONDS,
            &[("method", method), ("path", path), ("status", status)]
        )
        .increment(1);
    }
    #[allow(dead_code)]
    pub fn increment_proposed_local_blocks_count() {
        counter!(PROPOSED_LOCAL_BLOCKS_COUNTER).increment(1);
    }
    #[allow(dead_code)]
    pub fn increment_proposed_remote_blocks_count() {
        counter!(PROPOSED_REMOTE_BLOCKS_COUNTER).increment(1);
    }

    pub fn increment_received_commitments_count() {
        counter!(RECEIVED_COMMITMENTS_COUNTER).increment(1);
    }
    #[allow(dead_code)]
    pub fn increment_approved_commitments_count() {
        counter!(APPROVED_COMMITMENTS_COUNTER).increment(1);
    }
    #[allow(dead_code)]
    pub fn increment_gross_tip_revenue_count(mut tip: u128) {
        // If the tip is too large, we need to split it into multiple u64 parts
        if tip > u64::MAX as u128 {
            let mut parts = Vec::new();
            while tip > u64::MAX as u128 {
                parts.push(u64::MAX);
                tip -= u64::MAX as u128;
            }

            parts.push(tip as u64);

            for part in parts {
                counter!(GROSS_TIP_REVENUE_COUNTER).increment(part);
            }
        } else {
            counter!(GROSS_TIP_REVENUE_COUNTER).increment(tip as u64);
        }
    }

    pub fn increment_preconfirmed_transactions_count(tx_type: TxType) {
        counter!(
            PRECONFIRMED_TRANSACTIONS_COUNTER,
            &[("type", tx_type_str(tx_type))]
        )
        .increment(1);
    }

    pub fn increment_validation_errors_count(err_type: String) {
        counter!(VALIDATION_ERRORS_COUNTER, &[("type", err_type)]).increment(1);
    }

    /// Gauges ----------------------------------------------------------------

    pub fn set_latest_head(slot: u32) {
        gauge!(LATEST_HEAD).set(slot);
    }

    /// Mixed ----------------------------------------------------------------

    /// Observes the duration of an HTTP request by storing it in a histogram,
    /// and incrementing the total number of HTTP requests received.
    pub fn observe_http_request(duration: Duration, method: String, path: String, status: String) {
        let labels = [("method", method), ("path", path), ("status", status)];
        counter!(HTTP_REQUESTS_COUNTER, &labels).increment(1);
        histogram!(HTTP_REQUESTS_DURATION_SECONDS, &labels,).record(duration.as_secs_f64());
    }
}

pub fn run_metrics_server(metrics_port: u16) -> Result<()> {
    let prometheus_addr = SocketAddr::from(([0, 0, 0, 0], metrics_port));
    let builder = PrometheusBuilder::new().with_http_listener(prometheus_addr);

    if let Err(e) = builder.install() {
        bail!("failed to run a metrics server {:?}", e);
    } else {
        info!(
            "a metrics server running. Serving Prometheus metrics at: http://{}",
            prometheus_addr
        );
    }

    ApiMetrics::describe_all();

    Ok(())
}

fn tx_type_str(tx_type: TxType) -> &'static str {
    match tx_type {
        TxType::Legacy => "legacy",
        TxType::Eip2930 => "eip2930",
        TxType::Eip1559 => "eip1559",
        TxType::Eip4844 => "eip4844",
        TxType::Eip7702 => "eip7702",
    }
}
