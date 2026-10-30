//! 新图装配（上游 `new-diagram.ts`）。
//!
//! 把模型写出的裸 cell 装配成单页 `<mxfile>`：先取出命名样式，再做保留字
//! 检查、截断检查，包成模型后校验并自动修复，最后展开紧凑 cell、展开命名
//! 样式、补默认样式。

use std::collections::HashSet;

use regex::Regex;

use super::compact_cells::expand_compact_cells;
use super::pages::{
    generate_page_id, has_cells, normalize_to_mxfile, parse_mxfile, serialize_mxfile,
    wrap_cells_in_model,
};
use super::style_classes::{
    add_default_styles, apply_style_classes, read_style_classes, StyleClasses,
};
use super::xml_attributes::read_attributes;
use super::xml_syntax::get_xml_syntax_error;
use super::xml_validation::{repair_quote_before_slash, validate_and_fix_xml};

/// 新图装配结果。
#[derive(Debug, Clone)]
pub enum NewDiagram {
    Ok { xml: String, fixes: Vec<String> },
    Err(String),
}

/// 裸 cell 会得到 root cell "0"/"1"；用了这些 id 的 shape/edge 会被重命名而断边。
/// 返回给模型的错误，或 None。
pub fn reserved_id_error(input: &str) -> Option<String> {
    if Regex::new(r"<(mxGraphModel|mxfile)\b")
        .expect("reserved probe")
        .is_match(input)
    {
        return None;
    }
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r#"<(mxCell|UserObject|object)\b((\s+[A-Za-z0-9_:.\-]+\s*=\s*("[^"]*"|'[^']*'))*)\s*/?>"#,
        )
        .expect("reserved tag regex")
    });
    for caps in re.captures_iter(input) {
        let tag = caps.get(1).expect("tag").as_str();
        let attr_text = caps.get(2).expect("attrs").as_str();
        let attrs: std::collections::HashMap<String, String> = read_attributes(attr_text)
            .into_iter()
            .map(|a| (a.name, a.value))
            .collect();
        let id = attrs.get("id").map(|s| s.as_str());
        if id != Some("0") && id != Some("1") {
            continue;
        }
        let structural = tag != "mxCell"
            || attrs.get("vertex").map(|v| v == "1").unwrap_or(false)
            || attrs.get("edge").map(|v| v == "1").unwrap_or(false)
            || ["x", "y", "w", "h", "width", "height", "source", "target"]
                .iter()
                .any(|a| attrs.contains_key(*a));
        if structural {
            return Some(
                r#"Cell ids "0" and "1" are the root cells, which are added automatically. Give shapes and edges ids starting at "2"."#
                    .to_string(),
            );
        }
    }
    None
}

/// mxCell XML 是否完整（未截断）。完整 XML 以自闭合或闭合 mxCell 结尾，其后
/// 只可能是闭合标签（任意 provider 的包装）。
pub fn is_mx_cell_xml_complete(xml: &str) -> bool {
    let (_, rest) = read_style_classes(xml);
    let trimmed = repair_quote_before_slash(&rest).trim().to_string();
    if trimmed.is_empty() {
        return false;
    }
    let last_self_close = trimmed.rfind("/>");
    let last_mxcell_close = trimmed.rfind("</mxCell>");
    let last_valid_end = match (last_self_close, last_mxcell_close) {
        (None, None) => return false,
        (a, b) => a.unwrap_or(0).max(b.unwrap_or(0)),
    };
    let last_cell_start = trimmed.rfind("<mxCell");
    if let Some(start) = last_cell_start {
        if start > last_mxcell_close.unwrap_or(0) {
            static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            let re = RE.get_or_init(|| {
                Regex::new(r#"^<mxCell\b(?:[^<>"']|"[^"]*"|'[^']*')*/>"#).expect("self close")
            });
            if !re.is_match(&trimmed[start..]) {
                return false;
            }
        }
    }
    let end_offset = if last_mxcell_close
        .map(|c| c > last_self_close.unwrap_or(0))
        .unwrap_or(false)
    {
        9
    } else {
        2
    };
    let suffix = &trimmed[last_valid_end + end_offset..];
    static SUFFIX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let suffix_re = SUFFIX.get_or_init(|| Regex::new(r"^(\s*</[^>]+>)*\s*$").expect("suffix"));
    suffix_re.is_match(suffix)
}

/// 输出在 mxCell 内中断会被包装并报错；返回给模型的错误，或 None。
pub fn truncated_cell_error(input: &str) -> Option<String> {
    if Regex::new(r"<(mxGraphModel|mxfile)\b")
        .expect("trunc probe")
        .is_match(input)
    {
        return None;
    }
    static COMMENT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static BADCLOSE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static HASCELL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let comment = COMMENT.get_or_init(|| Regex::new(r"<!--[\s\S]*?-->").expect("comment"));
    let badclose =
        BADCLOSE.get_or_init(|| Regex::new(r#"</([a-zA-Z][a-zA-Z0-9]*)\s*/>"#).expect("badclose"));
    let without_comments = comment.replace_all(input, "").to_string();
    let stripped = badclose.replace_all(&without_comments, "</$1>");
    let has_cell = HASCELL.get_or_init(|| Regex::new(r"<mxCell\b").expect("hascell"));
    if !has_cell.is_match(&stripped) || is_mx_cell_xml_complete(&stripped) {
        return None;
    }
    let tail: String = input
        .chars()
        .rev()
        .take(300)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Some(format!(
        "The XML ends inside an unfinished mxCell, so the output was probably cut off. Nothing was drawn. It ended with:\n{tail}\nResend the complete diagram, or send the first cells now and add the remaining cells with edit_diagram add operations."
    ))
}

/// 取出命名样式定义（style-classes）。定义未闭合或只发了定义时返回错误。
fn take_style_definitions(input: &str) -> (StyleClasses, String, Option<String>) {
    let (classes, xml) = read_style_classes(input);
    let mut error = None;
    if Regex::new(r"(?i)<mxStyle\b")
        .expect("mxstyle")
        .is_match(&xml)
    {
        error = Some(
            r#"A named style definition is not closed. Write it as <mxStyle name="..." value="..."/> before the cells."#
                .to_string(),
        );
    } else if !classes.is_empty() && !has_cells(&xml) {
        error = Some(
            "Only named style definitions were sent, no cells. Send the mxCell elements after the definitions."
                .to_string(),
        );
    }
    (classes, xml, error)
}

/// 模型写的一页模型（也作 add_page 的起始内容）。先校验并修复裸模型，再展开
/// 紧凑 cell、命名样式、补默认样式。
pub fn prepare_new_model(input: &str) -> NewDiagram {
    let (classes, cells, style_error) = take_style_definitions(input);
    if let Some(e) = style_error {
        return NewDiagram::Err(e);
    }
    if let Some(e) = reserved_id_error(&cells) {
        return NewDiagram::Err(e);
    }
    if let Some(e) = truncated_cell_error(&cells) {
        return NewDiagram::Err(e);
    }
    let mut xml = wrap_cells_in_model(&cells);
    let check = validate_and_fix_xml(&xml, true);
    if let Some(fixed) = check.fixed {
        xml = fixed;
    }
    if !check.valid {
        return NewDiagram::Err(format!(
            "XML validation failed - {}",
            check.error.unwrap_or_default()
        ));
    }
    xml = add_default_styles(
        &apply_style_classes(&expand_compact_cells(&xml, "1"), &classes),
        &HashSet::new(),
    );
    if let Some(e) = get_xml_syntax_error(&xml) {
        return NewDiagram::Err(format!(
            "XML validation failed after expanding the cells - {e}"
        ));
    }
    NewDiagram::Ok {
        xml,
        fixes: check.fixes,
    }
}

/// `prepare_new_model` + 转成 `<mxfile>`。
pub fn prepare_new_diagram(
    input: &str,
    page_id: Option<&str>,
    page_name: Option<&str>,
) -> NewDiagram {
    let model = match prepare_new_model(input) {
        NewDiagram::Ok { xml, fixes } => NewDiagram::Ok { xml, fixes },
        NewDiagram::Err(e) => return NewDiagram::Err(e),
    };
    let NewDiagram::Ok {
        xml: model_xml,
        fixes,
    } = model
    else {
        unreachable!()
    };
    let Some(mut normalized) = normalize_to_mxfile(&model_xml, page_id, page_name) else {
        return NewDiagram::Err(
            "XML must be the mxCell elements of one page, a <mxGraphModel>, or an <mxfile> with one or more <diagram> children."
                .to_string(),
        );
    };
    // 模型发的无 id diagram 补一个（否则 draw.io 自建 id，服务器不知道）
    if let Some(mut doc) = parse_mxfile(&normalized) {
        let mut added = false;
        let diagrams = doc.by_name("diagram");
        for d in diagrams {
            if d.attr("id").is_none() {
                added = true;
            }
        }
        if added {
            // 逐个补 id
            let new_id = generate_page_id();
            set_missing_diagram_id(&mut doc, &new_id);
            normalized = serialize_mxfile(&doc);
        }
    }
    NewDiagram::Ok {
        xml: normalized,
        fixes,
    }
}

fn set_missing_diagram_id(doc: &mut super::xml_tree::XElement, id: &str) {
    for child in &mut doc.children {
        if let super::xml_tree::XmlNode::Element(el) = child {
            if el.name == "diagram" && el.attr("id").is_none() {
                el.set_attr("id", id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_bare_cells() {
        let input = r#"<mxCell id="2" value="Start" style="rounded=1;" x="40" y="40" w="120" h="60"/>
<mxCell id="3" value="End" style="rounded=1;" x="40" y="200" w="120" h="60"/>
<mxCell id="4" style="edgeStyle=orthogonalEdgeStyle;" source="2" target="3"/>"#;
        match prepare_new_diagram(input, None, None) {
            NewDiagram::Ok { xml, .. } => {
                assert!(xml.contains("<mxfile"));
                assert!(xml.contains(r#"id="2""#));
                assert!(xml.contains("vertex=\"1\""));
                assert!(xml.contains("edge=\"1\""));
                assert!(!xml.contains("mxStyle"));
            }
            NewDiagram::Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn rejects_reserved_ids() {
        let err = reserved_id_error(r#"<mxCell id="1" value="X" x="1" y="1" w="10" h="10"/>"#);
        assert!(err.is_some());
    }

    #[test]
    fn detects_truncated() {
        let err = truncated_cell_error(
            r#"<mxCell id="2" value="Start" style="rounded=1;" x="40" y="40" w="120" h="60"#,
        );
        assert!(err.is_some());
        assert_eq!(truncated_cell_error(r#"<mxCell id="2" value="A"/>"#), None);
    }
}
