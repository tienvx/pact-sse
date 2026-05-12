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

    let source = PactSource::File("../pacts/sseConsumer-sseProvider.json".to_string());

    let options: VerificationOptions<NullRequestFilterExecutor> = VerificationOptions::default();
    let ps_executor = NoopProviderStateExecutor {};

    // Start the provider server in the background
    let _server_handle = std::thread::spawn(|| {
        actix_web::rt::System::new().block_on(async {
            let _ = simple_log::quick();

            let server = actix_web::HttpServer::new(|| {
                actix_web::App::new().route(
                    "/events",
                    actix_web::web::get().to(|| async {
                        let sse_data = "id:100
retry:3000

data: I am

event:user
data: id: 12, name: John

data: I have many books

event:count
data:12

data: I love this book. I read it many times

event:count
data:34

data: Last time I read it

event:time
data:2015-02-21

".to_string();

                        actix_web::HttpResponse::Ok()
                            .content_type("text/event-stream")
                            .append_header(("Cache-Control", "no-cache"))
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

    // Server thread is detached; it will be cleaned up when the process exits
}
