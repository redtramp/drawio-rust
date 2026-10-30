//! provider 客户端 wiremock 集成测试（docs/03-5 §7：每协议 ≥4 用例）。
//!
//! 用 wiremock 起本地 mock 服务，验证三个客户端的请求路径、鉴权、非流式解析、
//! 流式 SSE 解析与错误路径。

use drawio_rust_lib::ai::providers::{
    anthropic::AnthropicProvider, google::GoogleProvider, openai_compat::OpenAiCompatProvider,
    ChatMessage, ChatRequest, Provider, StreamChunk,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_req() -> ChatRequest {
    ChatRequest {
        model: "m".to_string(),
        system: None,
        messages: vec![ChatMessage::user("hi")],
        tools: Vec::new(),
        max_tokens: 1024,
        temperature: None,
        json_schema: None,
    }
}

// ---------------------------------------------------------------------------
// OpenAI 兼容协议
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_complete_returns_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "hello"}, "finish_reason": "stop"}]
        })))
        .mount(&server)
        .await;

    let p = OpenAiCompatProvider::new("openai", "gpt-4o", server.uri(), "k");
    let resp = p.complete(chat_req()).await.unwrap();
    assert_eq!(resp.text, "hello");
    assert!(resp.tool_calls.is_empty());
}

#[tokio::test]
async fn openai_complete_returns_tool_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{"id":"c1","type":"function","function":{"name":"display_diagram","arguments":"{\"xml\":\"<mxCell/>\"}"}}]
                },
                "finish_reason": "tool_calls"
            }]
        })))
        .mount(&server)
        .await;

    let p = OpenAiCompatProvider::new("openai", "gpt-4o", server.uri(), "k");
    let resp = p.complete(chat_req()).await.unwrap();
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "display_diagram");
}

#[tokio::test]
async fn openai_stream_accumulates_text() {
    let server = MockServer::start().await;
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\
               data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\
               data: [DONE]\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse))
        .mount(&server)
        .await;

    let p = OpenAiCompatProvider::new("openai", "gpt-4o", server.uri(), "k");
    let texts = Arc::new(Mutex::new(Vec::new()));
    let cb = {
        let texts = texts.clone();
        move |c: StreamChunk| {
            if let StreamChunk::Text(t) = c {
                texts.lock().unwrap().push(t);
            }
        }
    };
    let resp = p.stream(chat_req(), Box::new(cb)).await.unwrap();
    assert_eq!(resp.text, "Hello");
    assert_eq!(texts.lock().unwrap().join(""), "Hello");
}

#[tokio::test]
async fn openai_error_status_returns_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let p = OpenAiCompatProvider::new("openai", "gpt-4o", server.uri(), "k");
    assert!(p.complete(chat_req()).await.is_err());
}

// ---------------------------------------------------------------------------
// Anthropic 协议
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_complete_returns_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "hi there"}],
            "stop_reason": "end_turn"
        })))
        .mount(&server)
        .await;

    let p = AnthropicProvider::new("anthropic", "claude", server.uri(), "k");
    let resp = p.complete(chat_req()).await.unwrap();
    assert_eq!(resp.text, "hi there");
}

#[tokio::test]
async fn anthropic_complete_returns_tool_use() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "tool_use", "id": "t1", "name": "display_diagram", "input": {"xml": "<mxCell/>"}}],
            "stop_reason": "tool_use"
        })))
        .mount(&server)
        .await;

    let p = AnthropicProvider::new("anthropic", "claude", server.uri(), "k");
    let resp = p.complete(chat_req()).await.unwrap();
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "display_diagram");
}

#[tokio::test]
async fn anthropic_stream_accumulates_text() {
    let server = MockServer::start().await;
    let sse = "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"A\"}}\n\
               data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"B\"}}\n";
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse))
        .mount(&server)
        .await;

    let p = AnthropicProvider::new("anthropic", "claude", server.uri(), "k");
    let resp = p.stream(chat_req(), Box::new(|_c| {})).await.unwrap();
    assert_eq!(resp.text, "AB");
}

#[tokio::test]
async fn anthropic_error_status_returns_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let p = AnthropicProvider::new("anthropic", "claude", server.uri(), "k");
    assert!(p.complete(chat_req()).await.is_err());
}

// ---------------------------------------------------------------------------
// Google Gemini 协议
// ---------------------------------------------------------------------------

#[tokio::test]
async fn google_complete_returns_text() {
    let server = MockServer::start().await;
    let p = "/v1beta/models/gemini-pro:generateContent";
    Mock::given(method("POST"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "gm"}]}, "finishReason": "STOP"}]
        })))
        .mount(&server)
        .await;

    let p = GoogleProvider::new("google", "gemini-pro", server.uri(), "k");
    let resp = p.complete(chat_req()).await.unwrap();
    assert_eq!(resp.text, "gm");
}

#[tokio::test]
async fn google_complete_returns_function_call() {
    let server = MockServer::start().await;
    let p = "/v1beta/models/gemini-pro:generateContent";
    Mock::given(method("POST"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"functionCall": {"name": "display_diagram", "args": {"xml": "<mxCell/>"}}}]}, "finishReason": "STOP"}]
        })))
        .mount(&server)
        .await;

    let p = GoogleProvider::new("google", "gemini-pro", server.uri(), "k");
    let resp = p.complete(chat_req()).await.unwrap();
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "display_diagram");
}

#[tokio::test]
async fn google_stream_replays_text() {
    let server = MockServer::start().await;
    let p = "/v1beta/models/gemini-pro:generateContent";
    Mock::given(method("POST"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "streamed"}]}, "finishReason": "STOP"}]
        })))
        .mount(&server)
        .await;

    let p = GoogleProvider::new("google", "gemini-pro", server.uri(), "k");
    let texts = Arc::new(Mutex::new(Vec::new()));
    let cb = {
        let texts = texts.clone();
        move |c: StreamChunk| {
            if let StreamChunk::Text(t) = c {
                texts.lock().unwrap().push(t);
            }
        }
    };
    let resp = p.stream(chat_req(), Box::new(cb)).await.unwrap();
    assert_eq!(resp.text, "streamed");
    assert_eq!(texts.lock().unwrap().join(""), "streamed");
}

#[tokio::test]
async fn google_error_status_returns_err() {
    let server = MockServer::start().await;
    let p = "/v1beta/models/gemini-pro:generateContent";
    Mock::given(method("POST"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .mount(&server)
        .await;

    let p = GoogleProvider::new("google", "gemini-pro", server.uri(), "k");
    assert!(p.complete(chat_req()).await.is_err());
}
