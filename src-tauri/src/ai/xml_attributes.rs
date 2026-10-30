//! 属性读取与转义工具（上游 `xml-attributes.ts` + 各处的 `&`/`<` 转义）。
//!
//! 把一个 tag 的属性文本解析为 `name="value"` / `name='value'` 对；引号内
//! 的内容整体读取，因此 `value="Use parent='1'"` 不会被误拆。返回的
//! `start`/`end` 是相对传入字符串的偏移，含属性前导空白，供原位重写使用。
//!
//! [`escape_ampersands`] 与 [`escape_for_attribute`] 是上游
//! `&(?!(?:lt|gt|amp|quot|apos|#\d+|#x[0-9a-fA-F]+);)` 与
//! `value.replace(/</g,"&lt;")` 的等价实现（Rust regex 不支持 lookahead）。

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagAttribute {
    pub name: String,
    pub value: String,
    /// 匹配起点（含前导空白），相对传入字符串。
    pub start: usize,
    /// 匹配终点，相对传入字符串。
    pub end: usize,
}

pub fn read_attributes(tag: &str) -> Vec<TagAttribute> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        // 上游: /\s*([A-Za-z_:][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')/g
        // Rust 无 (?:)；value 交替直接做捕获组，编号 2=双引号、3=单引号。
        Regex::new(r#"\s*([A-Za-z_:][A-Za-z0-9_:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
            .expect("static attribute regex")
    });
    let mut out = Vec::new();
    for caps in re.captures_iter(tag) {
        let whole = caps.get(0).unwrap();
        let name = caps.get(1).unwrap().as_str().to_string();
        let value = caps
            .get(2)
            .or_else(|| caps.get(3))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        out.push(TagAttribute {
            name,
            value,
            start: whole.start(),
            end: whole.end(),
        });
    }
    out
}

/// 转义 `&`，除非它已经是合法实体的开头
/// （`&lt; &gt; &amp; &quot; &apos; &#123; &#x1F;` 均保留）。
pub fn escape_ampersands(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(end) = entity_end(&s[i + 1..]) {
                out.push_str(&s[i..i + 1 + end]);
                i += 1 + end;
            } else {
                out.push_str("&amp;");
                i += 1;
            }
        } else {
            let ch = s[i..].chars().next().expect("valid utf-8 boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// 属性值转义：`&` → `&amp;`（已实体除外）、`<` → `&lt;`
/// （上游 `style-classes.ts` 的 `escapeForAttribute`）。
pub fn escape_for_attribute(value: &str) -> String {
    escape_ampersands(value).replace('<', "&lt;")
}

/// `&` 之后若为合法实体则返回实体总长（含分号，相对 `&` 之后），否则 None。
fn entity_end(after_amp: &str) -> Option<usize> {
    for name in ["lt;", "gt;", "amp;", "quot;", "apos;"] {
        if after_amp.starts_with(name) {
            return Some(name.len());
        }
    }
    let b = after_amp.as_bytes();
    if b.first() == Some(&b'#') {
        let rest = &after_amp[1..];
        let (digits, radix_skip) = if rest.starts_with('x') || rest.starts_with('X') {
            (&rest[1..], 2)
        } else {
            (rest, 1)
        };
        let mut n = 0;
        let hex = radix_skip == 2;
        for c in digits.chars() {
            let ok = if hex {
                c.is_ascii_hexdigit()
            } else {
                c.is_ascii_digit()
            };
            if !ok {
                break;
            }
            n += 1;
        }
        if n > 0 && digits[n..].starts_with(';') {
            return Some(radix_skip + n + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_double_and_single() {
        let attrs = read_attributes(r#" id="2" value='Use parent="1"' style="x;""#);
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0].name, "id");
        assert_eq!(attrs[0].value, "2");
        assert_eq!(attrs[1].value, "Use parent=\"1\"");
        assert_eq!(attrs[2].name, "style");
    }

    #[test]
    fn records_positions() {
        let tag = r#" a="x" b="y""#;
        let attrs = read_attributes(tag);
        assert_eq!(attrs[0].start, 0);
        assert_eq!(attrs[1].start, 6);
        let mut s = String::new();
        let mut last = 0;
        for a in &attrs {
            s.push_str(&tag[last..a.start]);
            s.push_str(&tag[a.start..a.end]);
            last = a.end;
        }
        assert_eq!(s, tag);
    }

    #[test]
    fn escapes_ampersands_selectively() {
        assert_eq!(
            escape_ampersands("a & b &lt; c &amp; &#65; &#x41;"),
            "a &amp; b &lt; c &amp; &#65; &#x41;"
        );
        assert_eq!(escape_ampersands("no amp"), "no amp");
    }

    #[test]
    fn escapes_for_attribute() {
        assert_eq!(escape_for_attribute("a < b & c"), "a &lt; b &amp; c");
    }
}
