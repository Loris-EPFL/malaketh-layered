use clap::Parser;
use malachitebft_eth_proxy::{config::FaultInjectionConfig, EngineApiProxy};
use std::net::SocketAddr;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "malachitebft-eth-proxy")]
#[command(about = "A fault-injecting proxy for Ethereum Engine API")]
struct Args {
    /// Address to listen on
    #[arg(long, default_value = "127.0.0.1:8551")]
    listen: SocketAddr,

    /// Target Engine API URL to proxy to
    #[arg(long, default_value = "http://127.0.0.1:8550")]
    target: String,

    /// Enable fault injection
    #[arg(long)]
    enable_faults: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(args.log_level)
        .init();

    info!("Starting MalachiteBFT Ethereum Proxy");
    info!("Listen address: {}", args.listen);
    info!("Target URL: {}", args.target);
    info!("Fault injection: {}", if args.enable_faults { "enabled" } else { "disabled" });

    // Create fault injection config
    let fault_config = if args.enable_faults {
        Some(FaultInjectionConfig {
            enabled: true,
            scenarios: Vec::new(),
            global_probability: 0.1, // 10% fault injection rate
            engine_api_specific: None,
        })
    } else {
        None
    };

    // Create and start the proxy
    let proxy = EngineApiProxy::new(args.target, fault_config);
    
    if let Err(e) = proxy.start(args.listen).await {
        error!("Failed to start proxy: {}", e);
        std::process::exit(1);
    }

    Ok(())
}