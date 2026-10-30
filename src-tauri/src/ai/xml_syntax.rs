//! 严格 XML well-formedness 检查（上游 `xml-syntax.ts`，底层是 saxes）。
//!
//! 这里复用了 [`crate::ai::xml_tree::parse_strict`]，它对非法 XML 报
//! `line:col: message`，与 saxes 行为一致（含未声明命名空间前缀、重复属性、
//! 未定义实体、标签不配平等）。空输入视为无错（saxes 对空文档不报错）。

use crate::ai::xml_tree::parse_strict;

/// 首个 XML 语法错误（`line:col: message`），或 None（well-formed）。
/// 与上游一致，先对输入 trim 后再检查。
pub fn get_xml_syntax_error(xml: &str) -> Option<String> {
    let trimmed = xml.trim();
    if trimmed.is_empty() {
        return None;
    }
    match parse_strict(trimmed) {
        Ok(_) => None,
        Err(e) => Some(e.fmt()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_well_formed() {
        assert_eq!(
            get_xml_syntax_error(r#"<mxfile><diagram id="a"/></mxfile>"#),
            None
        );
    }

    #[test]
    fn reports_mismatched() {
        let err = get_xml_syntax_error("<a><b></a>").unwrap();
        assert!(err.contains("closing tag"));
    }

    #[test]
    fn empty_is_ok() {
        assert_eq!(get_xml_syntax_error("  "), None);
        assert_eq!(get_xml_syntax_error(""), None);
    }
}
