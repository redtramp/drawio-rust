//! 统一错误类型（docs/03.5 §5）：域错误 + 对外映射。

use thiserror::Error as ThisError;

/// drawio-core 统一错误。
#[derive(Debug, ThisError)]
pub enum Error {
    /// XML 结构错误（含校验失败信息，可供回喂 LLM）。
    #[error("xml error: {0}")]
    Xml(String),

    /// 文档读写/格式错误（.drawio 解析、zip/deflate）。
    #[error("document error: {0}")]
    Document(String),

    /// IO 错误。
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 序列化/反序列化。
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML（同步清单/配置）。
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),

    /// 网络/provider 错误。
    #[error("provider: {0}")]
    Provider(String),

    /// 配置/密钥错误（含降级提示）。
    #[error("settings: {0}")]
    Settings(String),

    /// 会话/版本存储错误。
    #[error("session: {0}")]
    Session(String),

    /// 同步（上游）错误。
    #[error("sync: {0}")]
    Sync(String),

    /// 导出（SVG/PNG/PDF/Visio）错误。
    #[error("export: {0}")]
    Export(String),
}

impl Error {
    /// 对外暴露的错误码（供 Tauri 事件/命令前端分类处理，docs/04 §2）。
    pub fn code(&self) -> &'static str {
        match self {
            Error::Xml(_) => "xml",
            Error::Document(_) => "document",
            Error::Io(_) => "io",
            Error::Json(_) => "json",
            Error::Toml(_) => "toml",
            Error::Provider(_) => "provider",
            Error::Settings(_) => "settings",
            Error::Session(_) => "session",
            Error::Sync(_) => "sync",
            Error::Export(_) => "export",
        }
    }
}

/// 可重试标记：provider 429/超时/网络抖动 = true；结构错误 = false。
pub trait Retryable {
    fn is_retryable(&self) -> bool;
}

impl Retryable for Error {
    fn is_retryable(&self) -> bool {
        matches!(self, Error::Provider(msg) if msg.contains("429") || msg.contains("timeout") || msg.contains("network"))
    }
}
