pub mod config;
pub mod engine_api;
pub mod engine_proxy;
pub mod fault_injector;
pub mod metrics;
pub mod middleware;
pub mod server;

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
    routing::post,
    Router,
};
use config::FaultInjectionConfig;
use fault_injector::FaultInjector;
use reqwest::Client;
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

/// Main fault-injecting proxy for Engine API calls
#[derive(Clone)]
pub struct EngineApiProxy {
    fault_injector: Arc<FaultInjector>,
    client: Client,
    target_url: String,
}

impl EngineApiProxy {
    /// Create a new Engine API proxy
    pub fn new(target_url: String, config: Option<FaultInjectionConfig>) -> Self {
        let fault_injector = config
            .map(|c| Arc::new(FaultInjector::new(c)))
            .unwrap_or_else(|| Arc::new(FaultInjector::disabled()));

        Self {
            fault_injector,
            client: Client::new(),
            target_url,
        }
    }

    /// Start the proxy server
    pub async fn start(&self, listen_addr: SocketAddr) -> Result<(), eyre::Report> {
        info!("Starting Engine API proxy on {}", listen_addr);
        info!("Forwarding requests to {}", self.target_url);

        let app = Router::new()
            .route("/", post(handle_engine_api))
            .with_state(self.clone());

        let listener = TcpListener::bind(listen_addr).await?;
        info!("Proxy server listening on {}", listen_addr);

        axum::serve(listener, app).await?;
        Ok(())
    }

    /// Update the fault injection configuration
    pub fn update_config(&self, config: FaultInjectionConfig) -> Result<(), eyre::Report> {
        self.fault_injector.update_config(config)
    }

    /// Get current fault injection statistics
    pub fn get_stats(&self) -> fault_injector::FaultStats {
        self.fault_injector.get_stats()
    }
}

/// Handle Engine API JSON-RPC requests
async fn handle_engine_api(
    State(proxy): State<EngineApiProxy>,
    request: Request,
) -> Response {
    // Extract body from request
    let body = match axum::body::to_bytes(request.into_body(), usize::MAX).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return create_error_response(-32700, "Failed to read request body");
        }
    };

    debug!("Received request: {}", body);

    // Parse the JSON-RPC request to extract the method
    let method = match extract_method(&body) {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to parse JSON-RPC request: {}", e);
            return create_error_response(-32700, "Parse error");
        }
    };

    info!("Processing Engine API method: {}", method);

    // Check if we should inject a fault
    if let Some(fault_response) = proxy.fault_injector.should_inject_fault(&method).await {
        warn!("Injecting fault for method: {}", method);
        return fault_response;
    }

    // Forward the request to the target
    match forward_request(&proxy.client, &proxy.target_url, &body).await {
        Ok(response) => {
            debug!("Successfully forwarded request for method: {}", method);
            response
        }
        Err(e) => {
            error!("Failed to forward request: {}", e);
            create_error_response(-32000, "Internal error")
        }
    }
}

/// Extract the method name from a JSON-RPC request
fn extract_method(body: &str) -> Result<String, serde_json::Error> {
    let parsed: Value = serde_json::from_str(body)?;
    
    if let Some(method) = parsed.get("method").and_then(|m| m.as_str()) {
        Ok(method.to_string())
    } else {
        Ok("unknown".to_string())
    }
}

/// Forward the request to the target server
async fn forward_request(
    client: &Client,
    target_url: &str,
    body: &str,
) -> Result<Response, reqwest::Error> {
    let response = client
        .post(target_url)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await?;

    let status_code = response.status().as_u16();
    let response_body = response.text().await?;

    let axum_status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    Ok(Response::builder()
        .status(axum_status)
        .header("content-type", "application/json")
        .body(Body::from(response_body))
        .unwrap())
}

/// Create a JSON-RPC error response
fn create_error_response(code: i32, message: &str) -> Response {
    let error_response = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": code,
            "message": message
        },
        "id": null
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(error_response.to_string()))
        .unwrap()
}