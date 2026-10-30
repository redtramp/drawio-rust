# 04 · 接口与协议规范

> **状态**：已定稿 ｜ **最后更新**：2026-10-29 ｜ **约定**：本文是前后端/Rust 之间的**契约**；实现须与之一致，变更须先改本文（评审）。

---

## 1. Tauri Commands（前端 invoke → Rust）

命名 `snake_case`；返回 `Result<T, AppError{code,message}>`。

| 命令 | 参数 | 返回 | 阶段 |
|------|------|------|------|
| `app_info` | — | `{ version, drawio_vendor_commit, platform }` | P0 |
| `file_dialog` | `{ kind: "open"\|"save", filters? }` | `path \| null` | P0 |
| `file_open` | `{ path }` | `{ xml, meta: { name, compressed } }` | P0 |
| `file_save` | `{ path, xml }` | `{ bytes_written }` | P0 |
| `export_file` | `{ format: "png"\|"svg"\|"xml"\|"pdf"\|"visio-svg", path?, scale?, border?, background? }` | `{ path, converted_count? }` | P0（visio-svg P3） |
| `ai_chat_send` | `{ session_id, message, canvas: { page_xml, page_name, page_count, selection? } }` | `{ turn_id }`（后续走事件） | P1 |
| `ai_turn_cancel` | `{ turn_id }` | `{ cancelled: bool }` | P1 |
| `ai_versions` | `{ session_id }` | `VersionMeta[]` | P2 |
| `ai_version_restore` | `{ session_id, version_id }` | `{ xml, touched_ids }` | P2 |
| `settings_get` | — | `Settings`（无 key 明文） | P1 |
| `settings_save` | `{ provider_id, model, base_url?, api_key?（空=不变）, validation_model?, ui? }` | `Settings` | P1 |
| `model_test` | `{ provider_id, model, base_url?, api_key? }` | `{ ok, latency_ms, error? }` | P1 |
| `providers_list` | — | `ProviderInfo[]`（数据表 + 已配置标记） | P1 |
| `sync_status` | — | `SyncStatus`（§5） | P4 |
| `sync_run` | `{ upstreams?: string[] }` | `{ report_id }`（进度/完成走事件） | P4 |

## 2. Tauri Events（Rust → 前端 `listen`）

载荷全部 `serde_json` 可序列化。

| 事件 | 载荷 | 阶段 |
|------|------|------|
| `ai_stream` | `{ turn_id, kind: "token", text }` / `{ turn_id, kind: "thinking", text }` / `{ turn_id, kind: "tool_call", tool, args }` | P1 |
| `ai_tool_result` | `{ turn_id, tool, xml?, operations?, version_id?, page_name? }` | P1 |
| `ai_turn_done` | `{ turn_id, version_id?, vlm: "pass"\|"warn"\|"skipped", warnings? }` | P2 |
| `ai_turn_error` | `{ turn_id, code, message, retryable }` | P1 |
| `version_new` | `{ session_id, version_id, thumb_data_uri, diff: { added, removed, changed }, touched_ids }` | P2 |
| `export_progress` | `{ job_id, pct, stage }` | P0 |
| `sync_progress` | `{ report_id, upstream, stage: "fetch"\|"apply-l1"\|"apply-l2"\|"report-l3" }` | P4 |
| `sync_done` | `{ report_id, report: SyncReport }` | P4 |

## 3. iframe postMessage 协议（宿主 ↔ draw.io）

`JSON` 序列化（`proto=json`）；消息形如 `{ action, ...payload }`；**双向均校验 origin**（白名单：dev `http://localhost:<vite_port>`、prod `tauri://localhost`；webapp 在前端 public/ 内、与主页面同源，ADR-1——校验为双保险）。

### 3.1 宿主 → drawio
| action | payload | 说明 |
|--------|---------|------|
| `load` | `{ xml, name? }` | 加载；失败时 drawio 回 `error` |
| `export` | `{ format: "svg"\|"png"\|"xml"\|"pdf", scale?, border?, background? }` | 结果经 `export` 事件回（dataURI） |
| `autosave` | `{ interval_ms }` | 开启周期 `save` 回传 |
| `configure` | `{ theme?: "light"\|"dark", lang? }` | 主题/语言 |

### 3.2 drawio → 宿主
| action | payload | 说明 |
|--------|---------|------|
| `init` | `{ version }` | 就绪（bridge 在此之前挂起 load） |
| `save` | `{ xml, name, autorouted }` | autosave/保存内容 |
| `export` | `{ data, format }` | 导出结果 |
| `error` | `{ message }` | 加载/导出错误 |
| `message` | `{ text }` | 一般通知 |

> 注：以上为 draw.io 官方 embed 协议在用集合（参照 next-ai-draw-io `drawio-frame.tsx` 与 draw.io embed 文档）；实施 Step 2 用实际 webview 抓取校准并**冻结**为本表。

## 4. LLM 工具 Schema（下发给模型的 JSON Schema）

```jsonc
// display_diagram
{ "name": "display_diagram",
  "description": "Render a new diagram. Provide <mxStyle> defs + sibling <mxCell> elements only; the file wrapper, root cells, and geometry defaults are completed by the system.",
  "parameters": { "type":"object", "properties": { "xml": { "type":"string" } }, "required":["xml"] } }

// edit_diagram
{ "name": "edit_diagram",
  "parameters": { "type":"object",
    "properties": { "operations": { "type":"array",
      "items": { "type":"object",
        "properties": {
          "operation": { "enum":["update","add","delete"] },
          "cell_id": { "type":"string" },
          "new_xml": { "type":"string", "description":"required for update/add" } },
        "required":["operation","cell_id"] } } },
    "required":["operations"] } }

// append_diagram
{ "name":"append_diagram",
  "description":"Continue a truncated display_diagram output.",
  "parameters":{ "type":"object","properties":{"xml":{"type":"string"}},"required":["xml"] } }

// get_shape_library
{ "name":"get_shape_library",
  "description":"Fetch a shape/icon library reference (AWS, Azure, GCP, K8s, ...). Never guess shape ids.",
  "parameters":{ "type":"object","properties":{"library":{"type":"string"}},"required":["library"] } }
```

> 字段名/描述与上游 `lib/chat-tools.ts:19-135` 对齐；提示词原文引用这些工具名（03.1 §2 的成对同步约束）。

## 5. 同步清单与报告格式

### 5.1 `upstream.toml`（仓库根）
```toml
[upstream.drawio]
remote = "https://github.com/jgraph/drawio.git"
branch = "dev"
pinned_commit = "9034053fb..."          # 当前 vendor 所用 commit
tier = "vendor"
source_path = "src/main/webapp"          # 上游内子路径
target_path = "public/drawio"        # drawio-rust 内目标（同源伺服，ADR-3）
note = "编辑器内核；整目录换版"

[upstream.next-ai-draw-io]
remote = "https://github.com/DayuanJiang/next-ai-draw-io.git"
branch = "main"
pinned_commit = "05adbe5..."
tier = "l2+l3"                           # 含 L2 资产 + L3 逻辑

[[upstream.next-ai-draw-io.assets]]      # L2：文本资源（hash 比对覆盖）
source = "lib/system-prompts.ts"         # 由 extract 规则生成 md（见 06 §4.2）
target = "src-tauri/assets/prompt/system-prompt.md"
[[upstream.next-ai-draw-io.assets]]
source_glob = "docs/shape-libraries/*.md"
target_dir = "src-tauri/assets/prompt/shape-libraries"
[[upstream.next-ai-draw-io.assets]]
source = "lib/model-catalog.json"
target = "src-tauri/assets/prompt/model-catalog.json"

[[upstream.next-ai-draw-io.logic]]       # L3：逻辑映射（变更→再移植报告）
source_glob = "packages/mcp-server/src/{new-diagram,xml-validation,compact-cells,style-classes,xml-attributes,edit-diagram,pages,embedded-images}.ts"
maps_to = "src-tauri/src/ai/"
[[upstream.next-ai-draw-io.logic]]
source_glob = "lib/{diagram-diff,diagram-pages}.ts"
maps_to = "src-tauri/src/doc/"

[upstream.drawio-to-visio]
remote = "https://github.com/PanJitao/drawio-to-visio.git"
branch = "main"
pinned_commit = "eed7363..."
tier = "logic"
[[upstream.drawio-to-visio.logic]]
source_glob = "drawio-to-visio/scripts/fix_svg_for_visio.py"
maps_to = "src-tauri/src/visio/"
```

### 5.2 `SyncReport`（事件 `sync_done` / 报告文件 `sync/reports/<ts>.md`）
```
{ report_id,
  upstreams: [{
    id, pinned_before, pinned_after, new_commits: [sha+subject],
    applied:   [ { tier:"L1"|"L2", path, action:"copy"|"overwrite", hash_before?, hash_after? } ],
    flagged:   [ { tier:"L3", source_file, maps_to, diff_stat, commit_shas, note } ],   # 需人工再移植
    skipped:   [ { path, reason } ],
    tests:     { embed_protocol: "pass"|"fail", upstream_alignment: "pass"|"fail", skipped? }
  }],
  status: "ok" | "needs_review"
}
```

## 6. 数据格式

- **`.drawio`**：`<mxfile>` 文档；明文或 deflate（zip）压缩双格式；`doc/fileio` 读写规则见 03.5 §3。
- **版本快照** `sessions/<id>/versions/<v>/`：`meta.json { id, ts, prompt_digest, page_name, diff:{added,removed,changed}, touched_ids[] }` + `page.xml` + `thumb.png`。
- **会话消息** `sessions/<id>/messages.jsonl`：逐消息追加 `{ ts, role, content, tool_calls?, version_id? }`（崩溃恢复用，03.5 §6）。
