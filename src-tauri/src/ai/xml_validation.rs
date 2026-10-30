//! XML 校验与自动修复（上游 `xml-validation.ts`）。
//!
//! `validate_mx_cell_structure` 做结构校验；`auto_fix_xml` 修复常见问题；
//! `validate_and_fix_xml` 组合两者。忠实要点与上游逐条对应（含 23 步修复）。

use std::collections::{HashMap, HashSet};

use regex::Regex;

use super::xml_attributes::read_attributes;
use super::xml_syntax::get_xml_syntax_error;
use super::xml_tree::{parse_lenient, XElement, XmlNode};

/// 不应重复的结构属性。
const STRUCTURAL_ATTRS: [&str; 6] = [
    "edge",
    "parent",
    "source",
    "target",
    "vertex",
    "connectable",
];

/// draw.io 认识的元素名（区分大小写）。
const VALID_DRAWIO_TAGS: [&str; 12] = [
    "mxfile",
    "diagram",
    "mxGraphModel",
    "root",
    "mxCell",
    "mxGeometry",
    "mxPoint",
    "Array",
    "Object",
    "mxRectangle",
    "UserObject",
    "object",
];

// ============================================================================
// XML 解析辅助
// ============================================================================

pub(crate) struct ParsedTag {
    pub tag: String,
    pub tag_name: String,
    pub is_closing: bool,
    pub is_self_closing: bool,
    pub start: usize,
    pub end: usize, // 含 '>'
}

/// 解析 XML 标签（正确处理引号内的 ">"）。
pub(crate) fn parse_xml_tags(xml: &str) -> Vec<ParsedTag> {
    let bytes = xml.as_bytes();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let tag_start = match xml[i..].find('<') {
            Some(r) => i + r,
            None => break,
        };
        let mut tag_end = tag_start + 1;
        let mut in_quote = false;
        let mut quote = b'\0';
        while tag_end < bytes.len() {
            let c = bytes[tag_end];
            if in_quote {
                if c == quote {
                    in_quote = false;
                }
            } else if c == b'"' || c == b'\'' {
                in_quote = true;
                quote = c;
            } else if c == b'>' {
                break;
            }
            tag_end += 1;
        }
        if tag_end >= bytes.len() {
            break;
        }
        let tag = &xml[tag_start..=tag_end];
        i = tag_end + 1;
        let is_closing = tag.as_bytes()[1] == b'/';
        let mut j = if is_closing { 2 } else { 1 };
        let name_start = j;
        while j < tag.len() {
            let b = tag.as_bytes()[j];
            if b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-') {
                j += 1;
            } else {
                break;
            }
        }
        let name = &tag[name_start..j];
        if name.is_empty() || !name.as_bytes()[0].is_ascii_alphabetic() {
            continue;
        }
        tags.push(ParsedTag {
            tag: tag.to_string(),
            tag_name: name.to_string(),
            is_closing,
            is_self_closing: tag.ends_with("/>"),
            start: tag_start,
            end: tag_end,
        });
    }
    tags
}

/// 增量引号追踪：判断某位置是否位于属性值引号内。位置须按递增顺序查询。
struct QuoteTracker<'a> {
    bytes: &'a [u8],
    i: usize,
    in_quote: bool,
    quote_char: u8,
}

impl<'a> QuoteTracker<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
            in_quote: false,
            quote_char: 0,
        }
    }

    fn is_inside(&mut self, pos: usize) -> bool {
        while self.i < pos && self.i < self.bytes.len() {
            let c = self.bytes[self.i];
            if self.in_quote {
                if c == self.quote_char {
                    self.in_quote = false;
                }
            } else if c == b'"' || c == b'\'' {
                // 仅跟在 "=" 后的引号开启属性值（跳过空白）
                let mut j = self.i as isize - 1;
                while j >= 0 && (self.bytes[j as usize] as char).is_ascii_whitespace() {
                    j -= 1;
                }
                if j >= 0 && self.bytes[j as usize] == b'=' {
                    self.in_quote = true;
                    self.quote_char = c;
                }
            }
            self.i += 1;
        }
        self.in_quote
    }
}

/// 用 fn 重写每个开标签，文本与闭标签原样保留。
pub(crate) fn replace_in_opening_tags(xml: &str, mut f: impl FnMut(&str) -> String) -> String {
    let tags = parse_xml_tags(xml);
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    for t in &tags {
        if t.is_closing {
            continue;
        }
        out.push_str(&xml[last..t.start]);
        out.push_str(&f(&t.tag));
        last = t.end + 1;
    }
    out.push_str(&xml[last..]);
    out
}

// ============================================================================
// 校验辅助
// ============================================================================

fn check_duplicate_attributes(xml: &str) -> Option<String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("tag regex"));
    let structural: HashSet<&str> = STRUCTURAL_ATTRS.iter().copied().collect();
    for m in re.find_iter(xml) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for a in read_attributes(m.as_str()) {
            *counts.entry(a.name).or_insert(0) += 1;
        }
        let dups: Vec<&String> = counts
            .iter()
            .filter(|(n, c)| **c > 1 && structural.contains(n.as_str()))
            .map(|(n, _)| n)
            .collect();
        if !dups.is_empty() {
            return Some(format!(
                "Invalid XML: Duplicate structural attribute(s): {}. Remove duplicate attributes.",
                dups.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    None
}

/// 收集 cell（mxCell/UserObject/object）的 (id, 是否被 wrapper 包裹)。
fn collect_cell_ids(el: &XElement, parent: Option<&str>, out: &mut Vec<String>) {
    let is_cell = matches!(el.name.as_str(), "mxCell" | "UserObject" | "object");
    if is_cell {
        if let Some(id) = el.attr("id") {
            let wrapped =
                el.name == "mxCell" && matches!(parent, Some("UserObject") | Some("object"));
            if !wrapped {
                out.push(id.to_string());
            }
        }
    }
    for child in el.child_elements() {
        collect_cell_ids(child, Some(&el.name), out);
    }
}

fn check_duplicate_ids(xml: &str) -> Option<String> {
    // DOM 路径：mxfile 按页 scope
    if Regex::new(r"<mxfile[\s>]")
        .expect("mxfile probe")
        .is_match(xml)
    {
        if let Some(doc) = parse_lenient(xml) {
            if doc.name == "mxfile" {
                let diagrams = doc.by_name("diagram");
                let mut diagram_ids: HashMap<String, usize> = HashMap::new();
                for d in &diagrams {
                    if let Some(id) = d.attr("id") {
                        *diagram_ids.entry(id.to_string()).or_insert(0) += 1;
                    }
                }
                let dup: Vec<String> = diagram_ids
                    .iter()
                    .filter(|(_, c)| **c > 1)
                    .map(|(id, _)| format!("'{id}'"))
                    .collect();
                if !dup.is_empty() {
                    return Some(format!(
                        "Invalid XML: Found duplicate <diagram> id(s): {}. Each page must have a unique id.",
                        dup.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                    ));
                }
                for (i, diagram) in diagrams.iter().enumerate() {
                    let page_id = diagram
                        .attr("id")
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("(index {i})"));
                    let mut cells: Vec<String> = Vec::new();
                    collect_cell_ids(diagram, None, &mut cells);
                    let mut cell_ids: HashMap<String, usize> = HashMap::new();
                    for id in &cells {
                        *cell_ids.entry(id.clone()).or_insert(0) += 1;
                    }
                    let dups: Vec<String> = cell_ids
                        .iter()
                        .filter(|(_, c)| **c > 1)
                        .map(|(id, c)| format!("'{id}' ({c}x)"))
                        .collect();
                    if !dups.is_empty() {
                        return Some(format!(
                            "Invalid XML: Found duplicate cell ID(s) in page \"{page_id}\": {}. All mxCell ids must be unique within a page.",
                            dups.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
                        ));
                    }
                }
                return None;
            }
        }
    }
    // regex fallback（裸 mxGraphModel）
    static ID_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let id_re =
        ID_RE.get_or_init(|| Regex::new(r#"(?i)\bid\s*=\s*["']([^"']+)["']"#).expect("id regex"));
    let mut ids: HashMap<String, usize> = HashMap::new();
    for caps in id_re.captures_iter(xml) {
        *ids.entry(caps[1].to_string()).or_insert(0) += 1;
    }
    let dup: Vec<String> = ids
        .iter()
        .filter(|(_, c)| **c > 1)
        .map(|(id, c)| format!("'{id}' ({c}x)"))
        .collect();
    if !dup.is_empty() {
        return Some(format!(
            "Invalid XML: Found duplicate ID(s): {}. All id attributes must be unique.",
            dup.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    None
}

fn check_nested_mx_cells(xml: &str) -> Option<String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static AS_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"</?mxCell\b([^<>"']|"[^"]*"|'[^']*')*>"#).expect("mxcell tag regex")
    });
    let as_re = AS_RE.get_or_init(|| {
        Regex::new(r#"\sas\s*=\s*["'](valueLabel|geometry)["']"#).expect("as regex")
    });
    let nested =
        "Invalid XML: Found nested mxCell tags. Cells should be siblings, not nested inside other mxCell elements.";
    let mut stack: Vec<usize> = Vec::new();
    for m in re.find_iter(xml) {
        let tag = m.as_str();
        let is_label_or_geometry = as_re.is_match(tag);
        if tag.starts_with("</mxCell>") {
            if !stack.is_empty() {
                stack.pop();
            }
        } else if tag.ends_with("/>") {
            if !stack.is_empty() && !is_label_or_geometry {
                return Some(nested.to_string());
            }
        } else if !is_label_or_geometry {
            stack.push(m.start());
            if stack.len() > 1 {
                return Some(nested.to_string());
            }
        }
    }
    None
}

fn check_unknown_elements(xml: &str) -> Option<String> {
    static COMMENT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let comment = COMMENT.get_or_init(|| Regex::new(r"<!--[\s\S]*?-->").expect("comment regex"));
    let stripped = comment.replace_all(xml, "");
    let valid: HashSet<&str> = VALID_DRAWIO_TAGS.iter().copied().collect();
    for t in parse_xml_tags(&stripped) {
        if !valid.contains(t.tag_name.as_str()) {
            return Some(format!(
                "Invalid XML: Unknown element <{}>. draw.io only understands {} (names are case-sensitive).",
                t.tag_name,
                VALID_DRAWIO_TAGS.join(", ")
            ));
        }
    }
    None
}

/// 非自闭合 `<Array ...>` 到其 `</Array>` 的范围。
fn find_array_ranges(xml: &str) -> Vec<(usize, usize)> {
    let bytes = xml.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let Some(rel) = xml[i..].find("<Array") else {
            break;
        };
        let start = i + rel;
        // 找该标签的 ">"（跳过引号）
        let mut j = start + 6; // "<Array".len()
        let mut in_quote = false;
        let mut quote = b'\0';
        let mut tag_end = None;
        while j < bytes.len() {
            let c = bytes[j];
            if in_quote {
                if c == quote {
                    in_quote = false;
                }
            } else if c == b'"' || c == b'\'' {
                in_quote = true;
                quote = c;
            } else if c == b'>' {
                tag_end = Some(j);
                break;
            }
            j += 1;
        }
        let Some(tag_end) = tag_end else { break };
        let tag = &xml[start..=tag_end];
        if tag.ends_with("/>") {
            i = tag_end + 1;
            continue;
        }
        let Some(crel) = xml[tag_end + 1..].find("</Array>") else {
            break;
        };
        let close_end = tag_end + 1 + crel + "</Array>".len();
        ranges.push((start, close_end));
        i = close_end;
    }
    ranges
}

/// 不在 `<Array as="points">` 内、且无 `as` 属性的 `<mxPoint>` 范围。
pub(crate) fn find_orphan_mx_points(xml: &str) -> Vec<(usize, usize)> {
    static POINT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static AS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let point = POINT
        .get_or_init(|| Regex::new(r"<mxPoint\b[^>]*?(/>|>\s*</mxPoint>)").expect("mxpoint regex"));
    let as_re = AS.get_or_init(|| Regex::new(r"\sas\s*=").expect("as regex"));
    let arrays = find_array_ranges(xml);
    let mut orphans = Vec::new();
    for m in point.find_iter(xml) {
        if as_re.is_match(m.as_str()) {
            continue;
        }
        if arrays.iter().any(|(s, e)| m.start() > *s && m.start() < *e) {
            continue;
        }
        orphans.push((m.start(), m.end()));
    }
    orphans
}

/// 标签之间的首个非空白文本（跳过压缩页数据），最多 40 字符。
fn find_text_between_tags(el: &XElement) -> Option<String> {
    let compressed = el.name == "diagram" && el.is_leaf();
    for node in &el.children {
        match node {
            XmlNode::Element(sub) => {
                if let Some(t) = find_text_between_tags(sub) {
                    return Some(t);
                }
            }
            XmlNode::Text(t) if !compressed => {
                let trimmed = t.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.chars().take(40).collect());
                }
            }
            _ => {}
        }
    }
    None
}

/// DOM 检查：mxCell 的 parent 是 mxCell → 返回该 cell id。
fn find_nested_mx_cell_dom(el: &XElement) -> Option<String> {
    for child in el.child_elements() {
        if child.name == "mxCell" && el.name == "mxCell" {
            return Some(child.attr("id").unwrap_or("unknown").to_string());
        }
        if let Some(id) = find_nested_mx_cell_dom(child) {
            return Some(id);
        }
    }
    None
}

// ============================================================================
// 主校验函数
// ============================================================================

/// 校验 draw.io XML 结构。`strict` 时额外拒绝未知元素与孤立 mxPoint。
/// 返回 None（合法）或错误消息。
pub fn validate_mx_cell_structure(xml: &str, strict: bool) -> Option<String> {
    // DOM 检查（宽松解析；parsererror 时跳过文本检查）
    if let Some(doc) = parse_lenient(xml) {
        if let Some(id) = find_nested_mx_cell_dom(&doc) {
            return Some(format!(
                r#"Invalid XML: Found nested mxCell (id="{id}"). Cells should be siblings, not nested inside other mxCell elements."#
            ));
        }
        if doc.name != "parsererror" {
            if let Some(text) = find_text_between_tags(&doc) {
                return Some(format!(
                    r#"Invalid XML: Found text "{text}" between tags. Labels belong in the value attribute; remove any other text between tags."#
                ));
            }
        }
    }

    if let Some(e) = check_duplicate_attributes(xml) {
        return Some(e);
    }
    if let Some(e) = check_duplicate_ids(xml) {
        return Some(e);
    }
    if empty_id_re().is_match(xml) {
        return Some("Invalid XML: Found mxCell element(s) with empty id attribute".to_string());
    }
    if let Some(e) = check_nested_mx_cells(xml) {
        return Some(e);
    }
    if strict {
        if let Some(e) = check_unknown_elements(xml) {
            return Some(e);
        }
        if !find_orphan_mx_points(xml).is_empty() {
            return Some(
                r#"Invalid XML: Found <mxPoint> without an "as" attribute outside <Array as="points">. Put waypoints inside <Array as="points"> or remove the point."#
                    .to_string(),
            );
        }
    }
    if let Some(e) = get_xml_syntax_error(xml) {
        return Some(format!(
            "Invalid XML: syntax error at {e} Escape special characters in attribute values (&lt; for <, &amp; for &, &quot; for \"), quote every attribute value, and do not repeat an attribute."
        ));
    }
    None
}

fn empty_id_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<mxCell[^>]*\sid\s*=\s*["']\s*["'][^>]*>"#).expect("empty id regex")
    })
}

/// 紧凑 cell 最后一个数字属性丢失闭合引号时补上
/// （`h="112/>` → `h="112"/>`）。
pub fn repair_quote_before_slash(xml: &str) -> String {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        // 上游: /<mxCell\b((?:\s+[\w:.-]+="[^"]*")*)(\s+(?:x|y|w|h|width|height)="-?\d+(?:\.\d+)?)\s*\/>/g
        // 组: 1=已引用属性, 2=单属性(忽略), 3=丢失引号的 size, 4=size名, 5=小数(忽略)
        Regex::new(
            r#"<mxCell\b((\s+[A-Za-z0-9_:.\-]+="[^"]*")*)(\s+(x|y|w|h|width|height)="-?\d+(\.\d+)?)\s*/>"#,
        )
        .expect("quote-before-slash regex")
    });
    re.replace_all(xml, "<mxCell$1$3\"/>").to_string()
}

// ============================================================================
// 自动修复辅助
// ============================================================================

/// Step 6：修复 `name=&quot;value&quot;` 畸形属性引号（跳过已正确引号的值）。
fn fix_malformed_quotes_in_tag(tag: &str) -> (String, bool) {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(\s[a-zA-Z][a-zA-Z0-9_:-]*)=&quot;([^&]*?)&quot;"#).expect("quote fix regex")
    });
    let mut out = String::with_capacity(tag.len());
    let mut fixed = false;
    let bytes = tag.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            let q = c;
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            out.push_str(&tag[start..i]);
        } else if (c as char).is_ascii_whitespace() {
            let hay = &tag[i..];
            if let Some(caps) = re.captures(hay) {
                if caps.get(0).unwrap().start() == 0 {
                    out.push_str(&format!("{}=\"{}\"", &caps[1], &caps[2]));
                    i += caps.get(0).unwrap().end();
                    fixed = true;
                    continue;
                }
            }
            let ch = tag[i..].chars().next().expect("utf-8");
            out.push(ch);
            i += ch.len_utf8();
        } else {
            let ch = tag[i..].chars().next().expect("utf-8");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    (out, fixed)
}

/// Step 8：属性间缺空格（`value="x"vertex="1"` → `value="x" vertex="1"`）。
fn add_missing_space_in_tag(tag: &str) -> (String, bool) {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r#"("[^"]*"|'[^']*')([a-zA-Z_:])"#).expect("space fix regex"));
    let mut fixed = false;
    let replaced = re.replace_all(tag, |caps: &regex::Captures<'_>| {
        fixed = true;
        format!("{} {}", &caps[1], &caps[2])
    });
    (replaced.to_string(), fixed)
}

/// Step 6c：闭合无斜杠的紧凑 cell（`<mxCell ... h="60">` 后跟 cell/定义/结束）。
fn close_slashless_compact_cells(xml: &str) -> (String, bool) {
    static SIZE_CONN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let size_conn = SIZE_CONN.get_or_init(|| {
        Regex::new(r#"\s(x|y|w|h|width|height|source|target)=""#).expect("size regex")
    });
    let bytes = xml.as_bytes();
    let mut out = String::with_capacity(xml.len());
    let mut changed = false;
    let mut i = 0;
    while i < bytes.len() {
        let Some(rel) = xml[i..].find("<mxCell") else {
            out.push_str(&xml[i..]);
            break;
        };
        let start = i + rel;
        out.push_str(&xml[i..start]);
        let mut j = start + 7;
        let mut in_quote = false;
        let mut quote = b'\0';
        let mut tag_end = None;
        let mut self_closing = false;
        while j < bytes.len() {
            let c = bytes[j];
            if in_quote {
                if c == quote {
                    in_quote = false;
                }
            } else if c == b'"' || c == b'\'' {
                in_quote = true;
                quote = c;
            } else if c == b'>' {
                self_closing = j > 0 && bytes[j - 1] == b'/';
                tag_end = Some(j);
                break;
            }
            j += 1;
        }
        let Some(tag_end) = tag_end else {
            out.push_str(&xml[start..]);
            break;
        };
        let tag = &xml[start..=tag_end];
        if self_closing {
            out.push_str(tag);
            i = tag_end + 1;
            continue;
        }
        let after = xml[tag_end + 1..].trim_start();
        let next_ok =
            after.is_empty() || after.starts_with("<mxCell") || after.starts_with("<mxStyle");
        if next_ok && size_conn.is_match(tag) {
            out.push_str(&tag[..tag.len() - 1]);
            out.push_str("/>");
            changed = true;
        } else {
            out.push_str(tag);
        }
        i = tag_end + 1;
    }
    (out, changed)
}

// ============================================================================
// 自动修复
// ============================================================================

/// 尝试修复常见 XML 问题，返回 (修复后的 XML, 修复记录)。
pub fn auto_fix_xml(xml: &str) -> (String, Vec<String>) {
    let mut fixed = xml.to_string();
    let mut fixes: Vec<String> = Vec::new();

    // 0. JSON 转义
    if fixed.contains("\\\"") {
        let before = fixed.clone();
        fixed = fixed.replace("\\\"", "\"").replace("\\n", "\n");
        if fixed != before {
            fixes.push("Fixed JSON-escaped XML".to_string());
        }
    }

    // 0b. 标签间字面 \n \t \r
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r#">(\s|\\[nrt])+<"#).expect("escaped gap regex"));
        let before = fixed.clone();
        fixed = re
            .replace_all(&fixed, |caps: &regex::Captures<'_>| {
                caps.get(0)
                    .expect("gap")
                    .as_str()
                    .replace("\\n", "\n")
                    .replace("\\t", "\t")
                    .replace("\\r", "")
            })
            .to_string();
        if fixed != before {
            fixes.push("Replaced literal \\n between tags with line breaks".to_string());
        }
    }

    // 1. CDATA wrapper
    {
        static START: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        static END: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let start = START.get_or_init(|| Regex::new(r"^\s*<!\[CDATA\[").expect("cdata start"));
        let end = END.get_or_init(|| Regex::new(r"\]\]>\s*$").expect("cdata end"));
        if start.is_match(&fixed) {
            fixed = start.replace(&fixed, "").to_string();
            fixed = end.replace(&fixed, "").to_string();
            fixes.push("Removed CDATA wrapper".to_string());
        }
    }

    // 2. 根之前文本
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        static PREFIX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re =
            RE.get_or_init(|| Regex::new(r"(?i)<(\?xml|mxGraphModel|mxfile)").expect("root regex"));
        let prefix = PREFIX.get_or_init(|| Regex::new(r"^<[a-zA-Z]").expect("prefix regex"));
        if let Some(m) = re.find(&fixed) {
            if m.start() > 0 && !prefix.is_match(fixed.trim()) {
                fixed = fixed[m.start()..].to_string();
                fixes.push("Removed text before XML root".to_string());
            }
        }
    }

    // 3. 修复重复结构属性
    {
        static TAG: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let tag_re = TAG.get_or_init(|| Regex::new(r"<[^>]+>").expect("tag regex"));
        let structural: HashSet<&str> = STRUCTURAL_ATTRS.iter().copied().collect();
        let mut dup_fixed = false;
        let mut out = String::with_capacity(fixed.len());
        let mut last = 0;
        for m in tag_re.find_iter(&fixed) {
            out.push_str(&fixed[last..m.start()]);
            let tag = m.as_str();
            let mut seen: HashSet<String> = HashSet::new();
            let mut new_tag = String::with_capacity(tag.len());
            let mut tag_last = 0;
            for a in read_attributes(tag) {
                if !structural.contains(a.name.as_str()) {
                    continue;
                }
                if seen.contains(&a.name) {
                    new_tag.push_str(&tag[tag_last..a.start]);
                    tag_last = a.end;
                    dup_fixed = true;
                } else {
                    seen.insert(a.name.clone());
                }
            }
            new_tag.push_str(&tag[tag_last..]);
            out.push_str(&new_tag);
            last = m.end();
        }
        out.push_str(&fixed[last..]);
        fixed = out;
        if dup_fixed {
            fixes.push("Removed duplicate structural attributes".to_string());
        }
    }

    // 4. 未转义 &
    {
        let escaped = super::xml_attributes::escape_ampersands(&fixed);
        if escaped != fixed {
            fixed = escaped;
            fixes.push("Escaped unescaped & characters".to_string());
        }
    }

    // 6. 畸形引号（name=&quot;value&quot;）
    {
        let mut quotes_fixed = false;
        fixed = replace_in_opening_tags(&fixed, |tag| {
            let (new_tag, fixed_here) = fix_malformed_quotes_in_tag(tag);
            quotes_fixed |= fixed_here;
            new_tag
        });
        if quotes_fixed {
            fixes.push("Fixed malformed attribute quotes".to_string());
        }
    }

    // 6b. 紧凑 cell 末尾数字属性丢失闭合引号
    {
        let quoted = repair_quote_before_slash(&fixed);
        if quoted != fixed {
            fixed = quoted;
            fixes.push("Added a missing closing quote before />".to_string());
        }
    }

    // 6c. 无斜杠紧凑 cell 闭合
    {
        let (closed, changed) = close_slashless_compact_cells(&fixed);
        if changed {
            fixed = closed;
            fixes.push("Closed compact cells written without the slash".to_string());
        }
    }

    // 7. 畸形闭合标签 </x/>
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r#"</([a-zA-Z][a-zA-Z0-9]*)\s*/>"#).expect("closing tag regex")
        });
        if re.is_match(&fixed) {
            fixed = re.replace_all(&fixed, "</$1>").to_string();
            fixes.push("Fixed malformed closing tags".to_string());
        }
    }

    // 8. 属性间缺空格
    {
        let mut space_added = false;
        fixed = replace_in_opening_tags(&fixed, |tag| {
            let (new_tag, added) = add_missing_space_in_tag(tag);
            space_added |= added;
            new_tag
        });
        if space_added {
            fixes.push("Added missing space between attributes".to_string());
        }
    }

    // 9. style 颜色值引号
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re =
            RE.get_or_init(|| Regex::new(r##";([a-zA-Z]*[Cc]olor)="#""##).expect("color regex"));
        if re.is_match(&fixed) {
            fixed = re.replace_all(&fixed, ";$1=#").to_string();
            fixes.push("Removed quotes around color values in style".to_string());
        }
    }

    // 10. 属性值未转义 < >
    {
        static HAS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        static VAL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let has = HAS.get_or_init(|| Regex::new(r#"=\s*"[^"]*<[^"]*""#).expect("lt probe"));
        if has.is_match(&fixed) {
            let val = VAL.get_or_init(|| Regex::new(r#"=\s*"([^"]*)""#).expect("val regex"));
            fixed = val
                .replace_all(&fixed, |caps: &regex::Captures<'_>| {
                    let value = &caps[1];
                    format!("=\"{}\"", value.replace('<', "&lt;").replace('>', "&gt;"))
                })
                .to_string();
            fixes.push("Escaped <> characters in attribute values".to_string());
        }
    }

    // 13. 注释语法（-- 非法）
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"<!--([\s\S]*?)-->").expect("comment fix regex"));
        let mut comment_fixed = false;
        fixed = re
            .replace_all(&fixed, |caps: &regex::Captures<'_>| {
                let content = &caps[1];
                if content.contains("--") {
                    let mut fc = content.to_string();
                    while fc.contains("--") {
                        fc = fc.replace("--", "-");
                    }
                    comment_fixed = true;
                    format!("<!--{fc}-->")
                } else {
                    caps.get(0).expect("comment").as_str().to_string()
                }
            })
            .to_string();
        if comment_fixed {
            fixes.push("Fixed invalid comment syntax".to_string());
        }
    }

    // 14. <Cell> → <mxCell>
    {
        static HAS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let has = HAS.get_or_init(|| Regex::new(r"(?i)</?Cell[\s>]").expect("cell probe"));
        if has.is_match(&fixed) {
            let s1 = Regex::new(r"(?i)<Cell(\s)")
                .expect("cell1")
                .replace_all(&fixed, "<mxCell$1");
            let s2 = Regex::new(r"(?i)<Cell>")
                .expect("cell2")
                .replace_all(&s1, "<mxCell>");
            let s3 = Regex::new(r"(?i)</Cell>")
                .expect("cell3")
                .replace_all(&s2, "</mxCell>");
            fixed = s3.to_string();
            fixes.push("Fixed <Cell> tags to <mxCell>".to_string());
        }
    }

    // 15. 闭合标签 typo 与大小写
    {
        let before15 = fixed.clone();
        fixed = Regex::new(r"(?i)</mxElement>")
            .expect("mxelement")
            .replace_all(&fixed, "</mxCell>")
            .to_string();
        if fixed != before15 {
            fixes.push("Fixed typo </mxElement> to </mxCell>".to_string());
        }
        for name in ["mxCell", "mxGeometry", "mxPoint", "mxGraphModel"] {
            let re = Regex::new(&format!(r"(?i)<(/?){}([\s/>])", name)).expect("case regex");
            let mut changed = false;
            fixed = re
                .replace_all(&fixed, |caps: &regex::Captures<'_>| {
                    let slash = caps.get(1).expect("slash").as_str();
                    let tail = caps.get(2).expect("tail").as_str();
                    let correct = format!("<{slash}{name}{tail}");
                    if caps.get(0).expect("whole").as_str() != correct {
                        changed = true;
                    }
                    correct
                })
                .to_string();
            if changed {
                fixes.push(format!("Fixed tag case of <{name}>"));
            }
        }
    }

    // 16. 移除外来标签（跳过引号内）
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"</?([a-zA-Z][a-zA-Z0-9_]*)[^>]*>").expect("foreign tag regex")
        });
        let valid: HashSet<&str> = VALID_DRAWIO_TAGS.iter().copied().collect();
        let mut qt = QuoteTracker::new(&fixed);
        let mut foreign_tags: Vec<String> = Vec::new();
        let mut positions: Vec<(usize, usize)> = Vec::new();
        for m in re.find_iter(&fixed) {
            let tag_name = m.as_str().split_at(1).1.trim_start_matches('/');
            let tag_name = tag_name
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if valid.contains(tag_name) {
                continue;
            }
            if qt.is_inside(m.start()) {
                continue;
            }
            if !foreign_tags.iter().any(|t| t == tag_name) {
                foreign_tags.push(tag_name.to_string());
            }
            positions.push((m.start(), m.end()));
        }
        if !positions.is_empty() {
            for (s, e) in positions.iter().rev() {
                fixed = format!("{}{}", &fixed[..*s], &fixed[*e..]);
            }
            fixes.push(format!("Removed foreign tags: {}", foreign_tags.join(", ")));
        }
    }

    // 16b. 移除孤立 <mxPoint>
    {
        let orphans = find_orphan_mx_points(&fixed);
        if !orphans.is_empty() {
            for (s, e) in orphans.iter().rev() {
                fixed = format!("{}{}", &fixed[..*s], &fixed[*e..]);
            }
            fixes.push(format!("Removed {} orphan <mxPoint>(s)", orphans.len()));
        }
    }

    // 17. 闭合未闭合标签
    {
        let parsed = parse_xml_tags(&fixed);
        let mut tag_stack: Vec<String> = Vec::new();
        for t in &parsed {
            if t.is_closing {
                if let Some(idx) = tag_stack.iter().rposition(|n| n == &t.tag_name) {
                    tag_stack.remove(idx);
                }
            } else if !t.is_self_closing {
                tag_stack.push(t.tag_name.clone());
            }
        }
        if !tag_stack.is_empty() {
            let mut tags_to_close: Vec<String> = Vec::new();
            for tag_name in tag_stack.iter().rev() {
                let open_re = Regex::new(&format!(r"(?i)<{}[\s>]", regex::escape(tag_name)))
                    .expect("open count");
                let close_re = Regex::new(&format!(r"(?i)</{}>", regex::escape(tag_name)))
                    .expect("close count");
                if open_re.find_iter(&fixed).count() > close_re.find_iter(&fixed).count() {
                    tags_to_close.push(tag_name.clone());
                }
            }
            if !tags_to_close.is_empty() {
                let closing = tags_to_close
                    .iter()
                    .map(|t| format!("</{t}>"))
                    .collect::<Vec<_>>()
                    .join("\n");
                fixed = format!("{}\n{}", fixed.trim_end(), closing);
                fixes.push(format!(
                    "Closed {} unclosed tag(s): {}",
                    tags_to_close.len(),
                    tags_to_close.join(", ")
                ));
            }
        }
    }

    // 18. 移除多余闭合标签（计数，跳过引号内）
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"<(/?[a-zA-Z][a-zA-Z0-9]*)[^>]*>").expect("tag count regex")
        });
        let valid: HashSet<&str> = VALID_DRAWIO_TAGS.iter().copied().collect();
        let mut qt = QuoteTracker::new(&fixed);
        let mut counts: HashMap<String, (usize, usize)> = HashMap::new(); // (opens, closes)
        for m in re.find_iter(&fixed) {
            if qt.is_inside(m.start()) {
                continue;
            }
            let caps = re.captures(m.as_str()).expect("captures");
            let tag_part = caps.get(1).expect("tagpart").as_str();
            let is_closing = tag_part.starts_with('/');
            let tag_name = if is_closing { &tag_part[1..] } else { tag_part };
            if !valid.contains(tag_name) {
                continue;
            }
            let e = counts.entry(tag_name.to_string()).or_insert((0, 0));
            if is_closing {
                e.1 += 1;
            } else if !m.as_str().ends_with("/>") {
                e.0 += 1;
            }
        }
        for (tag_name, (opens, closes)) in &counts {
            let extra = *closes as isize - *opens as isize;
            if extra > 0 {
                let close_re = Regex::new(&format!("</{}>", tag_name)).expect("close regex");
                let matches: Vec<usize> = close_re.find_iter(&fixed).map(|m| m.start()).collect();
                let mut removed = 0isize;
                for &idx in matches.iter().rev() {
                    if removed >= extra {
                        break;
                    }
                    fixed = format!(
                        "{}{}",
                        &fixed[..idx],
                        &fixed[idx + close_re.as_str().len()..]
                    );
                    removed += 1;
                }
                if removed > 0 {
                    fixes.push(format!(
                        "Removed {removed} extra </{tag_name}> closing tag(s)"
                    ));
                }
            }
        }
    }

    // 19. 移除最后标签后的垃圾
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re =
            RE.get_or_init(|| Regex::new(r"</[a-zA-Z][a-zA-Z0-9]*>|/>").expect("last tag regex"));
        let mut last_end = None;
        for m in re.find_iter(&fixed) {
            last_end = Some(m.end());
        }
        if let Some(e) = last_end {
            if e < fixed.len() && !fixed[e..].trim().is_empty() {
                fixed = fixed[..e].to_string();
                fixes.push("Removed trailing garbage after last XML tag".to_string());
            }
        }
    }

    // 20. 展平重复 ID 嵌套（行级）
    {
        static OPEN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        static CLOSE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        static ID: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let open = OPEN.get_or_init(|| Regex::new(r"<mxCell\s").expect("open cell"));
        let close = CLOSE.get_or_init(|| Regex::new(r"^\s*</mxCell>\s*$").expect("close cell"));
        let id_re = ID.get_or_init(|| Regex::new(r#"\bid\s*=\s*["']([^"']+)["']"#).expect("id"));
        let lines: Vec<&str> = fixed.split('\n').collect();
        let mut new_lines: Vec<&str> = Vec::new();
        let mut nested_fixed = 0;
        let mut extra_close = 0usize;
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let mut skip = false;
            if let Some(next_line) = lines.get(i + 1) {
                let is_open = |l: &str| open.is_match(l) && !l.contains("/>");
                if is_open(line) && is_open(next_line) {
                    let id_of = |l: &str| {
                        id_re
                            .captures(l)
                            .and_then(|c| c.get(1))
                            .map(|m| m.as_str().to_string())
                    };
                    if let (Some(a), Some(b)) = (id_of(line), id_of(next_line)) {
                        if a == b {
                            nested_fixed += 1;
                            extra_close += 1;
                            skip = true;
                        }
                    }
                }
            }
            if !skip {
                if extra_close > 0 && close.is_match(line) {
                    extra_close -= 1;
                } else {
                    new_lines.push(line);
                }
            }
            i += 1;
        }
        if nested_fixed > 0 {
            fixed = new_lines.join("\n");
            fixes.push(format!(
                "Flattened {nested_fixed} duplicate-ID nested mxCell(s)"
            ));
        }
    }

    // 21. 展平真嵌套（行级，不同 ID）
    if check_nested_mx_cells(&fixed).is_some() {
        static OPEN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let open = OPEN.get_or_init(|| Regex::new(r"<mxCell\s").expect("open cell 21"));
        let lines: Vec<&str> = fixed.split('\n').collect();
        let mut new_lines: Vec<String> = Vec::new();
        let mut true_nested = 0;
        let mut cell_depth = 0usize;
        let mut pending_close = 0usize;
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();
            let is_open_cell = open.is_match(trimmed)
                && !trimmed.ends_with("/>")
                && !trimmed.ends_with("</mxCell>");
            let is_self_closing = open.is_match(trimmed) && trimmed.ends_with("/>");
            let is_close_cell = trimmed == "</mxCell>";
            let starts_with_close = !is_close_cell && trimmed.starts_with("</mxCell>");
            if is_open_cell || is_self_closing {
                if cell_depth > 0 {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    new_lines.push(format!("{indent}</mxCell>"));
                    true_nested += 1;
                    let next_open =
                        lines.iter().enumerate().skip(i + 1).position(|(_, l)| {
                            open.is_match(l.trim()) && !l.trim().ends_with("/>")
                        });
                    let next_close = lines
                        .iter()
                        .enumerate()
                        .skip(i + 1)
                        .position(|(_, l)| l.trim().starts_with("</mxCell>"));
                    let should_pending = is_open_cell
                        || (next_close.is_some()
                            && (next_open.is_none() || next_close.unwrap() < next_open.unwrap()));
                    if should_pending {
                        pending_close += 1;
                    }
                }
                cell_depth = if is_open_cell { 1 } else { 0 };
                new_lines.push(line.to_string());
            } else if is_close_cell {
                if pending_close > 0 {
                    pending_close -= 1;
                } else {
                    cell_depth = cell_depth.saturating_sub(1);
                    new_lines.push(line.to_string());
                }
            } else if starts_with_close && pending_close > 0 {
                pending_close -= 1;
                new_lines.push(line.replace("</mxCell>", ""));
            } else {
                new_lines.push(line.to_string());
            }
            i += 1;
        }
        if true_nested > 0 {
            fixed = new_lines.join("\n");
            fixes.push(format!("Fixed {true_nested} true nested mxCell(s)"));
        }
    }

    // 22. 修复重复 ID（加后缀；mxfile 跳过）
    if !Regex::new(r"<mxfile[\s>]")
        .expect("mxfile probe 22")
        .is_match(&fixed)
    {
        static ID: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        static PARTS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let id_re =
            ID.get_or_init(|| Regex::new(r#"(?i)\bid\s*=\s*["']([^"']+)["']"#).expect("id 22"));
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut duplicate: HashSet<String> = HashSet::new();
        for caps in id_re.captures_iter(&fixed) {
            let id = caps[1].to_string();
            let c = seen.entry(id.clone()).or_insert(0);
            *c += 1;
            if *c > 1 {
                duplicate.insert(id);
            }
        }
        if !duplicate.is_empty() {
            let parts = PARTS.get_or_init(|| {
                Regex::new(r#"(?i)(\bid\s*=\s*["'])([^"']+)(["'])"#).expect("id parts")
            });
            let mut counters: HashMap<String, usize> = HashMap::new();
            fixed = parts
                .replace_all(&fixed, |caps: &regex::Captures<'_>| {
                    let id = caps.get(2).expect("id").as_str();
                    if !duplicate.contains(id) {
                        return caps.get(0).expect("whole").as_str().to_string();
                    }
                    let c = counters.entry(id.to_string()).or_insert(0);
                    if *c == 0 {
                        *c = 1;
                        return caps.get(0).expect("whole").as_str().to_string();
                    }
                    let result = format!(
                        "{}{}_dup{}{}",
                        caps.get(1).expect("before").as_str(),
                        id,
                        c,
                        caps.get(3).expect("after").as_str()
                    );
                    *c += 1;
                    result
                })
                .to_string();
            fixes.push(format!("Renamed {} duplicate ID(s)", duplicate.len()));
        }
    }

    // 23. 修复空 id
    {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r#"<mxCell([^>]*)\sid\s*=\s*["']\s*["']([^>]*)>"#).expect("empty id fix")
        });
        let mut empty_count = 0usize;
        fixed = re
            .replace_all(&fixed, |caps: &regex::Captures<'_>| {
                empty_count += 1;
                let new_id = format!("cell_{}_{}", now_millis(), empty_count);
                format!(
                    "<mxCell{} id=\"{}\"{}>",
                    caps.get(1).expect("before").as_str(),
                    new_id,
                    caps.get(2).expect("after").as_str()
                )
            })
            .to_string();
        if empty_count > 0 {
            fixes.push(format!("Generated {empty_count} missing ID(s)"));
        }
    }

    (fixed, fixes)
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ============================================================================
// 组合校验与修复
// ============================================================================

/// 校验结果。
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    pub valid: bool,
    pub error: Option<String>,
    pub fixed: Option<String>,
    pub fixes: Vec<String>,
}

/// 先校验；不合法则尝试修复后再校验。默认严格（未知元素、孤立 mxPoint 均拒绝）。
pub fn validate_and_fix_xml(xml: &str, strict: bool) -> ValidationResult {
    if validate_mx_cell_structure(xml, strict).is_none() {
        return ValidationResult {
            valid: true,
            ..Default::default()
        };
    }
    let (fixed, fixes) = auto_fix_xml(xml);
    let error = validate_mx_cell_structure(&fixed, strict);
    if error.is_none() {
        return ValidationResult {
            valid: true,
            error: None,
            fixed: Some(fixed),
            fixes,
        };
    }
    ValidationResult {
        valid: false,
        error,
        fixed: if fixes.is_empty() { None } else { Some(fixed) },
        fixes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_json_escaped() {
        let input = r#"<mxGraphModel><root><mxCell id=\"2\" value=\"A\"/></root></mxGraphModel>"#;
        let (fixed, fixes) = auto_fix_xml(input);
        assert!(fixed.contains(r#"id="2""#));
        assert!(fixes.iter().any(|f| f.contains("JSON-escaped")));
    }

    #[test]
    fn fixes_unescaped_ampersand() {
        let (fixed, fixes) = auto_fix_xml(r#"<mxCell id="2" value="a & b"/>"#);
        assert!(fixed.contains("a &amp; b"));
        assert!(fixes.iter().any(|f| f.contains("&")));
    }

    #[test]
    fn fixes_duplicate_ids_non_mxfile() {
        let (fixed, fixes) = auto_fix_xml(
            r#"<mxGraphModel><root><mxCell id="2"/><mxCell id="2"/></root></mxGraphModel>"#,
        );
        assert!(fixed.contains("2_dup1"));
        assert!(fixes.iter().any(|f| f.contains("duplicate ID")));
    }

    #[test]
    fn validate_then_fix_roundtrip() {
        // 非法输入（未转义 &），应先修复再通过
        let bad = r#"<mxGraphModel><root><mxCell id="2" value="a & b"/></root></mxGraphModel>"#;
        let r = validate_and_fix_xml(bad, true);
        assert!(r.valid);
        assert!(r.fixed.is_some());
    }

    #[test]
    fn valid_input_passes_unchanged() {
        let ok = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="A" vertex="1" parent="1"/></root></mxGraphModel>"#;
        let r = validate_and_fix_xml(ok, true);
        assert!(r.valid);
        assert!(r.fixed.is_none());
    }

    #[test]
    fn repair_quote_before_slash_works() {
        assert_eq!(
            repair_quote_before_slash(r#"<mxCell id="2" x="40" y="40" w="120" h="60/>"#),
            r#"<mxCell id="2" x="40" y="40" w="120" h="60"/>"#
        );
    }
}
