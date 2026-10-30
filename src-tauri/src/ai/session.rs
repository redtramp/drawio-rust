//! 回合内会话上下文（内存）。
//!
//! 持有对话历史、画布基线（上次发给模型的画布 XML，用于 canvas note）、图片
//! 引用存储与回合计数。磁盘持久化见 `crate::session::SessionStore`。

use super::embedded_images::ImageStore;
use super::providers::ChatMessage;

pub struct SessionContext {
    pub id: String,
    /// 对话历史（不含 system 消息）。
    pub history: Vec<ChatMessage>,
    /// 上次发给模型的画布完整文档（mxfile）。
    pub canvas_baseline: Option<String>,
    pub images: ImageStore,
    pub turn_counter: u64,
}

impl SessionContext {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            history: Vec::new(),
            canvas_baseline: None,
            images: ImageStore::new(),
            turn_counter: 0,
        }
    }

    pub fn next_turn(&mut self) -> u64 {
        self.turn_counter += 1;
        self.turn_counter
    }
}
