//! 紧凑 cell 展开/折叠（上游 `compact-cells.ts`）。
//!
//! 模型写 cell 的短形式（shape 带 x/y/w/h，edge 带 source/target），此处补上
//! draw.io 的固定部分（vertex/edge/parent/mxGeometry）。长形式仍被接受。
//! `fold_cells` 是反向：把明文 shape/edge 写成短形式给模型看。

use std::collections::HashMap;

use regex::Regex;

use super::xml_attributes::read_attributes;

// 整个 mxCell（自闭合，或带子节点——cell 从不嵌套）
fn cell_block_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        // 上游: /<mxCell\b((?:[^<>"']|"[^"]*"|'[^']*')*?)\s*(?:\/>|>([\s\S]*?)<\/mxCell>)/g
        // 组: 1=attrText, 2=内部单字符(忽略), 3=/>|>...</mxCell>, 4=body
        Regex::new(r#"<mxCell\b(([^<>"']|"[^"]*"|'[^']*')*?)\s*(/>|>([\s\S]*?)</mxCell>)"#)
            .expect("cell block regex")
    })
}

// cell 自己的 geometry（区别于自定义数据里的 geometry）
fn own_geometry_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<mxGeometry\b([^<>"']|"[^"]*"|'[^']*')*?\bas="geometry""#)
            .expect("own geometry regex")
    })
}

fn is_size(name: &str) -> bool {
    matches!(name, "x" | "y" | "w" | "h" | "width" | "height")
}

const FIRST: [&str; 3] = ["id", "value", "style"];
const LAST: [&str; 5] = ["vertex", "edge", "parent", "source", "target"];

fn attr(name: &str, value: &str) -> String {
    format!(" {}=\"{}\"", name, value.replace('"', "&quot;"))
}

/// 页第一个 layer（parent="0" 的 cell）的 id，缺省为 "1"。
pub fn default_layer_of(xml: &str) -> String {
    let re = cell_block_re();
    for caps in re.captures_iter(xml) {
        let attr_text = &caps[1];
        let attrs = read_attributes(attr_text);
        let id = attrs
            .iter()
            .find(|a| a.name == "id")
            .map(|a| a.value.as_str());
        let parent = attrs
            .iter()
            .find(|a| a.name == "parent")
            .map(|a| a.value.as_str());
        if parent == Some("0") {
            if let Some(id) = id {
                if !id.is_empty() {
                    return id.to_string();
                }
            }
        }
    }
    "1".to_string()
}

/// 把紧凑 shape/edge 转成标准 draw.io cell。无 parent 的 cell 放到 `layer`。
pub fn expand_compact_cells(xml: &str, layer: &str) -> String {
    let re = cell_block_re();
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    for caps in re.captures_iter(xml) {
        let m = caps.get(0).unwrap();
        out.push_str(&xml[last..m.start()]);
        let attr_text = &caps[1];
        let body = caps.get(4).map(|c| c.as_str());
        let attrs = read_attributes(attr_text);
        let by_name: HashMap<String, String> = attrs
            .iter()
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect();
        let size = attrs.iter().filter(|a| is_size(&a.name)).count();
        let inner = body.unwrap_or("");
        let has_geometry = own_geometry_re().is_match(inner);
        let is_edge = by_name.get("edge").map(|v| v == "1").unwrap_or(false)
            || (by_name.get("vertex").map(|v| v != "1").unwrap_or(true)
                && (by_name.contains_key("source") || by_name.contains_key("target")));
        let is_vertex = !is_edge
            && (by_name.get("vertex").map(|v| v == "1").unwrap_or(false)
                || size > 0
                || has_geometry);
        // root cell 或已完整书写的 cell 不动
        if !is_edge && !is_vertex {
            out.push_str(m.as_str());
            last = m.end();
            continue;
        }
        let complete = size == 0
            && by_name.contains_key(if is_edge { "edge" } else { "vertex" })
            && by_name.contains_key("parent")
            && has_geometry;
        if complete {
            out.push_str(m.as_str());
            last = m.end();
            continue;
        }
        let consumed = !has_geometry && is_vertex;
        let rest: Vec<&super::xml_attributes::TagAttribute> = attrs
            .iter()
            .filter(|a| {
                !FIRST.contains(&a.name.as_str())
                    && !LAST.contains(&a.name.as_str())
                    && !(consumed && is_size(&a.name))
                    && !(is_edge && is_size(&a.name))
            })
            .collect();
        let mut ordered: Vec<(String, String)> = Vec::new();
        for n in FIRST {
            if let Some(v) = by_name.get(n) {
                ordered.push((n.to_string(), v.clone()));
            }
        }
        for a in &rest {
            ordered.push((a.name.clone(), a.value.clone()));
        }
        ordered.push((
            if is_edge { "edge" } else { "vertex" }.to_string(),
            "1".to_string(),
        ));
        ordered.push((
            "parent".to_string(),
            by_name
                .get("parent")
                .cloned()
                .unwrap_or_else(|| layer.to_string()),
        ));
        for n in ["source", "target"] {
            if let Some(v) = by_name.get(n) {
                ordered.push((n.to_string(), v.clone()));
            }
        }
        let written: String = ordered.iter().map(|(n, v)| attr(n, v)).collect();
        let mut geometry = String::new();
        if !has_geometry {
            if is_edge {
                geometry = r#"<mxGeometry relative="1" as="geometry"/>"#.to_string();
            } else {
                let get = |short: &str, long: &str, fallback: &str| -> String {
                    by_name
                        .get(short)
                        .or_else(|| by_name.get(long))
                        .cloned()
                        .unwrap_or_else(|| fallback.to_string())
                };
                geometry = format!(
                    "<mxGeometry{}{}{}{} as=\"geometry\"/>",
                    attr("x", &get("x", "x", "0")),
                    attr("y", &get("y", "y", "0")),
                    attr("width", &get("w", "width", "120")),
                    attr("height", &get("h", "height", "60")),
                );
            }
        }
        out.push_str(&format!(
            "<mxCell{}>{}{}</mxCell>",
            written, geometry, inner
        ));
        last = m.end();
    }
    out.push_str(&xml[last..]);
    out
}

/// 单个自闭合 mxGeometry 的属性（`as="geometry"`），否则 None。
fn plain_geometry(body: &str) -> Option<HashMap<String, String>> {
    let trimmed = body.trim();
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"^<mxGeometry\b(([^<>"']|"[^"]*"|'[^']*')*?)\s*/>$"#)
            .expect("plain geometry regex")
    });
    let caps = re.captures(trimmed)?;
    let attr_text = caps.get(1).unwrap().as_str();
    let attrs: HashMap<String, String> = read_attributes(attr_text)
        .into_iter()
        .map(|a| (a.name, a.value))
        .collect();
    if attrs.get("as").map(|v| v.as_str()) == Some("geometry") {
        Some(attrs)
    } else {
        None
    }
}

/// 把明文 shape/edge 写成短形式（给模型看）。其余内容原样保留；内嵌图片数据
/// 在 `hide_image_data` 里已替换为引用（见 embedded_images）。
pub fn fold_cells(xml: &str) -> String {
    let re = cell_block_re();
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    for caps in re.captures_iter(xml) {
        let m = caps.get(0).unwrap();
        out.push_str(&xml[last..m.start()]);
        let attr_text = &caps[1];
        let body = caps.get(4).map(|c| c.as_str());
        let Some(body) = body else {
            out.push_str(m.as_str());
            last = m.end();
            continue;
        };
        let attrs = read_attributes(attr_text);
        let by_name: HashMap<String, String> = attrs
            .iter()
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect();
        if attrs.iter().any(|a| is_size(&a.name)) {
            out.push_str(m.as_str());
            last = m.end();
            continue;
        }
        if !by_name.contains_key("parent") {
            out.push_str(m.as_str());
            last = m.end();
            continue;
        }
        let Some(geometry) = plain_geometry(body) else {
            out.push_str(m.as_str());
            last = m.end();
            continue;
        };
        let mut geometry_keys: Vec<&str> = geometry
            .keys()
            .filter(|k| k.as_str() != "as")
            .map(|s| s.as_str())
            .collect();
        geometry_keys.sort_unstable();
        let kept: Vec<(&str, &str)> = attrs
            .iter()
            .filter(|a| {
                a.name != "vertex" && a.name != "edge" && !(a.name == "parent" && a.value == "1")
            })
            .map(|a| (a.name.as_str(), a.value.as_str()))
            .collect();
        let is_vertex = by_name.get("vertex").map(|v| v == "1").unwrap_or(false)
            && by_name.get("edge").map(|v| v != "1").unwrap_or(true);
        if is_vertex {
            let known: Vec<&str> = geometry_keys
                .iter()
                .copied()
                .filter(|k| *k != "x" && *k != "y")
                .collect();
            if known != vec!["height", "width"] {
                out.push_str(m.as_str());
                last = m.end();
                continue;
            }
            let size = format!(
                "{}{}{}{}",
                attr("x", geometry.get("x").map(|s| s.as_str()).unwrap_or("0")),
                attr("y", geometry.get("y").map(|s| s.as_str()).unwrap_or("0")),
                attr("w", geometry.get("width").map(|s| s.as_str()).unwrap_or("")),
                attr(
                    "h",
                    geometry.get("height").map(|s| s.as_str()).unwrap_or("")
                ),
            );
            let attrs_s: String = kept.iter().map(|(n, v)| attr(n, v)).collect();
            out.push_str(&format!("<mxCell{}{}/>", attrs_s, size));
        } else if by_name.get("edge").map(|v| v == "1").unwrap_or(false)
            && by_name.get("vertex").map(|v| v != "1").unwrap_or(true)
        {
            let geometry_keys_s: Vec<&str> = {
                let mut k = geometry_keys.clone();
                k.sort_unstable();
                k
            };
            if geometry_keys_s != vec!["relative"]
                || geometry.get("relative").map(|s| s.as_str()) != Some("1")
            {
                out.push_str(m.as_str());
                last = m.end();
                continue;
            }
            if !by_name.contains_key("source") || !by_name.contains_key("target") {
                out.push_str(m.as_str());
                last = m.end();
                continue;
            }
            let attrs_s: String = kept.iter().map(|(n, v)| attr(n, v)).collect();
            out.push_str(&format!("<mxCell{}/>", attrs_s));
        } else {
            out.push_str(m.as_str());
        }
        last = m.end();
    }
    out.push_str(&xml[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_compact_shape() {
        let xml = r#"<mxCell id="2" value="Start" style="step;" x="40" y="40" w="120" h="60"/>"#;
        let out = expand_compact_cells(xml, "1");
        assert!(out.contains(r#"vertex="1" parent="1""#));
        assert!(
            out.contains(r#"<mxGeometry x="40" y="40" width="120" height="60" as="geometry"/>"#)
        );
    }

    #[test]
    fn expands_compact_edge() {
        let xml = r#"<mxCell id="5" style="down;" source="2" target="3"/>"#;
        let out = expand_compact_cells(xml, "1");
        assert!(out.contains(r#"edge="1" parent="1" source="2" target="3""#));
        assert!(out.contains(r#"<mxGeometry relative="1" as="geometry"/>"#));
    }

    #[test]
    fn fold_roundtrips_plain_shape() {
        let expanded = r#"<mxCell id="2" value="A" style="rounded=1;" vertex="1" parent="1"><mxGeometry x="100" y="100" width="120" height="60" as="geometry"/></mxCell>"#;
        let folded = fold_cells(expanded);
        assert!(folded.contains(
            r#"<mxCell id="2" value="A" style="rounded=1;" x="100" y="100" w="120" h="60"/>"#
        ));
        // 折叠再展开回到等价 cell
        let back = expand_compact_cells(&folded, "1");
        assert!(back.contains(r#"vertex="1" parent="1""#));
    }

    #[test]
    fn default_layer_finds_first_layer() {
        let xml = r#"<mxCell id="0"/><mxCell id="bg" parent="0"/><mxCell id="2" parent="bg"/>"#;
        assert_eq!(default_layer_of(xml), "bg");
    }
}
