pub use pact_plugin_driver::proto;
mod mock_server;
pub mod server;
pub mod sse_content;
pub mod tcp;
pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

use prost_types::value::Kind;
use serde_json::{json, Value};

fn to_object(s: &prost_types::Struct) -> Value {
    Value::Object(
        s.fields
            .iter()
            .map(|(k, v)| (k.clone(), to_value(v)))
            .collect(),
    )
}

fn to_value(v: &prost_types::Value) -> Value {
    match &v.kind {
        Some(kind) => match kind {
            Kind::NullValue(_) => Value::Null,
            Kind::NumberValue(n) => json!(n),
            Kind::StringValue(s) => Value::String(s.clone()),
            Kind::BoolValue(b) => Value::Bool(*b),
            Kind::StructValue(s) => to_object(s),
            Kind::ListValue(l) => Value::Array(l.values.iter().map(to_value).collect()),
        },
        None => Value::Null,
    }
}

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use maplit::hashmap;
use pact_models::matchingrules::{RuleList, RuleLogic};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tonic::{Response, Status};
use uuid::Uuid;

use crate::proto::body::ContentTypeHint;
use crate::proto::catalogue_entry::EntryType;
use crate::sse_content::{compare_sse_events, format_sse_content, parse_sse_content, SseEvent};

pub type MockServerMap = Arc<Mutex<HashMap<String, MockServer>>>;

#[derive(Debug)]
pub struct MockServer {
    pub port: u32,
    pub address: String,
    pub pact: String,
    pub results: Vec<proto::MockServerResult>,
}

#[tonic::async_trait]
impl proto::pact_plugin_server::PactPlugin for server::SsePactPlugin {
    async fn init_plugin(
        &self,
        request: tonic::Request<proto::InitPluginRequest>,
    ) -> Result<tonic::Response<proto::InitPluginResponse>, Status> {
        let message = request.get_ref();
        tracing::info!(
            "InitPlugin: implementation={}/{}",
            message.implementation,
            message.version
        );

        Ok(Response::new(proto::InitPluginResponse {
            catalogue: vec![
                proto::CatalogueEntry {
                    r#type: EntryType::Transport as i32,
                    key: "sse".to_string(),
                    values: HashMap::new(),
                },
                proto::CatalogueEntry {
                    r#type: EntryType::ContentMatcher as i32,
                    key: "sse".to_string(),
                    values: hashmap! {
                        "content-types".to_string() => "text/event-stream".to_string()
                    },
                },
                proto::CatalogueEntry {
                    r#type: EntryType::ContentGenerator as i32,
                    key: "sse".to_string(),
                    values: hashmap! {
                        "content-types".to_string() => "text/event-stream".to_string()
                    },
                },
            ],
        }))
    }

    async fn update_catalogue(
        &self,
        request: tonic::Request<proto::Catalogue>,
    ) -> Result<tonic::Response<()>, Status> {
        let catalogue = request.get_ref();
        tracing::debug!("UpdateCatalogue: {} entries", catalogue.catalogue.len());
        Ok(Response::new(()))
    }

    async fn compare_contents(
        &self,
        request: tonic::Request<proto::CompareContentsRequest>,
    ) -> Result<tonic::Response<proto::CompareContentsResponse>, Status> {
        let request = request.get_ref();
        tracing::info!(
            "CompareContents: expected={:?}, actual={:?}",
            request.expected.is_some(),
            request.actual.is_some()
        );

        match (request.expected.as_ref(), request.actual.as_ref()) {
            (Some(expected), Some(actual)) => {
                let expected_content = expected
                    .content
                    .as_ref()
                    .ok_or_else(|| Status::aborted("No expected content"))?;
                let actual_content = actual
                    .content
                    .as_ref()
                    .ok_or_else(|| Status::aborted("No actual content"))?;

                let expected_events = parse_sse_content(expected_content)
                    .map_err(|e| Status::aborted(format!("Failed to parse expected SSE: {}", e)))?;
                let actual_events = parse_sse_content(actual_content)
                    .map_err(|e| Status::aborted(format!("Failed to parse actual SSE: {}", e)))?;

                let rules: HashMap<String, RuleList> = request
                    .rules
                    .iter()
                    .map(|(key, rules)| {
                        let rule_list = rules.rule.iter().fold(
                            RuleList::empty(RuleLogic::And),
                            |mut list, rule| {
                                if let Some(values) = &rule.values {
                                    let obj = to_object(values);
                                    if let Value::Object(mut map) = obj {
                                        map.insert(
                                            "match".to_string(),
                                            Value::String(rule.r#type.clone()),
                                        );
                                        if let Ok(matching_rule) =
                                            pact_models::matchingrules::MatchingRule::from_json(
                                                &Value::Object(map),
                                            )
                                        {
                                            list.add_rule(&matching_rule);
                                        }
                                    }
                                }
                                list
                            },
                        );
                        (key.clone(), rule_list)
                    })
                    .collect();

                let mismatches = compare_sse_events(&expected_events, &actual_events, &rules);

                Ok(Response::new(proto::CompareContentsResponse {
                    error: String::default(),
                    type_mismatch: None,
                    results: hashmap! {
                        String::default() => proto::ContentMismatches { mismatches }
                    },
                }))
            }
            (None, Some(actual)) => {
                let contents = actual.content.as_ref().unwrap();
                Ok(Response::new(proto::CompareContentsResponse {
                    error: String::default(),
                    type_mismatch: None,
                    results: hashmap! {
                        String::default() => proto::ContentMismatches {
                            mismatches: vec![proto::ContentMismatch {
                                expected: None,
                                actual: Some(contents.clone()),
                                mismatch: format!("Expected no SSE content, but got {} bytes", contents.len()),
                                path: "".to_string(),
                                diff: "".to_string(),
                                mismatch_type: "body".to_string(),
                            }],
                        }
                    },
                }))
            }
            (Some(expected), None) => {
                let contents = expected.content.as_ref().unwrap();
                Ok(Response::new(proto::CompareContentsResponse {
                    error: String::default(),
                    type_mismatch: None,
                    results: hashmap! {
                        String::default() => proto::ContentMismatches {
                            mismatches: vec![proto::ContentMismatch {
                                expected: Some(contents.clone()),
                                actual: None,
                                mismatch: "Expected SSE content, but did not get any".to_string(),
                                path: "".to_string(),
                                diff: "".to_string(),
                                mismatch_type: "body".to_string(),
                            }],
                        }
                    },
                }))
            }
            (None, None) => Ok(Response::new(proto::CompareContentsResponse {
                error: String::default(),
                type_mismatch: None,
                results: hashmap! {},
            })),
        }
    }

    async fn configure_interaction(
        &self,
        request: tonic::Request<proto::ConfigureInteractionRequest>,
    ) -> Result<tonic::Response<proto::ConfigureInteractionResponse>, Status> {
        tracing::debug!(
            "ConfigureInteraction: content_type='{}'",
            request.get_ref().content_type
        );

        let contents_config = request.get_ref().contents_config.as_ref().unwrap();
        let config_map = to_object(contents_config);

        let mut interactions = Vec::new();
        let plugin_config = proto::PluginConfiguration::default();

        if let Value::Object(map) = config_map {
            if let Some(Value::Object(resp_map)) = map.get("response") {
                if let Some(Value::String(data)) = resp_map.get("data") {
                    let events = SseEvent::parse(data);
                    let sse_content = format_sse_content(&events);

                    interactions.push(proto::InteractionResponse {
                        contents: Some(proto::Body {
                            content_type: "text/event-stream".to_string(),
                            content: Some(sse_content),
                            content_type_hint: ContentTypeHint::Default as i32,
                        }),
                        rules: HashMap::new(),
                        generators: HashMap::new(),
                        message_metadata: None,
                        plugin_configuration: None,
                        interaction_markup: String::new(),
                        interaction_markup_type: proto::interaction_response::MarkupType::CommonMark
                            as i32,
                        part_name: "response".to_string(),
                        metadata_rules: HashMap::new(),
                        metadata_generators: HashMap::new(),
                    });
                }
            }
        }

        Ok(Response::new(proto::ConfigureInteractionResponse {
            error: String::default(),
            interaction: interactions,
            plugin_configuration: Some(plugin_config),
        }))
    }

    async fn generate_content(
        &self,
        request: tonic::Request<proto::GenerateContentRequest>,
    ) -> Result<tonic::Response<proto::GenerateContentResponse>, Status> {
        let req = request.get_ref();
        tracing::info!("GenerateContent: test_mode={}", req.test_mode);

        let contents = req.contents.as_ref().unwrap();
        let content_bytes = contents.content.as_ref().unwrap();
        let content_str = String::from_utf8_lossy(content_bytes);
        tracing::debug!("GenerateContent: input length={}", content_str.len());

        let events = SseEvent::parse(&content_str);
        tracing::info!("GenerateContent: parsed {} events", events.len());

        let generated = format_sse_content(&events);
        tracing::debug!("GenerateContent: generated {} bytes", generated.len());

        Ok(Response::new(proto::GenerateContentResponse {
            contents: Some(proto::Body {
                content_type: "text/event-stream".to_string(),
                content: Some(generated),
                content_type_hint: ContentTypeHint::Default as i32,
            }),
        }))
    }

    async fn start_mock_server(
        &self,
        request: tonic::Request<proto::StartMockServerRequest>,
    ) -> Result<tonic::Response<proto::StartMockServerResponse>, Status> {
        let req = request.get_ref();
        tracing::info!(
            "StartMockServer: host={:?}, port={}, tls={}",
            req.host_interface,
            req.port,
            req.tls
        );
        tracing::debug!("StartMockServer: pact JSON length={}", req.pact.len());
        let pact = req.pact.clone();
        let host_interface = req.host_interface.clone();
        let port = if req.port == 0 { 0 } else { req.port };
        tracing::info!(
            "StartMockServer: port={}, pact_preview={}",
            port,
            &pact[..pact.len().min(100)]
        );

        let server_key = Uuid::new_v4().to_string();
        let server_key_for_response = server_key.clone();

        let addr = format!("{}:{}", host_interface, port)
            .parse::<SocketAddr>()
            .map_err(|e| Status::aborted(format!("Invalid address: {}", e)))?;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Status::aborted(format!("Failed to bind: {}", e)))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| Status::aborted(format!("Failed to get local addr: {}", e)))?;

        let mock_server = MockServer {
            port: local_addr.port() as u32,
            address: local_addr.ip().to_string(),
            pact: pact.clone(),
            results: Vec::new(),
        };

        let server_key_for_map = server_key.clone();
        let mock_servers_clone = self.mock_servers.clone();
        self.mock_servers
            .lock()
            .await
            .insert(server_key_for_map, mock_server);

        tokio::spawn(async move {
            crate::mock_server::run_sse_mock_server(listener, server_key, pact, mock_servers_clone)
                .await;
        });

        Ok(Response::new(proto::StartMockServerResponse {
            response: Some(proto::start_mock_server_response::Response::Details(
                proto::MockServerDetails {
                    key: server_key_for_response,
                    port: local_addr.port() as u32,
                    address: local_addr.ip().to_string(),
                },
            )),
        }))
    }

    async fn shutdown_mock_server(
        &self,
        request: tonic::Request<proto::ShutdownMockServerRequest>,
    ) -> Result<tonic::Response<proto::ShutdownMockServerResponse>, Status> {
        let server_key = request.get_ref().server_key.clone();
        tracing::info!("ShutdownMockServer: server_key={}", server_key);

        let servers = self.mock_servers.lock().await;
        let result = if let Some(server) = servers.get(&server_key) {
            tracing::info!(
                "ShutdownMockServer: found server with {} results",
                server.results.len()
            );
            proto::ShutdownMockServerResponse {
                ok: server.results.is_empty(),
                results: server.results.clone(),
            }
        } else {
            tracing::warn!("ShutdownMockServer: server not found: {}", server_key);
            proto::ShutdownMockServerResponse {
                ok: true,
                results: Vec::new(),
            }
        };
        Ok(Response::new(result))
    }

    async fn get_mock_server_results(
        &self,
        request: tonic::Request<proto::MockServerRequest>,
    ) -> Result<tonic::Response<proto::MockServerResults>, Status> {
        let server_key = request.get_ref().server_key.clone();
        tracing::info!("GetMockServerResults: server_key={}", server_key);

        let servers = self.mock_servers.lock().await;
        let result = if let Some(server) = servers.get(&server_key) {
            tracing::debug!(
                "GetMockServerResults: found server with {} results",
                server.results.len()
            );
            proto::MockServerResults {
                ok: server.results.is_empty(),
                results: server.results.clone(),
            }
        } else {
            tracing::warn!("GetMockServerResults: server not found: {}", server_key);
            proto::MockServerResults {
                ok: true,
                results: Vec::new(),
            }
        };
        Ok(Response::new(result))
    }

    async fn prepare_interaction_for_verification(
        &self,
        request: tonic::Request<proto::VerificationPreparationRequest>,
    ) -> Result<tonic::Response<proto::VerificationPreparationResponse>, Status> {
        let req = request.get_ref();
        tracing::info!(
            "PrepareInteractionForVerification: interaction_key={}, pact_length={}",
            req.interaction_key,
            req.pact.len()
        );
        Ok(Response::new(proto::VerificationPreparationResponse {
            response: Some(
                proto::verification_preparation_response::Response::InteractionData(
                    proto::InteractionData {
                        body: Some(proto::Body {
                            content_type: "text/event-stream".to_string(),
                            content: None,
                            content_type_hint: ContentTypeHint::Default as i32,
                        }),
                        metadata: HashMap::new(),
                    },
                ),
            ),
        }))
    }

    async fn verify_interaction(
        &self,
        request: tonic::Request<proto::VerifyInteractionRequest>,
    ) -> Result<tonic::Response<proto::VerifyInteractionResponse>, Status> {
        let req = request.get_ref();
        tracing::info!(
            "VerifyInteraction: interaction_key={}, pact_length={}",
            req.interaction_key,
            req.pact.len()
        );
        todo!("verify_interaction not yet implemented")
    }
}
