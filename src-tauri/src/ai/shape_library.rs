//! 图标库（上游 `shape-library.ts`）。
//!
//! 30 个图标库的文档编译期嵌入（assets/prompt/shape-libraries/*.md）。工具
//! `get_shape_library` 按名字返回文档，供模型在画图标前查询语法。

/// 库分组（上游 SHAPE_LIBRARY_GROUPS）。
const SHAPE_LIBRARY_GROUPS: &[(&str, &[&str])] = &[
    (
        "Cloud",
        &[
            "aws4",
            "azure2",
            "gcp2",
            "alibaba_cloud",
            "openstack",
            "salesforce",
        ],
    ),
    (
        "Networking",
        &["cisco19", "network", "kubernetes", "vvd", "rack"],
    ),
    ("Business", &["bpmn", "lean_mapping"]),
    (
        "General",
        &["flowchart", "basic", "arrows2", "infographic", "sitemap"],
    ),
    ("UI/Mockups", &["android", "material_design"]),
    ("Enterprise", &["citrix", "sap", "mscae", "atlassian"]),
    (
        "Engineering",
        &["fluidpower", "electrical", "pid", "cabinets", "floorplan"],
    ),
    ("Icons", &["webicons"]),
];

/// 库名列表（用于工具描述，一行一组）。
pub fn shape_library_list() -> String {
    SHAPE_LIBRARY_GROUPS
        .iter()
        .map(|(group, names)| format!("- {group}: {}", names.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 所有库名（小写）。
pub fn library_names() -> Vec<&'static str> {
    SHAPE_LIBRARY_GROUPS
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .collect()
}

/// 库文档内容（编译期嵌入）。
pub fn shape_library_content(library: &str) -> Option<&'static str> {
    Some(match library {
        "aws4" => include_str!("../../assets/prompt/shape-libraries/aws4.md"),
        "azure2" => include_str!("../../assets/prompt/shape-libraries/azure2.md"),
        "gcp2" => include_str!("../../assets/prompt/shape-libraries/gcp2.md"),
        "alibaba_cloud" => include_str!("../../assets/prompt/shape-libraries/alibaba_cloud.md"),
        "openstack" => include_str!("../../assets/prompt/shape-libraries/openstack.md"),
        "salesforce" => include_str!("../../assets/prompt/shape-libraries/salesforce.md"),
        "cisco19" => include_str!("../../assets/prompt/shape-libraries/cisco19.md"),
        "network" => include_str!("../../assets/prompt/shape-libraries/network.md"),
        "kubernetes" => include_str!("../../assets/prompt/shape-libraries/kubernetes.md"),
        "vvd" => include_str!("../../assets/prompt/shape-libraries/vvd.md"),
        "rack" => include_str!("../../assets/prompt/shape-libraries/rack.md"),
        "bpmn" => include_str!("../../assets/prompt/shape-libraries/bpmn.md"),
        "lean_mapping" => include_str!("../../assets/prompt/shape-libraries/lean_mapping.md"),
        "flowchart" => include_str!("../../assets/prompt/shape-libraries/flowchart.md"),
        "basic" => include_str!("../../assets/prompt/shape-libraries/basic.md"),
        "arrows2" => include_str!("../../assets/prompt/shape-libraries/arrows2.md"),
        "infographic" => include_str!("../../assets/prompt/shape-libraries/infographic.md"),
        "sitemap" => include_str!("../../assets/prompt/shape-libraries/sitemap.md"),
        "android" => include_str!("../../assets/prompt/shape-libraries/android.md"),
        "material_design" => include_str!("../../assets/prompt/shape-libraries/material_design.md"),
        "citrix" => include_str!("../../assets/prompt/shape-libraries/citrix.md"),
        "sap" => include_str!("../../assets/prompt/shape-libraries/sap.md"),
        "mscae" => include_str!("../../assets/prompt/shape-libraries/mscae.md"),
        "atlassian" => include_str!("../../assets/prompt/shape-libraries/atlassian.md"),
        "fluidpower" => include_str!("../../assets/prompt/shape-libraries/fluidpower.md"),
        "electrical" => include_str!("../../assets/prompt/shape-libraries/electrical.md"),
        "pid" => include_str!("../../assets/prompt/shape-libraries/pid.md"),
        "cabinets" => include_str!("../../assets/prompt/shape-libraries/cabinets.md"),
        "floorplan" => include_str!("../../assets/prompt/shape-libraries/floorplan.md"),
        "webicons" => include_str!("../../assets/prompt/shape-libraries/webicons.md"),
        _ => return None,
    })
}

/// 获取库文档；未知名或缺失返回错误消息（喂给模型）。
pub fn get_shape_library(name: &str) -> Result<String, String> {
    let library = name.trim().to_lowercase();
    if !library_names().contains(&library.as_str()) {
        return Err(format!(
            "Library \"{name}\" not found. Available: {}",
            library_names().join(", ")
        ));
    }
    shape_library_content(&library)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Library \"{library}\" is missing from this installation."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_all_groups() {
        let list = shape_library_list();
        assert!(list.contains("Cloud:"));
        assert!(list.contains("Icons: webicons"));
    }

    #[test]
    fn resolves_known_library() {
        let text = get_shape_library("aws4").unwrap();
        assert!(!text.is_empty());
        assert!(text.contains("aws") || text.contains("AWS"));
    }

    #[test]
    fn rejects_unknown_library() {
        assert!(get_shape_library("bogus").is_err());
    }

    #[test]
    fn all_30_embedded() {
        assert_eq!(library_names().len(), 30);
        for name in library_names() {
            assert!(shape_library_content(name).is_some(), "missing {name}");
        }
    }
}
