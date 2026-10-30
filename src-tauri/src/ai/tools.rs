//! 工具协议与参数解析（上游 `lib/chat-tools.ts`）。
//!
//! 四个工具：display_diagram / edit_diagram / append_diagram / get_shape_library。
//! 提供参数结构与三种 provider 的工具 schema 序列化（OpenAI / Anthropic / Google）。

use serde::Deserialize;
use serde_json::{json, Value};

use super::edit::DiagramOperation;
use super::shape_library::shape_library_list;

pub const DISPLAY_DIAGRAM: &str = "display_diagram";
pub const EDIT_DIAGRAM: &str = "edit_diagram";
pub const APPEND_DIAGRAM: &str = "append_diagram";
pub const GET_SHAPE_LIBRARY: &str = "get_shape_library";

/// 工具调用（从 LLM 响应解析）。
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DisplayArgs {
    pub xml: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EditArgs {
    pub operations: Vec<DiagramOperation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppendArgs {
    pub xml: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShapeLibraryArgs {
    pub library: String,
}

/// 解析工具参数；失败返回错误（喂给模型）。
pub fn parse_tool_args(name: &str, arguments_json: &str) -> Result<ToolArgs, String> {
    match name {
        DISPLAY_DIAGRAM => {
            let args: DisplayArgs = serde_json::from_str(arguments_json)
                .map_err(|e| format!("display_diagram: invalid arguments: {e}"))?;
            Ok(ToolArgs::Display(args.xml))
        }
        EDIT_DIAGRAM => {
            let args: EditArgs = serde_json::from_str(arguments_json)
                .map_err(|e| format!("edit_diagram: invalid arguments: {e}"))?;
            Ok(ToolArgs::Edit(args.operations))
        }
        APPEND_DIAGRAM => {
            let args: AppendArgs = serde_json::from_str(arguments_json)
                .map_err(|e| format!("append_diagram: invalid arguments: {e}"))?;
            Ok(ToolArgs::Append(args.xml))
        }
        GET_SHAPE_LIBRARY => {
            let args: ShapeLibraryArgs = serde_json::from_str(arguments_json)
                .map_err(|e| format!("get_shape_library: invalid arguments: {e}"))?;
            Ok(ToolArgs::ShapeLibrary(args.library))
        }
        other => Err(format!("unknown tool \"{other}\"")),
    }
}

/// 解析后的工具参数。
#[derive(Debug, Clone)]
pub enum ToolArgs {
    Display(String),
    Edit(Vec<DiagramOperation>),
    Append(String),
    ShapeLibrary(String),
}

const DISPLAY_DESC: &str = "Display a diagram on draw.io. Pass ONLY the mxStyle definitions and the mxCell elements - wrapper tags and root cells are added automatically.\n\nVALIDATION RULES (XML will be rejected if violated):\n1. Generate ONLY mxStyle definitions and mxCell elements (an mxCell with custom data inside an <object> wrapper) - NO wrapper tags (<mxfile>, <mxGraphModel>, <root>)\n2. Do NOT include root cells (id=\"0\" or id=\"1\") - they are added automatically\n3. All mxCell elements must be siblings - never nested\n4. Every mxCell needs a unique id (start from \"2\")\n5. parent defaults to \"1\"; write parent=\"<container-id>\" only for shapes inside a container\n6. Escape special chars in values: &lt; &gt; &amp; &quot;\n\nA shape is one self-closing mxCell with x, y, w and h; an edge is one with source and target.";

const EDIT_DESC: &str = "Edit the current diagram by ID-based operations (update/add/delete cells).\n\nOperations:\n- update: Replace an existing cell by its id. Provide cell_id and complete new_xml.\n- add: Add a new cell. Provide cell_id (new unique id) and new_xml.\n- delete: Remove a cell. Cascade is automatic: children AND edges (source/target) are auto-deleted. Only specify ONE cell_id.\n\nFor update/add, new_xml is the complete mxCell in the compact form (a shape with x, y, w, h; an edge with source and target), with its complete style.\n\nJSON ESCAPING: Every \" inside new_xml MUST be escaped as \\\".";

const APPEND_DESC: &str = "Continue generating diagram XML when previous display_diagram output was truncated due to length limits. Do NOT include any wrapper tags - just continue the mxCell elements from EXACTLY where your previous output stopped.";

const SHAPE_DESC: &str = "Get draw.io shape/icon library documentation with style syntax and shape names. Call this tool to get shape names and usage syntax for a specific library.";

/// 工具 schema（OpenAI 格式）。
pub fn openai_tools() -> Vec<Value> {
    vec![
        fn_tool(
            DISPLAY_DIAGRAM,
            DISPLAY_DESC,
            json!({
                "type": "object",
                "properties": { "xml": { "type": "string", "description": "XML string to be displayed on draw.io" } },
                "required": ["xml"]
            }),
        ),
        fn_tool(
            EDIT_DIAGRAM,
            EDIT_DESC,
            json!({
                "type": "object",
                "properties": {
                    "operations": {
                        "type": "array",
                        "description": "Array of operations to apply",
                        "items": {
                            "type": "object",
                            "properties": {
                                "operation": { "type": "string", "enum": ["update", "add", "delete"] },
                                "cell_id": { "type": "string", "description": "The id of the mxCell" },
                                "new_xml": { "type": "string", "description": "Complete mxCell XML element (required for update/add)" }
                            },
                            "required": ["operation", "cell_id"]
                        }
                    }
                },
                "required": ["operations"]
            }),
        ),
        fn_tool(
            APPEND_DIAGRAM,
            APPEND_DESC,
            json!({
                "type": "object",
                "properties": { "xml": { "type": "string", "description": "Continuation XML fragment to append (NO wrapper tags)" } },
                "required": ["xml"]
            }),
        ),
        fn_tool(
            GET_SHAPE_LIBRARY,
            &format!(
                "{SHAPE_DESC}\n\nAvailable libraries:\n{}",
                shape_library_list()
            ),
            json!({
                "type": "object",
                "properties": { "library": { "type": "string", "description": "Library name (e.g., 'aws4', 'kubernetes', 'flowchart')" } },
                "required": ["library"]
            }),
        ),
    ]
}

fn fn_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

/// 工具 schema（Anthropic 格式）。
pub fn anthropic_tools() -> Vec<Value> {
    openai_tools()
        .into_iter()
        .map(|t| {
            let f = &t["function"];
            json!({
                "name": f["name"],
                "description": f["description"],
                "input_schema": f["parameters"]
            })
        })
        .collect()
}

/// 工具 schema（Google 格式）。
pub fn google_tools() -> Vec<Value> {
    openai_tools()
        .into_iter()
        .map(|t| {
            let f = &t["function"];
            json!({
                "name": f["name"],
                "description": f["description"],
                "parameters": f["parameters"]
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_display_args() {
        let a = parse_tool_args(DISPLAY_DIAGRAM, r#"{"xml":"<mxCell id=\"2\"/>"}"#).unwrap();
        assert!(matches!(a, ToolArgs::Display(x) if x.contains("mxCell")));
    }

    #[test]
    fn parses_edit_args() {
        let a = parse_tool_args(
            EDIT_DIAGRAM,
            r#"{"operations":[{"operation":"delete","cell_id":"2"}]}"#,
        )
        .unwrap();
        match a {
            ToolArgs::Edit(ops) => {
                assert_eq!(ops.len(), 1);
                assert_eq!(ops[0].cell_id, "2");
            }
            _ => panic!("expected edit"),
        }
    }

    #[test]
    fn rejects_unknown_tool() {
        assert!(parse_tool_args("bogus", "{}").is_err());
    }

    #[test]
    fn all_schema_shapes_have_names() {
        assert_eq!(openai_tools().len(), 4);
        assert_eq!(anthropic_tools().len(), 4);
        assert_eq!(google_tools().len(), 4);
    }
}
