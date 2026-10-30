//! cell 级差异（上游 `lib/diagram-diff.ts`）。
//!
//! 提供单页 diff（新增/删除/变更 + 高亮 id）、标签提取，以及"两个文档是否相同"
//! （版本卡/一步提交的判据）。

use std::collections::{HashMap, HashSet};

use super::pages::{file_vars, model_of_page, page_element, parse_xml};
use super::xml_tree::XElement;

/// 单页变更摘要。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSummary {
    pub shapes_added: usize,
    pub shapes_removed: usize,
    pub shapes_changed: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
    pub edges_changed: usize,
}

/// 页 diff。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramDiff {
    pub summary: ChangeSummary,
    pub touched_ids: Vec<String>,
    pub from_scratch: bool,
}

#[derive(Debug, Clone)]
struct CellInfo {
    is_edge: bool,
    signature: String,
}

#[derive(Debug, Clone)]
struct CellRef {
    id: String,
    cell: XElement,
    node: XElement,
}

const DEFAULT_VALUES: &[(&str, &[(&str, &str)])] = &[
    (
        "mxCell",
        &[
            ("vertex", "0"),
            ("edge", "0"),
            ("connectable", "1"),
            ("visible", "1"),
            ("collapsed", "0"),
            ("style", ""),
        ],
    ),
    (
        "mxGeometry",
        &[
            ("x", "0"),
            ("y", "0"),
            ("width", "0"),
            ("height", "0"),
            ("relative", "0"),
        ],
    ),
    ("mxPoint", &[("x", "0"), ("y", "0")]),
];

fn is_geometry(name: &str) -> bool {
    matches!(name, "mxGeometry" | "mxPoint" | "mxRectangle")
}

fn is_number(name: &str) -> bool {
    matches!(name, "x" | "y" | "width" | "height")
}

fn page_model(xml: &str, page_id: Option<&str>) -> Option<XElement> {
    let doc = parse_xml(xml)?;
    if let Some(diagram) = page_element(&doc, page_id) {
        return model_of_page(diagram);
    }
    if let Some(model) = doc.by_name("mxGraphModel").first() {
        Some((*model).clone())
    } else {
        Some(doc)
    }
}

/// style 按 draw.io 读取方式归一化：空 token 去掉，前导 ";" 保留。
fn normal_style(style: &str) -> String {
    let entries: Vec<&str> = style.split(';').filter(|e| !e.trim().is_empty()).collect();
    let prefix = if style.starts_with(';') { ";" } else { "" };
    format!("{prefix}{}", entries.join(";"))
}

fn format_number(n: f64) -> String {
    // JS String(number)：整数不带小数。Rust f64 Display 对整数同样。
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// 一个 cell 的签名（与书写方式无关：属性顺序无关、默认值省略、style 保序）。
fn signature_of(node: &XElement) -> String {
    let defaults: HashMap<&str, &str> = DEFAULT_VALUES
        .iter()
        .find(|(n, _)| *n == node.name)
        .map(|(_, entries)| entries.iter().copied().collect())
        .unwrap_or_default();
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (name, value) in &node.attrs {
        let mut v = value.clone();
        if name == "style" && node.name == "mxCell" {
            v = normal_style(&v);
        } else if is_geometry(&node.name) && is_number(name) && !v.trim().is_empty() {
            if let Ok(n) = v.parse::<f64>() {
                if n.is_finite() {
                    v = format_number(n);
                }
            }
        }
        if defaults.get(name.as_str()).copied() == Some(v.as_str()) {
            continue;
        }
        attrs.push((name.clone(), v));
    }
    attrs.sort();
    let children: Vec<String> = node
        .child_elements()
        .iter()
        .map(|c| signature_of(c))
        .collect();
    serde_json::to_string(&(node.name.clone(), attrs, children)).expect("json")
}

/// 深度优先收集所有 mxCell，记录其 id / cell / node（wrapper 或自身）。
fn walk_cells(el: &XElement, parent: Option<&XElement>, out: &mut Vec<CellRef>) {
    if el.name == "mxCell" {
        let wrapped = matches!(parent, Some(w) if w.name == "object" || w.name == "UserObject");
        let id = el.attr("id").or_else(|| parent.and_then(|w| w.attr("id")));
        if let Some(id) = id {
            if !id.is_empty() {
                let node = if wrapped {
                    parent.expect("wrapper").clone()
                } else {
                    el.clone()
                };
                out.push(CellRef {
                    id: id.to_string(),
                    cell: el.clone(),
                    node,
                });
            }
        }
    }
    for child in el.child_elements() {
        walk_cells(child, Some(el), out);
    }
}

fn collect_cells(xml: &str, page_id: Option<&str>) -> HashMap<String, CellInfo> {
    let Some(model) = page_model(xml, page_id) else {
        return HashMap::new();
    };
    let mut refs = Vec::new();
    walk_cells(&model, None, &mut refs);
    let mut cells = HashMap::new();
    for r in refs {
        if r.id == "0" || r.cell.attr("parent") == Some("0") {
            continue;
        }
        cells.insert(
            r.id,
            CellInfo {
                is_edge: r.cell.attr("edge") == Some("1"),
                signature: signature_of(&r.node),
            },
        );
    }
    cells
}

fn lacks_page(xml: &str, page_id: &str) -> bool {
    let Some(doc) = parse_xml(xml) else {
        return false;
    };
    let diagrams = doc.by_name("diagram");
    !diagrams.is_empty() && !diagrams.iter().any(|d| d.attr("id") == Some(page_id))
}

/// 单页 diff（无 id 时取第一页；新页视为全部新增）。
pub fn diff_diagrams(before: &str, after: &str, page_id: Option<&str>) -> DiagramDiff {
    let is_new_page = page_id
        .map(|pid| lacks_page(before, pid) && !lacks_page(after, pid))
        .unwrap_or(false);
    let before_cells = if is_new_page {
        HashMap::new()
    } else {
        collect_cells(before, page_id)
    };
    let after_cells = collect_cells(after, page_id);

    let mut summary = ChangeSummary::default();
    let mut touched: Vec<String> = Vec::new();

    for (id, cell) in &after_cells {
        match before_cells.get(id) {
            None => {
                touched.push(id.clone());
                if cell.is_edge {
                    summary.edges_added += 1;
                } else {
                    summary.shapes_added += 1;
                }
            }
            Some(old) if old.signature != cell.signature => {
                touched.push(id.clone());
                if cell.is_edge {
                    summary.edges_changed += 1;
                } else {
                    summary.shapes_changed += 1;
                }
            }
            _ => {}
        }
    }
    for (id, cell) in &before_cells {
        if after_cells.contains_key(id) {
            continue;
        }
        if cell.is_edge {
            summary.edges_removed += 1;
        } else {
            summary.shapes_removed += 1;
        }
    }
    DiagramDiff {
        summary,
        touched_ids: touched,
        from_scratch: before_cells.is_empty(),
    }
}

/// 标签转纯文本（HTML 剥离、长度截断）。
pub fn plain_label(text: &str, max: usize) -> String {
    static BR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static TAG: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static WS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let br = BR.get_or_init(|| regex::Regex::new(r"(?i)<br\s*/?>").expect("br"));
    let tag = TAG.get_or_init(|| regex::Regex::new(r"<[^>]+>").expect("tag"));
    let ws = WS.get_or_init(|| regex::Regex::new(r"\s+").expect("ws"));
    let mut plain = br.replace_all(text, " ").to_string();
    plain = tag.replace_all(&plain, "").to_string();
    plain = plain
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    plain = ws.replace_all(&plain, " ").trim().to_string();
    if plain.chars().count() > max {
        let cut: String = plain.chars().take(max).collect();
        format!("{cut}…")
    } else {
        plain
    }
}

/// 指定 id 的 shape 标签（按 id 顺序），跳过边及其标签、无标签 shape。
pub fn cell_labels(xml: &str, ids: &[String], page_id: Option<&str>) -> Vec<String> {
    let Some(model) = page_model(xml, page_id) else {
        return Vec::new();
    };
    let mut refs = Vec::new();
    walk_cells(&model, None, &mut refs);
    let edges: HashSet<String> = refs
        .iter()
        .filter(|r| r.cell.attr("edge") == Some("1"))
        .map(|r| r.id.clone())
        .collect();
    let mut labels: HashMap<String, String> = HashMap::new();
    for r in &refs {
        let parent = r.cell.attr("parent").unwrap_or("");
        if edges.contains(&r.id) || edges.contains(parent) {
            continue;
        }
        let value = if r.cell.name == "mxCell" && r.node.name == "mxCell" {
            r.cell.attr("value")
        } else {
            r.node.attr("label")
        };
        let label = plain_label(value.unwrap_or(""), 40);
        if !label.is_empty() {
            labels.insert(r.id.clone(), label);
        }
    }
    ids.iter()
        .filter_map(|id| labels.get(id).cloned())
        .collect()
}

// ============================================================================
// 文档等同性（isSameDocument）
// ============================================================================

const VIEW_ATTRIBUTES: [&str; 2] = ["dx", "dy"];

fn page_defaults(name: &str) -> &'static [&'static str] {
    match name {
        "background" => &["none"],
        "backgroundImage" | "extFonts" => &[],
        "math" | "shadow" | "page" | "pageScale" | "grid" | "guides" | "tooltips" | "connect"
        | "arrows" | "fold" => &["1", "0", "10"],
        "gridSize" => &["10"],
        _ => &[],
    }
}

const PAGE_NUMBERS: [&str; 4] = ["gridSize", "pageScale", "pageWidth", "pageHeight"];

fn same_page_settings(a: &XElement, b: &XElement) -> bool {
    let mut names: HashSet<&str> = HashSet::new();
    for (n, _) in &a.attrs {
        names.insert(n);
    }
    for (n, _) in &b.attrs {
        names.insert(n);
    }
    let read = |page: &XElement, name: &str| -> Option<String> {
        let value = page.attr(name).map(|s| s.to_string());
        match value {
            Some(v) if PAGE_NUMBERS.contains(&name) && !v.trim().is_empty() => {
                v.parse::<f64>().ok().map(format_number).or(Some(v))
            }
            other => other,
        }
    };
    for name in names {
        if VIEW_ATTRIBUTES.contains(&name) {
            continue;
        }
        let va = read(a, name);
        let vb = read(b, name);
        if let (Some(x), Some(y)) = (&va, &vb) {
            if x != y {
                return false;
            }
            continue;
        }
        let defaults = page_defaults(name);
        if !defaults.is_empty() {
            let actual = va.as_deref().or(vb.as_deref()).unwrap_or("");
            if !defaults.contains(&actual) {
                return false;
            }
        }
    }
    true
}

fn content_of(model: &XElement) -> (HashMap<String, String>, HashMap<String, Vec<String>>) {
    let mut refs = Vec::new();
    walk_cells(model, None, &mut refs);
    let mut cells: HashMap<String, String> = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for r in refs {
        cells.insert(r.id.clone(), signature_of(&r.node));
        let parent = r.cell.attr("parent").unwrap_or("").to_string();
        children.entry(parent).or_default().push(r.id);
    }
    (cells, children)
}

fn same_content(a: &XElement, b: &XElement) -> bool {
    let (ca_cells, ca_children) = content_of(a);
    let (cb_cells, cb_children) = content_of(b);
    if ca_cells.len() != cb_cells.len() {
        return false;
    }
    for (id, sig) in &ca_cells {
        if cb_cells.get(id) != Some(sig) {
            return false;
        }
    }
    for (parent, ids) in &ca_children {
        match cb_children.get(parent) {
            Some(other) if other.join("\n") == ids.join("\n") => {}
            _ => return false,
        }
    }
    true
}

/// 文件变量（%name%）按名字/值集合比较，顺序无关；none 等同 "{}"。
pub fn same_file_vars(a: Option<&str>, b: Option<&str>) -> bool {
    if a == b {
        return true;
    }
    let sorted = |vars: Option<&str>| -> Option<String> {
        let obj: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(vars.unwrap_or("{}")).ok()?;
        let mut pairs: Vec<(String, serde_json::Value)> = obj.into_iter().collect();
        pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
        serde_json::to_string(&pairs).ok()
    };
    match (sorted(a), sorted(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// 两个文档是否持有相同的页（名、cell、顺序）、页设置与文件变量。
pub fn is_same_document(a: &str, b: &str) -> bool {
    let pages_of = |doc: &XElement| -> Vec<(Option<String>, Option<XElement>)> {
        let diagrams = doc.by_name("diagram");
        if diagrams.is_empty() {
            let model = doc.by_name("mxGraphModel").first().cloned();
            return vec![(None, model.cloned())];
        }
        diagrams
            .iter()
            .map(|d| (d.attr("name").map(|s| s.to_string()), model_of_page(d)))
            .collect()
    };
    let Some(doc_a) = parse_xml(a) else {
        return a == b;
    };
    let Some(doc_b) = parse_xml(b) else {
        return a == b;
    };
    if !same_file_vars(file_vars(&doc_a).as_deref(), file_vars(&doc_b).as_deref()) {
        return false;
    }
    let pages_a = pages_of(&doc_a);
    let pages_b = pages_of(&doc_b);
    if pages_a.len() != pages_b.len() {
        return false;
    }
    pages_a.iter().enumerate().all(|(i, (name_a, model_a))| {
        let (name_b, model_b) = &pages_b[i];
        if let (Some(na), Some(nb)) = (name_a, name_b) {
            if na != nb {
                return false;
            }
        }
        if let (Some(ma), Some(mb)) = (model_a, model_b) {
            if !same_page_settings(ma, mb) {
                return false;
            }
        }
        match (model_a, model_b) {
            (Some(ma), Some(mb)) => same_content(ma, mb),
            _ => model_a.is_none() && model_b.is_none(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="A" vertex="1" parent="1"><mxGeometry x="40" y="40" width="120" height="60" as="geometry"/></mxCell></root></mxGraphModel></diagram></mxfile>"#;

    #[test]
    fn diffs_added_and_removed() {
        let after = DOC.replace("</root>", r#"<mxCell id="3" value="B" vertex="1" parent="1"><mxGeometry x="200" y="40" width="120" height="60" as="geometry"/></mxCell></root>"#);
        let d = diff_diagrams(DOC, &after, Some("p1"));
        assert_eq!(d.summary.shapes_added, 1);
        assert!(d.touched_ids.contains(&"3".to_string()));
    }

    #[test]
    fn from_scratch_on_empty() {
        let blank = r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/></root></mxGraphModel></diagram></mxfile>"#;
        let d = diff_diagrams(blank, DOC, Some("p1"));
        assert!(d.from_scratch);
        assert_eq!(d.summary.shapes_added, 1);
    }

    #[test]
    fn same_document_ignores_attr_order() {
        let reordered = DOC.replace(r#"vertex="1" parent="1""#, r#"parent="1" vertex="1""#);
        assert!(is_same_document(DOC, &reordered));
    }

    #[test]
    fn plain_label_strips_html() {
        assert_eq!(plain_label("<b>Hello</b><br>World", 80), "Hello World");
        assert_eq!(
            plain_label("a very long label that should be cut", 10),
            "a very lon…"
        );
    }

    #[test]
    fn normal_style_drops_empty_tokens() {
        assert_eq!(
            normal_style("rounded=1;;fontStyle=1;"),
            "rounded=1;fontStyle=1"
        );
        assert_eq!(normal_style(";rounded=1"), ";rounded=1");
    }

    #[test]
    fn cell_labels_skip_edges_and_edges_of() {
        let xml = r#"<mxfile><diagram id="p1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="Alpha" vertex="1" parent="1"/><mxCell id="b" value="Beta" vertex="1" parent="1"/><mxCell id="e" value="edge" edge="1" parent="1" source="a" target="b"/><mxCell id="lbl" value="edge-label" vertex="1" parent="e"/></root></mxGraphModel></diagram></mxfile>"#;
        let labels = cell_labels(
            xml,
            &["a".into(), "b".into(), "e".into(), "lbl".into()],
            Some("p1"),
        );
        assert_eq!(labels, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn geometry_numbers_normalized_in_signature() {
        // x=40 vs x=40.0 视为同一 cell（几何数字归一化）
        let a = r#"<mxfile><diagram id="p1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="c" vertex="1" parent="1"><mxGeometry x="40" y="40" width="120" height="60" as="geometry"/></mxCell></root></mxGraphModel></diagram></mxfile>"#;
        let b = a.replace(r#"x="40""#, r#"x="40.0""#);
        assert!(is_same_document(a, &b));
    }

    #[test]
    fn same_file_vars_order_independent() {
        assert!(same_file_vars(
            Some(r#"{"a":1,"b":2}"#),
            Some(r#"{"b":2,"a":1}"#)
        ));
        assert!(!same_file_vars(Some(r#"{"a":1}"#), Some(r#"{"a":2}"#)));
        // none 等同空对象
        assert!(same_file_vars(None, Some("{}")));
        assert!(same_file_vars(None, None));
        // 相同字符串直接相等（含非法 JSON）
        assert!(same_file_vars(Some("not json"), Some("not json")));
    }

    #[test]
    fn same_document_detects_content_change() {
        let changed = DOC.replace(r#"value="A""#, r#"value="Z""#);
        assert!(!is_same_document(DOC, &changed));
        // 不同页数不相等
        let extra = DOC.replace(
            "</root>",
            r#"<mxCell id="x" value="X" vertex="1" parent="1"/></root>"#,
        );
        assert!(!is_same_document(DOC, &extra));
    }

    #[test]
    fn diff_counts_edges_and_removals() {
        let before = r#"<mxfile><diagram id="p1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="A" vertex="1" parent="1"/><mxCell id="b" value="B" vertex="1" parent="1"/><mxCell id="e" edge="1" parent="1" source="a" target="b"/></root></mxGraphModel></diagram></mxfile>"#;
        let after = r#"<mxfile><diagram id="p1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="A2" vertex="1" parent="1"/><mxCell id="e2" edge="1" parent="1" source="a" target="b"/></root></mxGraphModel></diagram></mxfile>"#;
        let d = diff_diagrams(before, after, Some("p1"));
        assert_eq!(d.summary.shapes_changed, 1);
        assert_eq!(d.summary.shapes_removed, 1); // b 删除
        assert_eq!(d.summary.edges_removed, 1); // e 删除
        assert_eq!(d.summary.edges_added, 1); // e2 新增
        assert!(!d.from_scratch);
    }
}
