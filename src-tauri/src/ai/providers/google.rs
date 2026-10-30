//! Google Gemini 客户端（generateContent）。
//!
//! MVP 实现非流式；`stream` 复用 `complete` 后以单个文本增量回放（Gemini 的
//! 工具/多模态在此管线中用量少）。

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{ChatRequest, ChatResponse, ContentBlock, Provider, Role, StreamChunk, ToolCall};
use crate::error::Error;

pub struct GoogleProvider {
    id: String,
    model: String,
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl GoogleProvider {
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
        // 模型名可能含路径，URL 编码
        let model = self.model.replace('/', "%2F");
        if self.base_url.ends_with("/v1beta") {
            format!("{}/models/{model}:generateContent", self.base_url)
        } else {
            format!(
                "{}/v1beta/models/{model}:generateContent",
                self.base_url.trim_end_matches('/')
            )
        }
    }

    fn build_body(&self, req: &ChatRequest) -> Value {
        let mut contents: Vec<Value> = Vec::new();
        let mut system_parts: Vec<Value> = Vec::new();
        if let Some(system) = &req.system {
            system_parts.push(json!({"text": system}));
        }
        for msg in &req.messages {
            let role = match msg.role {
                Role::System | Role::User | Role::Tool => "user",
                Role::Assistant => "model",
            };
            let mut parts: Vec<Value> = Vec::new();
            for b in &msg.blocks {
                match b {
                    ContentBlock::Text(t) => parts.push(json!({"text": t})),
                    ContentBlock::Image { mime, data_base64 } => {
                        parts.push(json!({"inline_data": {"mime_type": mime, "data": data_base64}}))
                    }
                }
            }
            for tc in &msg.tool_calls {
                let args: Value = serde_json::from_str(&tc.arguments_json).unwrap_or(Value::Null);
                parts.push(json!({"functionCall": {"name": tc.name, "args": args}}));
            }
            if msg.tool_call_id.is_some() {
                let text = msg.text();
                let name = msg.name.clone().unwrap_or_default();
                parts.push(
                    json!({"functionResponse": {"name": name, "response": {"result": text}}}),
                );
            }
            if !parts.is_empty() {
                contents.push(json!({"role": role, "parts": parts}));
            }
        }
        let mut body = json!({
            "contents": contents,
            "generationConfig": { "maxOutputTokens": req.max_tokens }
        });
        if !system_parts.is_empty() {
            body["systemInstruction"] = json!({"parts": system_parts});
        }
        if !req.tools.is_empty() {
            let decls: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    let f = &t["function"];
                    json!({"name": f["name"], "description": f["description"], "parameters": f["parameters"]})
                })
                .collect();
            body["tools"] = json!([{"functionDeclarations": decls}]);
        }
        if let Some(t) = req.temperature {
            body["generationConfig"]["temperature"] = json!(t);
        }
        if let Some(schema) = &req.json_schema {
            body["generationConfig"]["responseMimeType"] = json!("application/json");
            body["generationConfig"]["responseSchema"] = schema.clone();
        }
        body
    }
}

fn parse_generate_content(data: &Value) -> ChatResponse {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut finish_reason = None;
    if let Some(candidate) = data["candidates"][0].as_object() {
        finish_reason = candidate["finishReason"].as_str().map(|s| s.to_string());
        for part in candidate["content"]["parts"]
            .as_array()
            .map(|a| a.iter())
            .unwrap_or_default()
        {
            if let Some(t) = part["text"].as_str() {
                text.push_str(t);
            }
            if let Some(fc) = part.get("functionCall") {
                let args = fc.get("args").cloned().unwrap_or(Value::Null);
                tool_calls.push(ToolCall {
                    id: format!("call_{}", tool_calls.len()),
                    name: fc["name"].as_str().unwrap_or("").to_string(),
                    arguments_json: args.to_string(),
                });
            }
        }
    }
    ChatResponse {
        text,
        tool_calls,
        finish_reason,
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse, Error> {
        let body = self.build_body(&req);
        let resp = self
            .client
            .post(self.endpoint())
            .header("x-goog-api-key", &self.api_key)
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
        Ok(parse_generate_content(&data))
    }

    async fn stream(
        &self,
        req: ChatRequest,
        mut on_event: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<ChatResponse, Error> {
        let resp = self.complete(req).await?;
        if !resp.text.is_empty() {
            on_event(StreamChunk::Text(resp.text.clone()));
        }
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_function_call() {
        let data = json!({
            "candidates": [{
                "content": { "parts": [{"functionCall": {"name": "display_diagram", "args": {"xml": "<mxCell/>"}}}] },
                "finishReason": "STOP"
            }]
        });
        let resp = parse_generate_content(&data);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "display_diagram");
    }
}
