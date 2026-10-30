# 07.3 · 调研：drawio-to-visio（PanJitao/drawio-to-visio）

> **状态**：已完成 ｜ **调研时点**：2026-10-29 ｜ **上游**：`github.com/PanJitao/drawio-to-visio` branch `main` @ `eed7363`

---

## 1. 项目形态

极小型项目（4 个跟踪文件）：`LICENSE`、`README.md`、`drawio-to-visio/SKILL.md`（AI 技能说明）、`drawio-to-visio/scripts/fix_svg_for_visio.py`（**232 行，唯一功能代码**）。形态是一个「AI 技能 + 脚本」，不是应用。

## 2. 解决的问题

draw.io 导出的 SVG 用 `<foreignObject>` 内嵌 HTML 渲染文本；Visio 对 `foreignObject` 支持有限 → **文字位置全部偏移、文本无法编辑**。解决：把 `<switch>` 块中的 `foreignObject` 文本转换为标准 SVG `<text>`，保留字体、字号、颜色、位置、对齐。

## 3. 使用流程（SKILL.md）

1. 定位 draw.io CLI（自动检测常见安装路径，找不到则询问用户）；
2. `draw.io --export --format svg --output out.svg in.drawio` 导出 SVG；
3. `python fix_svg_for_visio.py in.svg out-fixed.svg` 修复；
4. 指导用户 Visio 导入：文件→打开→转换对话框保持默认→导入后为组合对象，`Ctrl+Shift+U` 取消组合后可单独编辑元素；
5. 仍有问题时的退路：**PNG 底图法**（导出 PNG 作底图 + 手动文本框）或 **Visio 重绘**。

## 4. 脚本算法（`fix_svg_for_visio.py` 全文分析）

**依赖**：`lxml`（选择 lxml 是为标准库 xml.etree 的 UTF-8 中文问题）。

| 函数 | 行号 | 逻辑 |
|------|------|------|
| `parse_color(raw)` | 19-31 | `light-dark(#a, #b)` → 取第一个 `#hex`；普通 `#hex` 原样；兜底 `#000000` |
| `extract_text_info(switch_el, nsmap)` | 34-142 | 找 `switch//foreignObject` → 外层 `div`（xhtml ns）style 正则提取 `padding-top`→y、`margin-left`→x_offset、`width`→width_val；最内层含文本 div（跳过 `font-size: 0` 中间层）style 提取 font-size（默认 12px）/font-family（逗号前第一个，默认 Helvetica）/font-weight/color/text-align（默认 center）；文本 = itertext 去空白；锚点：left→start、right→end、center→middle；x：middle 且 width>0 → margin_left+width/2，end 且 width>0 → margin_left+width，否则 margin_left |
| `fix_svg(input, output)` | 145-206 | 遍历全部 `<svg:switch>`；对每个成功提取的 → 父节点下建 `<text x y font-size font-family font-weight fill text-anchor pointer-events=none dominant-baseline=hanging>` → 删除原 switch；统计转换数；序列化带 XML 声明 + SVG 1.1 DOCTYPE，UTF-8 落盘 |

## 5. 已知限制（SKILL.md 明示）

- 复杂文本布局（**多行、自动换行**）可能无法完美还原；
- 部分 CSS 样式（如 `light-dark()`）被简化（按取第一个颜色处理）。

## 6. 可移植性判断

- **全部逻辑 = 纯 XML/字符串处理**，无浏览器、无状态 → **100% 可平移 Rust**（quick-xml + regex，Rust 原生 UTF-8 使 lxml 的动机消失）；
- 算法规模小（232 行）→ 移植 + 双跑对齐测试（Python 原版 vs Rust 输出，text 节点属性级 diff）成本低、风险低；
- 「draw.io CLI 导出 SVG」环节在 drawio-rust 中由 **iframe bridge.export("svg")** 替代（无需 CLI/安装路径检测——桌面应用自带画布）。

## 7. 对 drawio-rust 的结论

1. 移植为 `drawio-core::visio::converter`（ADR-4），管线 = iframe 导出 SVG → 转换 → 落盘（03.3）；
2. 同步层级 = **L3**（脚本变更 → 再移植报告，映射到 `visio/converter.rs`）；
3. SKILL.md 的「Visio 导入引导/取消组合/降级方案」是产品文案与流程知识，**原样继承**到导出完成提示（03.3 §4）；
4. PNG 底图降级法列入 v1.x 增强（Q3）。
