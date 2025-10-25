use axum::{
    body::Body,
    http::StatusCode,
    response::Response,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::config::{FaultInjectionConfig, FaultType};

#[derive(Debug, Clone)]
pub struct FaultStats {
    pub total_requests: u64,
    pub faults_injected: u64,
    pub faults_by_type: HashMap<String, u64>,
    pub faults_by_method: HashMap<String, u64>,
}

impl Default for FaultStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            faults_injected: 0,
            faults_by_type: HashMap::new(),
            faults_by_method: HashMap::new(),
        }
    }
}

pub struct FaultInjector {
    config: FaultInjectionConfig,
    stats: Arc<Mutex<FaultStats>>,
    request_count: Arc<Mutex<u64>>,
    start_time: Instant,
}

impl FaultInjector {
    /// Add a disabled constructor for backward compatibility
    pub fn disabled() -> Self {
        Self::new(FaultInjectionConfig::default())
    }

    pub fn new(config: FaultInjectionConfig) -> Self {
        Self {
            config,
            stats: Arc::new(Mutex::new(FaultStats::default())),
            request_count: Arc::new(Mutex::new(0)),
            start_time: Instant::now(),
        }
    }

    pub async fn should_inject_fault(&self, method: &str) -> Option<Response> {
        if !self.config.enabled {
            return None;
        }

        // Increment request count
        {
            let mut count = self.request_count.lock().unwrap();
            *count += 1;
        }

        // Simple probability-based fault injection (10% chance)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        method.hash(&mut hasher);
        let hash = hasher.finish();
        
        if (hash % 100) < 10 { // 10% chance based on method hash
            // Record the fault
            self.record_fault(&FaultType::Delay, method);
            
            // Inject a simple delay fault
            sleep(Duration::from_millis(100)).await;
            
            // Return an error response
            let error_response = Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Fault injected"},"id":null}"#,
                ))
                .unwrap();
            
            return Some(error_response);
        }

        None
    }

    fn record_fault(&self, fault_type: &FaultType, method: &str) {
        let mut stats = self.stats.lock().unwrap();
        stats.total_requests += 1;
        stats.faults_injected += 1;
        
        let fault_type_str = format!("{:?}", fault_type);
        *stats.faults_by_type.entry(fault_type_str).or_insert(0) += 1;
        *stats.faults_by_method.entry(method.to_string()).or_insert(0) += 1;
    }

    pub fn get_stats(&self) -> FaultStats {
        let stats = self.stats.lock().unwrap();
        stats.clone()
    }

    pub fn update_config(&self, _new_config: FaultInjectionConfig) -> Result<(), eyre::Report> {
        // TODO: Implement config update logic
        // For now, just return Ok
        warn!("Config update not implemented in this simplified version");
        Ok(())
    }
}