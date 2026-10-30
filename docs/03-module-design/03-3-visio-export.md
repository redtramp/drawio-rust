# 03.3 · Visio 导出模块设计

> **状态**：已定稿 ｜ **最后更新**：2026-10-29 ｜ **上游依据**：07.3（drawio-to-visio 调研，`fix_svg_for_visio.py` 全文 232 行）

---

## 1. 问题背景（继承自上游）

draw.io 导出的 SVG 用 `<foreignObject>` 内嵌 HTML 渲染文本；Visio 对 `foreignObject` 支持有限 → **文字位置全部偏移、文本不可编辑**。解决方案：把 `<switch>` 块中的 `foreignObject` 文本转换为标准 SVG `<text>` 元素，保留字体、字号、颜色、位置、对齐（07.3）。

## 2. 模块职责（`src-tauri/src/visio/converter.rs`）

`pub fn fix_svg_for_visio(input_svg: &str) -> Result<FixedSvg, VisioError>`
- 纯函数，无 I/O → 100% 单测；
- I/O 编排（取 SVG → 转换 → 落盘）在 `export/service.rs`。

## 3. 转换算法（逐段对齐 Python 原实现）

### 3.1 解析
- quick-xml 解析 SVG（保留命名空间；上游用 lxml 是为 UTF-8/中文，Rust 无此问题，天然解决）；
- 遍历全部 `<svg:switch>` 元素（上游 `root.findall('.//{svg}switch')`）。

### 3.2 每个 switch 的提取（对齐 `extract_text_info`，py:34-142）
1. 找 `switch//foreignObject`（svg 命名空间）→ 无则跳过；
2. 找 `foreignObject//div`（xhtml 命名空间，外层 div），取其 `style` 属性，正则提取：
   - `padding-top: Npx` → `y`
   - `margin-left: Npx` → `x_offset`
   - `width: Npx` → `width_val`
3. 找最内层含文本的 div（跳过 `font-size: 0` 的中间层）；
4. 从内层 div 的 style 正则提取：`font-size`（默认 12px）、`font-family`（取逗号前第一个，默认 Helvetica）、`font-weight`、`color`、`text-align`（默认 center）；
5. **颜色解析**（对齐 `parse_color`，py:19-31）：`light-dark(#a, #b)` → 取第一个参数；普通 `#hex` → 原样；兜底 `#000000`；
6. 文本内容 = 内层 div 全部文本（`itertext()` 语义）去首尾空白；空则跳过；
7. **锚点与坐标**（对齐 py:116-131）：
   - `text-align: left → text-anchor=start`；`right → end`；`center → middle`；
   - `x`：middle 且 width>0 → `margin_left + width/2`；end 且 width>0 → `margin_left + width`；否则 `margin_left`；
   - `y = padding_top`。

### 3.3 替换（对齐 `fix_svg`，py:145-206）
- 在 switch 的父节点下新建 `<text>`：`x, y, font-size, font-family, font-weight, fill, text-anchor`，并固定 `pointer-events="none"`、`dominant-baseline="hanging"`（帮助 Visio 定位）；
- 删除原 `<switch>`；
- 统计转换数量（供日志/报告）。

### 3.4 输出
- 序列化回 SVG（保留 XML 声明 + `<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" ...>`，对齐上游）；
- 返回转换计数 + 输出文本。

## 4. 导出管线（`export/service.rs`）

```
export_file({ format: "visio-svg", path })
  → 向 iframe bridge.export("svg") 取 dataURI（复用 03.2 §6）
  → decode → converter::fix_svg_for_visio
  → 写盘（UTF-8）
  → 返回 { path, converted_count }
```

UI 侧：导出完成提示转换的文本元素数；附「如何导入 Visio」引导文案（对齐上游 SKILL.md 步骤 4：文件→打开→转换对话框保持默认→取消组合 `Ctrl+Shift+U` 后可单独编辑元素）。

> **复用**：PDF 主路径（02 ADR-9）同样调用本转换器（SVG → fix → resvg → printpdf）——converter 在 S4 随 PDF 一并实现（见 05 S4/S9）。

## 5. 已知限制（继承上游，需 UI 提示）

- 复杂文本布局（**多行、自动换行**）无法完美还原（上游 SKILL.md「已知限制」）；
- 部分 CSS 函数（如 `light-dark()`）按规则简化；
- **降级方案（Q3，v1.x）**：PNG 底图法（导出 PNG 作底图 + 文本层）或引导 Visio 重绘——上游已给出的两条退路。

## 6. 测试要求

| 用例组 | 说明 |
|--------|------|
| 基础 | 单 switch 单文本 → text 属性逐字段断言（x/y/anchor/font/fill） |
| 对齐 | left/center/right 三种 text-anchor 与 x 计算 |
| light-dark | `light-dark(#111,#222)` → `#111`；纯 hex；缺失 → 兜底 |
| 结构变体 | 无 foreignObject 的 switch 跳过；font-size:0 中间层；嵌套 div 层级差异 |
| 中文/UTF-8 | 含中文文本完整保留（回归上游 lxml 动机） |
| 端到端 | 用 draw.io 真实导出 SVG 样例（fixtures 从 sample 中导出保存）→ Visio 人工验收（P3 退出标准） |
| 上游对齐 | 同一 fixture 分别跑 Python 原版与 Rust 版，输出 text 节点集合一致（属性级 diff，忽略序列化格式差异） |
