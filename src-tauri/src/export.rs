//! 导出编排（docs/03-2 §6、03-5 §5）：SVG/PNG/XML/Visio 落盘与 PDF。
//!
//! PDF 采用纯 Rust（resvg → PNG → printpdf 栅格页，ADR-9 主路径）；Visio 调用
//! drawio-core 的 foreignObject→text 转换器。

use std::path::Path;

use crate::Error;
use base64::Engine as _;

/// 解码 base64（容忍 `data:...;base64,` 前缀）。
pub fn decode_base64(data: &str) -> Result<Vec<u8>, Error> {
    let body = data.split_once(";base64,").map(|(_, b)| b).unwrap_or(data);
    base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .map_err(|e| Error::Export(format!("base64: {e}")))
}

/// 保存导出数据。format: png/svg/xml/visio-svg。返回 (是否 Visio, 转换数)。
pub fn save_export(format: &str, path: &Path, data_base64: &str) -> Result<usize, Error> {
    let bytes = decode_base64(data_base64)?;
    match format {
        "visio-svg" => {
            let svg = String::from_utf8(bytes).map_err(|e| Error::Export(format!("utf8: {e}")))?;
            let (fixed, n) = crate::visio::fix_svg_for_visio(&svg)?;
            std::fs::write(path, fixed.as_bytes()).map_err(Error::Io)?;
            Ok(n)
        }
        _ => {
            std::fs::write(path, &bytes).map_err(Error::Io)?;
            Ok(0)
        }
    }
}

/// PDF 导出：SVG → resvg 栅格 → printpdf 单页（栅格主路径，ADR-9）。
pub fn export_pdf(svg: &str, path: &Path, scale: f32, border: u32) -> Result<(), Error> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg;

    let opts = usvg::Options::default();
    let tree =
        usvg::Tree::from_str(svg, &opts).map_err(|e| Error::Export(format!("svg parse: {e}")))?;
    let size = tree.size();
    let w = (size.width() * scale).ceil() as u32;
    let h = (size.height() * scale).ceil() as u32;
    if w == 0 || h == 0 {
        return Err(Error::Export("empty diagram".to_string()));
    }

    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| Error::Export("pixmap alloc".to_string()))?;
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png = pixmap
        .encode_png()
        .map_err(|e| Error::Export(format!("png: {e}")))?;
    let img = image::load_from_memory(&png).map_err(|e| Error::Export(format!("decode: {e}")))?;

    // 页面尺寸（mm）：原始尺寸 + 边距；图按 dpi=72*scale 缩回原始尺寸（pt）。
    let pt_to_mm = 0.352778f32;
    let page_w_mm = (size.width() + border as f32 * 2.0) * pt_to_mm;
    let page_h_mm = (size.height() + border as f32 * 2.0) * pt_to_mm;
    let (doc, page_idx, layer_idx) = printpdf::PdfDocument::new(
        "drawio-rust diagram",
        printpdf::Mm(page_w_mm),
        printpdf::Mm(page_h_mm),
        "Diagram",
    );
    let layer = doc.get_page(page_idx).get_layer(layer_idx);
    let pdf_img = printpdf::Image::from_dynamic_image(&img);
    pdf_img.add_to_layer(
        layer,
        printpdf::ImageTransform {
            translate_x: Some(printpdf::Mm(border as f32 * pt_to_mm)),
            translate_y: Some(printpdf::Mm(border as f32 * pt_to_mm)),
            dpi: Some(72.0 * scale),
            ..Default::default()
        },
    );
    let file = std::fs::File::create(path).map_err(Error::Io)?;
    doc.save(&mut std::io::BufWriter::new(file))
        .map_err(|e| Error::Export(format!("pdf save: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_data_uri() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"hello");
        assert_eq!(
            decode_base64(&format!("data:image/png;base64,{b64}")).unwrap(),
            b"hello"
        );
        assert_eq!(decode_base64(&b64).unwrap(), b"hello");
    }

    #[test]
    fn saves_visio() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.svg");
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><switch><foreignObject><div xmlns="http://www.w3.org/1999/xhtml" style="padding-top:10px;margin-left:5px;width:100px;"><div style="font-size:12px;">Hi</div></div></foreignObject></switch></g></svg>"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
        let n = save_export("visio-svg", &path, &b64).unwrap();
        assert_eq!(n, 1);
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("<text"));
    }
}
