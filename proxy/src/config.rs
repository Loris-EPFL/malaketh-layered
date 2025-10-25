use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub server: ServerConfig,
    pub upstream_url: String,
    pub fault_injection: FaultInjectionConfig,
    pub metrics: MetricsConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub listen_addr: SocketAddr,
    pub timeout_seconds: u64,
    pub max_connections: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub listen_addr: SocketAddr,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjectionConfig {
    pub enabled: bool,
    pub scenarios: Vec<FaultScenario>,
    pub global_probability: f64,
    pub engine_api_specific: Option<EngineApiConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineApiConfig {
    pub method_specific_faults: std::collections::HashMap<String, FaultConfig>,
    pub critical_method_protection: bool,
    pub track_method_stats: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultScenario {
    pub name: String,
    pub target: FaultTarget,
    pub fault: FaultConfig,
    pub trigger: TriggerConfig,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FaultTarget {
    Method(String),
    MethodPattern(String), // Regex pattern
    All,
    EngineApi,
    EthApi,
    // Engine API specific targets
    EngineNewPayload,
    EngineForkchoiceUpdated,
    EngineGetPayload,
    EngineCriticalMethods,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultConfig {
    pub fault_type: FaultType,
    pub probability: f64,
    pub parameters: FaultParameters,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FaultType {
    Delay,
    Drop,
    Error,
    Corrupt,
    Duplicate,
    Byzantine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultParameters {
    // Delay parameters
    pub delay_ms: Option<u64>,
    pub delay_variance_ms: Option<u64>,
    
    // Error parameters
    pub error_code: Option<i32>,
    pub error_message: Option<String>,
    
    // Corruption parameters
    pub corruption_type: Option<CorruptionType>,
    
    // Byzantine parameters
    pub byzantine_behavior: Option<ByzantineBehavior>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CorruptionType {
    RandomByte,
    FlipBit,
    TruncateResponse,
    InvalidJson,
    WrongId,
    WrongMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ByzantineBehavior {
    WrongBlockNumber,
    InvalidHash,
    FutureTimestamp,
    WrongGasLimit,
    InvalidSignature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    pub probability: f64,
    pub after_requests: Option<u64>,
    pub time_window_seconds: Option<u64>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                listen_addr: "127.0.0.1:8080".parse().unwrap(),
                timeout_seconds: 30,
                max_connections: Some(1000),
            },
            upstream_url: "http://127.0.0.1:8545".to_string(),
            fault_injection: FaultInjectionConfig {
                enabled: false,
                scenarios: vec![],
                global_probability: 0.0,
                engine_api_specific: None,
            },
            metrics: MetricsConfig {
                enabled: true,
                listen_addr: "127.0.0.1:9090".parse().unwrap(),
                path: "/metrics".to_string(),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: LogFormat::Pretty,
                file: None,
            },
        }
    }
}

impl Default for FaultInjectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            scenarios: vec![],
            global_probability: 0.0,
            engine_api_specific: None,
        }
    }
}

impl Default for FaultParameters {
    fn default() -> Self {
        Self {
            delay_ms: None,
            delay_variance_ms: None,
            error_code: None,
            error_message: None,
            corruption_type: None,
            byzantine_behavior: None,
        }
    }
}