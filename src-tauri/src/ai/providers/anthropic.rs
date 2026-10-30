//! Anthropic Messages 客户端（Claude）。

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::{
    ChatRequest, ChatResponse, ContentBlock, Provider, ResponseAccumulator, Role, StreamChunk,
    ToolCall,
};
use crate::error::Error;

pub struct AnthropicProvider {
    id: String,
    model: String,
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
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
            client: reqwest::Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        if self.base_url.ends_with("/messages") {
            self.base_url.clone()
        } else {
            format!("{}/messages", self.base_url.trim_end_matches('/'))
        }
    }

    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut messages: Vec<Value> = Vec::new();
        for msg in &req.messages {
            let role = match msg.role {
                Role::System | Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "user",
            };
            let mut content: Vec<Value> = Vec::new();
            for b in &msg.blocks {
                match b {
                    ContentBlock::Text(t) => content.push(json!({"type": "text", "text": t})),
                    ContentBlock::Image { mime, data_base64 } => content.push(json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": mime, "data": data_base64 }
                    })),
                }
            }
            for tc in &msg.tool_calls {
                let input: Value = serde_json::from_str(&tc.arguments_json).unwrap_or(Value::Null);
                content.push(
                    json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": input}),
                );
            }
            if let Some(tid) = &msg.tool_call_id {
                // tool 结果：合并为一条 user 消息的 tool_result
                let text = msg.text();
                content.push(json!({"type": "tool_result", "tool_use_id": tid, "content": [{"type": "text", "text": text}]}));
            }
            messages.push(json!({"role": role, "content": content}));
        }
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
            "stream": stream,
        });
        if let Some(system) = &req.system {
            body["system"] = json!(system);
        }
        if !req.tools.is_empty() {
            let tools: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    let f = &t["function"];
                    json!({ "name": f["name"], "description": f["description"], "input_schema": f["parameters"] })
                })
                .collect();
            body["tools"] = json!(tools);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        body
    }
}

fn parse_message(data: &Value) -> ChatResponse {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in data["content"]
        .as_array()
        .map(|a| a.iter())
        .unwrap_or_default()
    {
        match block["type"].as_str() {
            Some("text") => text.push_str(block["text"].as_str().unwrap_or("")),
            Some("tool_use") => {
                let input = block["input"].clone();
                tool_calls.push(ToolCall {
                    id: block["id"].as_str().unwrap_or("").to_string(),
                    name: block["name"].as_str().unwrap_or("").to_string(),
                    arguments_json: input.to_string(),
                });
            }
            _ => {}
        }
    }
    ChatResponse {
        text,
        tool_calls,
        finish_reason: data["stop_reason"].as_str().map(|s| s.to_string()),
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse, Error> {
        let body = self.build_body(&req, false);
        let resp = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("network: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("{} {text}", status.as_u16())));
        }
        let data: Value = resp
            .json()
            .await
            .map_err(|e| Error::Provider(format!("json: {e}")))?;
        Ok(parse_message(&data))
    }

    async fn stream(
        &self,
        req: ChatRequest,
        mut on_event: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<ChatResponse, Error> {
        let body = self.build_body(&req, true);
        let resp = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("network: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("{} {text}", status.as_u16())));
        }
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
                if data.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                match v["type"].as_str() {
                    Some("content_block_delta") => match v["delta"]["type"].as_str() {
                        Some("text_delta") => {
                            let t = v["delta"]["text"].as_str().unwrap_or("").to_string();
                            if !t.is_empty() {
                                let chunk = StreamChunk::Text(t);
                                acc.push(&chunk);
                                on_event(chunk);
                            }
                        }
                        Some("input_json_delta") => {
                            let partial = v["delta"]["partial_json"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            let index = v["index"].as_u64().unwrap_or(0) as usize;
                            let chunk = StreamChunk::ToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments: Some(partial),
                            };
                            acc.push(&chunk);
                            on_event(chunk);
                        }
                        _ => {}
                    },
                    Some("content_block_start") => {
                        let b = &v["content_block"];
                        if b["type"] == "tool_use" {
                            let index = v["index"].as_u64().unwrap_or(0) as usize;
                            let id = b["id"].as_str().map(|s| s.to_string());
                            let name = b["name"].as_str().map(|s| s.to_string());
                            let chunk = StreamChunk::ToolCallDelta {
                                index,
                                id,
                                name,
                                arguments: None,
                            };
                            acc.push(&chunk);
                            on_event(chunk);
                        }
                    }
                    Some("message_delta") => {
                        if let Some(reason) = v["delta"]["stop_reason"].as_str() {
                            acc.finish_reason = Some(reason.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(acc.finish())
    }
}
