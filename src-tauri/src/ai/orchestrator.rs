//! AI 回合编排（上游 `contexts/diagram-context.tsx` + `app/api/chat` 的 Rust 对应）。
//!
//! 一轮对话：组装画布备注与消息 → 流式请求 → 执行工具（display/edit/append/
//! get_shape_library）→ 截图 + VLM 校验 → 失败反馈回喂（≤3 次）→ 完成。
//! 全程 Key 留在主进程，纯逻辑可单测。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use super::compact_cells::fold_cells;
use super::diff::{diff_diagrams, is_same_document};
use super::edit::{edit_diagram, DiagramOperation};
use super::embedded_images::{hide_image_data, image_ref_note};
use super::pages::{page_model_xml, place_on_page, PageSelector};
use super::prepare::{prepare_new_diagram, NewDiagram};
use super::prompt::get_system_prompt;
use super::providers::{ChatMessage, ChatRequest, ContentBlock, Provider, StreamChunk, ToolCall};
use super::session::SessionContext;
use super::shape_library::get_shape_library;
use super::tools::{openai_tools, parse_tool_args, ToolArgs};
use super::vlm::{
    format_validation_feedback, validation_schema, ValidationResult, VALIDATION_SYSTEM_PROMPT,
};
use crate::error::Error;

/// 画布上下文（turn 开始时）。
#[derive(Debug, Clone)]
pub struct CanvasContext {
    pub full_xml: String,
    pub page_id: Option<String>,
    pub page_name: Option<String>,
    pub page_count: usize,
    pub selection: Vec<String>,
}

/// 回合输入。
#[derive(Debug, Clone)]
pub struct TurnInput {
    pub message: String,
    pub canvas: CanvasContext,
    pub minimal_style: bool,
}

/// 图操作（发给前端加载）。
#[derive(Debug, Clone)]
pub enum DiagramOp {
    Display,
    Edit {
        operations: Vec<DiagramOperation>,
        unchanged: Vec<String>,
    },
}

/// VLM 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VlmStatus {
    Pass,
    Warn,
    #[default]
    Skipped,
}

/// 回合事件（Tauri 层转发为事件）。
#[derive(Debug, Clone)]
pub enum TurnEvent {
    Token(String),
    Thinking(String),
    ToolCall {
        name: String,
        args_preview: String,
    },
    DiagramReady {
        xml: String,
        op: DiagramOp,
        page_name: Option<String>,
        /// 本次改动触及的 cell id（供前端高亮，FR-A8）。
        touched_ids: Vec<String>,
    },
    Vlm {
        status: VlmStatus,
        warnings: Vec<String>,
    },
    Done {
        text: String,
    },
}

/// 回合结果。
#[derive(Debug, Clone, Default)]
pub struct TurnOutcome {
    pub text: String,
    pub diagrams_displayed: usize,
    pub vlm_status: VlmStatus,
}

/// 前端桥接：获取实时画布 XML 与截图（Tauri 层实现，测试层用假实现）。
#[async_trait]
pub trait TurnBridge: Send + Sync {
    async fn current_xml(&self) -> Result<String, Error>;
    async fn screenshot(&self) -> Result<Vec<u8>, Error>;
}

/// 工具执行结果。
enum ToolExecution {
    Text(String),
    Diagram,
    AppendPending,
}

const MAX_LLM_CALLS: usize = 8;
const MAX_VLM_FEEDBACKS: usize = 3;

fn args_preview(arguments_json: &str, max: usize) -> String {
    let s: String = arguments_json.chars().take(max).collect();
    if arguments_json.chars().count() > max {
        format!("{s}…")
    } else {
        s
    }
}

/// 生成画布备注（上游 "The diagram on the canvas"）。
fn canvas_note(canvas: &CanvasContext, baseline: Option<&str>) -> String {
    let page_xml = page_model_xml(&canvas.full_xml, canvas.page_id.as_deref()).unwrap_or_default();
    let empty = page_xml.trim().is_empty() || !super::pages::has_cells(&canvas.full_xml);
    let mut note = match baseline {
        None => {
            if empty {
                "[Canvas: empty, nothing is drawn yet.]".to_string()
            } else {
                let folded = fold_cells(&hide_image_data(&page_xml));
                let note = image_ref_note(&folded);
                format!("[Canvas when this message was sent:]\n{folded}\n{note}")
            }
        }
        Some(b) if is_same_document(b, &canvas.full_xml) => {
            "[Canvas: unchanged from the last shown version.]".to_string()
        }
        Some(_) => {
            let folded = fold_cells(&hide_image_data(&page_xml));
            let note = image_ref_note(&folded);
            format!("[Canvas when this message was sent:]\n{folded}\n{note}")
        }
    };
    // FR-E6：告知模型用户当前选中的 shape，供「改选中」。
    if !canvas.selection.is_empty() {
        let ids = canvas.selection.join(", ");
        note.push_str(&format!("\n[Selected cells: {ids}]"));
    }
    note
}

/// 执行一个工具调用。
fn execute_tool(
    tc: &ToolCall,
    canvas: &CanvasContext,
    current_doc: &mut String,
    session: &mut SessionContext,
    pending_append: &mut Option<String>,
    on_event: &dyn Fn(TurnEvent),
) -> Result<ToolExecution, Error> {
    match parse_tool_args(&tc.name, &tc.arguments_json) {
        Ok(args) => match args {
            ToolArgs::Display(raw_xml) => {
                let prepared = prepare_new_diagram(
                    &raw_xml,
                    canvas.page_id.as_deref(),
                    canvas.page_name.as_deref(),
                );
                match prepared {
                    NewDiagram::Ok { xml, .. } => {
                        let before = current_doc.clone();
                        let placed = place_on_page(&xml, current_doc, canvas.page_id.as_deref());
                        let touched =
                            diff_diagrams(&before, &placed, canvas.page_id.as_deref()).touched_ids;
                        *current_doc = placed.clone();
                        session.canvas_baseline = Some(placed.clone());
                        on_event(TurnEvent::DiagramReady {
                            xml: placed.clone(),
                            op: DiagramOp::Display,
                            page_name: canvas.page_name.clone(),
                            touched_ids: touched,
                        });
                        Ok(ToolExecution::Diagram)
                    }
                    NewDiagram::Err(e) => Ok(ToolExecution::Text(e)),
                }
            }
            ToolArgs::Edit(operations) => {
                let current = current_doc.clone();
                let selector = PageSelector {
                    page_id: canvas.page_id.clone(),
                    ..Default::default()
                };
                match edit_diagram(&current, &operations, &selector, &mut session.images) {
                    super::edit::EditOutcome::Ok { xml, unchanged, .. } => {
                        let touched =
                            diff_diagrams(&current, &xml, canvas.page_id.as_deref()).touched_ids;
                        *current_doc = xml.clone();
                        session.canvas_baseline = Some(xml.clone());
                        on_event(TurnEvent::DiagramReady {
                            xml: xml.clone(),
                            op: DiagramOp::Edit {
                                operations: operations.clone(),
                                unchanged: unchanged.clone(),
                            },
                            page_name: canvas.page_name.clone(),
                            touched_ids: touched,
                        });
                        Ok(ToolExecution::Diagram)
                    }
                    super::edit::EditOutcome::Err { errors, .. } => {
                        Ok(ToolExecution::Text(errors.join("\n")))
                    }
                }
            }
            ToolArgs::Append(fragment) => {
                let combined = match pending_append.take() {
                    Some(p) => format!("{p}\n{fragment}"),
                    None => fragment,
                };
                match prepare_new_diagram(
                    &combined,
                    canvas.page_id.as_deref(),
                    canvas.page_name.as_deref(),
                ) {
                    NewDiagram::Ok { xml, .. } => {
                        let before = current_doc.clone();
                        let placed = place_on_page(&xml, current_doc, canvas.page_id.as_deref());
                        let touched =
                            diff_diagrams(&before, &placed, canvas.page_id.as_deref()).touched_ids;
                        *current_doc = placed.clone();
                        session.canvas_baseline = Some(placed.clone());
                        on_event(TurnEvent::DiagramReady {
                            xml: placed.clone(),
                            op: DiagramOp::Display,
                            page_name: canvas.page_name.clone(),
                            touched_ids: touched,
                        });
                        Ok(ToolExecution::Diagram)
                    }
                    NewDiagram::Err(e)
                        if e.contains("unfinished mxCell") || e.contains("cut off") =>
                    {
                        *pending_append = Some(combined);
                        Ok(ToolExecution::AppendPending)
                    }
                    NewDiagram::Err(e) => Ok(ToolExecution::Text(e)),
                }
            }
            ToolArgs::ShapeLibrary(library) => match get_shape_library(&library) {
                Ok(text) => Ok(ToolExecution::Text(text)),
                Err(e) => Ok(ToolExecution::Text(e)),
            },
        },
        Err(e) => Ok(ToolExecution::Text(e)),
    }
}

/// 执行 VLM 校验；critical 时返回反馈文本，否则 None。
async fn run_vlm(
    validation: &Arc<dyn Provider>,
    bridge: &dyn TurnBridge,
    vlm_feedback_count: usize,
    on_event: &(dyn Fn(TurnEvent) + Send + Sync),
) -> Result<(VlmStatus, Option<String>), Error> {
    if vlm_feedback_count >= MAX_VLM_FEEDBACKS {
        return Ok((VlmStatus::Skipped, None));
    }
    let png = bridge.screenshot().await?;
    if png.is_empty() {
        return Ok((VlmStatus::Skipped, None));
    }
    let data_base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png);
    let req = ChatRequest {
        model: validation.model().to_string(),
        system: Some(VALIDATION_SYSTEM_PROMPT.to_string()),
        messages: vec![ChatMessage {
            role: super::providers::Role::User,
            blocks: vec![
                ContentBlock::text("Validate this diagram image for visual issues."),
                ContentBlock::image("image/png", data_base64),
            ],
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }],
        tools: Vec::new(),
        max_tokens: 1024,
        temperature: None,
        json_schema: Some(validation_schema()),
    };
    let resp = validation.complete(req).await?;
    let result: Result<ValidationResult, _> = serde_json::from_str(&resp.text);
    match result {
        Ok(v) if v.valid => {
            let warnings: Vec<String> = v
                .issues
                .iter()
                .filter(|i| i.severity == "warning")
                .map(|i| i.description.clone())
                .collect();
            let status = if warnings.is_empty() {
                VlmStatus::Pass
            } else {
                VlmStatus::Warn
            };
            on_event(TurnEvent::Vlm { status, warnings });
            Ok((status, None))
        }
        Ok(v) => {
            let warnings: Vec<String> = v.issues.iter().map(|i| i.description.clone()).collect();
            on_event(TurnEvent::Vlm {
                status: VlmStatus::Warn,
                warnings: warnings.clone(),
            });
            Ok((VlmStatus::Warn, Some(format_validation_feedback(&v))))
        }
        Err(_) => {
            // 解析失败视为通过（上游行为：VLM 失败 → 默认有效）
            on_event(TurnEvent::Vlm {
                status: VlmStatus::Skipped,
                warnings: Vec::new(),
            });
            Ok((VlmStatus::Skipped, None))
        }
    }
}

/// 执行一轮对话。
pub async fn run_turn(
    provider: Arc<dyn Provider>,
    validation: Option<Arc<dyn Provider>>,
    bridge: &dyn TurnBridge,
    input: TurnInput,
    session: &mut SessionContext,
    on_event: Arc<dyn Fn(TurnEvent) + Send + Sync>,
    cancel: &AtomicBool,
) -> Result<TurnOutcome, Error> {
    let model_name = provider.model().to_string();
    let note = canvas_note(&input.canvas, session.canvas_baseline.as_deref());
    session.canvas_baseline = Some(input.canvas.full_xml.clone());

    let mut messages: Vec<ChatMessage> = Vec::new();
    messages.push(ChatMessage::system(get_system_prompt(
        &model_name,
        input.minimal_style,
    )));
    messages.extend(session.history.clone());
    messages.push(ChatMessage::user(format!("{}\n\n{}", input.message, note)));

    let mut current_doc = input.canvas.full_xml.clone();
    let mut pending_append: Option<String> = None;
    let mut vlm_feedback_count = 0usize;
    let mut diagrams_displayed = 0usize;
    let mut final_text = String::new();
    let mut vlm_status = VlmStatus::Skipped;

    for _ in 0..MAX_LLM_CALLS {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Provider("cancelled".to_string()));
        }
        let req = ChatRequest {
            model: model_name.clone(),
            system: None,
            messages: messages.clone(),
            tools: openai_tools(),
            max_tokens: 32768,
            temperature: Some(0.2),
            json_schema: None,
        };
        let on_event2 = on_event.clone();
        let sink = move |chunk: StreamChunk| match &chunk {
            StreamChunk::Text(t) => on_event2(TurnEvent::Token(t.clone())),
            StreamChunk::Thinking(t) => on_event2(TurnEvent::Thinking(t.clone())),
            _ => {}
        };
        let resp = provider.stream(req, Box::new(sink)).await?;

        if resp.tool_calls.is_empty() {
            final_text = resp.text.clone();
            if !resp.text.is_empty() {
                messages.push(ChatMessage {
                    role: super::providers::Role::Assistant,
                    blocks: vec![ContentBlock::text(resp.text)],
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                    name: None,
                });
            }
            break;
        }

        // 工具调用轮
        messages.push(ChatMessage::assistant_with_tool_calls(
            resp.tool_calls.clone(),
        ));
        let mut tool_results: Vec<ChatMessage> = Vec::new();
        let mut diagram_this_round = false;
        for tc in &resp.tool_calls {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Provider("cancelled".to_string()));
            }
            on_event(TurnEvent::ToolCall {
                name: tc.name.clone(),
                args_preview: args_preview(&tc.arguments_json, 200),
            });
            let execution = execute_tool(
                tc,
                &input.canvas,
                &mut current_doc,
                session,
                &mut pending_append,
                &*on_event,
            )?;
            match execution {
                ToolExecution::Text(t) => {
                    tool_results.push(ChatMessage::tool_result(&tc.id, &tc.name, t));
                }
                ToolExecution::Diagram => {
                    diagram_this_round = true;
                    diagrams_displayed += 1;
                    // 成功：VLM 校验（若配置）
                    let (status, feedback) = match &validation {
                        Some(v) => run_vlm(v, bridge, vlm_feedback_count, &*on_event).await?,
                        None => (VlmStatus::Skipped, None),
                    };
                    vlm_status = status;
                    let result_text = match feedback {
                        Some(f) => {
                            vlm_feedback_count += 1;
                            f
                        }
                        None => "Diagram displayed successfully.".to_string(),
                    };
                    tool_results.push(ChatMessage::tool_result(&tc.id, &tc.name, result_text));
                }
                ToolExecution::AppendPending => {
                    tool_results.push(ChatMessage::tool_result(
                        &tc.id,
                        &tc.name,
                        "Received continuation fragment. The diagram is still incomplete; call append_diagram again with the remaining cells.",
                    ));
                }
            }
        }
        messages.extend(tool_results);
        // 若本轮有图且 VLM 通过（无 critical），结束循环等待模型下一步
        if diagram_this_round && matches!(vlm_status, VlmStatus::Pass | VlmStatus::Skipped) {
            // 允许模型继续（可能追加说明）；继续循环，模型可发纯文本或更多工具
            // 若下一轮纯文本则 break。此处不强制 break。
        }
    }

    session.history = messages;
    Ok(TurnOutcome {
        text: final_text,
        diagrams_displayed,
        vlm_status,
    })
}

/// 截断探测辅助：判断 prepare 错误是否为截断（append 续写）。
pub fn is_truncation_error(msg: &str) -> bool {
    msg.contains("unfinished mxCell") || msg.contains("cut off") || msg.contains("truncated")
}

#[cfg(test)]
mod tests {
    use super::super::pages::BLANK_MXFILE;
    use super::super::providers::ChatResponse;
    use super::super::tools::DISPLAY_DIAGRAM;
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct FakeProvider {
        responses: Mutex<VecDeque<ChatResponse>>,
    }

    #[async_trait::async_trait]
    impl Provider for FakeProvider {
        fn id(&self) -> &str {
            "fake"
        }
        fn model(&self) -> &str {
            "fake-model"
        }
        async fn complete(&self, _req: ChatRequest) -> Result<ChatResponse, Error> {
            Ok(self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default())
        }
        async fn stream(
            &self,
            _req: ChatRequest,
            mut on_event: Box<dyn FnMut(StreamChunk) + Send>,
        ) -> Result<ChatResponse, Error> {
            let resp = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default();
            if !resp.text.is_empty() {
                on_event(StreamChunk::Text(resp.text.clone()));
            }
            Ok(resp)
        }
    }

    struct FakeBridge;

    #[async_trait::async_trait]
    impl TurnBridge for FakeBridge {
        async fn current_xml(&self) -> Result<String, Error> {
            Ok(BLANK_MXFILE.to_string())
        }
        async fn screenshot(&self) -> Result<Vec<u8>, Error> {
            Ok(vec![0, 1, 2, 3])
        }
    }

    fn canvas() -> CanvasContext {
        CanvasContext {
            full_xml: BLANK_MXFILE.to_string(),
            page_id: None,
            page_name: None,
            page_count: 1,
            selection: Vec::new(),
        }
    }

    #[tokio::test]
    async fn pure_text_turn_completes() {
        let provider = Arc::new(FakeProvider {
            responses: Mutex::new(VecDeque::from([ChatResponse {
                text: "Hello!".to_string(),
                ..Default::default()
            }])),
        });
        let bridge = FakeBridge;
        let mut session = SessionContext::new("s1");
        let input = TurnInput {
            message: "hi".to_string(),
            canvas: canvas(),
            minimal_style: false,
        };
        let events: Arc<Mutex<Vec<TurnEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events2 = events.clone();
        let cancel = AtomicBool::new(false);
        let outcome = run_turn(
            provider,
            None,
            &bridge,
            input,
            &mut session,
            Arc::new(move |ev| events2.lock().unwrap().push(ev)),
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(outcome.text, "Hello!");
        assert_eq!(outcome.diagrams_displayed, 0);
        // 历史里含 assistant 回复
        assert!(session.history.iter().any(|m| m.text().contains("Hello!")));
    }

    #[tokio::test]
    async fn display_tool_applies_diagram_then_text() {
        let tool_resp = ChatResponse {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: "c1".to_string(),
                name: DISPLAY_DIAGRAM.to_string(),
                arguments_json: r#"{"xml":"<mxCell id=\"2\" value=\"A\" x=\"40\" y=\"40\" w=\"120\" h=\"60\"/>"}"#.to_string(),
            }],
            finish_reason: Some("tool_calls".to_string()),
        };
        let text_resp = ChatResponse {
            text: "Done".to_string(),
            ..Default::default()
        };
        let provider = Arc::new(FakeProvider {
            responses: Mutex::new(VecDeque::from([tool_resp, text_resp])),
        });
        let bridge = FakeBridge;
        let mut session = SessionContext::new("s2");
        let input = TurnInput {
            message: "draw a box".to_string(),
            canvas: canvas(),
            minimal_style: false,
        };
        let events: Arc<Mutex<Vec<TurnEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events2 = events.clone();
        let cancel = AtomicBool::new(false);
        let outcome = run_turn(
            provider,
            None,
            &bridge,
            input,
            &mut session,
            Arc::new(move |ev| events2.lock().unwrap().push(ev)),
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(outcome.diagrams_displayed, 1);
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, TurnEvent::DiagramReady { .. })));
        // 画布基线更新为含 id="2" 的文档
        assert!(session
            .canvas_baseline
            .as_deref()
            .unwrap()
            .contains(r#"id="2""#));
    }
}
