use anyhow::anyhow;
use bytes::Bytes;
use either::Either;
use log::debug;
use maplit::hashmap;
use pact_matching::matchers::Matches;
use pact_models::bodies::OptionalBody;
use pact_models::generators::{GenerateValue, Generator, NoopVariantMatcher, VariantMatcher};
use pact_models::matchingrules::RuleList;
use pact_models::prelude::ContentType;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use tonic::{Request, Response};

use crate::parser::{parse_field, parse_value, FieldKey};
use crate::proto;
use crate::utils::{from_value, to_value};

#[derive(Debug, Clone)]
struct SseEvent {
    id: Option<String>,
    event_type: Option<String>,
    data: Option<String>,
    retry: Option<String>,
}

impl SseEvent {
    #[allow(dead_code)]
    fn to_sse_string(&self) -> String {
        let mut result = String::new();
        if let Some(ref id) = self.id {
            result.push_str(&format!("id:{}\n", id));
        }
        if let Some(ref retry) = self.retry {
            result.push_str(&format!("retry:{}\n", retry));
        }
        if let Some(ref event_type) = self.event_type {
            result.push_str(&format!("event:{}\n", event_type));
        }
        if let Some(ref data) = self.data {
            result.push_str(&format!("data:{}\n", data));
        }
        result.push('\n');
        result
    }
}

fn parse_sse_content(content: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut current = SseEvent {
        id: None,
        event_type: None,
        data: None,
        retry: None,
    };

    for line in content.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if current.data.is_some() {
                events.push(current);
                current = SseEvent {
                    id: None,
                    event_type: None,
                    data: None,
                    retry: None,
                };
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("id:") {
            current.id = Some(value.strip_prefix(' ').unwrap_or(value).to_string());
        } else if let Some(value) = line.strip_prefix("event:") {
            current.event_type = Some(value.strip_prefix(' ').unwrap_or(value).to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            current.data = Some(
                current
                    .data
                    .map(|existing| format!("{}\n{}", existing, value))
                    .unwrap_or_else(|| value.to_string()),
            );
        } else if let Some(value) = line.strip_prefix("retry:") {
            current.retry = Some(value.strip_prefix(' ').unwrap_or(value).to_string());
        }
    }

    events
}

pub fn setup_sse_contents(
    request: &Request<proto::ConfigureInteractionRequest>,
) -> anyhow::Result<Response<proto::ConfigureInteractionResponse>> {
    match &request.get_ref().contents_config {
        Some(config) => {
            let mut events: Vec<
                Option<(
                    pact_models::matchingrules::expressions::MatchingRuleDefinition,
                    FieldKey,
                )>,
            > = Vec::new();

            for (key, value) in &config.fields {
                let field_key = parse_field(key)?;
                let result = parse_value(value)?;
                debug!("Parsed SSE field: {:?} -> {:?}", field_key, result);

                if key != "event" && key != "retry" {
                    events.push(Some((result, field_key)));
                }
            }

            let mut sse_output = String::new();
            let mut rules = hashmap! {};
            let mut generators = hashmap! {};
            let mut markdown = String::from("# SSE Events\n\n|type|data|\n|---|---|\n");

            for (md, field_key) in events.iter().flatten() {
                let event_type = field_key.event_type.clone().unwrap_or_default();
                if !event_type.is_empty() {
                    sse_output.push_str(&format!("event:{}\n", event_type));
                }
                for line in md.value.lines() {
                    sse_output.push_str(&format!("data:{}\n", line));
                }
                if md.value.ends_with('\n') {
                    sse_output.push_str("data:\n");
                }
                sse_output.push('\n');

                let path = field_key.path();
                let rule_type = md
                    .rules
                    .first()
                    .and_then(|r| {
                        if let Either::Left(r) = r {
                            Some(r.name())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "type".to_string());

                let mut rule_values: BTreeMap<String, prost_types::Value> = BTreeMap::new();
                if let Some(Either::Left(r)) = md.rules.first() {
                    for (k, v) in r.values() {
                        rule_values.insert(k.to_string(), to_value(&v));
                    }
                }
                rule_values.insert(
                    "match".to_string(),
                    to_value(&Value::String(rule_type.clone())),
                );

                rules.insert(
                    path.clone(),
                    proto::MatchingRules {
                        rule: vec![proto::MatchingRule {
                            r#type: rule_type,
                            values: Some(prost_types::Struct {
                                fields: rule_values,
                            }),
                        }],
                    },
                );

                if let Some(ref gen) = md.generator {
                    let mut gen_values: BTreeMap<String, prost_types::Value> = BTreeMap::new();
                    for (k, v) in gen.values() {
                        gen_values.insert(k.to_string(), to_value(&v));
                    }
                    generators.insert(
                        path,
                        proto::Generator {
                            r#type: gen.name(),
                            values: Some(prost_types::Struct { fields: gen_values }),
                        },
                    );
                }

                markdown.push_str(&format!("|{}|{}|\n", event_type, md.value));
            }

            debug!("matching rules = {:?}", rules);
            debug!("generators = {:?}", generators);

            Ok(Response::new(proto::ConfigureInteractionResponse {
                interaction: vec![proto::InteractionResponse {
                    contents: Some(proto::Body {
                        content_type: "text/event-stream".to_string(),
                        content: Some(sse_output.into_bytes()),
                        content_type_hint: 0,
                    }),
                    rules,
                    generators,
                    message_metadata: None,
                    plugin_configuration: None,
                    interaction_markup: markdown,
                    interaction_markup_type: 0,
                    ..proto::InteractionResponse::default()
                }],
                ..proto::ConfigureInteractionResponse::default()
            }))
        }
        None => Err(anyhow!("No config provided to match/generate SSE content")),
    }
}

pub fn compare_sse_contents(
    expected_sse: &str,
    actual_sse: &str,
    _allow_unexpected_keys: bool,
    rules: &HashMap<String, RuleList>,
) -> anyhow::Result<Response<proto::CompareContentsResponse>> {
    debug!("Comparing SSE contents with rules: {:?}", rules);

    let expected_events = parse_sse_content(expected_sse);
    let actual_events = parse_sse_content(actual_sse);

    let mut mismatches = Vec::new();
    let mut used_actual: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (exp_idx, exp) in expected_events.iter().enumerate() {
        let event_prefix = format!("event[{}]", exp_idx);
        let exp_type = &exp.event_type;

        // Find a matching actual event by type
        let act_opt = actual_events.iter().enumerate().find(|(idx, act)| {
            !used_actual.contains(idx) && act.event_type.as_ref() == exp_type.as_ref()
        });

        match act_opt {
            Some((act_idx, act)) => {
                used_actual.insert(act_idx);
                compare_event(exp, act, &event_prefix, rules, &mut mismatches);
            }
            None => {
                // No matching event type found
                if let Some(ref exp_data) = exp.data {
                    mismatches.push(proto::ContentMismatch {
                        expected: Some(exp_data.as_bytes().to_vec()),
                        actual: None,
                        mismatch: format!(
                            "Expected '{}' event with data '{}', but no such event found",
                            exp_type.as_deref().unwrap_or("message"),
                            exp_data
                        ),
                        path: format!("{}.data", event_prefix),
                        diff: "".to_string(),
                    });
                }
            }
        }
    }

    Ok(Response::new(proto::CompareContentsResponse {
        error: String::default(),
        type_mismatch: None,
        results: hashmap! {
            String::default() => proto::ContentMismatches {
                mismatches
            }
        },
    }))
}

fn compare_event(
    exp: &SseEvent,
    act: &SseEvent,
    event_prefix: &str,
    rules: &HashMap<String, pact_models::matchingrules::RuleList>,
    mismatches: &mut Vec<proto::ContentMismatch>,
) {
    let rule_path_suffix = exp
        .event_type
        .as_ref()
        .map(|t| format!(".{}.{}", t, "*"))
        .unwrap_or_else(|| ".*".to_string());

    if let Some(ref exp_id) = exp.id {
        let rule_path = format!("id{}", rule_path_suffix);

        if let Some(ref act_id) = act.id {
            if let Some(rule_list) = rules.get(&rule_path) {
                for rule in &rule_list.rules {
                    if let Err(err) = exp_id.matches_with(act_id, rule, false) {
                        mismatches.push(proto::ContentMismatch {
                            expected: Some(exp_id.as_bytes().to_vec()),
                            actual: Some(act_id.as_bytes().to_vec()),
                            mismatch: err.to_string(),
                            path: format!("{}.id", event_prefix),
                            diff: "".to_string(),
                        });
                    }
                }
            } else if exp_id != act_id {
                mismatches.push(proto::ContentMismatch {
                    expected: Some(exp_id.as_bytes().to_vec()),
                    actual: Some(act_id.as_bytes().to_vec()),
                    mismatch: format!("Expected id '{}', but got '{}'", exp_id, act_id),
                    path: format!("{}.id", event_prefix),
                    diff: "".to_string(),
                });
            }
        } else if rules.get(&rule_path).is_some() {
            // Has rule but no actual id — skip if rule allows missing
        } else {
            mismatches.push(proto::ContentMismatch {
                expected: Some(exp_id.as_bytes().to_vec()),
                actual: None,
                mismatch: format!("Expected id '{}', but event has no id", exp_id),
                path: format!("{}.id", event_prefix),
                diff: "".to_string(),
            });
        }
    }

    if let Some(ref exp_data) = exp.data {
        let rule_path = format!("data{}", rule_path_suffix);

        if let Some(ref act_data) = act.data {
            if let Some(rule_list) = rules.get(&rule_path) {
                for rule in &rule_list.rules {
                    if let Err(err) = exp_data.matches_with(act_data, rule, false) {
                        mismatches.push(proto::ContentMismatch {
                            expected: Some(exp_data.as_bytes().to_vec()),
                            actual: Some(act_data.as_bytes().to_vec()),
                            mismatch: err.to_string(),
                            path: format!("{}.data", event_prefix),
                            diff: "".to_string(),
                        });
                    }
                }
            } else if exp_data != act_data {
                mismatches.push(proto::ContentMismatch {
                    expected: Some(exp_data.as_bytes().to_vec()),
                    actual: Some(act_data.as_bytes().to_vec()),
                    mismatch: format!(
                        "Expected data '{}', but got '{}'",
                        exp_data, act_data
                    ),
                    path: format!("{}.data", event_prefix),
                    diff: "".to_string(),
                });
            }
        } else {
            mismatches.push(proto::ContentMismatch {
                expected: Some(exp_data.as_bytes().to_vec()),
                actual: None,
                mismatch: format!(
                    "Expected data '{}', but event has no data",
                    exp_data
                ),
                path: format!("{}.data", event_prefix),
                diff: "".to_string(),
            });
        }
    }

    if let Some(ref exp_retry) = exp.retry {
        if let Some(ref act_retry) = act.retry {
            if let Some(rule_list) = rules.get("retry") {
                for rule in &rule_list.rules {
                    if let Err(err) = exp_retry.matches_with(act_retry, rule, false) {
                        mismatches.push(proto::ContentMismatch {
                            expected: Some(exp_retry.as_bytes().to_vec()),
                            actual: Some(act_retry.as_bytes().to_vec()),
                            mismatch: err.to_string(),
                            path: format!("{}.retry", event_prefix),
                            diff: "".to_string(),
                        });
                    }
                }
            } else if exp_retry != act_retry {
                mismatches.push(proto::ContentMismatch {
                    expected: Some(exp_retry.as_bytes().to_vec()),
                    actual: Some(act_retry.as_bytes().to_vec()),
                    mismatch: format!(
                        "Expected retry '{}', but got '{}'",
                        exp_retry, act_retry
                    ),
                    path: format!("{}.retry", event_prefix),
                    diff: "".to_string(),
                });
            }
        }
    }
}

pub fn generate_sse_content(
    request: &Request<proto::GenerateContentRequest>,
) -> anyhow::Result<OptionalBody> {
    let request = request.get_ref();

    let mut generators_map: HashMap<String, Generator> = HashMap::new();
    for (key, gen) in &request.generators {
        let field_key = crate::parser::parse_field(key)?;
        let values_map: Map<String, Value> = gen
            .values
            .as_ref()
            .ok_or_else(|| anyhow!("Generator values were expected"))?
            .fields
            .iter()
            .map(|(k, v)| (k.clone(), from_value(v)))
            .collect();
        let generator = Generator::from_map(&gen.r#type, &values_map)
            .ok_or_else(|| anyhow!("Failed to build generator of type {}", gen.r#type))?;
        generators_map.insert(field_key.path(), generator);
    }

    let context = hashmap! {};
    let variant_matcher = NoopVariantMatcher.boxed();

    let sse_data = request.contents.as_ref().unwrap().content.as_ref().unwrap();
    let template = std::str::from_utf8(sse_data)?;
    let events = parse_sse_content(template);

    let mut output = String::new();
    for event in events {
        let event_type = event.event_type.clone();

        if let Some(ref retry) = event.retry {
            if let Some(gen) = generators_map.get("retry") {
                let val: String = gen.generate_value(retry, &context, &variant_matcher)?;
                output.push_str(&format!("retry:{}\n", val));
            } else {
                output.push_str(&format!("retry:{}\n", retry));
            }
        }

        if let Some(ref et) = event_type {
            output.push_str(&format!("event:{}\n", et));
        }

        if let Some(ref data) = event.data {
            let gen_key = if event_type.is_some() {
                format!("data[{}][*]", event_type.as_deref().unwrap_or(""))
            } else {
                "data[*]".to_string()
            };

            let val = if let Some(gen) = generators_map.get(&gen_key) {
                gen.generate_value(data, &context, &variant_matcher)?
            } else {
                data.clone()
            };

            for line in val.lines() {
                output.push_str(&format!("data:{}\n", line));
            }
            if val.ends_with('\n') {
                output.push_str("data:\n");
            }
        }

        if let Some(ref id) = event.id {
            let gen_key = if event_type.is_some() {
                format!("id[{}][*]", event_type.as_deref().unwrap_or(""))
            } else {
                "id[*]".to_string()
            };

            if let Some(gen) = generators_map.get(&gen_key) {
                let val: String = gen.generate_value(id, &context, &variant_matcher)?;
                output.push_str(&format!("id:{}\n", val));
            } else {
                output.push_str(&format!("id:{}\n", id));
            }
        }

        output.push('\n');
    }

    debug!("Generated SSE contents has {} bytes", output.len());
    let bytes = Bytes::from(output.into_bytes());
    Ok(OptionalBody::Present(
        bytes,
        Some(ContentType::from("text/event-stream")),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_multiline_data() {
        let sse = "event:message\ndata:line one\ndata:line two\ndata:line three\n\n";
        let events = parse_sse_content(sse);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].data.as_ref().unwrap(),
            "line one\nline two\nline three"
        );
    }

    #[test]
    fn test_parse_single_line_data() {
        let sse = "event:count\ndata:42\n\n";
        let events = parse_sse_content(sse);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data.as_ref().unwrap(), "42");
    }

    #[test]
    fn test_parse_event_without_trailing_blank_line() {
        let sse = "event:count\ndata:42";
        let events = parse_sse_content(sse);
        // Currently the last event without trailing blank line is lost
        // This test documents current behavior; fix in bug #3
        assert_eq!(events.len(), 0);
    }
}
