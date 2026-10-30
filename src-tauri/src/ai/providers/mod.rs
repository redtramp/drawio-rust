//! LLM provider 抽象与请求/响应类型（上游 `lib/providers.ts` 的 Rust 对应）。
//!
//! 三个客户端实现：openai 兼容（含 OpenAI/DeepSeek/Qwen/Ollama 等）、
//! Anthropic、Google Gemini。协议细节各自转换请求/响应。

use serde_json::Value;

use crate::error::Error;

pub mod anthropic;
pub mod google;
pub mod openai_compat;
pub mod table;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// 多模态内容块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlock {
    Text(String),
    Image { mime: String, data_base64: String },
}

impl ContentBlock {
    pub fn text(t: impl Into<String>) -> Self {
        ContentBlock::Text(t.into())
    }
    pub fn image(mime: impl Into<String>, data_base64: impl Into<String>) -> Self {
        ContentBlock::Image {
            mime: mime.into(),
            data_base64: data_base64.into(),
        }
    }
}

/// 工具调用（assistant 请求工具，或 tool 消息的调用上下文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

/// 一条消息。
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            blocks: vec![ContentBlock::text(text)],
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            blocks: vec![ContentBlock::text(text)],
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }
    pub fn assistant_with_tool_calls(calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            blocks: Vec::new(),
            tool_calls: calls,
            tool_call_id: None,
            name: None,
        }
    }
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            blocks: vec![ContentBlock::text(text)],
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }
    /// 消息的纯文本（取第一个文本块）。
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 请求。
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    /// 工具 schema（OpenAI 格式，客户端各自转换）。
    pub tools: Vec<Value>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    /// VLM 结构化输出的 JSON schema。
    pub json_schema: Option<Value>,
}

/// 非流式响应。
#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
}

/// 流式增量。
#[derive(Debug, Clone)]
pub enum StreamChunk {
    Text(String),
    Thinking(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
}

/// 最终响应的组装器：把流式增量累积为 ChatResponse。
#[derive(Debug, Default)]
pub struct ResponseAccumulator {
    pub text: String,
    pub tool_calls: Vec<Option<ToolCallPartial>>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ToolCallPartial {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl ResponseAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &StreamChunk) {
        match chunk {
            StreamChunk::Text(t) => self.text.push_str(t),
            StreamChunk::Thinking(_) => {}
            StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => {
                while self.tool_calls.len() <= *index {
                    self.tool_calls.push(None);
                }
                let slot = self.tool_calls[*index].get_or_insert_with(ToolCallPartial::default);
                if let Some(id) = id {
                    if !id.is_empty() {
                        slot.id.push_str(id);
                    }
                }
                if let Some(name) = name {
                    slot.name.push_str(name);
                }
                if let Some(arguments) = arguments {
                    slot.arguments.push_str(arguments);
                }
            }
        }
    }

    pub fn finish(&self) -> ChatResponse {
        ChatResponse {
            text: self.text.clone(),
            tool_calls: self
                .tool_calls
                .iter()
                .flatten()
                .map(|p| ToolCall {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    arguments_json: p.arguments.clone(),
                })
                .collect(),
            finish_reason: self.finish_reason.clone(),
        }
    }
}

/// Provider 抽象。
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn model(&self) -> &str;

    /// 非流式请求（model_test、VLM 校验）。
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse, Error>;

    /// 流式请求；增量回调 `on_event`，返回累积的最终响应。
    async fn stream(
        &self,
        req: ChatRequest,
        on_event: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<ChatResponse, Error>;
}
