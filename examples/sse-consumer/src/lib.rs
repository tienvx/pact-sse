use reqwest::Url;

struct SseClient {
    url: Url,
}

impl SseClient {
    pub fn new<S>(url: S) -> SseClient
    where
        S: Into<Url>,
    {
        SseClient { url: url.into() }
    }

    pub async fn get_events(&self, path: &str) -> anyhow::Result<Vec<SseEvent>> {
        let client = reqwest::Client::new();
        let mut response = client
            .get(self.url.join(path)?)
            .header("Accept", "text/event-stream")
            .send()
            .await?;

        let mut body = String::new();
        while let Some(chunk) = response.chunk().await? {
            body.push_str(&String::from_utf8_lossy(&chunk));
        }

        parse_sse_events(&body)
    }
}

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub id: Option<String>,
    pub event_type: Option<String>,
    pub data: String,
    pub retry: Option<String>,
}

fn parse_sse_events(content: &str) -> anyhow::Result<Vec<SseEvent>> {
    eprintln!("=== RAW SSE CONTENT ===");
    eprintln!("{}", content);
    eprintln!("=========================");
    let mut events = Vec::new();
    let mut current = SseEvent {
        id: None,
        event_type: None,
        data: String::new(),
        retry: None,
    };
    let mut has_content = false;

    for line in content.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if has_content {
                events.push(current);
                current = SseEvent {
                    id: None,
                    event_type: None,
                    data: String::new(),
                    retry: None,
                };
                has_content = false;
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("id:") {
            current.id = Some(value.trim().to_string());
            has_content = true;
        } else if let Some(value) = line.strip_prefix("event:") {
            current.event_type = Some(value.trim().to_string());
            has_content = true;
        } else if let Some(value) = line.strip_prefix("data:") {
            if !current.data.is_empty() {
                current.data.push('\n');
            }
            current.data.push_str(value);
            has_content = true;
        } else if let Some(value) = line.strip_prefix("retry:") {
            current.retry = Some(value.trim().to_string());
            has_content = true;
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use crate::{parse_sse_events, SseClient};
    use expectest::prelude::*;
    use pact_consumer::prelude::*;
    use pact_consumer::mock_server::StartMockServerAsync;
    use pact_models::prelude::*;
    use regex::Regex;
    use serde_json::json;

    #[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 1))]
    async fn test_sse_client_get_events() {
        let mut builder = PactBuilder::new_v4("sseConsumer", "sseProvider")
            .using_plugin("sse", None)
            .await;
        builder.output_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../pacts"));

      builder
            .interaction("request for SSE events", "", |mut i| async move {
                i.request.path("/events");
                i.response
                    .ok()
                    .header("Cache-Control", "no-cache")
                    // .header("Connection", "keep-alive") // actix_web doesn't include Connection header in response
                    .header("X-Accel-Buffering", "no")
                    .contents(
                        ContentType::from("text/event-stream"),
                        json!({
                            "id": "matching(number, 100)",
                            "retry": "matching(integer, 3000)",
                            "event": "matching(type, 'count')",
                            "data": "matching(type, 'simple text\nother text')",
                            "data[count]": "matching(number, 100)",
                            "data[time]": "matching(datetime, 'yyyy-MM-dd', '2000-01-01')",
                            "data[user]": "matching(regex, 'id: \\d+, name: \\w+', 'id: 123, name: Bob')"
                        }),
                    )
                    .await;
                i
            })
            .await;

        let mock_server = builder.start_mock_server_async(None, None).await;

        let client = SseClient::new(mock_server.url().clone());
        let events = client.get_events("/events").await.unwrap();

        let expected_data = vec![
            "simple text\nother text",
            "100",
            "2000-01-01",
            "id: 123, name: Bob",
        ];

        let mut index = 0u32;
        for event in &events {
            if index >= expected_data.len() as u32 {
                break;
            }
            expect!(&event.data).to(be_equal_to(&expected_data[index as usize]));
            index += 1;
        }
        expect!(index).to(be_equal_to(expected_data.len() as u32));
   }

    #[test]
    fn test_parse_sse_events_basic() {
        let sse = "id:1\nevent:count\ndata:100\n\nid:2\ndata:hello\n\n";
        let events = parse_sse_events(sse).unwrap();
        expect!(events.len()).to(be_equal_to(2));
        expect!(events[0].event_type.clone()).to(be_some());
       expect!(events[0].event_type.as_ref().unwrap()).to(be_equal_to("count"));
        expect!(&events[0].data).to(be_equal_to("100"));
        expect!(events[1].event_type.as_ref()).to(be_none());
        expect!(&events[1].data).to(be_equal_to("hello"));
    }

    #[test]
    fn test_parse_sse_events_with_retry() {
        let sse = "retry:3000\nevent:ping\ndata:pong\n\n";
        let events = parse_sse_events(sse).unwrap();
        expect!(events.len()).to(be_equal_to(1));
     expect!(events[0].retry.clone()).to(be_some());
       expect!(events[0].retry.as_ref().unwrap()).to(be_equal_to("3000"));
       expect!(events[0].event_type.clone()).to(be_some());
       expect!(events[0].event_type.as_ref().unwrap()).to(be_equal_to("ping"));
    }
}
