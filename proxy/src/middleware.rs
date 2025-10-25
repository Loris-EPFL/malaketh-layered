use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use rand::Rng;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::time::sleep;
use tower::{Layer, Service};
use tracing::{debug, info, warn};

use crate::{
    config::CorruptionType,
    engine_api::{EngineApiHandler, EngineApiMethod},
    fault_injector::FaultInjector,
};

/// Main fault injection middleware layer with Engine API awareness
#[derive(Clone)]
pub struct FaultInjectionLayer {
    fault_injector: Arc<FaultInjector>,
    engine_api_handler: Arc<EngineApiHandler>,
}

impl FaultInjectionLayer {
    pub fn new(fault_injector: Arc<FaultInjector>) -> Self {
        Self {
            fault_injector,
            engine_api_handler: Arc::new(EngineApiHandler::new()),
        }
    }

    pub fn with_engine_api_handler(fault_injector: Arc<FaultInjector>, engine_api_handler: Arc<EngineApiHandler>) -> Self {
        Self {
            fault_injector,
            engine_api_handler,
        }
    }
}

impl<S> Layer<S> for FaultInjectionLayer {
    type Service = FaultInjectionService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        FaultInjectionService {
            inner,
            fault_injector: self.fault_injector.clone(),
            engine_api_handler: self.engine_api_handler.clone(),
        }
    }
}

#[derive(Clone)]
pub struct FaultInjectionService<S> {
    inner: S,
    fault_injector: Arc<FaultInjector>,
    engine_api_handler: Arc<EngineApiHandler>,
}

impl<S> Service<Request> for FaultInjectionService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let fault_injector = self.fault_injector.clone();
        let engine_api_handler = self.engine_api_handler.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Extract the request body to parse the JSON-RPC method
            let (parts, body) = req.into_parts();
            let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    // If we can't read the body, pass through without fault injection
                    let req = Request::from_parts(parts, Body::empty());
                    return inner.call(req).await;
                }
            };

            // Try to parse as Engine API request
            let method = if let Ok(engine_request) = engine_api_handler.parse_request(&body_bytes) {
                if engine_api_handler.is_engine_api_method(&engine_request.method) {
                    info!("Detected Engine API method: {}", engine_request.method);
                    Some(engine_request.method)
                } else {
                    None
                }
            } else {
                None
            };

            // Apply method-specific fault injection if we identified an Engine API method
            let modified_body = if let Some(method_name) = method {
                // Apply fault injection based on the specific Engine API method
                if let Some(fault_response) = fault_injector.should_inject_fault(&method_name).await {
                    debug!("Applying fault injection for method: {}", method_name);
                    return Ok(fault_response);
                }
                body_bytes
            } else {
                body_bytes
            };

            // Reconstruct the request with the (potentially modified) body
            let req = Request::from_parts(parts, Body::from(modified_body));
            inner.call(req).await
        })
    }
}

/// Extract JSON-RPC method from request body
async fn extract_method_from_request(_req: &Request) -> Option<String> {
    // This is a simplified extraction - in a real implementation,
    // you might want to clone the body or use a more sophisticated approach
    // For now, we'll return a default method
    Some("unknown".to_string())
}

/// Delay middleware layer
#[derive(Clone)]
pub struct DelayLayer {
    delay_ms: u64,
    variance_ms: u64,
}

impl DelayLayer {
    pub fn new(delay_ms: u64, variance_ms: u64) -> Self {
        Self {
            delay_ms,
            variance_ms,
        }
    }
}

impl<S> Layer<S> for DelayLayer {
    type Service = DelayService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DelayService {
            inner,
            delay_ms: self.delay_ms,
            variance_ms: self.variance_ms,
        }
    }
}

#[derive(Clone)]
pub struct DelayService<S> {
    inner: S,
    delay_ms: u64,
    variance_ms: u64,
}

impl<S> Service<Request> for DelayService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let delay_ms = self.delay_ms;
        let variance_ms = self.variance_ms;
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Calculate actual delay with variance
            let actual_delay = if variance_ms > 0 {
                let mut rng = rand::thread_rng();
                let variance_range = variance_ms as i64;
                let offset = rng.gen_range(-variance_range..=variance_range);
                (delay_ms as i64 + offset).max(0) as u64
            } else {
                delay_ms
            };

            if actual_delay > 0 {
                debug!("Applying delay of {}ms", actual_delay);
                sleep(Duration::from_millis(actual_delay)).await;
            }

            inner.call(req).await
        })
    }
}

/// Drop middleware layer
#[derive(Clone)]
pub struct DropLayer {
    drop_probability: f64,
}

impl DropLayer {
    pub fn new(drop_probability: f64) -> Self {
        Self { drop_probability }
    }
}

impl<S> Layer<S> for DropLayer {
    type Service = DropService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DropService {
            inner,
            drop_probability: self.drop_probability,
        }
    }
}

#[derive(Clone)]
pub struct DropService<S> {
    inner: S,
    drop_probability: f64,
}

impl<S> Service<Request> for DropService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let drop_probability = self.drop_probability;
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if rand::random::<f64>() < drop_probability {
                warn!("Dropping request due to drop fault injection");
                let drop_response = Response::builder()
                    .status(StatusCode::GATEWAY_TIMEOUT)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Request dropped"},"id":null}"#,
                    ))
                    .unwrap();
                return Ok(drop_response);
            }

            inner.call(req).await
        })
    }
}

/// Corruption middleware layer
#[derive(Clone)]
pub struct CorruptionLayer {
    corruption_probability: f64,
    corruption_type: CorruptionType,
}

impl CorruptionLayer {
    pub fn new(corruption_probability: f64, corruption_type: CorruptionType) -> Self {
        Self {
            corruption_probability,
            corruption_type,
        }
    }
}

impl<S> Layer<S> for CorruptionLayer {
    type Service = CorruptionService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CorruptionService {
            inner,
            corruption_probability: self.corruption_probability,
            corruption_type: self.corruption_type.clone(),
        }
    }
}

#[derive(Clone)]
pub struct CorruptionService<S> {
    inner: S,
    corruption_probability: f64,
    corruption_type: CorruptionType,
}

impl<S> Service<Request> for CorruptionService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let corruption_probability = self.corruption_probability;
        let corruption_type = self.corruption_type.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let response = inner.call(req).await?;

            if rand::random::<f64>() < corruption_probability {
                info!("Applying corruption to response");
                return Ok(corrupt_response(response, &corruption_type).await);
            }

            Ok(response)
        })
    }
}

/// Corrupt a response based on the corruption type
async fn corrupt_response(response: Response, corruption_type: &CorruptionType) -> Response {
    match corruption_type {
        CorruptionType::RandomByte => {
            // Corrupt random bytes in the response
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
            let corrupted_bytes = corrupt_bytes(&bytes);
            
            Response::from_parts(parts, Body::from(corrupted_bytes))
        }
        CorruptionType::InvalidJson => {
            // Return invalid JSON
            let (parts, _body) = response.into_parts();
            Response::from_parts(parts, Body::from("{invalid json"))
        }
        CorruptionType::TruncateResponse => {
            // Truncate the response
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
            let truncated = if bytes.len() > 10 {
                bytes.slice(0..bytes.len() / 2)
            } else {
                bytes
            };
            Response::from_parts(parts, Body::from(truncated))
        }
        CorruptionType::FlipBit => {
            // Flip random bits
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
            let corrupted_bytes = flip_random_bits(&bytes);
            Response::from_parts(parts, Body::from(corrupted_bytes))
        }
        CorruptionType::WrongId => {
            // Corrupt JSON-RPC ID
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
            
            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                if let Ok(mut json) = serde_json::from_str::<Value>(&text) {
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("id".to_string(), Value::String("wrong_id".to_string()));
                    }
                    let corrupted_text = serde_json::to_string(&json).unwrap_or(text);
                    return Response::from_parts(parts, Body::from(corrupted_text));
                }
            }
            
            Response::from_parts(parts, Body::from(bytes))
        }
        CorruptionType::WrongMethod => {
            // Corrupt JSON-RPC method in response
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
            
            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                if let Ok(mut json) = serde_json::from_str::<Value>(&text) {
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("method".to_string(), Value::String("wrong_method".to_string()));
                    }
                    let corrupted_text = serde_json::to_string(&json).unwrap_or(text);
                    return Response::from_parts(parts, Body::from(corrupted_text));
                }
            }
            
            Response::from_parts(parts, Body::from(bytes))
        }
    }
}

/// Corrupt random bytes in the response
fn corrupt_bytes(bytes: &Bytes) -> Bytes {
    let mut corrupted = bytes.to_vec();
    
    // Corrupt 1-5% of bytes
    let corruption_count = (corrupted.len() / 20).max(1);
    
    for _ in 0..corruption_count {
        if !corrupted.is_empty() {
            let index = rand::random::<usize>() % corrupted.len();
            corrupted[index] = rand::random::<u8>();
        }
    }
    
    Bytes::from(corrupted)
}

/// Flip random bits in the response
fn flip_random_bits(bytes: &Bytes) -> Bytes {
    let mut corrupted = bytes.to_vec();
    
    // Flip 1-3 random bits
    let bit_flips = (rand::random::<u8>() % 3) + 1;
    
    for _ in 0..bit_flips {
        if !corrupted.is_empty() {
            let byte_index = rand::random::<usize>() % corrupted.len();
            let bit_index = rand::random::<u8>() % 8;
            corrupted[byte_index] ^= 1 << bit_index;
        }
    }
    
    Bytes::from(corrupted)
}