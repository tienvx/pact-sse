use std::sync::Arc;

use async_trait::async_trait;
use expectest::{expect, prelude::*};
use maplit::hashmap;
use pact_models::prelude::ProviderState;
use pact_verifier::{
    FilterInfo,
    NullRequestFilterExecutor,
    PactSource,
    ProviderInfo,
    VerificationOptions,
    verify_provider_async,
};
use pact_verifier::callback_executors::ProviderStateExecutor;
use serde_json::Value;
use std::collections::HashMap;
use test_log::test;

#[derive(Debug)]
struct NoopProviderStateExecutor {}

#[async_trait]
impl ProviderStateExecutor for NoopProviderStateExecutor {
    async fn call(
        self: Arc<Self>,
        _interaction_id: Option<String>,
        _provider_state: &ProviderState,
        _setup: bool,
        _client: Option<&reqwest::Client>,
    ) -> anyhow::Result<HashMap<String, Value>> {
        Ok(hashmap! {})
    }

    fn teardown(self: &Self) -> bool {
        false
    }
}

#[test(tokio::test(flavor = "multi_thread", worker_threads = 1))]
async fn verify_sse_provider() {
    #[allow(deprecated)]
    let provider_info = ProviderInfo {
        name: "sseProvider".to_string(),
        host: "127.0.0.1".to_string(),
        port: Some(8081),
        ..ProviderInfo::default()
    };

    let source = PactSource::File("pacts/sseConsumer-sseProvider.json".to_string());

    let options: VerificationOptions<NullRequestFilterExecutor> = VerificationOptions::default();
    let ps_executor = NoopProviderStateExecutor {};

    // Start the provider server in the background
    let server_handle = std::thread::spawn(|| {
        actix_web::rt::System::new().block_on(async {
            let _ = simple_log::quick();

            let server = actix_web::HttpServer::new(|| {
                actix_web::App::new().route(
                    "/events",
                    actix_web::web::get().to(|| async {
                        let count_value = 100i32;
                        let date = "2000-01-01";
                        let user_id = "5d03dc45-96f6-4c0c-b1ad-aa67242058cc";

                        let sse_data = format!(
                            "data:simple text\n\nevent:count\ndata:{}\n\nevent:time\ndata:{}\n\nevent:user\ndata:user data payload\n\ndata:100\n\nevent:user\ndata:{}\n\n",
                            count_value, date, user_id
                        );

                        actix_web::HttpResponse::Ok()
                            .content_type("text/event-stream")
                            .append_header(("Cache-Control", "no-cache"))
                            .append_header(("Connection", "keep-alive"))
                            .append_header(("X-Accel-Buffering", "no"))
                            .body(sse_data)
                    }),
                )
            })
            .bind("127.0.0.1:8081")
            .expect("Could not bind to 127.0.0.1:8081")
            .run();

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            server.await.ok();
        })
    });

    // Give the server time to start
    std::thread::sleep(std::time::Duration::from_secs(2));

    let result = verify_provider_async(
        provider_info,
        vec![source],
        FilterInfo::None,
        vec![],
        &options,
        None,
        &Arc::new(ps_executor),
        None,
    )
    .await;

    let verification_result = result.expect("Verification failed");
    expect!(verification_result.result).to(be_true());

    let _ = server_handle.join();
}
