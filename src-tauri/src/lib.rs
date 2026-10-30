//! Tauri 2 主进程：命令注册、AI 回合循环（Key 全程留在主进程）、事件发射。

pub mod ai;
pub mod doc;
mod error;
mod export;
pub mod session;
mod settings;
pub mod sync;
pub mod visio;

pub use error::{Error, Retryable};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ai::orchestrator::{
    run_turn, CanvasContext, DiagramOp, TurnBridge, TurnEvent, TurnInput, VlmStatus,
};
use crate::ai::providers::table::{build_provider, catalog_json};
use crate::ai::providers::ChatMessage;
use crate::ai::session::SessionContext;

/// 前端响应（ai_submit 命令）。
#[derive(Debug, Deserialize)]
pub struct FrontendReply {
    #[serde(default)]
    pub xml: Option<String>,
    #[serde(default)]
    pub png_base64: Option<String>,
}

/// 画布上下文（前端传入）。
#[derive(Debug, Deserialize)]
pub struct CanvasInput {
    pub full_xml: String,
    #[serde(default)]
    pub page_id: Option<String>,
    #[serde(default)]
    pub page_name: Option<String>,
    #[serde(default)]
    pub page_count: usize,
    #[serde(default)]
    pub selection: Vec<String>,
}

impl From<CanvasInput> for CanvasContext {
    fn from(c: CanvasInput) -> Self {
        CanvasContext {
            full_xml: c.full_xml,
            page_id: c.page_id,
            page_name: c.page_name,
            page_count: c.page_count,
            selection: c.selection,
        }
    }
}

/// 前端响应通道：`(turn_id, kind)` → 一次性回执发送端。
type ReplyChannels = Mutex<HashMap<(String, String), tokio::sync::oneshot::Sender<FrontendReply>>>;

/// 应用状态。
pub struct AppState {
    pub data_dir: PathBuf,
    pub keyring: Arc<settings::KeyringBackend>,
    pub turns: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    pub channels: Arc<ReplyChannels>,
    pub sessions: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<SessionContext>>>>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            keyring: Arc::new(settings::KeyringBackend::new(&data_dir)),
            data_dir,
            turns: Arc::new(Mutex::new(HashMap::new())),
            channels: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// 前端桥接：通过事件请求前端 current_xml / screenshot，前端用 ai_submit 回应。
struct TauriBridge {
    app: AppHandle,
    turn_id: String,
    channels: Arc<ReplyChannels>,
}

impl TauriBridge {
    async fn request(&self, kind: &str) -> Result<FrontendReply, crate::Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.channels
            .lock()
            .unwrap()
            .insert((self.turn_id.clone(), kind.to_string()), tx);
        let _ = self.app.emit(
            "ai_request",
            json!({ "turn_id": self.turn_id, "kind": kind }),
        );
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(crate::Error::Provider(
                "frontend channel closed".to_string(),
            )),
            Err(_) => {
                self.channels
                    .lock()
                    .unwrap()
                    .remove(&(self.turn_id.clone(), kind.to_string()));
                Err(crate::Error::Provider(format!(
                    "timeout waiting for {kind}"
                )))
            }
        }
    }
}

#[async_trait::async_trait]
impl TurnBridge for TauriBridge {
    async fn current_xml(&self) -> Result<String, crate::Error> {
        self.request("current_xml")
            .await?
            .xml
            .ok_or_else(|| crate::Error::Provider("frontend returned no XML".to_string()))
    }
    async fn screenshot(&self) -> Result<Vec<u8>, crate::Error> {
        let reply = self.request("screenshot").await?;
        let b64 = reply
            .png_base64
            .ok_or_else(|| crate::Error::Provider("frontend returned no screenshot".to_string()))?;
        export::decode_base64(&b64)
    }
}

// ============================================================================
// 命令
// ============================================================================

#[derive(Serialize)]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "drawio-rust",
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[derive(Serialize)]
pub struct ExportResult {
    path: String,
    converted: usize,
}

#[tauri::command]
fn export_file(format: String, path: String, data: String) -> Result<ExportResult, String> {
    let converted = export::save_export(&format, std::path::Path::new(&path), &data)
        .map_err(|e| e.to_string())?;
    Ok(ExportResult { path, converted })
}

#[tauri::command]
fn export_pdf(
    path: String,
    svg: String,
    scale: Option<f32>,
    border: Option<u32>,
) -> Result<ExportResult, String> {
    let scale = scale.unwrap_or(2.0);
    let border = border.unwrap_or(20);
    export::export_pdf(&svg, std::path::Path::new(&path), scale, border)
        .map_err(|e| e.to_string())?;
    Ok(ExportResult { path, converted: 0 })
}

/// 打开 .drawio 文件（明文 / 压缩双格式），返回规范化 mxfile XML。
#[derive(Serialize)]
pub struct FileOpenResult {
    path: String,
    xml: String,
}

#[tauri::command]
fn file_open(path: String) -> Result<FileOpenResult, String> {
    let (xml, _) = crate::doc::fileio::read_drawio(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;
    Ok(FileOpenResult { path, xml })
}

/// 保存 .drawio 文件（明文 mxfile）。
#[tauri::command]
fn file_save(path: String, xml: String) -> Result<(), String> {
    crate::doc::fileio::write_drawio(std::path::Path::new(&path), &xml)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct TurnInfo {
    turn_id: String,
}

#[tauri::command]
async fn ai_chat_send(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    message: String,
    canvas: CanvasInput,
) -> Result<TurnInfo, String> {
    let settings = settings::Settings::load(&state.data_dir)?;
    if !settings.ai_enabled {
        return Err("AI 未启用（设置里开启）".to_string());
    }
    let api_key = state
        .keyring
        .get(&settings.provider.provider_id)?
        .unwrap_or_default();
    let provider = build_provider(
        &settings.provider.provider_id,
        &settings.provider.model,
        settings.provider.base_url.as_deref(),
        &api_key,
    )
    .map_err(|e| e.to_string())?;

    let validation = match (
        &settings.provider.validation_provider_id,
        &settings.provider.validation_model,
    ) {
        (Some(vid), Some(vmodel)) => {
            let vkey = state.keyring.get(vid)?.unwrap_or_default();
            Some(build_provider(vid, vmodel, None, &vkey).map_err(|e| e.to_string())?)
        }
        _ => None,
    };

    let turn_id = uuid::Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .turns
        .lock()
        .unwrap()
        .insert(turn_id.clone(), cancel.clone());

    // 会话（内存）
    let session = {
        let mut sessions = state.sessions.lock().unwrap();
        sessions
            .entry(session_id.clone())
            .or_insert_with(|| {
                Arc::new(tokio::sync::Mutex::new(SessionContext::new(
                    session_id.clone(),
                )))
            })
            .clone()
    };

    let channels = state.channels.clone();
    let bridge = TauriBridge {
        app: app.clone(),
        turn_id: turn_id.clone(),
        channels,
    };

    let on_event = {
        let app = app.clone();
        let turn_id = turn_id.clone();
        Arc::new(move |ev: TurnEvent| match ev {
            TurnEvent::Token(t) => {
                let _ = app.emit(
                    "ai_stream",
                    json!({ "turn_id": turn_id, "kind": "token", "text": t }),
                );
            }
            TurnEvent::Thinking(t) => {
                let _ = app.emit(
                    "ai_stream",
                    json!({ "turn_id": turn_id, "kind": "thinking", "text": t }),
                );
            }
            TurnEvent::ToolCall { name, args_preview } => {
                let _ = app.emit("ai_stream", json!({ "turn_id": turn_id, "kind": "tool_call", "tool": name, "args": args_preview }));
            }
            TurnEvent::DiagramReady { xml, op, page_name } => {
                let op_json = match op {
                    DiagramOp::Display => json!({ "tool": "display_diagram" }),
                    DiagramOp::Edit {
                        operations,
                        unchanged,
                    } => json!({
                        "tool": "edit_diagram",
                        "operations": operations,
                        "unchanged": unchanged
                    }),
                };
                let mut payload = json!({ "turn_id": turn_id, "xml": xml, "page_name": page_name });
                if let Some(obj) = payload.as_object_mut() {
                    obj.extend(op_json.as_object().unwrap().clone());
                }
                let _ = app.emit("ai_tool_result", payload);
            }
            TurnEvent::Vlm { status, warnings } => {
                let v = match status {
                    VlmStatus::Pass => "pass",
                    VlmStatus::Warn => "warn",
                    VlmStatus::Skipped => "skipped",
                };
                let _ = app.emit(
                    "ai_stream",
                    json!({ "turn_id": turn_id, "kind": "vlm", "status": v, "warnings": warnings }),
                );
            }
            TurnEvent::Done { text } => {
                let _ = app.emit("ai_turn_done", json!({ "turn_id": turn_id, "text": text }));
            }
        }) as Arc<dyn Fn(TurnEvent) + Send + Sync>
    };

    let session2 = session.clone();
    let state_turns = state.turns.clone();
    let state_sessions = state.sessions.clone();
    let turn_id2 = turn_id.clone();
    let minimal_style = false;

    tauri::async_runtime::spawn(async move {
        let mut session_guard = session2.lock().await;
        let input = TurnInput {
            message,
            canvas: canvas.into(),
            minimal_style,
        };
        match run_turn(
            provider,
            validation,
            &bridge,
            input,
            &mut session_guard,
            on_event,
            &cancel,
        )
        .await
        {
            Ok(_) => {
                let _ = app.emit("ai_turn_complete", json!({ "turn_id": turn_id2 }));
            }
            Err(e) => {
                let _ = app.emit(
                    "ai_turn_error",
                    json!({ "turn_id": turn_id2, "code": e.code(), "message": e.to_string(), "retryable": e.is_retryable() }),
                );
            }
        }
        state_turns.lock().unwrap().remove(&turn_id2);
        // 会话结束时清理 channels 残留
        drop(state_sessions.lock().unwrap());
    });

    Ok(TurnInfo { turn_id })
}

#[tauri::command]
async fn ai_turn_cancel(state: State<'_, AppState>, turn_id: String) -> Result<(), String> {
    if let Some(flag) = state.turns.lock().unwrap().get(&turn_id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
async fn ai_submit(
    state: State<'_, AppState>,
    turn_id: String,
    kind: String,
    xml: Option<String>,
    png_base64: Option<String>,
) -> Result<(), String> {
    let tx = state
        .channels
        .lock()
        .unwrap()
        .remove(&(turn_id.clone(), kind));
    if let Some(tx) = tx {
        let _ = tx.send(FrontendReply { xml, png_base64 });
    }
    Ok(())
}

#[tauri::command]
async fn settings_get(state: State<'_, AppState>) -> Result<settings::Settings, String> {
    settings::Settings::load(&state.data_dir)
}

#[tauri::command]
async fn settings_save(
    state: State<'_, AppState>,
    provider_id: String,
    model: String,
    base_url: Option<String>,
    validation_provider_id: Option<String>,
    validation_model: Option<String>,
    api_key: Option<String>,
) -> Result<(), String> {
    let mut s = settings::Settings::load(&state.data_dir)?;
    s.provider.provider_id = provider_id.clone();
    s.provider.model = model;
    s.provider.base_url = base_url;
    s.provider.validation_provider_id = validation_provider_id;
    s.provider.validation_model = validation_model;
    s.save(&state.data_dir)?;
    if let Some(key) = api_key {
        if !key.is_empty() {
            state.keyring.set(&provider_id, &key)?;
        }
    }
    Ok(())
}

#[tauri::command]
fn providers_list() -> serde_json::Value {
    catalog_json()
}

#[derive(Serialize)]
pub struct ModelTestResult {
    ok: bool,
    latency_ms: u64,
    message: String,
}

#[tauri::command]
async fn model_test(
    provider_id: String,
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<ModelTestResult, String> {
    // SSRF 校验（本地 provider 放行）
    let spec = crate::ai::providers::table::find_provider(&provider_id)
        .ok_or_else(|| format!("unknown provider {provider_id}"))?;
    let url = base_url
        .clone()
        .unwrap_or_else(|| spec.default_base_url.to_string());
    let allow_local = !spec.requires_api_key;
    settings::validate_endpoint(&url, allow_local)?;

    let key = api_key.unwrap_or_default();
    let provider = build_provider(&provider_id, &model, base_url.as_deref(), &key)
        .map_err(|e| e.to_string())?;
    let req = crate::ai::providers::ChatRequest {
        model: model.clone(),
        system: None,
        messages: vec![ChatMessage::user("ping — reply with exactly: pong")],
        tools: vec![],
        max_tokens: 8,
        temperature: None,
        json_schema: None,
    };
    let start = std::time::Instant::now();
    match provider.complete(req).await {
        Ok(resp) => Ok(ModelTestResult {
            ok: true,
            latency_ms: start.elapsed().as_millis() as u64,
            message: resp.text.trim().to_string(),
        }),
        Err(e) => Ok(ModelTestResult {
            ok: false,
            latency_ms: start.elapsed().as_millis() as u64,
            message: e.to_string(),
        }),
    }
}

#[tauri::command]
async fn sync_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let root = project_root(&app)?;
    let report = crate::sync::fetch::status(root, &[])?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sync_run(app: AppHandle, all: bool, dry_run: bool) -> Result<serde_json::Value, String> {
    let root = project_root(&app)?;
    let mut args = vec!["--dry-run".to_string()];
    if all {
        args.push("--all".to_string());
    }
    if !dry_run {
        args.retain(|a| a != "--dry-run");
    }
    let report = crate::sync::apply::run(root, &args)?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// 项目根（含 upstream.toml 的目录）。打包后为可执行文件旁的资源目录。
fn project_root(app: &AppHandle) -> Result<PathBuf, String> {
    // 开发期：当前工作目录（drawio-rust）。打包期：exe 旁。
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if cwd.join("upstream.toml").exists() {
        return Ok(cwd);
    }
    if let Ok(res) = app.path().resource_dir() {
        if res.join("upstream.toml").exists() {
            return Ok(res);
        }
    }
    Err("upstream.toml not found (run from project root)".to_string())
}

pub fn run() {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("drawio-rust");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(data_dir))
        .invoke_handler(tauri::generate_handler![
            app_info,
            export_file,
            export_pdf,
            file_open,
            file_save,
            ai_chat_send,
            ai_turn_cancel,
            ai_submit,
            settings_get,
            settings_save,
            providers_list,
            model_test,
            sync_status,
            sync_run,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
