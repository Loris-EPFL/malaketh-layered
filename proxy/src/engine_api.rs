//! Engine API handler for intercepting and manipulating JSON-RPC requests
//! 
//! This module provides functionality to parse, intercept, and modify Engine API
//! requests and responses. It supports all standard Engine API methods as defined
//! in the Ethereum Engine API specification.

use eyre::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, info, warn};

// Re-export Engine API constants and types from the engine crate
pub use malachitebft_eth_engine::engine_rpc::{
    ENGINE_NEW_PAYLOAD_V1, ENGINE_NEW_PAYLOAD_V2, ENGINE_NEW_PAYLOAD_V3, ENGINE_NEW_PAYLOAD_V4,
    ENGINE_GET_PAYLOAD_V1, ENGINE_GET_PAYLOAD_V2, ENGINE_GET_PAYLOAD_V3, ENGINE_GET_PAYLOAD_V4,
    ENGINE_FORKCHOICE_UPDATED_V1, ENGINE_FORKCHOICE_UPDATED_V2, ENGINE_FORKCHOICE_UPDATED_V3,
    ENGINE_GET_PAYLOAD_BODIES_BY_HASH_V1, ENGINE_GET_PAYLOAD_BODIES_BY_RANGE_V1,
    ENGINE_EXCHANGE_CAPABILITIES, ENGINE_GET_CLIENT_VERSION_V1, ENGINE_GET_BLOBS_V1,
};

pub use malachitebft_eth_engine::json_structures::{
    JsonRequestBody, JsonResponseBody, JsonExecutionPayloadV3, JsonPayloadAttributes,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

/// Represents a JSON-RPC request that can be intercepted and modified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineApiRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
    pub id: Value,
}

/// Represents a JSON-RPC response that can be intercepted and modified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineApiResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonError>,
    pub id: Value,
}

/// Engine API method categories for targeted fault injection
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EngineApiMethod {
    NewPayload(String),      // V1, V2, V3, V4
    GetPayload(String),      // V1, V2, V3, V4
    ForkchoiceUpdated(String), // V1, V2, V3
    GetPayloadBodies(String), // ByHash, ByRange
    ExchangeCapabilities,
    GetClientVersion,
    GetBlobs,
    Unknown(String),
}

impl EngineApiMethod {
    /// Parse a method string into an EngineApiMethod enum
    pub fn from_str(method: &str) -> Self {
        match method {
            ENGINE_NEW_PAYLOAD_V1 => Self::NewPayload("V1".to_string()),
            ENGINE_NEW_PAYLOAD_V2 => Self::NewPayload("V2".to_string()),
            ENGINE_NEW_PAYLOAD_V3 => Self::NewPayload("V3".to_string()),
            ENGINE_NEW_PAYLOAD_V4 => Self::NewPayload("V4".to_string()),
            ENGINE_GET_PAYLOAD_V1 => Self::GetPayload("V1".to_string()),
            ENGINE_GET_PAYLOAD_V2 => Self::GetPayload("V2".to_string()),
            ENGINE_GET_PAYLOAD_V3 => Self::GetPayload("V3".to_string()),
            ENGINE_GET_PAYLOAD_V4 => Self::GetPayload("V4".to_string()),
            ENGINE_FORKCHOICE_UPDATED_V1 => Self::ForkchoiceUpdated("V1".to_string()),
            ENGINE_FORKCHOICE_UPDATED_V2 => Self::ForkchoiceUpdated("V2".to_string()),
            ENGINE_FORKCHOICE_UPDATED_V3 => Self::ForkchoiceUpdated("V3".to_string()),
            ENGINE_GET_PAYLOAD_BODIES_BY_HASH_V1 => Self::GetPayloadBodies("ByHashV1".to_string()),
            ENGINE_GET_PAYLOAD_BODIES_BY_RANGE_V1 => Self::GetPayloadBodies("ByRangeV1".to_string()),
            ENGINE_EXCHANGE_CAPABILITIES => Self::ExchangeCapabilities,
            ENGINE_GET_CLIENT_VERSION_V1 => Self::GetClientVersion,
            ENGINE_GET_BLOBS_V1 => Self::GetBlobs,
            _ => Self::Unknown(method.to_string()),
        }
    }

    /// Check if this is a critical Engine API method that affects consensus
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            Self::NewPayload(_) | Self::ForkchoiceUpdated(_) | Self::GetPayload(_)
        )
    }

    /// Get the base method name without version
    pub fn base_name(&self) -> &str {
        match self {
            Self::NewPayload(_) => "engine_newPayload",
            Self::GetPayload(_) => "engine_getPayload",
            Self::ForkchoiceUpdated(_) => "engine_forkchoiceUpdated",
            Self::GetPayloadBodies(_) => "engine_getPayloadBodies",
            Self::ExchangeCapabilities => "engine_exchangeCapabilities",
            Self::GetClientVersion => "engine_getClientVersionV1",
            Self::GetBlobs => "engine_getBlobsV1",
            Self::Unknown(method) => method,
        }
    }
}

/// Engine API handler for processing requests and responses
pub struct EngineApiHandler {
    /// Statistics for tracking method calls
    method_stats: HashMap<String, u64>,
}

impl EngineApiHandler {
    pub fn new() -> Self {
        Self {
            method_stats: HashMap::new(),
        }
    }

    /// Parse a raw JSON-RPC request into an EngineApiRequest
    pub fn parse_request(&self, body: &[u8]) -> Result<EngineApiRequest> {
        let request: EngineApiRequest = serde_json::from_slice(body)?;
        debug!("Parsed Engine API request: method={}", request.method);
        Ok(request)
    }

    /// Parse a raw JSON-RPC response into an EngineApiResponse
    pub fn parse_response(&self, body: &[u8]) -> Result<EngineApiResponse> {
        let response: EngineApiResponse = serde_json::from_slice(body)?;
        debug!("Parsed Engine API response for request ID: {:?}", response.id);
        Ok(response)
    }

    /// Check if a request is an Engine API method
    pub fn is_engine_api_method(&self, method: &str) -> bool {
        !matches!(EngineApiMethod::from_str(method), EngineApiMethod::Unknown(_))
    }

    /// Get method category for targeted fault injection
    pub fn get_method_category(&self, method: &str) -> EngineApiMethod {
        EngineApiMethod::from_str(method)
    }

    /// Modify request parameters for fault injection
    pub fn modify_request(&mut self, mut request: EngineApiRequest, modifications: &HashMap<String, Value>) -> Result<EngineApiRequest> {
        let method_category = self.get_method_category(&request.method);
        
        // Track method usage
        *self.method_stats.entry(request.method.clone()).or_insert(0) += 1;
        
        info!("Processing Engine API request: {} (category: {:?})", request.method, method_category);

        // Apply modifications based on method type
        if let Some(new_params) = modifications.get("params") {
            request.params = new_params.clone();
            warn!("Modified request parameters for method: {}", request.method);
        }

        // Method-specific modifications
        match method_category {
            EngineApiMethod::NewPayload(_) => {
                if let Some(corruption) = modifications.get("corrupt_payload") {
                    if corruption.as_bool().unwrap_or(false) {
                        self.corrupt_new_payload_request(&mut request)?;
                    }
                }
            }
            EngineApiMethod::ForkchoiceUpdated(_) => {
                if let Some(corruption) = modifications.get("corrupt_forkchoice") {
                    if corruption.as_bool().unwrap_or(false) {
                        self.corrupt_forkchoice_request(&mut request)?;
                    }
                }
            }
            _ => {}
        }

        Ok(request)
    }

    /// Modify response for fault injection
    pub fn modify_response(&self, mut response: EngineApiResponse, modifications: &HashMap<String, Value>) -> Result<EngineApiResponse> {
        // Apply general modifications
        if let Some(new_result) = modifications.get("result") {
            response.result = Some(new_result.clone());
            warn!("Modified response result for request ID: {:?}", response.id);
        }

        if let Some(error_injection) = modifications.get("inject_error") {
            if error_injection.as_bool().unwrap_or(false) {
                response.error = Some(JsonError {
                    code: -32603,
                    message: "Injected fault: Internal error".to_string(),
                    data: None,
                });
                response.result = None;
                warn!("Injected error into response for request ID: {:?}", response.id);
            }
        }

        Ok(response)
    }

    /// Corrupt a newPayload request for testing
    fn corrupt_new_payload_request(&self, request: &mut EngineApiRequest) -> Result<()> {
        if let Some(params) = request.params.as_array_mut() {
            if let Some(payload) = params.get_mut(0) {
                if let Some(payload_obj) = payload.as_object_mut() {
                    // Corrupt the block hash
                    payload_obj.insert(
                        "blockHash".to_string(),
                        json!("0x0000000000000000000000000000000000000000000000000000000000000000")
                    );
                    warn!("Corrupted newPayload request: modified blockHash");
                }
            }
        }
        Ok(())
    }

    /// Corrupt a forkchoiceUpdated request for testing
    fn corrupt_forkchoice_request(&self, request: &mut EngineApiRequest) -> Result<()> {
        if let Some(params) = request.params.as_array_mut() {
            if let Some(forkchoice) = params.get_mut(0) {
                if let Some(forkchoice_obj) = forkchoice.as_object_mut() {
                    // Corrupt the head block hash
                    forkchoice_obj.insert(
                        "headBlockHash".to_string(),
                        json!("0x0000000000000000000000000000000000000000000000000000000000000000")
                    );
                    warn!("Corrupted forkchoiceUpdated request: modified headBlockHash");
                }
            }
        }
        Ok(())
    }

    /// Get statistics about method usage
    pub fn get_method_stats(&self) -> &HashMap<String, u64> {
        &self.method_stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.method_stats.clear();
    }
}

impl Default for EngineApiHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_parsing() {
        assert_eq!(
            EngineApiMethod::from_str("engine_newPayloadV3"),
            EngineApiMethod::NewPayload("V3".to_string())
        );
        
        assert_eq!(
            EngineApiMethod::from_str("engine_forkchoiceUpdatedV2"),
            EngineApiMethod::ForkchoiceUpdated("V2".to_string())
        );
        
        assert_eq!(
            EngineApiMethod::from_str("unknown_method"),
            EngineApiMethod::Unknown("unknown_method".to_string())
        );
    }

    #[test]
    fn test_critical_methods() {
        assert!(EngineApiMethod::NewPayload("V3".to_string()).is_critical());
        assert!(EngineApiMethod::ForkchoiceUpdated("V2".to_string()).is_critical());
        assert!(!EngineApiMethod::ExchangeCapabilities.is_critical());
    }

    #[test]
    fn test_request_parsing() {
        let handler = EngineApiHandler::new();
        let json_request = r#"{"jsonrpc":"2.0","method":"engine_newPayloadV3","params":[],"id":1}"#;
        
        let request = handler.parse_request(json_request.as_bytes()).unwrap();
        assert_eq!(request.method, "engine_newPayloadV3");
        assert_eq!(request.jsonrpc, "2.0");
    }
}