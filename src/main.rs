use std::env;
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tonic::transport::Server;
use uuid::Uuid;

use pact_sse_plugin::{SsePactPlugin, TcpIncoming};
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "INFO".to_string());
    let _ = fs::create_dir_all("./log");
    let file_appender = tracing_appender::rolling::daily("./log", "plugin.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::filter::EnvFilter::new(&log_level);
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(non_blocking.and(std::io::stdout))
        .init();

    tracing::info!("SSE Plugin starting: LOG_LEVEL={}, PID={}",
        log_level, std::process::id());

    let addr: SocketAddr = "0.0.0.0:0".parse()?;
    let listener = TcpListener::bind(addr).await?;
    let address = listener.local_addr()?;
    tracing::info!("SSE Plugin bound to port {}", address.port());

    let server_key = Uuid::new_v4().to_string();
    tracing::info!("SSE Plugin generated server_key: {}", server_key);
    println!(
        r#"{{"port":{}, "serverKey":"{}"}}"#,
        address.port(),
        server_key
    );
    let _ = std::io::stdout().flush();

    let plugin = SsePactPlugin {
        mock_servers: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    };
    tracing::info!("SSE Plugin initialized, starting gRPC server");

    Server::builder()
        .add_service(pact_sse_plugin::proto::pact_plugin_server::PactPluginServer::new(plugin))
        .serve_with_incoming(TcpIncoming { inner: listener })
        .await?;

    tracing::info!("SSE Plugin gRPC server stopped");
    Ok(())
}
