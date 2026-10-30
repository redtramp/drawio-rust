//! OpenAI 兼容客户端（OpenAI / DeepSeek / Qwen / Doubao / GLM / Kimi /
//! Minimax / OpenRouter / Ollama / SGLang 等共用协议）。
//!
//! 支持多模态（image_url）、工具调用、流式 SSE（含 reasoning_content）。

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::{
    ChatMessage, ChatRequest, ChatResponse, ContentBlock, Provider, ResponseAccumulator,
    StreamChunk, ToolCall,
};
use crate::error::Error;

pub struct OpenAiCompatProvider {
    id: String,
    model: String,
    base_url: String,
    api_key: String,
    extra_headers: Vec<(String, String)>,
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    pub fn new(
        id: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            model: model.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            extra_headers: Vec::new(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((key.into(), value.into()));
        self
    }

    fn endpoint(&self) -> String {
        if self.base_url.ends_with("/chat/completions") {
            self.base_url.clone()
        } else {
            format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
        }
    }

    fn apply_headers(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        for (k, v) in &self.extra_headers {
            req = req.header(k, v);
        }
        req
    }

    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut messages: Vec<Value> = Vec::new();
        if let Some(system) = &req.system {
            messages.push(json!({"role": "system", "content": system}));
        }
        messages.extend(req.messages.iter().map(message_to_openai));
        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": stream,
            "max_tokens": req.max_tokens,
        });
        if !req.tools.is_empty() {
            body["tools"] = json!(req.tools);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(schema) = &req.json_schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": "validation", "schema": schema, "strict": true }
            });
        }
        body
    }

    async fn send(&self, body: &Value) -> Result<reqwest::Response, Error> {
        let req = self.client.post(self.endpoint()).bearer_auth(&self.api_key);
        let resp = self
            .apply_headers(req)
            .json(body)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("network: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("{} {text}", status.as_u16())));
        }
        Ok(resp)
    }
}

fn message_to_openai(msg: &ChatMessage) -> Value {
    let mut m = json!({ "role": msg.role.as_str() });
    if !msg.blocks.is_empty() {
        let has_image = msg
            .blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::Image { .. }));
        if has_image {
            let content: Vec<Value> = msg
                .blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text(t) => json!({"type": "text", "text": t}),
                    ContentBlock::Image { mime, data_base64 } => json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{mime};base64,{data_base64}") }
                    }),
                })
                .collect();
            m["content"] = json!(content);
        } else {
            m["content"] = json!(msg.text());
        }
    } else if !msg.tool_calls.is_empty() {
        m["content"] = Value::Null;
    }
    if !msg.tool_calls.is_empty() {
        m["tool_calls"] = json!(msg
            .tool_calls
            .iter()
            .map(|tc| json!({
                "id": tc.id,
                "type": "function",
                "function": { "name": tc.name, "arguments": tc.arguments_json }
            }))
            .collect::<Vec<_>>());
    }
    if let Some(tid) = &msg.tool_call_id {
        m["tool_call_id"] = json!(tid);
    }
    if let Some(name) = &msg.name {
        m["name"] = json!(name);
    }
    m
}

fn parse_completion(data: &Value) -> ChatResponse {
    let choice = &data["choices"][0];
    let text = choice["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let tool_calls = choice["message"]["tool_calls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or("").to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments_json: tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    ChatResponse {
        text,
        tool_calls,
        finish_reason: choice["finish_reason"].as_str().map(|s| s.to_string()),
    }
}

fn handle_stream_chunk(
    v: &Value,
    acc: &mut ResponseAccumulator,
    on_event: &mut dyn FnMut(StreamChunk),
) {
    if let Some(reason) = v["choices"][0]["finish_reason"].as_str() {
        acc.finish_reason = Some(reason.to_string());
    }
    let delta = &v["choices"][0]["delta"];
    if let Some(content) = delta["content"].as_str() {
        if !content.is_empty() {
            let chunk = StreamChunk::Text(content.to_string());
            acc.push(&chunk);
            on_event(chunk);
        }
    }
    if let Some(rc) = delta["reasoning_content"].as_str() {
        if !rc.is_empty() {
            let chunk = StreamChunk::Thinking(rc.to_string());
            acc.push(&chunk);
            on_event(chunk);
        }
    }
    if let Some(tcs) = delta["tool_calls"].as_array() {
        for tc in tcs {
            let index = tc["index"].as_u64().unwrap_or(0) as usize;
            let id = tc["id"].as_str().map(|s| s.to_string());
            let name = tc["function"]["name"].as_str().map(|s| s.to_string());
            let arguments = tc["function"]["arguments"].as_str().map(|s| s.to_string());
            let chunk = StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            };
            acc.push(&chunk);
            on_event(chunk);
        }
    }
}

#[async_trait]
impl Provider for OpenAiCompatProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse, Error> {
        let body = self.build_body(&req, false);
        let resp = self.send(&body).await?;
        let data: Value = resp
            .json()
            .await
            .map_err(|e| Error::Provider(format!("json: {e}")))?;
        Ok(parse_completion(&data))
    }

    async fn stream(
        &self,
        req: ChatRequest,
        mut on_event: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<ChatResponse, Error> {
        let body = self.build_body(&req, true);
        let resp = self.send(&body).await?;
        let mut acc = ResponseAccumulator::new();
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::Provider(format!("stream: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..=pos);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    handle_stream_chunk(&v, &mut acc, &mut *on_event);
                }
            }
        }
        Ok(acc.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_completion_with_tool_call() {
        let data = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{"id":"call_1","type":"function","function":{"name":"display_diagram","arguments":"{\"xml\":\"<mxCell/>\"}"}}]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let resp = parse_completion(&data);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "display_diagram");
    }

    #[test]
    fn message_with_image_is_multimodal() {
        let msg = ChatMessage {
            role: super::super::Role::User,
            blocks: vec![
                ContentBlock::text("look"),
                ContentBlock::image("image/png", "AAAA"),
            ],
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        };
        let v = message_to_openai(&msg);
        assert!(v["content"].is_array());
        assert_eq!(v["content"][1]["type"], "image_url");
    }
}
