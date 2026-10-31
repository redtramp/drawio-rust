//! 设置（docs/03-5 §6、§7）。
//!
//! 配置持久化到 data_dir/config.toml（含 api_key，文件 0600）。SSRF 校验在
//! model_test 与自定义 endpoint 生效，Ollama/SGLang 本地默认放行。

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub provider_id: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// 校验模型（VLM）；None 用主模型。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_model: Option<String>,
    /// API key：明文持久化到 config.toml（文件 0600）。前端「留空不修改」，
    /// 故 settings_get 会脱敏为 None，不回传明文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportDefaults {
    pub scale: f32,
    pub border: u32,
    pub transparent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    pub lang: String,
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSettings {
    pub auto_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub ai_enabled: bool,
    pub provider: ProviderSettings,
    pub export: ExportDefaults,
    pub ui: UiSettings,
    pub sync: SyncSettings,
    /// 首次引导是否完成（false 时前端弹出 Provider 配置向导）。
    #[serde(default = "default_false")]
    pub onboarded: bool,
}

fn default_false() -> bool {
    false
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ai_enabled: true,
            provider: ProviderSettings {
                provider_id: "openai".to_string(),
                model: "gpt-4o".to_string(),
                base_url: None,
                validation_provider_id: None,
                validation_model: None,
                api_key: None,
            },
            export: ExportDefaults {
                scale: 2.0,
                border: 20,
                transparent: false,
            },
            ui: UiSettings {
                lang: "zh-CN".to_string(),
                theme: "system".to_string(),
            },
            sync: SyncSettings { auto_check: true },
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        let path = data_dir.join("config.toml");
        if !path.exists() {
            let s = Settings::default();
            s.save(data_dir)?;
            return Ok(s);
        }
        let content = std::fs::read_to_string(&path).map_err(|e| format!("read config: {e}"))?;
        toml::from_str(&content).map_err(|e| format!("parse config: {e}"))
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(data_dir).map_err(|e| format!("mkdir: {e}"))?;
        let path = data_dir.join("config.toml");
        let content = toml::to_string_pretty(self).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&path, content).map_err(|e| format!("write config: {e}"))?;
        // 含 api_key，收紧为 0600
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        Ok(())
    }
}

/// SSRF 校验：拒绝私网/链路本地/环回，除非 allow_localhost（本地推理）。
pub fn validate_endpoint(url: &str, allow_localhost: bool) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("unsupported scheme \"{other}\"")),
    }
    let host = parsed.host_str().unwrap_or("");
    if host.is_empty() {
        return Err("missing host".to_string());
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if ip.is_loopback() {
            return if allow_localhost {
                Ok(())
            } else {
                Err("loopback address not allowed (enable localhost for Ollama/SGLang)".to_string())
            };
        }
        let private_or_link_local = match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_unique_local() || v6.is_unicast_link_local(),
        };
        if private_or_link_local {
            return Err("private/link-local address not allowed".to_string());
        }
        return Ok(());
    }
    if host.eq_ignore_ascii_case("localhost") {
        return if allow_localhost {
            Ok(())
        } else {
            Err("localhost not allowed (enable localhost for Ollama/SGLang)".to_string())
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Settings::default();
        s.save(tmp.path()).unwrap();
        let loaded = Settings::load(tmp.path()).unwrap();
        assert_eq!(loaded.provider.model, "gpt-4o");
    }

    #[test]
    fn ssrf_blocks_private() {
        assert!(validate_endpoint("http://169.254.169.254/latest", false).is_err());
        assert!(validate_endpoint("http://10.0.0.1/v1", false).is_err());
        assert!(validate_endpoint("https://api.openai.com/v1", false).is_ok());
        assert!(validate_endpoint("http://localhost:11434/v1", true).is_ok());
        assert!(validate_endpoint("http://localhost:11434/v1", false).is_err());
    }

    #[test]
    fn api_key_roundtrip_not_serialized_when_none() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.provider.api_key = Some("sk-test".to_string());
        s.save(tmp.path()).unwrap();
        let loaded = Settings::load(tmp.path()).unwrap();
        assert_eq!(loaded.provider.api_key.as_deref(), Some("sk-test"));
    }
}
