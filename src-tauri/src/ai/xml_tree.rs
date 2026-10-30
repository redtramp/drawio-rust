//! 最小 XML 树：上游管线中 DOMParser / XMLSerializer / saxes 的确定性替身。
//!
//! 忠实要点（与 Web 行为对齐，供 S8 对齐基线验证）：
//! - 严格解析按行/列报 well-formedness 错误（替代 saxes，`line:col: message`）；
//! - 宽松解析在首个错误处产出 `<parsererror>` 根（替代浏览器 DOMParser）；
//! - 序列化转义与 XMLSerializer 一致（属性 `& < > "`、文本 `& < >`，空元素 `<x ... />`）；
//! - 属性保持书写顺序与重复项（well-formedness 由解析器另行判定）。
//!
//! 上游对照：`packages/mcp-server/src/xml-syntax.ts`（saxes）、`pages.ts`
//! （parseMxfile/serializeMxfile）、各处 `new DOMParser()`。

use std::fmt;

/// XML 语法错误（saxes 风格的 `line:col: message`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlSyntaxError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl XmlSyntaxError {
    pub fn fmt(&self) -> String {
        format!("{}:{}: {}", self.line, self.col, self.message)
    }
}

impl fmt::Display for XmlSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.fmt())
    }
}

/// XML 节点：元素或文本（实体已解码）。
#[derive(Debug, Clone, PartialEq)]
pub enum XmlNode {
    Element(XElement),
    Text(String),
}

/// XML 元素。属性保持书写顺序；文本与元素子节点交错保存。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XElement {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XmlNode>,
}

impl XElement {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// 首个同名属性值（DOM `getAttribute` 语义）。
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// 覆盖（已存在则原位替换，保持位置）或追加属性。
    pub fn set_attr(&mut self, name: &str, value: &str) {
        if let Some(slot) = self.attrs.iter_mut().find(|(n, _)| n == name) {
            slot.1 = value.to_string();
        } else {
            self.attrs.push((name.to_string(), value.to_string()));
        }
    }

    /// 全部后代中名为 name 的元素（DOM `getElementsByTagName` 语义）。
    pub fn by_name(&self, name: &str) -> Vec<&XElement> {
        let mut out = Vec::new();
        for child in &self.children {
            if let XmlNode::Element(el) = child {
                if el.name == name {
                    out.push(el);
                }
                out.extend(el.by_name(name));
            }
        }
        out
    }

    /// 直接子元素（DOM `children` 语义）。
    pub fn child_elements(&self) -> Vec<&XElement> {
        self.children
            .iter()
            .filter_map(|c| match c {
                XmlNode::Element(el) => Some(el),
                _ => None,
            })
            .collect()
    }

    /// 递归文本（DOM `textContent` 语义）。
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            match child {
                XmlNode::Text(t) => out.push_str(t),
                XmlNode::Element(el) => out.push_str(&el.text_content()),
            }
        }
        out
    }

    /// 是否只有文本内容（无元素子节点）。
    pub fn is_leaf(&self) -> bool {
        !self
            .children
            .iter()
            .any(|c| matches!(c, XmlNode::Element(_)))
    }

    /// 追加子节点。
    pub fn push(&mut self, node: XmlNode) {
        self.children.push(node);
    }

    /// 用新节点替换第一个 name 匹配的直接子元素；返回是否替换成功。
    pub fn replace_direct_child(&mut self, name: &str, new: XElement) -> bool {
        for child in &mut self.children {
            if let XmlNode::Element(el) = child {
                if el.name == name {
                    *el = new;
                    return true;
                }
            }
        }
        false
    }
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 序列化元素（XMLSerializer 兼容：不加声明、不加缩进）。
pub fn serialize(el: &XElement) -> String {
    let mut out = String::with_capacity(256);
    write_element(&mut out, el);
    out
}

fn write_element(out: &mut String, el: &XElement) {
    out.push('<');
    out.push_str(&el.name);
    for (name, value) in &el.attrs {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(&escape_attr(value));
        out.push('"');
    }
    if el.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for child in &el.children {
        match child {
            XmlNode::Text(t) => out.push_str(&escape_text(t)),
            XmlNode::Element(sub) => write_element(out, sub),
        }
    }
    out.push_str("</");
    out.push_str(&el.name);
    out.push('>');
}

// ============================================================================
// 解析
// ============================================================================

struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    error: Option<XmlSyntaxError>,
    /// namespace 前缀作用域栈（saxes `xmlns: true` 行为）。
    ns: Vec<std::collections::HashMap<String, String>>,
}

impl Parser {
    fn new(xml: &str) -> Self {
        Self {
            chars: xml.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            error: None,
            ns: vec![Default::default()],
        }
    }

    fn err(&mut self, message: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some(XmlSyntaxError {
                line: self.line,
                col: self.col,
                message: message.into(),
            });
        }
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos)?;
        self.pos += 1;
        if *c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(*c)
    }

    fn starts_with(&self, s: &str) -> bool {
        self.chars[self.pos..]
            .iter()
            .take(s.chars().count())
            .copied()
            .eq(s.chars())
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(0), Some(c) if c.is_ascii_whitespace()) {
            self.bump();
        }
    }

    fn name_start_ok(c: char) -> bool {
        c.is_ascii_alphabetic() || c == '_' || c == ':'
    }
    fn name_ok(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ':')
    }

    fn parse_name(&mut self) -> Option<String> {
        let first = self.peek(0)?;
        if !Self::name_start_ok(first) {
            return None;
        }
        let mut name = String::new();
        while let Some(c) = self.peek(0) {
            if !Self::name_ok(c) {
                break;
            }
            name.push(c);
            self.bump();
        }
        Some(name)
    }

    /// 检查前缀是否已声明（saxes `xmlns: true`：未声明前缀报错）。
    fn check_ns(&mut self, name: &str) {
        let Some(prefix) = name.split(':').next().filter(|p| *p != name) else {
            return;
        };
        if prefix == "xml" {
            return;
        }
        for scope in self.ns.iter().rev() {
            if scope.contains_key(prefix) {
                return;
            }
        }
        self.err(format!(
            "XML parser error: namespace prefix not declared: {}",
            prefix
        ));
    }

    /// 解析实体引用（位于 `&`），把解码字符推入 text。
    fn parse_entity(&mut self, text: &mut String) {
        self.bump(); // &
        if self.peek(0) == Some('#') {
            self.bump();
            let hex = matches!(self.peek(0), Some('x') | Some('X'));
            if hex {
                self.bump();
            }
            let mut digits = String::new();
            while let Some(c) = self.peek(0) {
                let ok = if hex {
                    c.is_ascii_hexdigit()
                } else {
                    c.is_ascii_digit()
                };
                if !ok {
                    break;
                }
                digits.push(c);
                self.bump();
            }
            let code = u32::from_str_radix(&digits, if hex { 16 } else { 10 }).ok();
            match code.and_then(char::from_u32) {
                Some(c) if self.bump() == Some(';') => text.push(c),
                _ => self.err("XML parser error: invalid character entity"),
            }
            return;
        }
        let mut name = String::new();
        while let Some(c) = self.peek(0) {
            if c.is_ascii_alphanumeric() {
                name.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if self.bump() != Some(';') || name.is_empty() {
            self.err("XML parser error: invalid character entity");
            return;
        }
        match name.as_str() {
            "amp" => text.push('&'),
            "lt" => text.push('<'),
            "gt" => text.push('>'),
            "quot" => text.push('"'),
            "apos" => text.push('\''),
            other => self.err(format!("XML parser error: undefined entity: {}", other)),
        }
    }

    fn parse_comment(&mut self) {
        // 位于 <!--
        self.bump();
        self.bump();
        self.bump();
        self.bump();
        loop {
            match self.bump() {
                None => {
                    self.err("XML parser error: premature EOF");
                    return;
                }
                Some('-') if self.peek(0) == Some('-') => {
                    if self.peek(1) == Some('>') {
                        self.bump();
                        self.bump();
                        return;
                    }
                    self.err("XML parser error: comment must not contain '--'");
                }
                Some(_) => {}
            }
        }
    }

    fn parse_cdata(&mut self, text: &mut String) {
        // 位于 <![CDATA[
        for _ in 0..9 {
            self.bump();
        }
        loop {
            match self.bump() {
                None => {
                    self.err("XML parser error: premature EOF");
                    return;
                }
                Some(']') if self.peek(0) == Some(']') && self.peek(1) == Some('>') => {
                    self.bump();
                    self.bump();
                    return;
                }
                Some(c) => text.push(c),
            }
        }
    }

    fn parse_pi(&mut self) {
        // 位于 <?
        self.bump();
        self.bump();
        loop {
            match self.bump() {
                None => {
                    self.err("XML parser error: premature EOF");
                    return;
                }
                Some('?') if self.peek(0) == Some('>') => {
                    self.bump();
                    return;
                }
                Some(_) => {}
            }
        }
    }

    fn parse_doctype(&mut self) {
        // 位于 <!DOCTYPE
        for _ in 0..9 {
            self.bump();
        }
        let mut depth = 0usize;
        loop {
            match self.bump() {
                None => {
                    self.err("XML parser error: premature EOF");
                    return;
                }
                Some('[') => depth += 1,
                Some(']') => depth = depth.saturating_sub(1),
                Some('>') if depth == 0 => return,
                Some(_) => {}
            }
        }
    }

    /// 解析开标签 `<name ...>`，返回元素；push 该元素的 namespace 作用域。
    /// 出错返回 None（error 已记录）。
    fn parse_start_tag(&mut self) -> Option<(XElement, bool)> {
        self.bump(); // <
        let name = self.parse_name()?;
        self.check_ns(&name);
        let mut el = XElement::new(name);
        self.ns.push(Default::default());
        let mut seen: std::collections::HashSet<String> = Default::default();
        loop {
            self.skip_ws();
            match self.peek(0) {
                None => {
                    self.err("XML parser error: premature EOF");
                    return None;
                }
                Some('/') if self.peek(1) == Some('>') => {
                    self.bump();
                    self.bump();
                    return Some((el, true));
                }
                Some('>') => {
                    self.bump();
                    return Some((el, false));
                }
                Some(c) if Self::name_start_ok(c) => {
                    let aname = self.parse_name().unwrap_or_default();
                    let is_decl = aname == "xmlns" || aname.starts_with("xmlns:");
                    if !is_decl {
                        self.check_ns(&aname);
                    }
                    if !aname.is_empty() && !seen.insert(aname.clone()) {
                        self.err(format!("XML parser error: Duplicate attribute: {}", aname));
                    }
                    self.skip_ws();
                    let value = if self.bump() == Some('=') {
                        self.skip_ws();
                        match self.bump() {
                            Some(q @ ('"' | '\'')) => {
                                let mut v = String::new();
                                loop {
                                    match self.peek(0) {
                                        None => {
                                            self.err(
                                                "XML parser error: unterminated attribute value",
                                            );
                                            break;
                                        }
                                        Some(c) if c == q => {
                                            self.bump();
                                            break;
                                        }
                                        Some('&') => self.parse_entity(&mut v),
                                        Some('<') => {
                                            self.err("XML parser error: invalid character in attribute value");
                                            self.bump();
                                        }
                                        Some(_) => {
                                            let c = self.bump().expect("peeked");
                                            v.push(c);
                                        }
                                    }
                                }
                                v
                            }
                            _ => {
                                self.err("XML parser error: expected quoted attribute value");
                                String::new()
                            }
                        }
                    } else {
                        self.err("XML parser error: expected '=' after attribute name");
                        String::new()
                    };
                    // 记录 namespace 声明
                    if aname == "xmlns" {
                        if let Some(scope) = self.ns.last_mut() {
                            scope.insert(String::new(), value.clone());
                        }
                    } else if let Some(p) = aname.strip_prefix("xmlns:") {
                        if let Some(scope) = self.ns.last_mut() {
                            scope.insert(p.to_string(), value.clone());
                        }
                    }
                    el.attrs.push((aname, value));
                }
                Some(_) => {
                    self.err("XML parser error: invalid character in tag");
                    self.bump();
                }
            }
        }
    }

    /// 解析结束标签 `</name>`；name 不匹配时报错。
    fn parse_end_tag(&mut self, expected: &str) {
        self.bump(); // <
        self.bump(); // /
        let name = self.parse_name().unwrap_or_default();
        self.skip_ws();
        if self.bump() != Some('>') {
            self.err("XML parser error: expected '>' after closing tag name");
        }
        if name != expected {
            self.err(format!(
                "XML parser error: expected closing tag </{}>, got </{}>",
                expected, name
            ));
        }
    }

    /// 解析一个元素（位于 `<`），包括内容；出错返回 None。
    fn parse_element(&mut self) -> Option<XElement> {
        let (mut el, self_closing) = self.parse_start_tag()?;
        if self_closing {
            self.ns.pop();
            return Some(el);
        }
        let mut text = String::new();
        loop {
            if self.error.is_some() {
                return None;
            }
            match self.peek(0) {
                None => {
                    self.err("XML parser error: premature EOF");
                    return None;
                }
                Some('<') if self.starts_with("<!--") => self.parse_comment(),
                Some('<') if self.starts_with("<![CDATA[") => self.parse_cdata(&mut text),
                Some('<') if self.starts_with("<?") => {
                    self.err("XML parser error: processing instruction in element content");
                    return None;
                }
                Some('<') if self.starts_with("</") => {
                    if !text.is_empty() {
                        el.push(XmlNode::Text(std::mem::take(&mut text)));
                    }
                    self.parse_end_tag(&el.name);
                    self.ns.pop();
                    return Some(el);
                }
                Some('<') => {
                    if !text.is_empty() {
                        el.push(XmlNode::Text(std::mem::take(&mut text)));
                    }
                    let child = self.parse_element()?;
                    el.push(XmlNode::Element(child));
                }
                Some('&') => self.parse_entity(&mut text),
                Some(c) if c < '\u{20}' && !matches!(c, '\t' | '\n' | '\r') => {
                    self.err("XML parser error: invalid character in text");
                    self.bump();
                }
                Some(c) => {
                    text.push(c);
                    self.bump();
                }
            }
        }
    }

    /// 解析文档：跳过 prolog，解析根元素，检查尾随内容。
    fn parse_document(&mut self) -> Result<XElement, ()> {
        loop {
            self.skip_ws();
            match self.peek(0) {
                None => {
                    self.err("XML parser error: premature EOF");
                    return Err(());
                }
                Some('<') if self.starts_with("<?") => self.parse_pi(),
                Some('<') if self.starts_with("<!--") => self.parse_comment(),
                Some('<') if self.starts_with("<!DOCTYPE") => self.parse_doctype(),
                Some('<') if self.starts_with("<![CDATA[") => {
                    self.err("XML parser error: CDATA outside of element");
                    return Err(());
                }
                Some('<') if self.peek(1) == Some('/') => {
                    self.err("XML parser error: unexpected closing tag");
                    return Err(());
                }
                Some('<') => break,
                Some(_) => {
                    self.err("XML parser error: unexpected text before root");
                    return Err(());
                }
            }
            if self.error.is_some() {
                return Err(());
            }
        }
        let root = self.parse_element().ok_or(())?;
        if self.error.is_some() {
            return Err(());
        }
        // 尾随：只允许空白 / 注释 / PI
        loop {
            self.skip_ws();
            match self.peek(0) {
                None => return Ok(root),
                Some('<') if self.starts_with("<!--") => self.parse_comment(),
                Some('<') if self.starts_with("<?") => self.parse_pi(),
                Some('<') => {
                    self.err("XML parser error: text after root element");
                    return Err(());
                }
                Some(_) => {
                    self.err("XML parser error: text after root element");
                    return Err(());
                }
            }
            if self.error.is_some() {
                return Err(());
            }
        }
    }
}

/// 严格解析：返回根元素或 `line:col` 错误（saxes 等价物）。
pub fn parse_strict(xml: &str) -> Result<XElement, XmlSyntaxError> {
    let mut p = Parser::new(xml);
    match p.parse_document() {
        Ok(root) => Ok(root),
        Err(()) => Err(p.error.unwrap_or(XmlSyntaxError {
            line: 1,
            col: 1,
            message: "no root element".into(),
        })),
    }
}

/// 宽松解析（浏览器 DOMParser 语义）：语法错误或空输入时根为 `<parsererror>`。
pub fn parse_lenient(xml: &str) -> Option<XElement> {
    if !xml.trim().starts_with('<') {
        let mut el = XElement::new("parsererror");
        el.push(XmlNode::Text(xml.to_string()));
        return Some(el);
    }
    let mut p = Parser::new(xml);
    match p.parse_document() {
        Ok(root) => Some(root),
        Err(()) => {
            let mut el = XElement::new("parsererror");
            let msg = p
                .error
                .map(|e| e.fmt())
                .unwrap_or_else(|| "parse error".into());
            el.push(XmlNode::Text(msg));
            Some(el)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_simple() {
        let xml = r#"<mxfile host="app"><diagram id="p1" name="Page-1"><mxGraphModel/></diagram></mxfile>"#;
        let el = parse_strict(xml).unwrap();
        assert_eq!(el.name, "mxfile");
        assert_eq!(el.attr("host"), Some("app"));
        let diagrams = el.by_name("diagram");
        assert_eq!(diagrams.len(), 1);
        assert_eq!(diagrams[0].attr("id"), Some("p1"));
        assert_eq!(serialize(&el), xml);
    }

    #[test]
    fn escapes_and_entities() {
        let xml = r#"<mxCell id="2" value="a &amp; b &lt;c&gt;"/>"#;
        let el = parse_strict(xml).unwrap();
        assert_eq!(el.attr("value"), Some("a & b <c>"));
        assert_eq!(serialize(&el), xml);
    }

    #[test]
    fn text_content() {
        let el = parse_strict("<a>x<b>y</b>z</a>").unwrap();
        assert_eq!(el.text_content(), "xyz");
        assert!(!el.is_leaf());
    }

    #[test]
    fn cdata_and_comment() {
        let el = parse_strict("<a><![CDATA[<b>raw</b>]]><!-- c --></a>").unwrap();
        assert_eq!(el.text_content(), "<b>raw</b>");
    }

    #[test]
    fn reports_position() {
        let err = parse_strict("<a><b></a>").unwrap_err();
        assert!(err.line >= 1);
        assert!(err.message.contains("closing tag"));
    }

    #[test]
    fn duplicate_attribute() {
        let err = parse_strict(r#"<a x="1" x="2"/>"#).unwrap_err();
        assert!(err.message.contains("Duplicate"));
    }

    #[test]
    fn undefined_entity() {
        let err = parse_strict("<a>&bogus;</a>").unwrap_err();
        assert!(err.message.contains("undefined entity"));
    }

    #[test]
    fn lenient_returns_parsererror() {
        let el = parse_lenient("<a><b></a>").unwrap();
        assert_eq!(el.name, "parsererror");
    }

    #[test]
    fn undeclared_prefix() {
        let err = parse_strict(r#"<a xlink:href="x"/>"#).unwrap_err();
        assert!(err.message.contains("namespace prefix"));
        // 声明后合法
        parse_strict(r#"<a xmlns:xlink="u" xlink:href="x"/>"#).unwrap();
    }
}
