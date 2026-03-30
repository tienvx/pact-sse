use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use crate::proto;
use crate::sse_content::SseEvent;
use crate::MockServerMap;

pub async fn run_sse_mock_server(
    listener: TcpListener,
    server_key: String,
    pact: String,
    mock_servers: MockServerMap,
) {
    tracing::info!(
        "SSE mock server started: server_key={}, pact_length={}",
        server_key,
        pact.len()
    );

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let server_key = server_key.clone();
                let pact = pact.clone();
                let mock_servers = mock_servers.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_sse_connection(stream, server_key, &pact, &mock_servers).await {
                        tracing::error!("Error handling SSE connection: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept connection: {}", e);
                break;
            }
        }
    }
}

async fn handle_sse_connection(
    stream: TcpStream,
    server_key: String,
    pact: &str,
    mock_servers: &MockServerMap,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::debug!(
        "HandleSSEConnection: server_key={}, pact_length={}",
        server_key,
        pact.len()
    );

    let (read_half, mut write_half) = stream.into_split();

    let response_headers = "HTTP/1.1 200 OK\r\n\
        Content-Type: text/event-stream\r\n\
        Cache-Control: no-cache\r\n\
        Connection: keep-alive\r\n\
        \r\n";

    write_half.write_all(response_headers.as_bytes()).await?;
    write_half.flush().await?;
    tracing::debug!("HandleSSEConnection: sent response headers");

    let default_event = SseEvent {
        event: Some("message".to_string()),
        id: None,
        data: "Hello from SSE mock server!".to_string(),
        retry: Some(3000),
    };

    let event_str = default_event.format();
    tracing::debug!("HandleSSEConnection: sending SSE event: {}", event_str);
    write_half.write_all(event_str.as_bytes()).await?;
    write_half.flush().await?;

    let _ = read_half;

    tracing::debug!("HandleSSEConnection: connection closed");
    Ok(())
}
