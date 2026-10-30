//! AI 编排与移植逻辑（上游：next-ai-draw-io）。
//!
//! 模块地图（对照上游文件，见 docs/07-research/07-2）：
//! - [`xml_tree`]：DOMParser/XMLSerializer/saxes 替身
//! - [`xml_syntax`]：严格 well-formedness 检查（xml-syntax.ts）
//! - [`xml_attributes`]：属性读取与转义（xml-attributes.ts）
//! - [`style_classes`]：命名样式（style-classes.ts）
//! - [`compact_cells`]：紧凑 cell 展开/折叠（compact-cells.ts）
//! - [`embedded_images`]：内嵌图片引用（embedded-images.ts）
//! - [`xml_validation`]：校验 + 自动修复（xml-validation.ts）
//! - [`pages`]：多页 mxfile 助手（pages.ts + diagram-pages.ts + load-diagram.ts）
//! - [`prepare`]：新图装配（new-diagram.ts）
//! - [`edit`]：ID 级编辑（edit-diagram.ts + diagram-operations.ts）
//! - [`diff`]：cell 级差异（diagram-diff.ts）
//! - [`tools`]：工具协议与参数解析（chat-tools.ts）
//! - [`prompt`]：系统提示词组装（system-prompts.ts）
//! - [`shape_library`]：图标库（shape-library.ts）
//! - [`vlm`]：视觉校验（validation-schema.ts + diagram-validator.ts）
//! - [`providers`]：LLM 客户端（trait + 三个协议 + 目录）
//! - [`orchestrator`]：AI 回合循环
//! - [`session`]：回合内会话上下文

pub mod compact_cells;
pub mod diff;
pub mod edit;
pub mod embedded_images;
pub mod orchestrator;
pub mod pages;
pub mod prepare;
pub mod prompt;
pub mod providers;
pub mod session;
pub mod shape_library;
pub mod style_classes;
pub mod tools;
pub mod vlm;
pub mod xml_attributes;
pub mod xml_syntax;
pub mod xml_tree;
pub mod xml_validation;

pub use xml_tree::{parse_lenient, parse_strict, serialize, XElement, XmlNode, XmlSyntaxError};
