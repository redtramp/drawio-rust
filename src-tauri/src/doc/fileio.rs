//! 文档服务（.drawio 文件读写，S3）。
//!
//! `.drawio` 为 `<mxfile>`，页内容可为明文 `<mxGraphModel>` 或 draw.io 的压缩
//! 格式（URI 编码 → raw deflate → base64）。读取时解压所有页；写出默认明文。
//! 也接受裸 `<mxGraphModel>` 与 `.drawio.svg`（可编辑 SVG）。

use std::path::Path;

use crate::ai::pages::{
    decompress_page_content, is_mx_file, is_mx_graph_model, normalize_to_mxfile, parse_mxfile,
    serialize_mxfile,
};
use crate::ai::xml_syntax::get_xml_syntax_error;
use crate::ai::xml_tree::{parse_lenient, XElement, XmlNode};
use crate::error::Error;

/// 移除 XML prolog（声明 / 注释 / DOCTYPE）。
fn strip_xml_prolog(s: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"^(\s|<\?[\s\S]*?\?>|<!--[\s\S]*?-->|<!DOCTYPE[^>\[]*(\[[\s\S]*?\])?\s*>)*",
        )
        .expect("xml prolog regex")
    });
    re.replace(s, "").to_string()
}

/// 提取 .drawio.svg 根元素 content 属性里的内嵌 XML；非 SVG 返回 None。
fn extract_svg_embedded_xml(text: &str) -> Option<String> {
    let doc = parse_lenient(text)?;
    let root_name = doc.name.split(':').next_back().unwrap_or(&doc.name);
    if root_name != "svg" {
        return None;
    }
    doc.attr("content").map(|s| s.trim().to_string())
}

/// 解析 .drawio 文件内容到规范 mxfile（所有页明文）。返回 (xml, 是否含压缩页)。
pub fn parse_drawio_file_content(content: &str) -> Result<(String, bool), Error> {
    let trimmed = strip_xml_prolog(content).trim().to_string();
    if trimmed.is_empty() {
        return Err(Error::Document("File is empty.".to_string()));
    }
    let trimmed = if is_mx_graph_model(&trimmed) {
        normalize_to_mxfile(&trimmed, None, None)
            .ok_or_else(|| Error::Document("Failed to wrap <mxGraphModel>".to_string()))?
    } else {
        trimmed
    };
    if !is_mx_file(&trimmed) {
        let embedded = extract_svg_embedded_xml(content);
        let Some(embedded) = embedded else {
            return Err(Error::Document(
                "Not a draw.io file: expected an <mxfile> or <mxGraphModel> root element, or a .drawio.svg with the diagram embedded."
                    .to_string(),
            ));
        };
        if embedded.is_empty() {
            return Err(Error::Document(
                "This SVG has no embedded diagram. Only .drawio.svg (Editable SVG) files can be loaded."
                    .to_string(),
            ));
        }
        if is_mx_file(&embedded) || is_mx_graph_model(&embedded) {
            return parse_drawio_file_content(&embedded);
        }
        return Err(Error::Document(
            "The SVG's embedded content is not draw.io XML.".to_string(),
        ));
    }

    let mut doc = parse_mxfile(&trimmed)
        .ok_or_else(|| Error::Document("Failed to parse <mxfile> XML.".to_string()))?;

    let mut decompressed_any = false;
    let diagrams = doc.by_name("diagram").len();
    for idx in 0..diagrams {
        let diagram = get_diagram_mut(&mut doc, idx).expect("diagram exists");
        if !diagram.by_name("mxGraphModel").is_empty() {
            continue;
        }
        let text = diagram.text_content().trim().to_string();
        if text.is_empty() {
            continue;
        }
        let page_label = diagram
            .attr("name")
            .or_else(|| diagram.attr("id"))
            .unwrap_or("unnamed")
            .to_string();
        let xml = decompress_page_content(&text).ok_or_else(|| {
            Error::Document(format!(
                "Page \"{page_label}\" has content that is neither plain <mxGraphModel> XML nor draw.io's compressed format."
            ))
        })?;
        if !is_mx_graph_model(&xml) || get_xml_syntax_error(&xml).is_some() {
            return Err(Error::Document(format!(
                "Page \"{page_label}\" decompressed but its XML failed to parse."
            )));
        }
        let inner = crate::ai::xml_tree::parse_strict(&xml)
            .map_err(|e| Error::Document(format!("Page \"{page_label}\" failed to parse: {e}")))?;
        if inner.name != "mxGraphModel" {
            return Err(Error::Document(format!(
                "Page \"{page_label}\" decompressed but is not a <mxGraphModel>."
            )));
        }
        diagram.children.clear();
        diagram.children.push(XmlNode::Element(inner));
        decompressed_any = true;
    }

    let xml = if decompressed_any {
        serialize_mxfile(&doc)
    } else {
        trimmed
    };
    Ok((xml, decompressed_any))
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

/// 读取 .drawio 文件，返回 (规范化 XML, 是否含压缩页)。
pub fn read_drawio(path: &Path) -> Result<(String, bool), Error> {
    let content = std::fs::read_to_string(path).map_err(Error::Io)?;
    parse_drawio_file_content(&content)
}

/// 写出 .drawio 文件（明文 mxfile）。
pub fn write_drawio(path: &Path, xml: &str) -> Result<(), Error> {
    std::fs::write(path, xml.as_bytes()).map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_mxfile() {
        let xml = r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/></root></mxGraphModel></diagram></mxfile>"#;
        let (out, had_compressed) = parse_drawio_file_content(xml).unwrap();
        assert!(!had_compressed);
        assert!(out.contains("mxfile"));
    }

    #[test]
    fn wraps_bare_model() {
        let xml = r#"<mxGraphModel><root><mxCell id="0"/></root></mxGraphModel>"#;
        let (out, _) = parse_drawio_file_content(xml).unwrap();
        assert!(out.contains("<mxfile"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_drawio_file_content("not xml at all").is_err());
        assert!(parse_drawio_file_content("").is_err());
    }

    #[test]
    fn parses_compressed_page() {
        use base64::Engine as _;
        use flate2::write::DeflateEncoder;
        use std::io::Write;
        let model = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="A"/></root></mxGraphModel>"#;
        let mut enc = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(model.as_bytes()).expect("deflate");
        let compressed = enc.finish().expect("finish");
        let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
        let xml = format!(r#"<mxfile><diagram id="p1" name="Page-1">{b64}</diagram></mxfile>"#);
        let (out, had) = parse_drawio_file_content(&xml).unwrap();
        assert!(had);
        assert!(out.contains("mxGraphModel"));
        assert!(out.contains(r#"id="2""#));
        assert!(!out.contains(b64.as_str()));
    }

    #[test]
    fn write_then_read_roundtrip() {
        let xml = r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/></root></mxGraphModel></diagram></mxfile>"#;
        let dir = std::env::temp_dir().join(format!("drawio-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.drawio");
        write_drawio(&path, xml).unwrap();
        let (out, _) = read_drawio(&path).unwrap();
        assert!(out.contains("mxfile"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parses_embedded_svg() {
        // .drawio.svg：content 属性内嵌 mxfile（含 HTML 实体转义）
        let embedded = r#"<mxfile><diagram id="p1"><mxGraphModel><root><mxCell id="0"/></root></mxGraphModel></diagram></mxfile>"#;
        let escaped = embedded
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" content="{escaped}"></svg>"#);
        let (out, _) = parse_drawio_file_content(&svg).unwrap();
        assert!(out.contains("mxfile"));
    }

    #[test]
    fn rejects_svg_without_embedded_diagram() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        assert!(parse_drawio_file_content(svg).is_err());
        // content 为空
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" content=""></svg>"#;
        assert!(parse_drawio_file_content(svg).is_err());
    }

    #[test]
    fn rejects_svg_with_non_drawio_content() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" content="not drawio at all"></svg>"#;
        assert!(parse_drawio_file_content(svg).is_err());
    }

    #[test]
    fn strips_prolog() {
        assert_eq!(
            strip_xml_prolog(r#"<?xml version="1.0"?><!-- comment --><mxfile/>"#),
            "<mxfile/>"
        );
    }
}
