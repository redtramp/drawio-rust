//! VLM 视觉校验（上游 `lib/validation-schema.ts` + `diagram-validator.ts`）。
//!
//! 校验结果的 schema 与给模型的反馈格式化。

use serde::{Deserialize, Serialize};

/// 视觉校验结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationResult {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationIssue {
    #[serde(rename = "type")]
    pub issue_type: String, // overlap | edge_routing | text | layout | rendering
    pub severity: String, // critical | warning
    pub description: String,
}

/// 校验结果 JSON schema（结构化输出用）。
pub fn validation_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "valid": { "type": "boolean", "description": "True if there are no critical issues" },
            "issues": {
                "type": "array",
                "description": "List of visual issues found",
                "items": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": ["overlap", "edge_routing", "text", "layout", "rendering"] },
                        "severity": { "type": "string", "enum": ["critical", "warning"] },
                        "description": { "type": "string" }
                    },
                    "required": ["type", "severity", "description"]
                }
            },
            "suggestions": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["valid", "issues", "suggestions"]
    })
}

/// VLM 校验系统提示词（上游 `validation-prompts.ts`）。
pub const VALIDATION_SYSTEM_PROMPT: &str = "You are a diagram quality validator. Analyze the rendered diagram image for visual issues.\n\nEvaluate the diagram for the following issues:\n\n1. **Overlapping elements** (critical): Shapes covering each other inappropriately, making content unreadable, or a label lying on an icon, another shape or another label\n2. **Edge routing issues** (critical): Lines/arrows crossing through shapes or labels that are not their source or target, or several lines running on top of each other\n3. **Text overflow** (critical): Text running outside its shape or cut off\n4. **Text readability** (warning): Text too small to read, or a single character wrapped onto its own line\n5. **Layout quality** (warning): Poor spacing, misalignment, or cramped elements\n6. **Rendering errors** (critical): Incomplete, corrupted, or missing visual elements\n\nRules:\n- Set \"valid\" to true ONLY if there are no critical issues\n- Be specific about which elements have problems\n- Provide actionable suggestions\n- Minor cosmetic issues should be warnings, not critical\n- Empty diagrams or diagrams with only 1-2 elements should pass unless they have obvious errors\n- If the diagram looks generally acceptable, set valid to true even with minor warnings";

/// 校验失败反馈（喂给模型，引导其修正）。上游 `formatValidationFeedback`。
pub fn format_validation_feedback(result: &ValidationResult) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("DIAGRAM VISUAL VALIDATION FAILED".to_string());
    lines.push(String::new());

    let critical: Vec<&ValidationIssue> = result
        .issues
        .iter()
        .filter(|i| i.severity == "critical")
        .collect();
    let warnings: Vec<&ValidationIssue> = result
        .issues
        .iter()
        .filter(|i| i.severity == "warning")
        .collect();

    if !critical.is_empty() {
        lines.push("Critical Issues (must fix):".to_string());
        for issue in &critical {
            lines.push(format!("  - [{}] {}", issue.issue_type, issue.description));
        }
        lines.push(String::new());
    }
    if !warnings.is_empty() {
        lines.push("Warnings:".to_string());
        for issue in &warnings {
            lines.push(format!("  - [{}] {}", issue.issue_type, issue.description));
        }
        lines.push(String::new());
    }
    if !result.suggestions.is_empty() {
        lines.push("Suggestions to fix:".to_string());
        for s in &result.suggestions {
            lines.push(format!("  - {s}"));
        }
        lines.push(String::new());
    }
    lines.push(
        "Please regenerate the diagram with corrected layout to fix these visual issues."
            .to_string(),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_feedback() {
        let result = ValidationResult {
            valid: false,
            issues: vec![ValidationIssue {
                issue_type: "overlap".into(),
                severity: "critical".into(),
                description: "boxes overlap".into(),
            }],
            suggestions: vec!["move it".into()],
        };
        let text = format_validation_feedback(&result);
        assert!(text.contains("DIAGRAM VISUAL VALIDATION FAILED"));
        assert!(text.contains("overlap"));
        assert!(text.contains("move it"));
    }

    #[test]
    fn feedback_handles_warnings_and_empty() {
        // 仅 warning，无 critical
        let result = ValidationResult {
            valid: true,
            issues: vec![ValidationIssue {
                issue_type: "text".into(),
                severity: "warning".into(),
                description: "small text".into(),
            }],
            suggestions: vec![],
        };
        let text = format_validation_feedback(&result);
        assert!(text.contains("Warnings:"));
        assert!(!text.contains("Critical Issues"));
        // 空 issues + 空 suggestions 也不 panic
        let empty = ValidationResult {
            valid: false,
            issues: vec![],
            suggestions: vec![],
        };
        assert!(format_validation_feedback(&empty).contains("VALIDATION FAILED"));
    }

    #[test]
    fn schema_has_required_fields() {
        let schema = validation_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "valid"));
        assert!(schema["properties"]["issues"]["items"]["required"].is_array());
    }
}
