//! 多页 mxfile 助手（上游 `pages.ts` + `lib/diagram-pages.ts`）。
//!
//! 集中页 CRUD：找页（id/name/index）、增/改/删页、单页投影、页模型替换、
//! 把 AI 画的单页放回画布多页文档。同时含压缩页解压（`load-diagram.ts`）。

use regex::Regex;

use super::xml_attributes::read_attributes;
use super::xml_syntax::get_xml_syntax_error;
use super::xml_tree::{parse_strict, serialize, XElement};

/// 页信息。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PageInfo {
    pub id: String,
    pub name: String,
    pub index: usize,
    pub cell_count: usize,
}

/// 页选择器（全字段可选）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PageSelector {
    #[serde(rename = "page_id", skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    #[serde(rename = "page_name", skip_serializing_if = "Option::is_none")]
    pub page_name: Option<String>,
    #[serde(rename = "page_index", skip_serializing_if = "Option::is_none")]
    pub page_index: Option<usize>,
}

pub fn has_page_selector(s: &PageSelector) -> bool {
    s.page_id.is_some() || s.page_name.is_some() || s.page_index.is_some()
}

/// 生成短页 id（12 字符，含一个连字符），形状近似 draw.io 自动 id。
pub fn generate_page_id() -> String {
    let a = base36(random_u64());
    let b = base36(random_u64());
    let a: String = a.chars().take(8).collect();
    let b: String = b.chars().take(4).collect();
    format!("{a}-{b}")
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).expect("ascii")
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn random_u64() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let c = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut x = nanos ^ c.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5_A5A5_A5A5_A5A5;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// 除 root cell "0"/"1" 外是否还有 cell（或含压缩页数据的 diagram）。
pub fn has_cells(xml: &str) -> bool {
    static CELL_ID: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static WRAPPED: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static DIAGRAM_TEXT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    // 1. 非 root cell（id != "0"/"1"）
    let cell_id = CELL_ID.get_or_init(|| {
        Regex::new(r#"<mxCell\b[^>]*\bid\s*=\s*["']([^"']+)["']"#).expect("cell id regex")
    });
    if cell_id.captures_iter(xml).any(|c| {
        let id = c.get(1).map(|m| m.as_str()).unwrap_or("");
        id != "0" && id != "1"
    }) {
        return true;
    }
    // 2. UserObject / object 包装
    let wrapped =
        WRAPPED.get_or_init(|| Regex::new(r#"<(UserObject\b|object\b)"#).expect("wrapped regex"));
    if wrapped.is_match(xml) {
        return true;
    }
    // 3. 压缩页（diagram 含非空白文本）
    let diagram_text = DIAGRAM_TEXT
        .get_or_init(|| Regex::new(r#"<diagram\b[^>]*>\s*[^\s<]"#).expect("diagram text regex"));
    diagram_text.is_match(xml)
}

pub fn is_mx_file(xml: &str) -> bool {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"^\s*(<\?xml[^>]*\?>\s*)?<mxfile[\s>]"#).expect("mxfile regex")
    });
    re.is_match(xml)
}

pub fn is_mx_graph_model(xml: &str) -> bool {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"^\s*(<\?xml[^>]*\?>\s*)?<mxGraphModel[\s>]"#).expect("model regex")
    });
    re.is_match(xml)
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn strip_xml_declaration(xml: &str) -> String {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)^\s*<\?xml[^>]*\?>\s*").expect("xml decl"));
    re.replace(xml, "").to_string()
}

const ROOT_CELLS: &str = r#"<mxCell id="0"/><mxCell id="1" parent="0"/>"#;

/// 单页空白文档。
pub const BLANK_MXFILE: &str = r#"<mxfile><diagram name="Page-1" id="page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/></root></mxGraphModel></diagram></mxfile>"#;

/// 把裸 cell（可选在 `<root>` 内）包成单页 `<mxGraphModel>`，补 "0"/"1" root cell。
/// `<mxfile>` / `<mxGraphModel>` 及其它内容原样返回。
pub fn wrap_cells_in_model(xml: &str) -> String {
    static START: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static PRE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static ROOT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static SUFFIX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static CELL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

    let mut content = strip_xml_declaration(xml.trim());
    let start_re =
        START.get_or_init(|| Regex::new(r"<(mxCell|UserObject|object|root)[\s/>]").expect("start"));
    let Some(m) = start_re.find(&content) else {
        return xml.to_string();
    };
    let start = m.start();
    let pre = PRE.get_or_init(|| Regex::new(r"^(?:<!--[\s\S]*?-->|[^<])*$").expect("pre"));
    if !pre.is_match(&content[..start]) {
        return xml.to_string();
    }
    let root = ROOT.get_or_init(|| Regex::new(r"</?root>").expect("root"));
    content = root.replace_all(&content[start..], "").trim().to_string();

    let mut end = 0usize;
    for close in ["/>", "</mxCell>", "</UserObject>", "</object>"] {
        if let Some(at) = content.rfind(close) {
            end = end.max(at + close.len());
        }
    }
    let suffix = SUFFIX.get_or_init(|| Regex::new(r"^(\s*</[^>]+>)*\s*$").expect("suffix"));
    if end != 0 && suffix.is_match(&content[end..]) {
        content = content[..end].to_string();
    }

    let cell =
        CELL.get_or_init(|| Regex::new(r"<mxCell\b[^>]*?(?:/>|>\s*</mxCell>)").expect("cell"));
    {
        let mut out = String::with_capacity(content.len());
        let mut last = 0;
        for m in cell.find_iter(&content) {
            out.push_str(&content[last..m.start()]);
            let tag = m.as_str();
            let id = read_attributes(tag)
                .into_iter()
                .find(|a| a.name == "id")
                .map(|a| a.value);
            if !matches!(id.as_deref(), Some("0") | Some("1")) {
                out.push_str(tag);
            }
            last = m.end();
        }
        out.push_str(&content[last..]);
        content = out.trim().to_string();
    }

    format!("<mxGraphModel><root>{ROOT_CELLS}{content}</root></mxGraphModel>")
}

/// 把裸 `<mxGraphModel>` 包成单页 `<mxfile>`；已是 mxfile 则原样；都不是则 None。
pub fn normalize_to_mxfile(
    xml: &str,
    page_id: Option<&str>,
    page_name: Option<&str>,
) -> Option<String> {
    let trimmed = xml.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_mx_file(trimmed) {
        return Some(trimmed.to_string());
    }
    if !is_mx_graph_model(trimmed) {
        return None;
    }
    let page_id = page_id
        .map(|s| s.to_string())
        .unwrap_or_else(generate_page_id);
    let page_name = page_name.unwrap_or("Page-1");
    let inner = strip_xml_declaration(trimmed);
    Some(format!(
        r#"<mxfile host="app.diagrams.net"><diagram id="{}" name="{}">{inner}</diagram></mxfile>"#,
        escape_attr(&page_id),
        escape_attr(page_name)
    ))
}

/// 解析 mxfile；语法错误或根非 `<mxfile>` 返回 None。
pub fn parse_mxfile(xml: &str) -> Option<XElement> {
    if get_xml_syntax_error(xml).is_some() {
        return None;
    }
    let doc = parse_strict(xml).ok()?;
    if doc.name != "mxfile" {
        return None;
    }
    Some(doc)
}

pub fn serialize_mxfile(doc: &XElement) -> String {
    serialize(doc)
}

/// 单页投影结果。
#[derive(Debug, Clone)]
pub enum PageProjection {
    Ok {
        xml: String,
        index: usize,
        name: String,
    },
    Parse,
    NotFound,
}

/// 从 mxfile 投影出单页 `<mxfile>`。
pub fn project_page(xml: &str, selector: &PageSelector) -> PageProjection {
    let doc = match parse_mxfile(xml) {
        Some(d) => d,
        None => return PageProjection::Parse,
    };
    let Some((index, el)) = find_page_element(&doc, selector) else {
        return PageProjection::NotFound;
    };
    PageProjection::Ok {
        xml: format!(
            r#"<mxfile host="app.diagrams.net">{}</mxfile>"#,
            serialize(el)
        ),
        index,
        name: el.attr("name").unwrap_or("").to_string(),
    }
}

/// 列出所有页摘要。
pub fn list_pages_from_doc(doc: &XElement) -> Vec<PageInfo> {
    doc.by_name("diagram")
        .iter()
        .enumerate()
        .map(|(idx, d)| {
            let cell_count = d
                .by_name("root")
                .first()
                .map(|r| r.by_name("mxCell").len())
                .unwrap_or(0);
            PageInfo {
                id: d.attr("id").unwrap_or("").to_string(),
                name: d
                    .attr("name")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("Page-{}", idx + 1)),
                index: idx,
                cell_count,
            }
        })
        .collect()
}

/// 解析选择器到 `<diagram>` 元素；解析顺序 page_id → page_name → page_index → 默认第一页。
pub fn find_page_element<'a>(
    doc: &'a XElement,
    selector: &PageSelector,
) -> Option<(usize, &'a XElement)> {
    let diagrams = doc.by_name("diagram");
    if diagrams.is_empty() {
        return None;
    }
    if !has_page_selector(selector) {
        return Some((0, diagrams[0]));
    }
    if let Some(page_id) = &selector.page_id {
        for (i, d) in diagrams.iter().enumerate() {
            if d.attr("id") == Some(page_id.as_str()) {
                return Some((i, d));
            }
        }
        return None;
    }
    if let Some(page_name) = &selector.page_name {
        for (i, d) in diagrams.iter().enumerate() {
            if d.attr("name") == Some(page_name.as_str()) {
                return Some((i, d));
            }
        }
        return None;
    }
    if let Some(idx) = selector.page_index {
        if idx < diagrams.len() {
            return Some((idx, diagrams[idx]));
        }
        return None;
    }
    None
}

/// 追加新页；opts.xml 须为裸 `<mxGraphModel>`。返回新 PageInfo，出错返回错误消息。
pub fn add_page_to_doc(
    doc: &mut XElement,
    id: Option<&str>,
    name: Option<&str>,
    xml: Option<&str>,
) -> Result<PageInfo, String> {
    let existing = list_pages_from_doc(doc);
    let id = id.map(|s| s.to_string()).unwrap_or_else(generate_page_id);
    if existing.iter().any(|p| p.id == id) {
        return Err(format!("Page id \"{id}\" already exists"));
    }
    let name = name
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Page-{}", existing.len() + 1));

    let inner = match xml {
        Some(x) if !x.trim().is_empty() => {
            let trimmed = strip_xml_declaration(x.trim());
            if is_mx_file(&trimmed) {
                return Err(
                    "addPageToDoc: opts.xml must be a bare <mxGraphModel>; received a full <mxfile>."
                        .to_string(),
                );
            }
            if !is_mx_graph_model(&trimmed) {
                return Err("addPageToDoc: opts.xml must be a bare <mxGraphModel>.".to_string());
            }
            trimmed
        }
        _ => format!("<mxGraphModel><root>{ROOT_CELLS}</root></mxGraphModel>"),
    };

    let snippet = format!(
        r#"<wrapper><diagram id="{}" name="{}">{inner}</diagram></wrapper>"#,
        escape_attr(&id),
        escape_attr(&name)
    );
    if get_xml_syntax_error(&snippet).is_some() {
        return Err(
            "Failed to parse new page xml — make sure it is a valid <mxGraphModel>".to_string(),
        );
    }
    let temp = parse_strict(&snippet).map_err(|e| e.fmt())?;
    let new_diagram = temp
        .by_name("diagram")
        .first()
        .cloned()
        .ok_or("Failed to construct <diagram> element for new page")?;
    doc.children
        .push(super::xml_tree::XmlNode::Element(new_diagram.clone()));
    Ok(PageInfo {
        id,
        name,
        index: existing.len(),
        cell_count: new_diagram.by_name("mxCell").len(),
    })
}

/// 重命名选择器匹配的页；成功返回 true。
pub fn rename_page_in_doc(doc: &mut XElement, selector: &PageSelector, new_name: &str) -> bool {
    let idx = match find_page_element(doc, selector) {
        Some((i, _)) => i,
        None => return false,
    };
    rename_diagram_by_index(doc, idx, new_name)
}

fn rename_diagram_by_index(doc: &mut XElement, idx: usize, new_name: &str) -> bool {
    let mut current = 0;
    for child in &mut doc.children {
        if let super::xml_tree::XmlNode::Element(el) = child {
            if el.name == "diagram" {
                if current == idx {
                    el.set_attr("name", new_name);
                    return true;
                }
                current += 1;
            }
        }
    }
    false
}

/// 删除选择器匹配的页；拒绝删除最后一页。
pub fn delete_page_from_doc(
    doc: &mut XElement,
    selector: &PageSelector,
) -> Result<(String, usize), String> {
    let Some((idx, _)) = find_page_element(doc, selector) else {
        return Err("Page not found".to_string());
    };
    if list_pages_from_doc(doc).len() <= 1 {
        return Err("Cannot delete the only remaining page".to_string());
    }
    let deleted_id = find_page_element(doc, selector)
        .map(|(_, el)| el.attr("id").unwrap_or("").to_string())
        .unwrap_or_default();
    let mut current = 0;
    let mut remove_at = None;
    for (i, child) in doc.children.iter().enumerate() {
        if let super::xml_tree::XmlNode::Element(el) = child {
            if el.name == "diagram" {
                if current == idx {
                    remove_at = Some(i);
                    break;
                }
                current += 1;
            }
        }
    }
    if let Some(i) = remove_at {
        doc.children.remove(i);
        Ok((deleted_id, idx))
    } else {
        Err("Page not found".to_string())
    }
}

// ============================================================================
// diagram-pages.ts（页模型读写）
// ============================================================================

/// 宽松解析（空/不合法返回 None）。
pub fn parse_xml(xml: &str) -> Option<XElement> {
    if xml.trim().is_empty() {
        return None;
    }
    let doc = super::xml_tree::parse_lenient(xml)?;
    if doc.name == "parsererror" {
        None
    } else {
        Some(doc)
    }
}

/// `<mxfile>` 的文件变量（%name% 占位），无则 None。
pub fn file_vars(doc: &XElement) -> Option<String> {
    if doc.name == "mxfile" {
        doc.attr("vars").map(|s| s.to_string())
    } else {
        None
    }
}

/// AI 操作的页：指定 id 的 diagram，缺省第一页。
pub fn page_element<'a>(doc: &'a XElement, page_id: Option<&str>) -> Option<&'a XElement> {
    let diagrams = doc.by_name("diagram");
    if diagrams.is_empty() {
        return None;
    }
    match page_id {
        Some(pid) => diagrams.into_iter().find(|d| d.attr("id") == Some(pid)),
        None => Some(diagrams[0]),
    }
}

/// 多页时页名；单页无名字返回 None。
pub fn page_name_of(xml: &str, page_id: Option<&str>) -> Option<String> {
    let doc = parse_xml(xml)?;
    if doc.by_name("diagram").len() < 2 {
        return None;
    }
    page_element(&doc, page_id).and_then(|d| d.attr("name").map(|s| s.to_string()))
}

/// 解压 draw.io 压缩页内容（base64 → raw deflate → URI decode）。
/// 旧文件未 URI 编码时返回解压文本。
pub fn decompress_page_content(compressed: &str) -> Option<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(compressed.trim())
        .ok()?;
    let mut inflated = String::new();
    use std::io::Read;
    flate2::read::DeflateDecoder::new(&bytes[..])
        .read_to_string(&mut inflated)
        .ok()?;
    match uri_decode(&inflated) {
        Some(s) => Some(s),
        None => Some(inflated),
    }
}

fn uri_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let b = u8::from_str_radix(&s[i + 1..i + 3], 16).ok()?;
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 页的 `<mxGraphModel>` 元素（压缩页则解压）。
pub fn model_of_page(diagram: &XElement) -> Option<XElement> {
    if let Some(model) = diagram.by_name("mxGraphModel").first() {
        return Some((*model).clone());
    }
    let inflated = decompress_page_content(&diagram.text_content())?;
    parse_xml(&inflated)?
        .by_name("mxGraphModel")
        .first()
        .map(|m| (*m).clone())
}

/// 页选择器：文档含该 id 时为 `{page_id}`，否则空（默认第一页）。
pub fn page_selector_for(xml: &str, page_id: Option<&str>) -> PageSelector {
    let Some(pid) = page_id else {
        return PageSelector::default();
    };
    let found = parse_xml(xml)
        .map(|doc| {
            doc.by_name("diagram")
                .iter()
                .any(|d| d.attr("id") == Some(pid))
        })
        .unwrap_or(false);
    if found {
        PageSelector {
            page_id: Some(pid.to_string()),
            ..Default::default()
        }
    } else {
        PageSelector::default()
    }
}

/// 页的 `<mxGraphModel>` XML；裸 `<mxGraphModel>` 原样返回。
pub fn page_model_xml(xml: &str, page_id: Option<&str>) -> Option<String> {
    let doc = parse_xml(xml)?;
    if doc.name == "mxGraphModel" {
        return Some(xml.to_string());
    }
    if doc.name != "mxfile" {
        return None;
    }
    let diagram = page_element(&doc, page_id)?;
    model_of_page(diagram).map(|m| serialize(&m))
}

/// 用 `model_xml` 替换一页内容（保留该页 id/name 与其他页、文件属性）。
pub fn replace_page_model(
    file_xml: &str,
    page_id: Option<&str>,
    model_xml: &str,
) -> Result<String, String> {
    let model = parse_xml(model_xml).ok_or("replacePageModel needs an <mxGraphModel>")?;
    if model.name != "mxGraphModel" {
        return Err("replacePageModel needs an <mxGraphModel>".to_string());
    }
    let mut doc = parse_xml(file_xml)
        .filter(|d| d.name == "mxfile")
        .unwrap_or_else(|| parse_xml(BLANK_MXFILE).expect("blank mxfile"));
    let diagram_idx = match page_element(&doc, page_id) {
        Some(_) => find_page_element(&doc, &page_selector_for(file_xml, page_id)).map(|(i, _)| i),
        None => None,
    }
    .unwrap_or(0);
    // 清空目标 diagram 子节点，插入新 model
    replace_diagram_model(&mut doc, diagram_idx, model);
    Ok(serialize(&doc))
}

fn replace_diagram_model(doc: &mut XElement, idx: usize, model: XElement) {
    let mut current = 0;
    for child in &mut doc.children {
        if let super::xml_tree::XmlNode::Element(el) = child {
            if el.name == "diagram" {
                if current == idx {
                    el.children.clear();
                    el.children.push(super::xml_tree::XmlNode::Element(model));
                    return;
                }
                current += 1;
            }
        }
    }
}

/// 把 AI 画的单页放回画布文档：单页替换 AI 页，多页替换整份文档；
/// 文件变量随画的文档（画的文档无变量则保留画布的）。
pub fn place_on_page(drawn_xml: &str, canvas_xml: &str, page_id: Option<&str>) -> String {
    let doc = parse_xml(drawn_xml);
    let drawn_file = doc.as_ref().filter(|d| d.name == "mxfile");
    let several_pages = drawn_file
        .map(|d| d.by_name("diagram").len() > 1)
        .unwrap_or(false);
    let model = if several_pages {
        None
    } else {
        page_model_xml(drawn_xml, None)
    };
    match model {
        None => {
            let canvas = parse_xml(canvas_xml);
            let canvas_vars = canvas.as_ref().and_then(file_vars);
            if doc.is_none()
                || drawn_file.is_none()
                || file_vars(drawn_file.expect("checked")).is_some()
                || canvas_vars.is_none()
            {
                return drawn_xml.to_string();
            }
            let mut drawn_file = drawn_file.expect("checked").clone();
            drawn_file.set_attr("vars", canvas_vars.as_deref().expect("checked"));
            serialize(&drawn_file)
        }
        Some(model) => {
            let placed = replace_page_model(canvas_xml, page_id, &model)
                .unwrap_or_else(|_| canvas_xml.to_string());
            let vars = drawn_file.and_then(|d| d.attr("vars").map(|s| s.to_string()));
            match vars {
                None => placed,
                Some(v) => {
                    let mut result = parse_xml(&placed).expect("placed parses");
                    result.set_attr("vars", &v);
                    serialize(&result)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use std::io::Write;

    fn two_page() -> &'static str {
        r#"<mxfile><diagram id="p1" name="Page-1"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="A"/></root></mxGraphModel></diagram><diagram id="p2" name="Page-2"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="b" value="B"/></root></mxGraphModel></diagram></mxfile>"#
    }

    #[test]
    fn detects_cells() {
        let has = r#"<mxfile><diagram><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2"/></root></mxGraphModel></diagram></mxfile>"#;
        assert!(has_cells(has));
        assert!(!has_cells(BLANK_MXFILE));
        // UserObject 包装也算有内容
        assert!(has_cells(
            r#"<mxGraphModel><root><UserObject label="x"/></root></mxGraphModel>"#
        ));
    }

    #[test]
    fn detects_file_and_model_kinds() {
        assert!(is_mx_file("<mxfile><diagram/></mxfile>"));
        assert!(!is_mx_file("<mxGraphModel/>"));
        assert!(is_mx_graph_model("<mxGraphModel><root/></mxGraphModel>"));
        assert!(!is_mx_graph_model("<mxfile/>"));
    }

    #[test]
    fn escapes_and_strips() {
        assert_eq!(escape_attr("a&b<c>\"d"), "a&amp;b&lt;c&gt;&quot;d");
        assert_eq!(
            strip_xml_declaration(r#"<?xml version="1.0"?><mxGraphModel/>"#),
            "<mxGraphModel/>"
        );
    }

    #[test]
    fn wraps_cells_into_model() {
        let wrapped = wrap_cells_in_model(r#"<mxCell id="a" value="A"/>"#);
        assert!(wrapped.contains("<mxGraphModel>"));
        assert!(wrapped.contains(r#"<mxCell id="0"/>"#));
        assert!(wrapped.contains(r#"value="A""#));
        // 已是 model 原样返回
        let model = "<mxGraphModel><root/></mxGraphModel>";
        assert_eq!(wrap_cells_in_model(model), model);
    }

    #[test]
    fn normalizes_to_mxfile() {
        let model = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/></root></mxGraphModel>"#;
        let out = normalize_to_mxfile(model, Some("page-x"), Some("My Page")).unwrap();
        assert!(out.contains("<mxfile"));
        assert!(out.contains(r#"id="page-x""#));
        assert!(out.contains(r#"name="My Page""#));
        // 已是 mxfile 原样返回
        let file = "<mxfile><diagram/></mxfile>";
        assert_eq!(normalize_to_mxfile(file, None, None).unwrap(), file);
        // 空返回 None
        assert!(normalize_to_mxfile("  ", None, None).is_none());
    }

    #[test]
    fn parses_and_serializes() {
        let doc = parse_mxfile("<mxfile><diagram/></mxfile>").unwrap();
        assert_eq!(doc.name, "mxfile");
        assert!(parse_mxfile("not xml").is_none());
        assert!(parse_mxfile("<mxGraphModel/>").is_none());
        assert!(serialize_mxfile(&doc).contains("<mxfile"));
    }

    #[test]
    fn projects_single_page() {
        let p = project_page(
            two_page(),
            &PageSelector {
                page_id: Some("p2".into()),
                ..Default::default()
            },
        );
        match p {
            PageProjection::Ok { xml, index, name } => {
                assert_eq!(index, 1);
                assert_eq!(name, "Page-2");
                assert!(xml.contains(r#"id="p2""#));
                assert!(!xml.contains(r#"id="p1""#));
            }
            _ => panic!("expected Ok"),
        }
    }

    #[test]
    fn lists_and_finds_pages() {
        let doc = parse_mxfile(two_page()).unwrap();
        let pages = list_pages_from_doc(&doc);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].id, "p1");
        assert_eq!(pages[1].name, "Page-2");
        assert_eq!(
            find_page_element(
                &doc,
                &PageSelector {
                    page_id: Some("p2".into()),
                    ..Default::default()
                }
            )
            .unwrap()
            .0,
            1
        );
        assert_eq!(
            find_page_element(
                &doc,
                &PageSelector {
                    page_name: Some("Page-1".into()),
                    ..Default::default()
                }
            )
            .unwrap()
            .0,
            0
        );
        assert_eq!(
            find_page_element(
                &doc,
                &PageSelector {
                    page_index: Some(1),
                    ..Default::default()
                }
            )
            .unwrap()
            .0,
            1
        );
        assert!(find_page_element(
            &doc,
            &PageSelector {
                page_id: Some("missing".into()),
                ..Default::default()
            }
        )
        .is_none());
    }

    #[test]
    fn adds_renames_and_deletes_pages() {
        let mut doc = parse_mxfile(two_page()).unwrap();
        let p = add_page_to_doc(&mut doc, None, Some("Third"), None).unwrap();
        assert_eq!(p.name, "Third");
        assert_eq!(list_pages_from_doc(&doc).len(), 3);
        // 重复 id 拒绝
        assert!(add_page_to_doc(&mut doc, Some("p1"), None, None).is_err());
        // 非法 xml 拒绝
        assert!(add_page_to_doc(&mut doc, None, None, Some("<mxfile/>")).is_err());
        // 重命名
        assert!(rename_page_in_doc(
            &mut doc,
            &PageSelector {
                page_id: Some("p2".into()),
                ..Default::default()
            },
            "Renamed"
        ));
        // 删除
        let (id, _idx) = delete_page_from_doc(
            &mut doc,
            &PageSelector {
                page_id: Some("p2".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(id, "p2");
        assert_eq!(list_pages_from_doc(&doc).len(), 2);
    }

    #[test]
    fn rejects_deleting_last_page() {
        let mut doc = parse_mxfile(BLANK_MXFILE).unwrap();
        assert!(delete_page_from_doc(
            &mut doc,
            &PageSelector {
                page_id: Some("page-1".into()),
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn decompresses_page_content() {
        let xml = r#"<mxGraphModel><root><mxCell id="0"/></root></mxGraphModel>"#;
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(xml.as_bytes()).unwrap();
        let compressed = enc.finish().unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
        let out = decompress_page_content(&b64).unwrap();
        assert!(out.contains("<mxGraphModel"));
        // 非 base64 返回 None
        assert!(decompress_page_content("!!!not base64!!!").is_none());
    }

    #[test]
    fn uri_decodes_partially() {
        assert_eq!(uri_decode("hello%20world").unwrap(), "hello world");
        assert!(uri_decode("bad%").is_none());
        assert!(uri_decode("bad%zz").is_none());
    }

    #[test]
    fn gets_page_model_xml() {
        let model = page_model_xml(two_page(), Some("p1")).unwrap();
        assert!(model.contains("<mxGraphModel"));
        assert!(model.contains(r#"value="A""#));
        // 裸 model 原样返回
        let bare = "<mxGraphModel><root/></mxGraphModel>";
        assert_eq!(page_model_xml(bare, None).unwrap(), bare);
    }

    #[test]
    fn selects_page_for_id() {
        let sel = page_selector_for(two_page(), Some("p2"));
        assert_eq!(sel.page_id.as_deref(), Some("p2"));
        // 不存在则回退默认
        let sel = page_selector_for(two_page(), Some("nope"));
        assert!(sel.page_id.is_none());
    }

    #[test]
    fn replaces_page_model_keeping_others() {
        let new_model = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="new" value="N"/></root></mxGraphModel>"#;
        let out = replace_page_model(two_page(), Some("p1"), new_model).unwrap();
        assert!(out.contains(r#"value="N""#));
        assert!(out.contains(r#"value="B""#)); // 其他页保留
        assert!(!out.contains(r#"value="A""#)); // 旧页被替换
                                                // 非法 model 报错
        assert!(replace_page_model(two_page(), None, "not a model").is_err());
    }

    #[test]
    fn places_single_page_onto_canvas() {
        let drawn = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="x" value="X"/></root></mxGraphModel>"#;
        let out = place_on_page(drawn, two_page(), Some("p2"));
        assert!(out.contains(r#"value="X""#));
        assert!(out.contains(r#"value="A""#)); // p1 保留
    }

    #[test]
    fn file_vars_and_page_name() {
        let doc = parse_mxfile(two_page()).unwrap();
        assert!(file_vars(&doc).is_none());
        let with_vars = r#"<mxfile vars="%name%"><diagram id="p1" name="X"><mxGraphModel><root/></mxGraphModel></diagram></mxfile>"#;
        assert_eq!(
            file_vars(&parse_mxfile(with_vars).unwrap()).as_deref(),
            Some("%name%")
        );
        // 多页才有页名
        assert_eq!(
            page_name_of(two_page(), Some("p2")).as_deref(),
            Some("Page-2")
        );
        assert!(page_name_of(BLANK_MXFILE, None).is_none());
    }
}
