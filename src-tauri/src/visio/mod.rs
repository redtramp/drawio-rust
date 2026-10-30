//! Visio 兼容 SVG 转换（S4）。
//!
//! foreignObject → text 的 Rust 移植（`fix_svg_for_visio.py`）。

pub mod converter;

pub use converter::fix_svg_for_visio;
