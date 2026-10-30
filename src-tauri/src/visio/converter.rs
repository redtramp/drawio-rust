//! Visio 兼容 SVG 转换（上游 `fix_svg_for_visio.py` 的 Rust 移植，零外部依赖）。
//!
//! draw.io 导出的 SVG 用 `<foreignObject>` 承载文本，Visio 无法正确导入。
//! 本模块把 `<switch>` 内的 foreignObject/div 文本提取为标准 `<text>` 元素。
//! 对齐要点与 Python 原版逐条对应。

use crate::ai::xml_tree::{serialize, XElement, XmlNode};
use crate::error::Error;

/// 从 CSS 内联样式精确取值（`key: value;`，key 前为开头/`;`/空白）。
fn css_value(style: &str, key: &str) -> Option<String> {
    let mut search = 0;
    while let Some(idx) = style[search..].find(key) {
        let start = search + idx;
        let before_ok = start == 0
            || matches!(
                style.as_bytes().get(start.wrapping_sub(1)),
                Some(b';' | b' ' | b'\t' | b'\n' | b'\r') | None
            );
        let after = start + key.len();
        if before_ok && style.as_bytes().get(after) == Some(&b':') {
            let rest = &style[after + 1..];
            let end = rest.find(';').unwrap_or(rest.len());
            let v = rest[..end].trim();
            return if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
        }
        search = start + key.len().max(1);
    }
    None
}

/// 提取 `key: <digits>px` 中的数字（无匹配返回 0）。
fn css_px(style: &str, key: &str) -> f64 {
    css_value(style, key)
        .and_then(|v| {
            let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().ok()
        })
        .unwrap_or(0.0)
}

/// 解析颜色：处理 `light-dark(#c1, #c2)`（取第一个）与普通 `#hex`，缺省黑。
fn parse_color(raw: &str) -> String {
    if raw.is_empty() {
        return "#000000".to_string();
    }
    static LIGHTDARK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static HEX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let lightdark = LIGHTDARK.get_or_init(|| {
        regex::Regex::new(r"light-dark\(\s*(#[0-9a-fA-F]{3,6})\s*,").expect("light-dark regex")
    });
    if let Some(c) = lightdark.captures(raw) {
        return c[1].to_string();
    }
    let hex = HEX.get_or_init(|| regex::Regex::new(r"(#[0-9a-fA-F]{3,6})").expect("hex regex"));
    hex.captures(raw)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| "#000000".to_string())
}

struct TextInfo {
    text: String,
    x: f64,
    y: f64,
    font_size: String,
    font_family: String,
    font_weight: String,
    color: String,
    text_anchor: String,
}

/// 从 `<switch>` 提取文本信息；无 foreignObject/文本则 None。
fn extract_text_info(switch: &XElement) -> Option<TextInfo> {
    let fo = *switch.by_name("foreignObject").first()?;
    let outer_div = *fo.by_name("div").first()?;
    let outer_style = outer_div.attr("style").unwrap_or("");

    let padding_top = css_px(outer_style, "padding-top");
    let margin_left = css_px(outer_style, "margin-left");
    let width_val = css_px(outer_style, "width");

    let mut inner_div = *outer_div.by_name("div").first()?;
    let mut inner_style = inner_div.attr("style").unwrap_or("").to_string();
    if inner_style.contains("font-size: 0") || inner_style.contains("font-size:0") {
        if let Some(deeper) = inner_div.by_name("div").first() {
            inner_div = *deeper;
            inner_style = inner_div.attr("style").unwrap_or("").to_string();
        }
    }

    let font_size = css_value(&inner_style, "font-size").unwrap_or_else(|| "12px".to_string());
    let font_family = css_value(&inner_style, "font-family")
        .map(|f| f.split(',').next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Helvetica".to_string());
    let font_weight =
        css_value(&inner_style, "font-weight").unwrap_or_else(|| "normal".to_string());
    let color = css_value(&inner_style, "color")
        .map(|c| parse_color(&c))
        .unwrap_or_else(|| "#000000".to_string());
    let text_align = css_value(&inner_style, "text-align").unwrap_or_else(|| "center".to_string());

    let text = inner_div.text_content().trim().to_string();
    if text.is_empty() {
        return None;
    }

    let text_anchor = match text_align.as_str() {
        "left" => "start",
        "right" => "end",
        _ => "middle",
    };

    let x = if text_anchor == "middle" && width_val > 0.0 {
        margin_left + width_val / 2.0
    } else if text_anchor == "end" && width_val > 0.0 {
        margin_left + width_val
    } else {
        margin_left
    };
    let y = padding_top;

    Some(TextInfo {
        text,
        x,
        y,
        font_size,
        font_family,
        font_weight,
        color,
        text_anchor: text_anchor.to_string(),
    })
}

/// Python `str(float)` 语义：整数 float 保留 `.0`。
fn format_py_float(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

fn make_text_element(info: &TextInfo) -> XElement {
    let mut el = XElement::new("text");
    el.children.push(XmlNode::Text(info.text.clone()));
    el.set_attr("x", &format_py_float(info.x));
    el.set_attr("y", &format_py_float(info.y));
    el.set_attr("font-size", &info.font_size);
    el.set_attr("font-family", &info.font_family);
    el.set_attr("font-weight", &info.font_weight);
    el.set_attr("fill", &info.color);
    el.set_attr("text-anchor", &info.text_anchor);
    el.set_attr("pointer-events", "none");
    el.set_attr("dominant-baseline", "hanging");
    el
}

fn fix_element(el: &mut XElement, converted: &mut usize) {
    // 先递归处理后代
    for child in &mut el.children {
        if let XmlNode::Element(sub) = child {
            fix_element(sub, converted);
        }
    }
    // 替换直接子 switch
    let mut new_children: Vec<XmlNode> = Vec::with_capacity(el.children.len());
    for child in std::mem::take(&mut el.children) {
        match child {
            XmlNode::Element(sub) if sub.name == "switch" => {
                if let Some(info) = extract_text_info(&sub) {
                    new_children.push(XmlNode::Element(make_text_element(&info)));
                    *converted += 1;
                } else {
                    new_children.push(XmlNode::Element(sub));
                }
            }
            other => new_children.push(other),
        }
    }
    el.children = new_children;
}

/// 修复 SVG：把 `<switch>` 内文本转成 `<text>`。返回 (修复后 SVG, 转换数)。
pub fn fix_svg_for_visio(svg: &str) -> Result<(String, usize), Error> {
    let mut doc = crate::ai::xml_tree::parse_lenient(svg)
        .ok_or_else(|| Error::Export("failed to parse SVG".to_string()))?;
    if doc.name == "parsererror" {
        return Err(Error::Export("SVG is not well-formed XML".to_string()));
    }
    let mut converted = 0usize;
    fix_element(&mut doc, &mut converted);
    Ok((serialize(&doc), converted))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
  <g>
    <switch>
      <foreignObject style="overflow: visible; text-align: left;" pointer-events="none" width="100%" height="100%">
        <div xmlns="http://www.w3.org/1999/xhtml" style="display: flex; align-items: unsafe center; justify-content: unsafe center; width: 118px; height: 1px; padding-top: 60px; margin-left: 40px;">
          <div style="box-sizing: border-box; font-size: 0; text-align: center;">
            <div style="display: inline-block; font-size: 12px; font-family: Helvetica; color: #333333; line-height: 1.2; pointer-events: none; font-weight: bold; text-align: center;">Hello World</div>
          </div>
        </div>
      </foreignObject>
    </switch>
  </g>
</svg>"#;

    #[test]
    fn converts_switch_to_text() {
        let (fixed, n) = fix_svg_for_visio(SAMPLE).unwrap();
        assert_eq!(n, 1);
        assert!(fixed.contains("<text"));
        assert!(fixed.contains("Hello World"));
        assert!(!fixed.contains("<switch"));
        assert!(fixed.contains(r#"text-anchor="middle""#));
        assert!(fixed.contains(r#"font-weight="bold""#));
        assert!(fixed.contains("fill=\"#333333\""));
    }

    #[test]
    fn parse_color_light_dark() {
        assert_eq!(parse_color("light-dark(#111111, #eeeeee)"), "#111111");
        assert_eq!(parse_color("#abc"), "#abc");
        assert_eq!(parse_color(""), "#000000");
    }

    #[test]
    fn css_values() {
        let style = "padding-top: 60px; margin-left: 40px; width: 118px;";
        assert_eq!(css_px(style, "padding-top"), 60.0);
        assert_eq!(css_px(style, "margin-left"), 40.0);
        assert_eq!(css_px(style, "width"), 118.0);
        assert_eq!(css_value(style, "nope"), None);
    }

    #[test]
    fn py_float_format() {
        assert_eq!(format_py_float(60.0), "60.0");
        assert_eq!(format_py_float(59.5), "59.5");
    }
}
