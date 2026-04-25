use crate::auth::SharedAuthProvider;
use crate::common::CompactionInput;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;
use codex_client::HttpTransport;
use codex_client::RequestTelemetry;
use codex_protocol::models::ResponseItem;
use http::HeaderMap;
use http::Method;
use serde::Deserialize;
use serde_json::Value;
use serde_json::to_value;
use std::sync::Arc;
use std::time::Duration;

pub struct CompactClient<T: HttpTransport> {
    session: EndpointSession<T>,
}

impl<T: HttpTransport> CompactClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
        }
    }

    pub fn with_telemetry(self, request: Option<Arc<dyn RequestTelemetry>>) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
        }
    }

    fn path() -> &'static str {
        "responses/compact"
    }

    pub async fn compact(
        &self,
        body: serde_json::Value,
        extra_headers: HeaderMap,
        request_timeout: Duration,
    ) -> Result<Vec<ResponseItem>, ApiError> {
        let resp = self
            .session
            .execute_with(
                Method::POST,
                Self::path(),
                extra_headers,
                Some(body),
                |req| {
                    req.timeout = Some(request_timeout);
                },
            )
            .await?;
        parse_compact_history_response(&resp.body)
    }

    pub async fn compact_input(
        &self,
        input: &CompactionInput<'_>,
        extra_headers: HeaderMap,
        request_timeout: Duration,
    ) -> Result<Vec<ResponseItem>, ApiError> {
        let body = to_value(input)
            .map_err(|e| ApiError::Stream(format!("failed to encode compaction input: {e}")))?;
        self.compact(body, extra_headers, request_timeout).await
    }
}

#[derive(Debug, Deserialize)]
struct CompactHistoryResponse {
    output: Vec<ResponseItem>,
}

fn parse_compact_history_response(body: &[u8]) -> Result<Vec<ResponseItem>, ApiError> {
    let mut parsed: Value =
        serde_json::from_slice(body).map_err(|e| ApiError::Stream(e.to_string()))?;
    sanitize_compact_output_messages(&mut parsed);
    let parsed: CompactHistoryResponse =
        serde_json::from_value(parsed).map_err(|e| ApiError::Stream(e.to_string()))?;
    Ok(parsed.output)
}

fn sanitize_compact_output_messages(payload: &mut Value) {
    let Some(output) = payload.get_mut("output").and_then(Value::as_array_mut) else {
        return;
    };

    for item in output {
        let is_message = item.get("type").and_then(Value::as_str) == Some("message");
        if !is_message {
            continue;
        }

        let Some(phase) = item.get_mut("phase") else {
            sanitize_compact_output_message_content_items(item);
            continue;
        };
        if phase.as_str().is_some_and(|value| value.trim().is_empty()) {
            *phase = Value::Null;
        }
        sanitize_compact_output_message_content_items(item);
    }
}

fn sanitize_compact_output_message_content_items(message: &mut Value) {
    let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };

    content.retain(|content_item| {
        let is_input_image =
            content_item.get("type").and_then(Value::as_str) == Some("input_image");
        if !is_input_image {
            return true;
        }

        content_item
            .get("image_url")
            .and_then(Value::as_str)
            .is_some_and(|image_url| !image_url.trim().is_empty())
    });
}

#[cfg(test)]
fn extract_message_content(items: &[ResponseItem]) -> &[codex_protocol::models::ContentItem] {
    let [ResponseItem::Message { content, .. }] = items else {
        panic!("expected exactly one message item");
    };
    content
}

#[cfg(test)]
fn output_text(text: &str) -> codex_protocol::models::ContentItem {
    codex_protocol::models::ContentItem::OutputText {
        text: text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use codex_client::Request;
    use codex_client::Response;
    use codex_client::StreamResponse;
    use codex_client::TransportError;

    #[derive(Clone, Default)]
    struct DummyTransport;

    #[async_trait]
    impl HttpTransport for DummyTransport {
        async fn execute(&self, _req: Request) -> Result<Response, TransportError> {
            Err(TransportError::Build("execute should not run".to_string()))
        }

        async fn stream(&self, _req: Request) -> Result<StreamResponse, TransportError> {
            Err(TransportError::Build("stream should not run".to_string()))
        }
    }

    #[test]
    fn path_is_responses_compact() {
        assert_eq!(CompactClient::<DummyTransport>::path(), "responses/compact");
    }

    #[test]
    fn compact_response_accepts_empty_message_phase() {
        let parsed = parse_compact_history_response(
            br#"{
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "hi"}],
                    "phase": ""
                }]
            }"#,
        )
        .expect("compact response should parse");

        assert!(matches!(
            parsed.as_slice(),
            [ResponseItem::Message {
                role,
                phase: None,
                ..
            }] if role == "assistant"
        ));
    }

    #[test]
    fn compact_response_filters_input_images_without_image_url() {
        let parsed = parse_compact_history_response(
            br#"{
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "hi"},
                        {"type": "input_image", "detail": "high"}
                    ]
                }]
            }"#,
        )
        .expect("compact response should parse");

        assert_eq!(extract_message_content(&parsed), [output_text("hi")]);
    }

    #[test]
    fn compact_response_filters_input_images_with_empty_image_url() {
        let parsed = parse_compact_history_response(
            br#"{
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "hi"},
                        {"type": "input_image", "image_url": "   ", "detail": "high"}
                    ]
                }]
            }"#,
        )
        .expect("compact response should parse");

        assert_eq!(extract_message_content(&parsed), [output_text("hi")]);
    }

    #[test]
    fn compact_response_still_rejects_unknown_non_empty_message_phase() {
        let err = parse_compact_history_response(
            br#"{
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "hi"}],
                    "phase": "background"
                }]
            }"#,
        )
        .expect_err("unknown non-empty phase should still fail");

        assert!(
            matches!(err, ApiError::Stream(message) if message.contains("unknown variant `background`"))
        );
    }
}
