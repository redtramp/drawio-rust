//! 系统提示词组装（上游 `lib/system-prompts.ts`）。
//!
//! 三部分纯文本资产编译期嵌入（assets/prompt/parts/*.md，由
//! `scripts/extract-prompt.mjs` 从上游精确抽取，L2 同步覆盖）。

/// 组装系统提示词。`minimal_style` 时在开头前置最简样式指令（更醒目），
/// 否则在末尾追加完整样式指令。
pub fn get_system_prompt(model_id: &str, minimal_style: bool) -> String {
    let default_prompt = include_str!("../../assets/prompt/parts/default.md");
    let style = include_str!("../../assets/prompt/parts/style.md");
    let minimal = include_str!("../../assets/prompt/parts/minimal.md");
    let prompt = if minimal_style {
        format!("{minimal}{default_prompt}")
    } else {
        format!("{default_prompt}{style}")
    };
    prompt.replace("{{MODEL_NAME}}", model_id)
}

/// 模型目录（assets/prompt/model-catalog.json，L2 同步覆盖）。
pub fn model_catalog() -> &'static str {
    include_str!("../../assets/prompt/model-catalog.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_prepends_instruction() {
        let p = get_system_prompt("test-model", true);
        assert!(p
            .trim_start()
            .starts_with("## ⚠️ MINIMAL STYLE MODE ACTIVE"));
        assert!(p.contains("test-model"));
    }

    #[test]
    fn normal_appends_style() {
        let p = get_system_prompt("test-model", false);
        assert!(p
            .trim_start()
            .starts_with("You are an expert diagram creation assistant"));
        assert!(p.contains("## Styles"));
        assert!(!p.contains("{{MODEL_NAME}}"));
    }

    #[test]
    fn catalog_parses() {
        let _: serde_json::Value =
            serde_json::from_str(model_catalog()).expect("model catalog json");
    }
}
