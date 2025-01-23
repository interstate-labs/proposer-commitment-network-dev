use cb_pbs::PbsService;
use lazy_static::lazy_static;
use prometheus::{
    register_histogram_vec_with_registry, register_int_counter_vec_with_registry,
    register_int_gauge_with_registry, HistogramVec, IntCounterVec, IntGauge, Registry,
};

pub(crate) const ERROR_CODE_TIMEOUT_STR: &str = "555";
pub(crate) const TAG_GET_HEADER_WITH_PROOFS: &str = "get_header_with_proofs";

pub(crate) fn initialize_metrics() -> eyre::Result<()> {
    // Register all the metrics for PBS Service
    PbsService::register_metric(Box::new(LATENCY_BY_RELAY.clone()));
    PbsService::register_metric(Box::new(RELAY_HTTP_STATUS.clone()));
    PbsService::register_metric(Box::new(INVALID_BIDS_COUNT.clone()));
    PbsService::register_metric(Box::new(CACHE_SIZE_CONSTRAINTS.clone()));

    // Initialize PBS Service metrics
    PbsService::init_metrics()
}

lazy_static! {
    pub static ref INTERSTATE_BOOST_METRICS: Registry =
        Registry::new_custom(Some("interstate_boost_metrics".to_string()), None).unwrap();

    // Metric to count HTTP status codes per relay and endpoint
    pub static ref RELAY_HTTP_STATUS: IntCounterVec = register_int_counter_vec_with_registry!(
        "relay_http_status_total",
        "Total number of HTTP status codes received by relays, categorized by status code, endpoint, and relay ID",
        &["http_status_code", "endpoint", "relay_id"],
        INTERSTATE_BOOST_METRICS
    )
    .unwrap();

    // Metric to track the size of constraints cache
    pub static ref CACHE_SIZE_CONSTRAINTS: IntGauge = register_int_gauge_with_registry!(
        "cache_size_constraints",
        "Current size of the constraints cache",
        INTERSTATE_BOOST_METRICS
    )
    .unwrap();

    /// Latency by relay by endpoint
    pub static ref LATENCY_BY_RELAY: HistogramVec = register_histogram_vec_with_registry!(
        "latency_by_relay",
        "Current size of the constraints cache",
        &["endpoint", "relay_id"],
        INTERSTATE_BOOST_METRICS
    )
    .unwrap();

    /// Invalid bids per relay
    pub static ref INVALID_BIDS_COUNT: IntCounterVec = register_int_counter_vec_with_registry!(
        "invalid_bids_total",
        "Total number of invalid bids received from relays, categorized by relay ID",
        &["relay_id"],
        INTERSTATE_BOOST_METRICS
    )
    .unwrap();

}
