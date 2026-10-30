//! ID 级编辑（上游 `edit-diagram.ts` + `diagram-operations.ts`）。
//!
//! 全有或全无：任一操作失败则整体不写入，把所有失败报给模型。级联删除：
//! 删除容器时自动删除其子与相连边（及其边标签）。

use std::collections::{HashMap, HashSet};

use regex::Regex;

use super::compact_cells::{default_layer_of, expand_compact_cells};
use super::embedded_images::ImageStore;
use super::pages::{
    decompress_page_content, find_page_element, has_page_selector, project_page, PageProjection,
    PageSelector,
};
use super::style_classes::{add_default_styles, edge_ids_of};
use super::xml_syntax::get_xml_syntax_error;
use super::xml_tree::{parse_strict, serialize, XElement, XmlNode};
use super::xml_validation::{validate_and_fix_xml, validate_mx_cell_structure};

/// 单个编辑操作。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiagramOperation {
    pub operation: String, // "update" | "add" | "delete"
    pub cell_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_xml: Option<String>,
}

/// 操作错误。
#[derive(Debug, Clone)]
pub struct OperationError {
    pub kind: String,
    pub cell_id: String,
    pub message: String,
}

/// 应用结果。
#[derive(Debug, Clone)]
pub struct ApplyOperationsResult {
    pub result: String,
    pub errors: Vec<OperationError>,
    pub unchanged: Vec<String>,
}

/// 编辑结果。
#[derive(Debug, Clone)]
pub enum EditOutcome {
    Ok {
        xml: String,
        applied: usize,
        fixes: Vec<String>,
        unchanged: Vec<String>,
    },
    Err {
        errors: Vec<String>,
        page_error: bool,
    },
}

const CELL_SELECTOR: [&str; 3] = ["mxCell", "UserObject", "object"];

/// 片段中的顶层元素数（用 parsererror 检测语法错误则返回 1）。
fn count_top_level_elements(fragment: &str) -> usize {
    let wrapped = format!("<wrapper>{fragment}</wrapper>");
    match super::xml_tree::parse_lenient(&wrapped) {
        None => 1,
        Some(doc) if doc.name == "parsererror" => 1,
        Some(doc) => doc.child_elements().len(),
    }
}

/// 目标页投影为单页 mxfile（投影失败则返回原文档）。
pub fn target_page_xml(xml: &str, selector: &PageSelector) -> String {
    match project_page(xml, selector) {
        PageProjection::Ok { xml, .. } => xml,
        _ => xml.to_string(),
    }
}

/// 核心编辑：逐操作准备（校验/展开/补样式），再应用。全有或全无。
pub fn edit_diagram(
    xml: &str,
    operations: &[DiagramOperation],
    selector: &PageSelector,
    images: &mut ImageStore,
) -> EditOutcome {
    let mut errors: Vec<String> = Vec::new();
    let mut fixes: Vec<String> = Vec::new();
    let mut prepared: Vec<DiagramOperation> = Vec::new();

    let page = target_page_xml(xml, selector);
    let edges = edge_ids_of(&page);
    let layer = default_layer_of(&page);
    let mx_style_re = Regex::new(r"(?i)<mxStyle\b").expect("mxstyle");

    for op in operations {
        if op.operation == "delete" || op.new_xml.is_none() {
            prepared.push(op.clone());
            continue;
        }
        let new_xml = op.new_xml.as_deref().expect("checked");
        if mx_style_re.is_match(new_xml) {
            errors.push(format!(
                "{} {}: named styles (mxStyle) are not available in edit_diagram; write the cell's complete style",
                op.operation, op.cell_id
            ));
            continue;
        }
        if count_top_level_elements(new_xml) > 1 {
            errors.push(format!(
                "{} {}: new_xml must contain exactly one cell; use one add operation per cell",
                op.operation, op.cell_id
            ));
            continue;
        }
        let check = validate_and_fix_xml(new_xml, true);
        if !check.valid {
            errors.push(format!(
                "{} {}: invalid new_xml: {}",
                op.operation,
                op.cell_id,
                check.error.unwrap_or_default()
            ));
            continue;
        }
        if let Some(fixed) = &check.fixed {
            fixes.push(format!("{}: {}", op.cell_id, check.fixes.join(", ")));
            let _ = fixed;
        }
        let restored = images.restore_image_data(check.fixed.as_deref().unwrap_or(new_xml), xml);
        let expanded = expand_compact_cells(&restored, &layer);
        let styled = add_default_styles(&expanded, &edges);
        prepared.push(DiagramOperation {
            operation: op.operation.clone(),
            cell_id: op.cell_id.clone(),
            new_xml: Some(styled),
        });
    }

    if !errors.is_empty() {
        return EditOutcome::Err {
            errors,
            page_error: false,
        };
    }

    let applied = apply_diagram_operations(xml, &prepared, selector);
    if let Some(page_level) = applied.errors.iter().find(|e| e.cell_id.is_empty()) {
        return EditOutcome::Err {
            errors: vec![page_level.message.clone()],
            page_error: true,
        };
    }
    if !applied.errors.is_empty() {
        return EditOutcome::Err {
            errors: applied
                .errors
                .iter()
                .map(|e| format!("{} {}: {}", e.kind, e.cell_id, e.message))
                .collect(),
            page_error: false,
        };
    }

    let after = validate_mx_cell_structure(&target_page_xml(&applied.result, selector), true);
    if let Some(after_msg) = after {
        if validate_mx_cell_structure(&target_page_xml(xml, selector), true).is_none() {
            return EditOutcome::Err {
                errors: vec![format!("the edit would make the page invalid: {after_msg}")],
                page_error: false,
            };
        }
    }

    EditOutcome::Ok {
        xml: applied.result,
        applied: operations.len(),
        fixes,
        unchanged: applied.unchanged,
    }
}

// ============================================================================
// 操作应用（diagram-operations.ts）
// ============================================================================

fn has_element(nodes: &[XmlNode], name: &str) -> bool {
    nodes
        .iter()
        .any(|n| matches!(n, XmlNode::Element(el) if el.name == name))
}

fn find_element_mut<'a>(nodes: &'a mut [XmlNode], name: &str) -> Option<&'a mut XElement> {
    nodes.iter_mut().find_map(|n| match n {
        XmlNode::Element(el) if el.name == name => Some(el),
        _ => None,
    })
}

fn get_diagram_mut(doc: &mut XElement, idx: usize) -> Option<&mut XElement> {
    let mut current = 0;
    for child in &mut doc.children {
        if let XmlNode::Element(el) = child {
            if el.name == "diagram" {
                if current == idx {
                    return Some(el);
                }
                current += 1;
            }
        }
    }
    None
}

/// 确保页有 `<root>`：空页建空白模型，压缩页解压。失败返回 None。
fn ensure_page_root(diagram: &mut XElement) -> Option<()> {
    if !diagram.by_name("root").is_empty() {
        return Some(());
    }
    if diagram.by_name("mxGraphModel").is_empty() {
        let text = diagram.text_content().trim().to_string();
        if !text.is_empty() {
            let xml = decompress_page_content(&text)?;
            if get_xml_syntax_error(&xml).is_some() {
                return None;
            }
            let parsed = parse_strict(&xml).ok()?;
            if parsed.name != "mxGraphModel" {
                return None;
            }
            diagram.children.clear();
            diagram.children.push(XmlNode::Element(parsed));
        } else {
            diagram
                .children
                .push(XmlNode::Element(XElement::new("mxGraphModel")));
        }
    }
    let model = find_element_mut(&mut diagram.children, "mxGraphModel")?;
    if has_element(&model.children, "root") {
        return Some(());
    }
    let root = parse_strict(r#"<root><mxCell id="0"/><mxCell id="1" parent="0"/></root>"#).ok()?;
    model.children.push(XmlNode::Element(root));
    Some(())
}

/// cell 的规范化文本形式（属性排序、style 去空 token），用于判断 update 是否无变化。
fn cell_key(el: &XElement) -> String {
    let mut attrs: Vec<String> = el
        .attrs
        .iter()
        .map(|(n, v)| {
            let value = if n == "style" {
                v.split(';')
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(";")
            } else {
                v.clone()
            };
            format!("{n}={}", serde_json::to_string(&value).expect("json value"))
        })
        .collect();
    attrs.sort();
    let children: String = el.child_elements().iter().map(|c| cell_key(c)).collect();
    format!(
        "<{} {}>{}</{}>",
        el.name,
        attrs.join(" "),
        children,
        el.name
    )
}

fn node_id(node: &XmlNode) -> Option<String> {
    match node {
        XmlNode::Element(el) => el
            .attr("id")
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// 读 parent/source/target；wrapper 时读 inner mxCell。
fn cell_attr(node: &XmlNode, name: &str) -> Option<String> {
    let XmlNode::Element(el) = node else {
        return None;
    };
    if el.name == "mxCell" {
        el.attr(name).map(|s| s.to_string())
    } else {
        el.by_name("mxCell")
            .first()
            .and_then(|inner| inner.attr(name))
            .map(|s| s.to_string())
    }
}

fn build_cell_map(root_children: &[XmlNode]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (i, node) in root_children.iter().enumerate() {
        if let Some(id) = node_id(node) {
            map.entry(id).or_insert(i);
        }
    }
    map
}

/// 深度优先找第一个 cell（mxCell/UserObject/object）。
fn find_first_cell(el: &XElement) -> Option<&XElement> {
    if CELL_SELECTOR.contains(&el.name.as_str()) {
        return Some(el);
    }
    for child in el.child_elements() {
        if let Some(found) = find_first_cell(child) {
            return Some(found);
        }
    }
    None
}

/// 把 new_xml 解析为 cell（import 进 doc 语义：返回独立克隆），或错误消息。
fn parse_new_cell(op: &DiagramOperation) -> Result<XElement, String> {
    let new_xml = op
        .new_xml
        .as_deref()
        .ok_or_else(|| format!("new_xml is required for {} operation", op.operation))?;
    let wrapped = format!("<wrapper>{new_xml}</wrapper>");
    if let Some(e) = get_xml_syntax_error(&wrapped) {
        return Err(format!("new_xml is not well-formed XML: {e}"));
    }
    let doc = parse_strict(&wrapped).map_err(|e| e.fmt())?;
    let cell = find_first_cell(&doc).ok_or("new_xml must contain an mxCell element")?;
    let new_cell_id = cell.attr("id").map(|s| s.to_string());
    if new_cell_id.as_deref() != Some(op.cell_id.as_str()) {
        return Err(format!(
            "ID mismatch: cell_id is \"{}\" but new_xml has id=\"{}\"",
            op.cell_id,
            new_cell_id.unwrap_or_default()
        ));
    }
    Ok(cell.clone())
}

fn collect_descendants(
    cell_id: &str,
    cell_map: &HashMap<String, usize>,
    root_children: &[XmlNode],
    deleted: &HashSet<String>,
    out: &mut HashSet<String>,
) {
    if out.contains(cell_id) {
        return;
    }
    out.insert(cell_id.to_string());
    for child_id in cell_map.keys() {
        if child_id == "0" || child_id == "1" {
            continue;
        }
        if deleted.contains(child_id) {
            continue;
        }
        let node = &root_children[cell_map[child_id]];
        if cell_attr(node, "parent").as_deref() == Some(cell_id) {
            collect_descendants(child_id, cell_map, root_children, deleted, out);
        }
    }
}

/// 在 root.children 上处理操作（物理删除推迟到最后）。
fn process_ops(
    root_children: &mut Vec<XmlNode>,
    ops: &[DiagramOperation],
) -> (Vec<OperationError>, Vec<String>) {
    let mut cell_map = build_cell_map(root_children);
    let mut deleted: HashSet<String> = HashSet::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut errors: Vec<OperationError> = Vec::new();

    for op in ops {
        match op.operation.as_str() {
            "update" => {
                let Some(&idx) = cell_map.get(&op.cell_id) else {
                    errors.push(OperationError {
                        kind: "update".into(),
                        cell_id: op.cell_id.clone(),
                        message: format!("Cell with id=\"{}\" not found", op.cell_id),
                    });
                    continue;
                };
                let new_cell = match parse_new_cell(op) {
                    Ok(c) => c,
                    Err(e) => {
                        errors.push(OperationError {
                            kind: "update".into(),
                            cell_id: op.cell_id.clone(),
                            message: e,
                        });
                        continue;
                    }
                };
                if cell_key(&new_cell) == cell_key(&root_children[idx].clone_element()) {
                    unchanged.push(op.cell_id.clone());
                }
                root_children[idx] = XmlNode::Element(new_cell);
            }
            "add" => {
                if cell_map.contains_key(&op.cell_id) {
                    errors.push(OperationError {
                        kind: "add".into(),
                        cell_id: op.cell_id.clone(),
                        message: format!("Cell with id=\"{}\" already exists", op.cell_id),
                    });
                    continue;
                }
                let new_cell = match parse_new_cell(op) {
                    Ok(c) => c,
                    Err(e) => {
                        errors.push(OperationError {
                            kind: "add".into(),
                            cell_id: op.cell_id.clone(),
                            message: e,
                        });
                        continue;
                    }
                };
                root_children.push(XmlNode::Element(new_cell));
                cell_map.insert(op.cell_id.clone(), root_children.len() - 1);
            }
            "delete" => {
                if op.cell_id == "0" || op.cell_id == "1" {
                    errors.push(OperationError {
                        kind: "delete".into(),
                        cell_id: op.cell_id.clone(),
                        message: format!("Cannot delete root cell \"{}\"", op.cell_id),
                    });
                    continue;
                }
                if !cell_map.contains_key(&op.cell_id) || deleted.contains(&op.cell_id) {
                    if !deleted.contains(&op.cell_id) {
                        errors.push(OperationError {
                            kind: "delete".into(),
                            cell_id: op.cell_id.clone(),
                            message: format!("Cell with id=\"{}\" not found", op.cell_id),
                        });
                    }
                    continue;
                }
                let mut to_delete: HashSet<String> = HashSet::new();
                collect_descendants(
                    &op.cell_id,
                    &cell_map,
                    root_children,
                    &deleted,
                    &mut to_delete,
                );
                // 相连边及其子（边标签）
                let snapshot: Vec<String> = to_delete.iter().cloned().collect();
                for cell_id in snapshot {
                    for (edge_id, &edge_idx) in &cell_map {
                        if edge_id == "0" || edge_id == "1" {
                            continue;
                        }
                        if deleted.contains(edge_id) {
                            continue;
                        }
                        let edge = &root_children[edge_idx];
                        let source = cell_attr(edge, "source");
                        let target = cell_attr(edge, "target");
                        if source.as_deref() == Some(cell_id.as_str())
                            || target.as_deref() == Some(cell_id.as_str())
                        {
                            collect_descendants(
                                edge_id,
                                &cell_map,
                                root_children,
                                &deleted,
                                &mut to_delete,
                            );
                        }
                    }
                }
                for id in to_delete {
                    deleted.insert(id);
                }
            }
            other => {
                errors.push(OperationError {
                    kind: other.to_string(),
                    cell_id: op.cell_id.clone(),
                    message: format!("unknown operation \"{other}\""),
                });
            }
        }
    }

    // 物理删除
    root_children.retain(|node| match node_id(node) {
        Some(id) => !deleted.contains(&id),
        None => true,
    });

    (errors, unchanged)
}

/// 应用 ID 级操作，返回（同形状的）结果 XML。
pub fn apply_diagram_operations(
    xml: &str,
    operations: &[DiagramOperation],
    selector: &PageSelector,
) -> ApplyOperationsResult {
    if let Some(e) = get_xml_syntax_error(xml) {
        return ApplyOperationsResult {
            result: xml.to_string(),
            unchanged: Vec::new(),
            errors: vec![OperationError {
                kind: "update".into(),
                cell_id: String::new(),
                message: format!("XML parse error: {e}"),
            }],
        };
    }
    let mut doc = match parse_strict(xml) {
        Ok(d) => d,
        Err(e) => {
            return ApplyOperationsResult {
                result: xml.to_string(),
                unchanged: Vec::new(),
                errors: vec![OperationError {
                    kind: "update".into(),
                    cell_id: String::new(),
                    message: format!("XML parse error: {e}"),
                }],
            };
        }
    };

    // 定位 root
    if doc.name == "mxfile" {
        let idx = find_page_element(&doc, selector).map(|(i, _)| i);
        let Some(idx) = idx else {
            return ApplyOperationsResult {
                result: xml.to_string(),
                unchanged: Vec::new(),
                errors: vec![OperationError {
                    kind: "update".into(),
                    cell_id: String::new(),
                    message: "Page not found in <mxfile>".to_string(),
                }],
            };
        };
        let Some(diagram) = get_diagram_mut(&mut doc, idx) else {
            return ApplyOperationsResult {
                result: xml.to_string(),
                unchanged: Vec::new(),
                errors: vec![OperationError {
                    kind: "update".into(),
                    cell_id: String::new(),
                    message: "Page not found in <mxfile>".to_string(),
                }],
            };
        };
        if ensure_page_root(diagram).is_none() {
            let page_id = diagram.attr("id").unwrap_or("(index)").to_string();
            return ApplyOperationsResult {
                result: xml.to_string(),
                unchanged: Vec::new(),
                errors: vec![OperationError {
                    kind: "update".into(),
                    cell_id: String::new(),
                    message: format!(
                        "Page \"{page_id}\" has no <root> element and its content could not be decompressed"
                    ),
                }],
            };
        }
        let model = find_element_mut(&mut diagram.children, "mxGraphModel").expect("model exists");
        let root = find_element_mut(&mut model.children, "root").expect("root exists");
        let (errors, unchanged) = process_ops(&mut root.children, operations);
        ApplyOperationsResult {
            result: serialize(&doc),
            errors,
            unchanged,
        }
    } else {
        if has_page_selector(selector) {
            return ApplyOperationsResult {
                result: xml.to_string(),
                unchanged: Vec::new(),
                errors: vec![OperationError {
                    kind: "update".into(),
                    cell_id: String::new(),
                    message:
                        "Page selector provided but document is not multi-page (no <mxfile> wrapper). Use create_new_diagram with a full <mxfile> first, or omit the page selector."
                            .to_string(),
                }],
            };
        }
        let root = doc
            .by_name("root")
            .first()
            .map(|_| ())
            .and_then(|_| find_element_mut(&mut doc.children, "root"));
        let Some(root) = root else {
            return ApplyOperationsResult {
                result: xml.to_string(),
                unchanged: Vec::new(),
                errors: vec![OperationError {
                    kind: "update".into(),
                    cell_id: String::new(),
                    message: "Could not find <root> element in XML".to_string(),
                }],
            };
        };
        let (errors, unchanged) = process_ops(&mut root.children, operations);
        ApplyOperationsResult {
            result: serialize(&doc),
            errors,
            unchanged,
        }
    }
}

/// XElement 的 clone_element（借用下安全取克隆）。
trait CloneElement {
    fn clone_element(&self) -> XElement;
}
impl CloneElement for XmlNode {
    fn clone_element(&self) -> XElement {
        match self {
            XmlNode::Element(el) => el.clone(),
            XmlNode::Text(_) => XElement::new("parsererror"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="A" vertex="1" parent="1"><mxGeometry x="40" y="40" width="120" height="60" as="geometry"/></mxCell></root></mxGraphModel></diagram></mxfile>"#.to_string()
    }

    #[test]
    fn adds_cell() {
        let ops = vec![DiagramOperation {
            operation: "add".into(),
            cell_id: "3".into(),
            new_xml: Some(
                r#"<mxCell id="3" value="B" style="rounded=1;" x="200" y="40" w="120" h="60"/>"#
                    .into(),
            ),
        }];
        let r = apply_diagram_operations(&sample(), &ops, &PageSelector::default());
        assert!(r.errors.is_empty());
        assert!(r.result.contains(r#"id="3""#));
        assert!(r.result.contains("vertex=\"1\""));
    }

    #[test]
    fn updates_and_reports_unchanged() {
        let ops = vec![DiagramOperation {
            operation: "update".into(),
            cell_id: "2".into(),
            new_xml: Some(r#"<mxCell id="2" value="A" vertex="1" parent="1"><mxGeometry x="40" y="40" width="120" height="60" as="geometry"/></mxCell>"#.into()),
        }];
        let r = apply_diagram_operations(&sample(), &ops, &PageSelector::default());
        assert!(r.errors.is_empty());
        assert_eq!(r.unchanged, vec!["2"]);
    }

    #[test]
    fn cascade_delete() {
        let xml = r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="A" vertex="1" parent="1"/><mxCell id="3" value="B" vertex="1" parent="2"/><mxCell id="4" edge="1" parent="1" source="3" target="2"/></root></mxGraphModel></diagram></mxfile>"#;
        let ops = vec![DiagramOperation {
            operation: "delete".into(),
            cell_id: "2".into(),
            new_xml: None,
        }];
        let r = apply_diagram_operations(xml, &ops, &PageSelector::default());
        assert!(r.errors.is_empty());
        assert!(!r.result.contains(r#"id="2""#));
        assert!(!r.result.contains(r#"id="3""#)); // child
        assert!(!r.result.contains(r#"id="4""#)); // connected edge
    }

    #[test]
    fn rejects_id_mismatch() {
        let ops = vec![DiagramOperation {
            operation: "update".into(),
            cell_id: "2".into(),
            new_xml: Some(r#"<mxCell id="99" value="X" vertex="1" parent="1"/>"#.into()),
        }];
        let r = apply_diagram_operations(&sample(), &ops, &PageSelector::default());
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].message.contains("ID mismatch"));
    }

    #[test]
    fn update_missing_cell_reports_error() {
        let ops = vec![DiagramOperation {
            operation: "update".into(),
            cell_id: "nope".into(),
            new_xml: Some(r#"<mxCell id="nope" value="X"/>"#.into()),
        }];
        let r = apply_diagram_operations(&sample(), &ops, &PageSelector::default());
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].message.contains("not found"));
    }

    #[test]
    fn add_duplicate_reports_error() {
        let ops = vec![DiagramOperation {
            operation: "add".into(),
            cell_id: "2".into(),
            new_xml: Some(r#"<mxCell id="2" value="B"/>"#.into()),
        }];
        let r = apply_diagram_operations(&sample(), &ops, &PageSelector::default());
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].message.contains("already exists"));
    }

    #[test]
    fn delete_root_cell_reports_error() {
        let ops = vec![DiagramOperation {
            operation: "delete".into(),
            cell_id: "1".into(),
            new_xml: None,
        }];
        let r = apply_diagram_operations(&sample(), &ops, &PageSelector::default());
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].message.contains("root"));
    }

    #[test]
    fn unknown_operation_reports_error() {
        let ops = vec![DiagramOperation {
            operation: "frobnicate".into(),
            cell_id: "2".into(),
            new_xml: None,
        }];
        let r = apply_diagram_operations(&sample(), &ops, &PageSelector::default());
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].message.contains("unknown operation"));
    }
}
