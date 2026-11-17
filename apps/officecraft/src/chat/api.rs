//! Claude's Messages API, streamed over HTTPS (docs/ai-chat.md). Anthropic has no Rust SDK, so
//! this is the documented raw HTTP: `POST /v1/messages` with `stream: true`, read as
//! server-sent events and put back together into the reply's content blocks, which go back into
//! the conversation unchanged (thinking blocks and their signatures included).

use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2026-10-21";
/// Retries a declined request on another model, server-side (`fallbacks: "default"`).
pub const BETA_FALLBACK: &str = "server-side-fallback-2026-07-01";
/// Needed with a sign-in token rather than a key.
pub const BETA_OAUTH: &str = "oauth-2025-04-20";

/// How the app signs its requests.
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    /// An API key from the Console.
    ApiKey(String),
    /// A short-lived token from the `ant` sign-in.
    Token(String),
}

impl std::fmt::Debug for Credential {
    // Never prints the secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Credential::ApiKey(_) => "ApiKey(…)",
            Credential::Token(_) => "Token(…)",
        })
    }
}

/// What the stream says as it goes, for the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Text for content block `index`.
    Text(usize, String),
    /// A tool call started (its name).
    ToolCall(usize, String),
}

/// A finished reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub content: Vec<Value>,
    pub stop_reason: String,
    pub model: String,
    pub usage: Value,
    /// Tool calls whose input didn't parse: content index and the raw text received.
    pub invalid_inputs: Vec<(usize, String)>,
}

#[derive(Debug, PartialEq)]
pub enum ApiError {
    /// The API answered with an error status and message.
    Status(u16, String),
    Network(String),
    /// The stream broke off or sent an error event.
    Stream(String),
    Cancelled,
}

impl ApiError {
    /// What to tell the person.
    pub fn message(&self) -> String {
        match self {
            ApiError::Status(401, _) => "The API key wasn't accepted. Check it in the panel's settings.".into(),
            ApiError::Status(403, m) => format!("The API refused this request: {m}"),
            ApiError::Status(429, _) => "Too many requests for this API key just now. Try again in a minute.".into(),
            ApiError::Status(529, _) => "Claude is overloaded right now. Try again in a minute.".into(),
            ApiError::Status(_, m) if m.contains("credit balance") => {
                "Your Anthropic account has no API credit left. Add credit in the Console under Billing.".into()
            }
            ApiError::Status(code, m) => format!("The API answered {code}: {m}"),
            ApiError::Network(m) => format!("Couldn't reach Claude: {m}"),
            ApiError::Stream(m) => format!("The answer broke off: {m}"),
            ApiError::Cancelled => "Stopped.".into(),
        }
    }
}

/// The error message in an API error body (`{"type":"error","error":{"message":…}}`).
fn error_text(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(200).collect())
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(ureq::Proxy::try_from_env())
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(20)))
        // A long answer streams for minutes; a silent connection for this long has died.
        .timeout_recv_body(None)
        .timeout_global(None)
        .http_status_as_error(false)
        .user_agent("OfficeCraft")
        .build()
        .new_agent()
}

/// Sends `body` (a Messages request with `stream: true`) and reads the streamed reply,
/// reporting text and tool calls through `on` as they arrive.
pub fn send(cred: &Credential, body: &Value, cancel: &AtomicBool, on: &mut dyn FnMut(Event)) -> Result<Reply, ApiError> {
    let mut betas = vec![BETA_FALLBACK];
    let mut req = agent().post(API_URL).header("anthropic-version", API_VERSION).header("content-type", "application/json");
    match cred {
        Credential::ApiKey(k) => req = req.header("x-api-key", k),
        Credential::Token(t) => {
            req = req.header("authorization", format!("Bearer {t}"));
            betas.push(BETA_OAUTH);
        }
    }
    let resp = req.header("anthropic-beta", betas.join(",")).send(body.to_string()).map_err(|e| ApiError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    let mut body = resp.into_body();
    if status != 200 {
        let text = body.read_to_string().unwrap_or_default();
        return Err(ApiError::Status(status, error_text(&text)));
    }
    let reader = body.into_with_config().limit(u64::MAX).reader();
    parse_stream(BufReader::new(reader), cancel, on)
}

/// Reads a Messages stream (server-sent events) into the finished reply.
pub fn parse_stream(reader: impl BufRead, cancel: &AtomicBool, on: &mut dyn FnMut(Event)) -> Result<Reply, ApiError> {
    let mut reply = Reply { usage: json!({}), ..Reply::default() };
    let mut partial: Vec<String> = Vec::new();
    let mut data = String::new();
    let mut finished = false;
    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ApiError::Cancelled);
        }
        let line = line.map_err(|e| ApiError::Network(e.to_string()))?;
        if let Some(d) = line.strip_prefix("data:") {
            data.push_str(d.trim_start());
            continue;
        }
        if !line.is_empty() || data.is_empty() {
            continue;
        }
        let ev: Value = serde_json::from_str(&data).map_err(|e| ApiError::Stream(format!("unreadable event ({e})")))?;
        data.clear();
        let index = ev["index"].as_u64().and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
        match ev["type"].as_str().unwrap_or_default() {
            "message_start" => {
                reply.model = ev["message"]["model"].as_str().unwrap_or_default().to_string();
                reply.usage = ev["message"]["usage"].clone();
            }
            "content_block_start" => {
                while reply.content.len() <= index {
                    reply.content.push(Value::Null);
                    partial.push(String::new());
                }
                reply.content[index] = ev["content_block"].clone();
                if ev["content_block"]["type"] == "tool_use" {
                    on(Event::ToolCall(index, ev["content_block"]["name"].as_str().unwrap_or_default().to_string()));
                }
            }
            "content_block_delta" => {
                let (Some(block), Some(buf)) = (reply.content.get_mut(index), partial.get_mut(index)) else { continue };
                let delta = &ev["delta"];
                let append = |block: &mut Value, key: &str, more: &str| {
                    let now = block[key].as_str().unwrap_or_default().to_string() + more;
                    block[key] = json!(now);
                };
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = delta["text"].as_str().unwrap_or_default();
                        append(block, "text", t);
                        on(Event::Text(index, t.to_string()));
                    }
                    "thinking_delta" => append(block, "thinking", delta["thinking"].as_str().unwrap_or_default()),
                    "signature_delta" => block["signature"] = delta["signature"].clone(),
                    "input_json_delta" => buf.push_str(delta["partial_json"].as_str().unwrap_or_default()),
                    "citations_delta" => {
                        if !block["citations"].is_array() {
                            block["citations"] = json!([]);
                        }
                        if let Some(list) = block["citations"].as_array_mut() {
                            list.push(delta["citation"].clone());
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(block) = reply.content.get_mut(index) else { continue };
                if block["type"] == "tool_use" {
                    let raw = partial.get(index).cloned().unwrap_or_default();
                    // Streamed tool input is checked here, strictly: a bad one isn't run.
                    match if raw.trim().is_empty() { Ok(json!({})) } else { serde_json::from_str::<Value>(&raw) } {
                        Ok(v) if v.is_object() => block["input"] = v,
                        _ => {
                            block["input"] = json!({});
                            reply.invalid_inputs.push((index, raw));
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(r) = ev["delta"]["stop_reason"].as_str() {
                    reply.stop_reason = r.to_string();
                }
                if let (Some(u), Some(more)) = (reply.usage.as_object_mut(), ev["usage"].as_object()) {
                    for (k, v) in more {
                        u.insert(k.clone(), v.clone());
                    }
                }
            }
            "message_stop" => {
                finished = true;
                break;
            }
            "error" => return Err(ApiError::Stream(ev["error"]["message"].as_str().unwrap_or("error").to_string())),
            _ => {}
        }
    }
    if !finished {
        return Err(ApiError::Stream("the connection closed before the end".into()));
    }
    reply.content.retain(|b| !b.is_null());
    Ok(reply)
}

/// The reply's content as it goes back into the conversation. After a server-side fallback
/// part-way through, the thinking and tool calls written before the last `fallback` block are
/// left out (the fallback block itself stays where it was); otherwise unchanged.
pub fn echo_content(content: &[Value]) -> Vec<Value> {
    let Some(last) = content.iter().rposition(|b| b["type"] == "fallback") else { return content.to_vec() };
    content
        .iter()
        .enumerate()
        .filter(|(i, b)| *i >= last || !matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking" | "tool_use")))
        .map(|(_, b)| b.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn sse(events: &[Value]) -> String {
        events.iter().map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap(), e)).collect()
    }

    #[test]
    fn a_streamed_reply_comes_back_together() {
        let stream = sse(&[
            json!({"type": "message_start", "message": {"model": "claude-opus-5-5", "usage": {"input_tokens": 10}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Revenue "}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "grew."}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "project_read", "input": {}}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"file\": \"a"}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": ".docx\"}"}}),
            json!({"type": "content_block_stop", "index": 2}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 42}}),
            json!({"type": "message_stop"}),
        ]);
        let mut seen = Vec::new();
        let r = parse_stream(stream.as_bytes(), &AtomicBool::new(false), &mut |e| seen.push(e)).unwrap();
        assert_eq!(r.model, "claude-opus-5-5");
        assert_eq!(r.stop_reason, "tool_use");
        assert_eq!(r.usage, json!({"input_tokens": 10, "output_tokens": 42}));
        assert_eq!(r.content[0], json!({"type": "thinking", "thinking": "", "signature": "sig"}));
        assert_eq!(r.content[1]["text"], "Revenue grew.");
        assert_eq!(r.content[2]["input"], json!({"file": "a.docx"}));
        assert!(r.invalid_inputs.is_empty());
        assert_eq!(seen, vec![Event::Text(1, "Revenue ".into()), Event::Text(1, "grew.".into()), Event::ToolCall(2, "project_read".into())]);
    }

    #[test]
    fn a_broken_tool_input_is_caught_and_a_cut_stream_is_an_error() {
        let mut events = vec![
            json!({"type": "message_start", "message": {"model": "m", "usage": {}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "t", "name": "project_search", "input": {}}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"query\": \"a"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}, "usage": {}}),
        ];
        let cut = sse(&events);
        assert!(matches!(parse_stream(cut.as_bytes(), &AtomicBool::new(false), &mut |_| {}), Err(ApiError::Stream(_))));
        events.push(json!({"type": "message_stop"}));
        let r = parse_stream(sse(&events).as_bytes(), &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(r.invalid_inputs, vec![(0, "{\"query\": \"a".to_string())]);
        assert_eq!(r.content[0]["input"], json!({}));
        let err = sse(&[json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})]);
        assert_eq!(parse_stream(err.as_bytes(), &AtomicBool::new(false), &mut |_| {}), Err(ApiError::Stream("Overloaded".into())));
        assert_eq!(parse_stream(sse(&events).as_bytes(), &AtomicBool::new(true), &mut |_| {}), Err(ApiError::Cancelled));
    }

    #[test]
    fn after_a_fallback_the_declined_partial_isnt_sent_back() {
        let content = vec![
            json!({"type": "thinking", "thinking": "", "signature": "a"}),
            json!({"type": "text", "text": "Part"}),
            json!({"type": "fallback", "from": {"model": "x"}, "to": {"model": "y"}}),
            json!({"type": "thinking", "thinking": "", "signature": "b"}),
            json!({"type": "text", "text": "Answer"}),
        ];
        let echoed = echo_content(&content);
        assert_eq!(echoed.len(), 4);
        assert_eq!(echoed[0]["type"], "text");
        assert_eq!(echoed[1]["type"], "fallback");
        assert_eq!(echo_content(&content[..2]), content[..2].to_vec(), "no fallback: unchanged");
    }

    #[test]
    fn errors_say_what_to_do() {
        assert!(ApiError::Status(401, String::new()).message().contains("key"));
        assert!(ApiError::Status(400, "Your credit balance is too low".into()).message().contains("credit"));
        assert_eq!(error_text(r#"{"type":"error","error":{"type":"x","message":"Bad thing"}}"#), "Bad thing");
        assert_eq!(format!("{:?}", Credential::ApiKey("sk-secret".into())), "ApiKey(…)");
    }
}
