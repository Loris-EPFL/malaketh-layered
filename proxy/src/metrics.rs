use metrics::{counter, histogram, Counter, Histogram};
use std::{collections::HashMap, sync::Arc, time::Instant};
use tracing::error;

pub struct ProxyMetrics {
    // Request metrics
    requests_total: Counter,
    requests_duration: Histogram,
    
    // Fault injection metrics
    faults_injected_total: Counter,
    
    // Error metrics
    errors_total: Counter,
    
    // Method-specific metrics
    method_counters: Arc<std::sync::Mutex<HashMap<String, Counter>>>,
}

impl ProxyMetrics {
    pub fn new() -> Self {
        Self {
            requests_total: counter!("proxy_requests_total"),
            requests_duration: histogram!("proxy_request_duration_seconds"),
            faults_injected_total: counter!("proxy_faults_injected_total"),
            errors_total: counter!("proxy_errors_total"),
            method_counters: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn increment_success(&self, method: &str) {
        self.requests_total.increment(1);
        self.increment_method_counter(method, "success");
    }

    pub fn increment_error(&self, error_type: &str) {
        self.errors_total.increment(1);
        counter!("proxy_errors_total", "type" => error_type.to_string()).increment(1);
    }

    pub fn increment_fault_injected(&self, method: &str) {
        self.faults_injected_total.increment(1);
        counter!("proxy_faults_injected_total", "method" => method.to_string()).increment(1);
    }

    pub fn record_request_duration(&self, duration: std::time::Duration) {
        self.requests_duration.record(duration.as_secs_f64());
    }

    pub fn start_request_timer(&self) -> RequestTimer {
        RequestTimer::new()
    }

    fn increment_method_counter(&self, method: &str, status: &str) {
        counter!("proxy_requests_by_method_total", "method" => method.to_string(), "status" => status.to_string()).increment(1);
    }

    pub fn render_prometheus(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("# Metrics not available in simplified mode\n".to_string())
    }
}

pub struct RequestTimer {
    start: Instant,
}

impl RequestTimer {
    fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn finish(self, metrics: &ProxyMetrics) {
        let duration = self.start.elapsed();
        metrics.record_request_duration(duration);
    }
}

// Fault injection specific metrics
pub struct FaultMetrics {
    pub delay_injected: Counter,
    pub drops_injected: Counter,
    pub corruptions_injected: Counter,
    pub duplications_injected: Counter,
    pub errors_injected: Counter,
}

impl FaultMetrics {
    pub fn new() -> Self {
        Self {
            delay_injected: counter!("proxy_delay_faults_total"),
            drops_injected: counter!("proxy_drop_faults_total"),
            corruptions_injected: counter!("proxy_corruption_faults_total"),
            duplications_injected: counter!("proxy_duplication_faults_total"),
            errors_injected: counter!("proxy_error_faults_total"),
        }
    }

    pub fn record_delay(&self) {
        self.delay_injected.increment(1);
    }

    pub fn record_drop(&self) {
        self.drops_injected.increment(1);
    }

    pub fn record_corruption(&self) {
        self.corruptions_injected.increment(1);
    }

    pub fn record_duplication(&self) {
        self.duplications_injected.increment(1);
    }

    pub fn record_error(&self) {
        self.errors_injected.increment(1);
    }
}

impl Default for FaultMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// Engine API specific metrics
pub struct EngineApiMetrics {
    pub new_payload_calls: Counter,
    pub forkchoice_updated_calls: Counter,
    pub get_payload_calls: Counter,
    pub exchange_capabilities_calls: Counter,
}

impl EngineApiMetrics {
    pub fn new() -> Self {
        Self {
            new_payload_calls: counter!("engine_api_new_payload_total"),
            forkchoice_updated_calls: counter!("engine_api_forkchoice_updated_total"),
            get_payload_calls: counter!("engine_api_get_payload_total"),
            exchange_capabilities_calls: counter!("engine_api_exchange_capabilities_total"),
        }
    }

    pub fn record_method_call(&self, method: &str) {
        match method {
            "engine_newPayloadV1" | "engine_newPayloadV2" | "engine_newPayloadV3" => {
                self.new_payload_calls.increment(1);
            }
            "engine_forkchoiceUpdatedV1" | "engine_forkchoiceUpdatedV2" | "engine_forkchoiceUpdatedV3" => {
                self.forkchoice_updated_calls.increment(1);
            }
            "engine_getPayloadV1" | "engine_getPayloadV2" | "engine_getPayloadV3" => {
                self.get_payload_calls.increment(1);
            }
            "engine_exchangeCapabilities" => {
                self.exchange_capabilities_calls.increment(1);
            }
            _ => {
                // Record unknown engine API methods
                counter!("engine_api_unknown_method_total", "method" => method.to_string()).increment(1);
            }
        }
    }
}

impl Default for EngineApiMetrics {
    fn default() -> Self {
        Self::new()
    }
}