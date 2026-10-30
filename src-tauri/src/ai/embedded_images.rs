//! 内嵌图片引用（上游 `embedded-images.ts`）。
//!
//! 粘贴进 draw.io 的图片以 base64 存在 cell style 里，可达几十万字符。给模型
//! 看时替换为短引用 `embedded-image-<id>`（同一图片始终同一 id），模型写出后
//! 再还原。id 由 cyrb53（53 位字符串哈希）生成，Node/浏览器/这里结果一致。

use std::collections::HashMap;

use regex::Regex;

const IMAGE_DATA: &str = r"(data:image/[A-Za-z0-9_.+\-]+(;base64)?,)([A-Za-z0-9+/]{200,}={0,2})";
const IMAGE_REF_PREFIX: &str = "embedded-image-";

fn image_data_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(IMAGE_DATA).expect("image data regex"))
}

fn image_ref_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"embedded-image-([0-9a-z]+)").expect("image ref regex"))
}

/// cyrb53：快速 53 位字符串哈希。JS `Math.imul` / 位运算的无符号等价。
fn image_id(data: &str) -> String {
    let mut h1: u32 = 0xdeadbeef;
    let mut h2: u32 = 0x41c6ce57;
    for &c in data.as_bytes() {
        let c = c as u32;
        h1 = (h1 ^ c).wrapping_mul(2654435761);
        h2 = (h2 ^ c).wrapping_mul(1597334677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2246822507);
    h1 ^= (h2 ^ (h2 >> 13)).wrapping_mul(3266489909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2246822507);
    h2 ^= (h1 ^ (h1 >> 13)).wrapping_mul(3266489909);
    let n: u64 = 4294967296u64 * ((2097151 & h2) as u64) + (h1 as u64);
    to_base36(n)
}

fn to_base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).expect("ascii")
}

/// 模型看到含引用的图时应附带的说明；无引用则空串。
pub fn image_ref_note(shown: &str) -> String {
    if shown.contains(IMAGE_REF_PREFIX) {
        "Pictures embedded in the diagram are shown as references (data:image/...,embedded-image-<id>). Keep a reference exactly as written to keep its picture, also when you redraw the whole diagram: the picture is put back when your XML is drawn.".to_string()
    } else {
        String::new()
    }
}

/// 每张内嵌图片的数据替换为引用。
pub fn hide_image_data(xml: &str) -> String {
    let re = image_data_re();
    let mut out = String::with_capacity(xml.len());
    let mut last = 0;
    for caps in re.captures_iter(xml) {
        let m = caps.get(0).unwrap();
        out.push_str(&xml[last..m.start()]);
        out.push_str(&caps[1]);
        out.push_str(IMAGE_REF_PREFIX);
        out.push_str(&image_id(&caps[3]));
        last = m.end();
    }
    out.push_str(&xml[last..]);
    out
}

/// 图片引用 → 图片数据的存储（有状态；每个会话一份，避免模块级全局）。
#[derive(Default)]
pub struct ImageStore {
    images: HashMap<String, String>,
    last_remembered: String,
}

impl ImageStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记住图里的图片，供后续 restore 使用。
    pub fn remember_images(&mut self, xml: &str) {
        if xml == self.last_remembered || !xml.contains("data:image/") {
            return;
        }
        self.last_remembered = xml.to_string();
        let re = image_data_re();
        for caps in re.captures_iter(xml) {
            let data = caps.get(3).unwrap().as_str();
            let id = image_id(data);
            self.images.entry(id).or_insert_with(|| data.to_string());
        }
    }

    /// 把 xml 里的引用替换为图片数据（来自 source 与已记住的图）。找不到的
    /// 引用保持原样。
    pub fn restore_image_data(&mut self, xml: &str, source: &str) -> String {
        if !xml.contains(IMAGE_REF_PREFIX) {
            return xml.to_string();
        }
        self.remember_images(source);
        let re = image_ref_re();
        let mut out = String::with_capacity(xml.len());
        let mut last = 0;
        for caps in re.captures_iter(xml) {
            let m = caps.get(0).unwrap();
            out.push_str(&xml[last..m.start()]);
            let id = caps.get(1).unwrap().as_str();
            match self.images.get(id) {
                Some(data) => out.push_str(data),
                None => out.push_str(m.as_str()),
            }
            last = m.end();
        }
        out.push_str(&xml[last..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data() -> String {
        "A".repeat(300)
    }

    #[test]
    fn hide_and_restore() {
        let xml = format!(
            r#"<mxCell style="image=data:image/png;base64,{};" vertex="1"/>"#,
            sample_data()
        );
        let hidden = hide_image_data(&xml);
        assert!(!hidden.contains(&sample_data()));
        assert!(hidden.contains(IMAGE_REF_PREFIX));

        let mut store = ImageStore::new();
        let restored = store.restore_image_data(&hidden, &xml);
        assert_eq!(restored, xml);
    }

    #[test]
    fn image_id_is_deterministic_base36() {
        let id = image_id("hello");
        assert!(!id.is_empty());
        assert!(id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_eq!(id, image_id("hello"));
    }

    #[test]
    fn short_data_not_hidden() {
        // 不足 200 字符的 data 保持原样（引用省不了多少）
        let xml = r#"<mxCell style="image=data:image/png;base64,abc;" vertex="1"/>"#;
        assert_eq!(hide_image_data(xml), xml);
    }
}
