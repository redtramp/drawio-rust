//! 命名样式（上游 `style-classes.ts`）。
//!
//! 模型用 `<mxStyle name="..." value="..."/>` 一次性定义、在 cell 的 style 里
//! 按名字引用，类似 CSS class。此处展开名字、按名字补 draw.io 默认样式
//! （`html=1` / `whiteSpace=wrap`）。上游对照见文件头注释。

use std::collections::{BTreeMap, HashMap, HashSet};

use regex::Regex;

use super::xml_attributes::{escape_for_attribute, read_attributes};

/// 命名样式表：name → 定义（已去尾分号、已做属性转义）。
pub type StyleClasses = BTreeMap<String, String>;

// <mxStyle .../> 连同其后的空白（也接受 <mxStyle ...>...</mxStyle>）；引号值可含 ">"
fn style_definition_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        // 上游: /<mxStyle\b((?:[^<>"']|"[^"]*"|'[^']*')*?)\s*\/?>(?:\s*<\/mxStyle>)?\s*/gi
        Regex::new(r#"<mxStyle\b(([^<>"']|"[^"]*"|'[^']*')*?)\s*/?>(\s*</mxStyle>)?\s*"#)
            .expect("style definition regex")
    })
}

// 一个 mxCell 开标签；属性在闭合 "/" 前结束，因此自闭合 cell 保留其 "/>"
fn cell_tag_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        // 上游: /<mxCell\b((?:[^<>"']|"[^"]*"|'[^']*')*?)\s*(\/?)>/g
        Regex::new(r#"<mxCell\b(([^<>"']|"[^"]*"|'[^']*')*?)\s*(/?)>"#).expect("cell tag regex")
    })
}

/// 是否在开头或 `;` 后设置了 `key=`（精确匹配，`(^|;)key=`）。
fn has_key(style: &str, key: &str) -> bool {
    let bytes = style.as_bytes();
    let mut pos = 0;
    while pos + key.len() <= bytes.len() {
        match style[pos..].find(key) {
            None => break,
            Some(idx) => {
                let start = pos + idx;
                let after = start + key.len();
                let before_ok = start == 0 || bytes[start - 1] == b';';
                let after_ok = bytes.get(after) == Some(&b'=');
                if before_ok && after_ok {
                    return true;
                }
                pos = start + key.len().max(1);
            }
        }
    }
    false
}

/// 是否含这个精确 token（`style.split(";").includes(token)`）。
fn has_token(style: &str, token: &str) -> bool {
    style.split(';').any(|t| t == token)
}

/// 把 style 中的名字替换为其定义。token 保持原样（含空 token：开头的 `;` 是
/// draw.io 的"无默认样式"标记）。定义为空的名字被丢弃。名字展开后若结果不含
/// `&` 则去除被覆盖的 key（`&` 可能来自 `&quot;` 等实体，避免误拆）。
fn apply_to_style(style: &str, classes: &StyleClasses) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut expanded = false;
    for token in style.split(';') {
        let definition = if token.contains('=') {
            None
        } else {
            classes.get(token.trim())
        };
        match definition {
            None => out.push(token.to_string()),
            Some(def) => {
                expanded = true;
                if !def.is_empty() {
                    out.push(def.clone());
                }
            }
        }
    }
    let result = out.join(";");
    if expanded && !result.contains('&') {
        drop_overridden(&result)
    } else {
        result
    }
}

/// 每个 key 只保留最后一次设置（名字展开在覆盖之后会留下重复 key）。
fn drop_overridden(style: &str) -> String {
    fn key_of(t: &str) -> Option<&str> {
        t.find('=').map(|i| &t[..i])
    }
    let tokens: Vec<&str> = style.split(';').collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let overridden = match key_of(token) {
            None => false,
            Some(k) => tokens[i + 1..].iter().any(|later| key_of(later) == Some(k)),
        };
        if !overridden {
            out.push(token);
        }
    }
    out.join(";")
}

/// 提取命名样式定义，返回 (样式表, 去掉定义后的 XML)。两轮解析支持定义间
/// 相互引用（不分前后）；自引用保持为裸 token。
pub fn read_style_classes(xml: &str) -> (StyleClasses, String) {
    let re = style_definition_re();
    let mut classes: StyleClasses = BTreeMap::new();
    let mut rest = String::with_capacity(xml.len());
    let mut last = 0;
    for caps in re.captures_iter(xml) {
        let m = caps.get(0).unwrap();
        rest.push_str(&xml[last..m.start()]);
        let attr_text = &caps[1];
        let attrs = read_attributes(attr_text);
        let name = attrs
            .iter()
            .find(|a| a.name == "name")
            .map(|a| a.value.trim().trim_end_matches(';').to_string());
        let value = attrs
            .iter()
            .find(|a| a.name == "value")
            .or_else(|| attrs.iter().find(|a| a.name == "style"))
            .map(|a| a.value.clone());
        if let (Some(name), Some(value)) = (name, value) {
            if !name.is_empty() {
                let v = escape_for_attribute(&value)
                    .trim_end_matches(';')
                    .to_string();
                classes.insert(name, v);
            }
        }
        last = m.end();
    }
    rest.push_str(&xml[last..]);
    // 两轮展开，解析定义间引用（在任一方向）
    for _ in 0..2 {
        let snapshot: Vec<(String, String)> = classes
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (name, value) in snapshot {
            let mut others = classes.clone();
            others.remove(&name);
            classes.insert(name, apply_to_style(&value, &others));
        }
    }
    (classes, rest)
}

/// 重写每个 mxCell 的 style 属性。回调返回 None 或与原值相同则不动该 cell；
/// 无 style 属性的 cell 在回调返回新 style 时补上。
fn rewrite_cell_styles<F>(xml: &str, mut rewrite: F) -> String
where
    F: FnMut(Option<&str>, &HashMap<String, String>) -> Option<String>,
{
    let re = cell_tag_re();
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    for caps in re.captures_iter(xml) {
        let m = caps.get(0).unwrap();
        out.push_str(&xml[last..m.start()]);
        let attr_text = &caps[1];
        let self_close = caps.get(3).map(|c| c.as_str()).unwrap_or("");
        let attributes = read_attributes(attr_text);
        let attrs: HashMap<String, String> = attributes
            .iter()
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect();
        let style = attributes.iter().find(|a| a.name == "style");
        let next = rewrite(style.map(|s| s.value.as_str()), &attrs);
        match next {
            None => out.push_str(m.as_str()),
            Some(n) if style.map(|s| s.value.as_str()) == Some(n.as_str()) => {
                out.push_str(m.as_str())
            }
            Some(n) => {
                let written = format!("style=\"{}\"", n.replace('"', "&quot;"));
                match style {
                    None => {
                        out.push_str(&format!("<mxCell{} {}{}>", attr_text, written, self_close))
                    }
                    Some(s) => {
                        let text = &attr_text[s.start..s.end];
                        let trimmed = text.trim_start();
                        let space = &text[..text.len() - trimmed.len()];
                        out.push_str(&format!(
                            "<mxCell{}{}{}{}{}>",
                            &attr_text[..s.start],
                            space,
                            written,
                            &attr_text[s.end..],
                            self_close
                        ));
                    }
                }
            }
        }
        last = m.end();
    }
    out.push_str(&xml[last..]);
    out
}

/// 展开每个 cell style 中的名字；名字后的覆盖仍生效。
pub fn apply_style_classes(xml: &str, classes: &StyleClasses) -> String {
    if classes.is_empty() {
        return xml.to_string();
    }
    rewrite_cell_styles(xml, |style, _| style.map(|s| apply_to_style(s, classes)))
}

/// XML 中 edge cell 的 id 集合（其子顶点是 edge 标签）。
pub fn edge_ids_of(xml: &str) -> HashSet<String> {
    let re = cell_tag_re();
    let mut ids = HashSet::new();
    for caps in re.captures_iter(xml) {
        let attr_text = &caps[1];
        let attrs = read_attributes(attr_text);
        let id = attrs
            .iter()
            .find(|a| a.name == "id")
            .map(|a| a.value.as_str());
        let edge = attrs
            .iter()
            .find(|a| a.name == "edge")
            .map(|a| a.value.as_str());
        if let (Some(id), Some("1")) = (id, edge) {
            if !id.is_empty() {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

/// 标签是否在形状外部（图标库/图片），此时不应按形状宽度换行。
fn has_label_outside(style: &str) -> bool {
    has_key(style, "verticalLabelPosition")
        || has_key(style, "labelPosition")
        || has_key(style, "image")
        || has_token(style, "shape=image")
}

/// 补 draw.io 的默认样式：`html=1`（全部 vertex/edge）、`whiteSpace=wrap`
/// （仅 vertex，且标签在内、非自适/纯 html）。root cell 与已设 key 者不动。
/// 编辑在页上新增 cell 时，调用方传入页上已有 edge 的 id，避免把 edge 标签
/// 误判为 shape 而换行。
pub fn add_default_styles(xml: &str, known_edge_ids: &HashSet<String>) -> String {
    let mut edges: HashSet<String> = known_edge_ids.clone();
    edges.extend(edge_ids_of(xml));
    rewrite_cell_styles(xml, |style, attrs| {
        let is_edge = attrs.get("edge").map(|v| v == "1").unwrap_or(false);
        let is_vertex = attrs.get("vertex").map(|v| v == "1").unwrap_or(false) && !is_edge;
        if !is_vertex && !is_edge {
            return None;
        }
        let current = style.unwrap_or("");
        let is_edge_label = has_token(current, "edgeLabel")
            || attrs.get("connectable").map(|v| v == "0").unwrap_or(false)
            || attrs
                .get("parent")
                .map(|p| edges.contains(p))
                .unwrap_or(false);
        let mut added: Vec<&str> = Vec::new();
        if is_vertex
            && !has_key(current, "whiteSpace")
            && !has_label_outside(current)
            && !is_edge_label
            && !has_token(current, "autosize=1")
            && !has_token(current, "html=0")
        {
            added.push("whiteSpace=wrap");
        }
        if !has_key(current, "html") {
            added.push("html=1");
        }
        if added.is_empty() {
            return None;
        }
        let separator = if current.is_empty() || current.ends_with(';') {
            ""
        } else {
            ";"
        };
        Some(format!("{}{}{};", current, separator, added.join(";")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_style_classes_and_expands() {
        let xml = r#"<mxStyle name="step" value="fillColor=#dae8fc;strokeColor=#6c8ebf;"/>
<mxCell id="2" value="Start" style="rounded=1;step;" vertex="1" parent="1"/>"#;
        let (classes, rest) = read_style_classes(xml);
        assert_eq!(
            classes.get("step").unwrap(),
            "fillColor=#dae8fc;strokeColor=#6c8ebf"
        );
        assert!(!rest.contains("mxStyle"));
        let expanded = apply_style_classes(&rest, &classes);
        assert!(expanded.contains("rounded=1;fillColor=#dae8fc;strokeColor=#6c8ebf;"));
    }

    #[test]
    fn adds_default_styles() {
        let xml = r#"<mxCell id="2" value="A" style="rounded=1;" vertex="1" parent="1"/>"#;
        let out = add_default_styles(xml, &HashSet::new());
        assert!(out.contains("html=1"));
        assert!(out.contains("whiteSpace=wrap"));
    }

    #[test]
    fn edge_gets_html_only() {
        let xml = r#"<mxCell id="3" style="edgeStyle=orthogonalEdgeStyle;" edge="1" parent="1" source="2" target="4"/>"#;
        let out = add_default_styles(xml, &HashSet::new());
        assert!(out.contains("html=1"));
        assert!(!out.contains("whiteSpace=wrap"));
    }

    #[test]
    fn drop_overridden_keeps_last() {
        assert_eq!(
            drop_overridden("fillColor=#a;fillColor=#b;strokeColor=#c"),
            "fillColor=#b;strokeColor=#c"
        );
    }

    #[test]
    fn has_key_exact() {
        assert!(has_key(";whiteSpace=wrap", "whiteSpace"));
        assert!(!has_key(";mywhiteSpace=wrap", "whiteSpace"));
        assert!(has_key("html=1;rounded=1", "html"));
    }
}
