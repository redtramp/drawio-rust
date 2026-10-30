# 03.1 · AI 生成模块设计

> **状态**：已定稿 ｜ **最后更新**：2026-10-29 ｜ **上游依据**：07.2（next-ai-draw-io 调研）

---

## 1. 模块职责

在 Rust 主进程中实现完整 AI 循环：**组 prompt → 调 LLM（流式）→ 解析工具调用 → 结构校验/自动修复 → 失败回喂重试 → VLM 视觉校验 → 结果事件**。对应需求 FR-A1~A10。

## 2. 工具协议（Tools）

完全对齐 next-ai-draw-io 的 4 工具（07.2 §1，`lib/chat-tools.ts:19-135`），AI 输出「紧凑 mxCell 片段」，程序补全外壳：

| 工具 | 入参 | 执行位置 | 说明 |
|------|------|----------|------|
| `display_diagram` | `{ xml: string }` | Rust 准备 + 前端注入 | AI 只给 `<mxStyle>` 定义 + 兄弟级 `<mxCell>`（shape 自闭合带 x,y,w,h；edge 带 source/target）；`<mxfile>/<mxGraphModel>/<root>`、root cells "0"/"1"、vertex/edge/parent="1"/mxGeometry 由 `prepare.rs` 自动补 |
| `edit_diagram` | `{ operations: [{ operation: "update"\|"add"\|"delete", cell_id: string, new_xml?: string }] }` | Rust 计算 + 前端注入 | ID 级指令集；delete 自动级联子 cell 与关联边（port `edit-diagram.ts`） |
| `append_diagram` | `{ xml: string }` | 同 display | `display_diagram` 输出被截断时的续写 |
| `get_shape_library` | `{ library: string }` | Rust（读本地 `assets/prompt/shape-libraries/*.md`） | 30+ 图标库；提示词强制「never guess」shape id（07.2 §2，system-prompts.ts:104） |

**工具 Schema** 以 JSON Schema 表达（`ai/tools.rs`），随 LLM 请求下发；字段名/语义与上游一致，保证提示词可原样复用（ADR：提示词与工具 schema 必须成对同步，见 06 L2 映射表）。

## 3. 系统提示词组装（`ai/prompt.rs`）

结构对齐上游 `getSystemPrompt(modelId, minimalStyle)`（system-prompts.ts:253-268）：

```
DEFAULT_SYSTEM_PROMPT            # 角色 → App 上下文 → 工具说明 → 画布状态协议
                               # → 工具选择规则 → 布局约束(x 0-800/y 0-600)
                               # → XML 结构参考 → 8 条边路由规则(waypoints/exit-entry/绕行)
+ (STYLE_INSTRUCTIONS           # 或
  | MINIMAL_STYLE_INSTRUCTION)  # 按模型能力二选一
```

- **来源**：`src-tauri/assets/prompt/system-prompt.md`（自上游同步，L2 级），含硬编码领域知识（mxGraphModel/mxCell 结构、style 语法、共享样式类规则）与边路由规则（:152-204）。
- **Canvas Context 协议**：对齐 `lib/diagram-context.ts`——「[Canvas when this message was sent: …]」，**仅当画布状态与模型已知状态有差异时**随消息附当前页 XML（控制 token）。差异判定用 `doc/diff.rs` 的 cell 级 diff 结果。
- **装配**：`build_system_prompt(model_id, minimal_style, canvas_xml, selection_details) -> String`；纯函数，单测覆盖。

## 4. 结构校验与自动修复（`ai/prepare.rs` + `ai/xmlfix.rs` + `ai/compact.rs`）

移植上游纯逻辑管线（07.2 §3 结构层，`mcp-server/src/new-diagram.ts` + `xml-validation.ts`）：

```
prepare_new_diagram(xml)
  → prepare_new_model:
      取 mxStyle 共享类
      → reserved_id_error: 禁止 AI 使用保留 id "0"/"1"
      → is_mx_cell_xml_complete: 截断检测（未闭合标签/属性）
      → 包 <mxGraphModel> 壳
  → validate_and_fix_xml（xmlfix.rs）:
      autoFixXml 平移：引号修复、重复 id/属性去重、嵌套 mxCell 展平、
      orphan mxPoint 检查（上游 xml-validation.ts:978，autoFix ~449 起，~500 行）
  → expand_compact_cells（compact.rs）: 紧凑 cell → 完整 mxCell
  → 展开样式类 → 补 html=1 / whiteSpace=wrap
返回 Ok(完整 xml) | Err(PrepError { 分类, 消息 })
```

**验收基线（ADR-2）**：建 `tests/fixtures/` 收 20 组用例（有效/各类残缺/截断/重复 id/样式类），Rust 输出与上游 TS 版（vitest 可跑 sample 中现成测试）逐组比对一致。

## 5. VLM 视觉校验（`ai/vlm.rs`）

对齐上游（07.2 §3 视觉层，`app/api/validate-diagram/route.ts:127-156`）：

1. 前端 commit 后等 ~100ms，向 iframe 请求 PNG 导出（postMessage `export {format:"png"}`）→ dataURI 回 Rust；
2. Rust 调 VLM（用户配置的校验模型，`getValidationModel` 语义）：system = `VALIDATION_SYSTEM_PROMPT`（validation-prompts.ts:6，6 类视觉问题清单，原样搬 assets/prompt/），user = PNG + 结构化输出 `ValidationResult { valid: bool, issues: [{type, severity, description}], suggestions: [...] }`（port validation-schema.ts:9-35，schemars 表达）；
3. 结果处理：
   - `valid` → 轮次完成；
   - 有 issues → `format_validation_feedback`（port diagram-validator.ts:18-58，生成 Critical/Warnings/Suggestions 文本）回喂 LLM 重试；
   - **最多 3 次**（MAX_VALIDATION_RETRIES，上游 use-diagram-tool-handlers.ts:34,336），末次带警告放行；
   - **VLM 服务故障 → 默认 valid**（上游 route.ts:172，不阻塞用户）。

## 6. 重试状态机（`ai/orchestrator.rs`）

```
        ┌─────────────────────────────────────────────┐
        ▼                                             │
IDLE → ASSEMBLE → STREAM ─token─► (事件: token)       │
        │                    │                        │
        │              tool_call 收齐                  │
        ▼                    ▼                        │
   (无工具=纯文本回复)   PREPARE+VALIDATE              │
        │              ├─ Err → FEEDBACK ─────────────┘（≤N 次，超→TURN_ERROR）
        │              └─ Ok  → EMIT(ai_tool_result)
        ▼                        ▼
   TURN_DONE               VLM_CHECK
                             ├─ valid → 记录版本 → TURN_DONE
                             ├─ issues → FEEDBACK（≤3 次，末次放行）
                             └─ vlm 故障 → 视 valid → TURN_DONE
```

- 流式：SSE 解析（`ai/stream.rs`）→ Tauri 事件 `ai_stream`（token/tool_call/turn_done/error），前端打字机 + 工具可视化；
- 事件驱动前端注入：`ai_tool_result` 携带 `{ tool, xml/operations, version_id }`；
- 并发：单会话串行（对齐上游 useChat 单流），多会话留 v1.x。

## 7. Provider 抽象（`ai/providers/`）

对齐上游数据表（07.2 §6，`lib/providers.ts:105-547` 25 家），Rust 侧分两层：

1. **协议客户端**（3 个）：
   - `openai_compat`：覆盖 openai / deepseek / qwen / doubao / glm / kimi / minimax / openrouter / aihubmix / siliconflow / modelscope / ollama / sglang / custom 等（请求/响应/SSE 同一套）；
   - `anthropic`：messages API + thinking 块；
   - `google`：generateContent（含 thoughtSummary）。
2. **Provider 数据表**（`providers.toml`，自上游 `model-catalog.json`/providers.ts 抽取同步，L2 级）：`{ id, name, endpoint_template, auth_style, sampling_map, model_style, env_prefix, base_url_default }`。

**MVP（P1）范围**：openai_compat 全家 + anthropic + google + ollama（≥10 家）；其余按 L2 同步表增量点亮。

**密钥**：设置页写入 → `keyring` 存储（NFR-1）；请求仅由 Rust 发起；自定义端点做 SSRF 防护（禁内网/元数据地址，port `lib/ssrf-protection.ts` 规则）。

## 8. 版本与快照（`ai/versions.rs`，配合 `doc/`）

- 每次 `display/edit` 成功提交 → 存快照（页 XML + 元数据：时间、prompt 摘要、touched_ids、before/after diff 计数）到 `sessions/<id>/versions/`，上限 20 份（对齐 MCP 侧 history.ts，07.2 §7）；
- `touched_ids` 来自 `doc/diff.rs`（signature = 规范化 style+geometry，port diagram-diff.ts:86）→ 前端高亮变更形状（ADR-1 同源桥接 `highlightCells` 能力）；
- 恢复 = 前端 `load` 快照 XML（单步 undo 语义）。

## 9. 复用与移植清单（对照 07.2 §8）

| 上游文件 | 移植到 | 级别 |
|----------|--------|------|
| mcp-server/src/new-diagram.ts | ai/prepare.rs | A 纯逻辑 |
| mcp-server/src/xml-validation.ts | ai/xmlfix.rs | A 纯逻辑 |
| mcp-server/src/compact-cells.ts / style-classes.ts / xml-attributes.ts | ai/compact.rs 等 | A 纯逻辑 |
| mcp-server/src/edit-diagram.ts | ai/edit.rs（edit 操作计算） | A 纯逻辑 |
| lib/diagram-diff.ts / diagram-pages.ts | doc/diff.rs / doc/pages.rs | A 纯逻辑 |
| lib/validation-schema.ts / diagram-validator.ts（格式化部分） | ai/vlm.rs | A 纯逻辑（zod→schemars） |
| lib/system-prompts.ts / validation-prompts.ts | assets/prompt/*.md | B 文本 |
| docs/shape-libraries/*.md（30 个） | assets/prompt/shape-libraries/ | B 文本 |
| lib/model-catalog.json | assets/prompt/model-catalog.json | B 文本 |
| use-diagram-tool-handlers.ts 状态机 | ai/orchestrator.rs | A（语义平移，非逐行） |
| lib/providers.ts / ai-providers.ts | ai/providers/（数据表+客户端重写） | A+C 混合 |
