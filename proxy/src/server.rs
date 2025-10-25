use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use hyper::body::Bytes;
use reqwest::Client;
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tower::ServiceBuilder;
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::{info, warn, error, debug};
use uuid::Uuid;

use crate::{
    config::ProxyConfig,
    engine_api::EngineApiHandler,
    fault_injector::FaultInjector,
    middleware::FaultInjectionLayer,
    metrics::ProxyMetrics,
};

#[derive(Clone)]
pub struct ProxyState {
    pub client: Client,
    pub config: ProxyConfig,
    pub fault_injector: Arc<FaultInjector>,
    pub metrics: Arc<ProxyMetrics>,
}

pub struct ProxyServer {
    state: Arc<ProxyState>,
}

impl ProxyServer {
    pub fn new(config: ProxyConfig) -> eyre::Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        let fault_injector = Arc::new(FaultInjector::new(config.fault_injection.clone()));
        let metrics = Arc::new(ProxyMetrics::new());

        let state = Arc::new(ProxyState {
            client,
            config,
            fault_injector,
            metrics,
        });

        Ok(Self { state })
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", post(json_rpc_handler))
            .route("/engine", post(engine_api_handler))
            .route("/health", axum::routing::get(health_handler))
            .route("/metrics", axum::routing::get(metrics_handler))
            .layer(
                ServiceBuilder::new()
                    .layer(
                        TraceLayer::new_for_http()
                            .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                            .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
                    )
                    .layer(CorsLayer::permissive())
                    .layer(FaultInjectionLayer::new(self.state.fault_injector.clone())),
            )
            .with_state(self.state.clone())
    }

    pub async fn serve(self, addr: SocketAddr) -> eyre::Result<()> {
        let app = self.router();
        
        info!("Starting proxy server on {}", addr);
        info!("Upstream URL: {}", self.state.config.upstream_url);
        
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        
        Ok(())
    }
}

async fn json_rpc_handler(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    request: Request<Body>,
) -> impl IntoResponse {
    let request_id = Uuid::new_v4().to_string();
    debug!("Processing request {}", request_id);

    // Extract body
    let body_bytes = match extract_body(request).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };

    // Parse JSON-RPC request
    let json_request: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(e) => {
            warn!("Invalid JSON in request {}: {}", request_id, e);
            state.metrics.increment_error("invalid_json");
            return create_error_response(
                StatusCode::BAD_REQUEST,
                "Invalid JSON",
                None,
            );
        }
    };

    // Extract method for fault injection decisions
    let method = json_request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown");

    debug!("Request {} method: {}", request_id, method);

    // Check if we should inject a fault
    if let Some(fault_response) = state.fault_injector.should_inject_fault(method).await {
        info!("Injecting fault for request {} method {}", request_id, method);
        state.metrics.increment_fault_injected(method);
        return fault_response;
    }

    // Forward to upstream
    match forward_request(&state, &body_bytes, &headers, &request_id).await {
        Ok(response) => {
            state.metrics.increment_success(method);
            response
        }
        Err(response) => {
            state.metrics.increment_error("upstream_error");
            response
        }
    }
}

async fn extract_body(request: Request<Body>) -> Result<Bytes, Response> {
    match to_bytes(request.into_body(), usize::MAX).await {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            warn!("Failed to read request body: {}", e);
            Err(create_error_response(
                StatusCode::BAD_REQUEST,
                "Failed to read request body",
                None,
            ))
        }
    }
}

async fn engine_api_handler(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    request: Request<Body>,
) -> impl IntoResponse {
    let request_id = Uuid::new_v4().to_string();
    debug!("Received Engine API request {}", request_id);

    // Extract request body
    let body_bytes = match extract_body(request).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };

    // Parse JSON-RPC request to identify Engine API method
    let json_request: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse JSON for {}: {}", request_id, e);
            return create_error_response(
                StatusCode::BAD_REQUEST,
                "Invalid JSON",
                None,
            );
        }
    };

    // Extract method name for Engine API specific handling
    let method = json_request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown");

    info!("Processing Engine API method: {} ({})", method, request_id);

    // Record metrics for Engine API requests
    state.metrics.increment_success(method);

    // Forward the request to upstream
    match forward_request(&state, &body_bytes, &headers, &request_id).await {
        Ok(response) => response,
        Err(response) => {
            state.metrics.increment_error("upstream_error");
            response
        }
    }
}

async fn forward_request(
    state: &ProxyState,
    body_bytes: &Bytes,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<Response, Response> {
    debug!("Forwarding request {} to upstream", request_id);

    let mut req_builder = state
        .client
        .post(&state.config.upstream_url)
        .body(body_bytes.clone())
        .header("content-type", "application/json");

    // Forward relevant headers
    for (name, value) in headers {
        if should_forward_header(name.as_str()) {
            if let Ok(header_value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                req_builder = req_builder.header(name.as_str(), header_value);
            }
        }
    }

    let response = match req_builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Upstream request failed for {}: {}", request_id, e);
            return Err(create_error_response(
                StatusCode::BAD_GATEWAY,
                "Upstream request failed",
                None,
            ));
        }
    };

    let status = response.status();
    let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let response_bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read upstream response for {}: {}", request_id, e);
            return Err(create_error_response(
                StatusCode::BAD_GATEWAY,
                "Failed to read upstream response",
                None,
            ));
        }
    };

    debug!("Upstream response for {} status: {}", request_id, status);

    Ok(Response::builder()
        .status(status_code)
        .header("content-type", "application/json")
        .body(Body::from(response_bytes))
        .unwrap())
}

fn should_forward_header(name: &str) -> bool {
    match name.to_lowercase().as_str() {
        "host" | "content-length" | "transfer-encoding" => false,
        _ => true,
    }
}

fn create_error_response(status: StatusCode, message: &str, id: Option<Value>) -> Response {
    let error_response = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": -32000,
            "message": message
        },
        "id": id.unwrap_or(Value::Null)
    });

    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(error_response.to_string()))
        .unwrap()
}

async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn metrics_handler(State(state): State<Arc<ProxyState>>) -> impl IntoResponse {
    match state.metrics.render_prometheus() {
        Ok(metrics) => Response::builder()
            .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
            .body(Body::from(metrics))
            .unwrap(),
        Err(e) => {
            error!("Failed to render metrics: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Failed to render metrics"))
                .unwrap()
        }
    }
}