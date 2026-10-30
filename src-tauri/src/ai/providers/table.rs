//! Provider 目录（上游 `lib/providers.ts` 的精简 Rust 版）。
//!
//! 数据表驱动：协议 + 默认端点 + 默认模型 + 密钥提示。运行时按此表实例化
//! 客户端（settings 里的自定义 base_url/model 覆盖默认）。

use std::sync::Arc;

use super::{
    anthropic::AnthropicProvider, google::GoogleProvider, openai_compat::OpenAiCompatProvider,
    Provider,
};
use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAiCompat,
    Anthropic,
    Google,
}

pub struct ProviderSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub protocol: Protocol,
    pub default_base_url: &'static str,
    pub default_model: &'static str,
    pub api_key_label: &'static str,
    pub supports_thinking: bool,
    pub requires_api_key: bool,
}

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        id: "openai",
        name: "OpenAI",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o",
        api_key_label: "OpenAI API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "deepseek",
        name: "DeepSeek",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
        api_key_label: "DeepSeek API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "qwen",
        name: "通义千问 (Qwen)",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen-max",
        api_key_label: "DashScope API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "doubao",
        name: "豆包 (Doubao)",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://ark.cn-beijing.volces.com/api/v3",
        default_model: "doubao-1-5-pro-32k",
        api_key_label: "火山方舟 API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "glm",
        name: "智谱 GLM",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        default_model: "glm-4-plus",
        api_key_label: "智谱 API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "kimi",
        name: "Kimi (Moonshot)",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://api.moonshot.cn/v1",
        default_model: "moonshot-v1-32k",
        api_key_label: "Moonshot API Key",
        supports_thinking: false,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "minimax",
        name: "MiniMax",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://api.minimax.chat/v1",
        default_model: "abab6.5s-chat",
        api_key_label: "MiniMax API Key",
        supports_thinking: false,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "openrouter",
        name: "OpenRouter",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://openrouter.ai/api/v1",
        default_model: "openai/gpt-4o",
        api_key_label: "OpenRouter API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "aihubmix",
        name: "AiHubMix",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://aihubmix.com/v1",
        default_model: "gpt-4o",
        api_key_label: "AiHubMix API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "siliconflow",
        name: "硅基流动",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://api.siliconflow.cn/v1",
        default_model: "Qwen/Qwen2.5-72B-Instruct",
        api_key_label: "SiliconFlow API Key",
        supports_thinking: false,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "modelscope",
        name: "ModelScope",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://api-inference.modelscope.cn/v1",
        default_model: "qwen-max",
        api_key_label: "ModelScope API Key",
        supports_thinking: false,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "ollama",
        name: "Ollama (本地)",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "http://localhost:11434/v1",
        default_model: "qwen2.5",
        api_key_label: "Ollama API Key (可选)",
        supports_thinking: false,
        requires_api_key: false,
    },
    ProviderSpec {
        id: "sglang",
        name: "SGLang (本地)",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "http://localhost:30000/v1",
        default_model: "Qwen/Qwen2.5-32B-Instruct",
        api_key_label: "SGLang API Key (可选)",
        supports_thinking: false,
        requires_api_key: false,
    },
    ProviderSpec {
        id: "anthropic",
        name: "Anthropic Claude",
        protocol: Protocol::Anthropic,
        default_base_url: "https://api.anthropic.com/v1",
        default_model: "claude-sonnet-4-5",
        api_key_label: "Anthropic API Key",
        supports_thinking: true,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "google",
        name: "Google Gemini",
        protocol: Protocol::Google,
        default_base_url: "https://generativelanguage.googleapis.com",
        default_model: "gemini-2.5-pro",
        api_key_label: "Google API Key",
        supports_thinking: false,
        requires_api_key: true,
    },
    ProviderSpec {
        id: "custom",
        name: "自定义 (OpenAI 兼容)",
        protocol: Protocol::OpenAiCompat,
        default_base_url: "https://example.com/v1",
        default_model: "model",
        api_key_label: "API Key",
        supports_thinking: false,
        requires_api_key: false,
    },
];

pub fn find_provider(id: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// 实例化 provider 客户端。
pub fn build_provider(
    id: &str,
    model: &str,
    base_url: Option<&str>,
    api_key: &str,
) -> Result<Arc<dyn Provider>, Error> {
    let spec =
        find_provider(id).ok_or_else(|| Error::Settings(format!("unknown provider \"{id}\"")))?;
    let base = base_url
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| spec.default_base_url.to_string());
    let client: Arc<dyn Provider> = match spec.protocol {
        Protocol::OpenAiCompat => {
            let mut c = OpenAiCompatProvider::new(id, model, base, api_key);
            if id == "openrouter" {
                c = c
                    .with_header("HTTP-Referer", "https://drawio-rust.app")
                    .with_header("X-Title", "drawio-rust");
            }
            Arc::new(c)
        }
        Protocol::Anthropic => Arc::new(AnthropicProvider::new(id, model, base, api_key)),
        Protocol::Google => Arc::new(GoogleProvider::new(id, model, base, api_key)),
    };
    Ok(client)
}

/// 目录列表（JSON，供前端设置面板）。
pub fn catalog_json() -> serde_json::Value {
    serde_json::json!(PROVIDERS
        .iter()
        .map(|p| serde_json::json!({
            "id": p.id,
            "name": p.name,
            "protocol": match p.protocol {
                Protocol::OpenAiCompat => "openai",
                Protocol::Anthropic => "anthropic",
                Protocol::Google => "google",
            },
            "default_base_url": p.default_base_url,
            "default_model": p.default_model,
            "api_key_label": p.api_key_label,
            "supports_thinking": p.supports_thinking,
            "requires_api_key": p.requires_api_key,
        }))
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_providers() {
        assert!(find_provider("openai").is_some());
        assert!(find_provider("ollama").is_some());
        assert!(find_provider("custom").is_some());
        assert!(find_provider("bogus").is_none());
    }

    #[test]
    fn builds_openai_compat() {
        let p = build_provider("openai", "gpt-4o", None, "key").unwrap();
        assert_eq!(p.id(), "openai");
        assert_eq!(p.model(), "gpt-4o");
    }

    #[test]
    fn catalog_has_all() {
        let v = catalog_json();
        assert_eq!(v.as_array().unwrap().len(), PROVIDERS.len());
    }
}
