//! Engine API proxy with fault injection capabilities

use crate::fault_injector::FaultInjector;
use std::sync::Arc;
use url::Url;
use eyre::Result;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

/// Engine API proxy that can inject faults
pub struct EngineProxy {
    target_url: Url,
    jwt_secret: String,
    fault_injector: Arc<FaultInjector>,
    listen_url: Url,
}

impl EngineProxy {
    /// Create a new engine proxy
    pub fn new(
        target_url: Url,
        jwt_secret_path: &std::path::Path,
        fault_injector: Arc<FaultInjector>,
    ) -> Result<Self> {
        // Read JWT secret from file
        let jwt_secret = std::fs::read_to_string(jwt_secret_path)
            .map_err(|e| eyre::eyre!("Failed to read JWT secret: {}", e))?
            .trim()
            .to_string();

        // For now, use a placeholder listen URL - this will be set when starting
        let listen_url = Url::parse("http://127.0.0.1:0")?;

        Ok(Self {
            target_url,
            jwt_secret,
            fault_injector,
            listen_url,
        })
    }

    /// Get the proxy's listening URL
    pub fn listen_url(&self) -> &Url {
        &self.listen_url
    }

    /// Start the proxy server
    pub async fn start(&self, _listen_addr: std::net::SocketAddr) -> Result<()> {
        // This is a simplified implementation
        // In a real implementation, this would start an HTTP server
        // that proxies Engine API calls with fault injection
        
        tracing::info!("Engine proxy would start on {:?}", _listen_addr);
        tracing::info!("Proxying to: {}", self.target_url);
        
        // For now, just return Ok to indicate the proxy is "started"
        Ok(())
    }

    /// Process an Engine API request with potential fault injection
    pub async fn process_request(&self, method: &str, params: Value) -> Result<Value> {
        // Check if we should inject a fault
        if let Some(_fault_response) = self.fault_injector.should_inject_fault(method).await {
            // Return a simple error response for now
            return Ok(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32603,
                    "message": "Fault injected"
                },
                "id": null
            }));
        }

        // Normal processing - forward the request
        self.forward_request(method, params).await
    }

    /// Forward a request to the target engine
    async fn forward_request(&self, method: &str, _params: Value) -> Result<Value> {
        // This is a simplified implementation
        // In a real implementation, this would make an HTTP request to the target engine
        
        tracing::debug!("Forwarding {} request to {}", method, self.target_url);
        
        // For now, return a mock successful response
        Ok(json!({
            "jsonrpc": "2.0",
            "result": {
                "status": "VALID",
                "latestValidHash": "0x0000000000000000000000000000000000000000000000000000000000000000"
            },
            "id": 1
        }))
    }

    /// Update fault injection configuration
    pub fn update_fault_config(&self, config: crate::config::FaultInjectionConfig) -> Result<()> {
        self.fault_injector.update_config(config)
    }

    /// Get fault injection statistics
    pub fn get_stats(&self) -> serde_json::Value {
        let stats = self.fault_injector.get_stats();
        json!({
            "total_requests": stats.total_requests,
            "faults_injected": stats.faults_injected,
            "faults_by_type": stats.faults_by_type
        })
    }
}