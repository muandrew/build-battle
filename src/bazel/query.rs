use crate::bazel::model::{RuleKind, TargetLabel, TargetRule};
use anyhow::{Context, Result};
use roxmltree::Document;
use std::collections::HashMap;

/// Parse Bazel XML query output into TargetRule objects
pub fn parse_bazel_query_xml(xml_content: &str) -> Result<HashMap<TargetLabel, TargetRule>> {
    let doc = Document::parse(xml_content).context("Failed to parse Bazel query XML output")?;
    let mut targets = HashMap::new();

    for node in doc.descendants() {
        if node.is_element() && node.tag_name().name() == "rule" {
            let class_name = node.attribute("class").unwrap_or("unknown");
            let raw_name = match node.attribute("name") {
                Some(name) => name,
                None => continue,
            };

            let label = TargetLabel::parse(raw_name);
            let mut kind = RuleKind::from_rule_class(class_name);

            let mut srcs = Vec::new();
            let mut deps = Vec::new();
            let mut runtime_deps = Vec::new();
            let mut exports = Vec::new();
            let mut resources = Vec::new();
            let mut javacopts = Vec::new();
            let mut main_class = None;
            let mut manifest = None;
            let mut custom_package = None;
            let mut resource_files = Vec::new();
            let mut manifest_values = HashMap::new();
            let mut plugins = Vec::new();

            for child in node.children() {
                if !child.is_element() {
                    continue;
                }

                let attr_name = child.attribute("name").unwrap_or("");
                match child.tag_name().name() {
                    "list" => match attr_name {
                        "srcs" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    srcs.push(val.to_string());
                                }
                            }
                        }
                        "deps" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    deps.push(TargetLabel::parse(val));
                                }
                            }
                        }
                        "runtime_deps" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    runtime_deps.push(TargetLabel::parse(val));
                                }
                            }
                        }
                        "exports" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    exports.push(TargetLabel::parse(val));
                                }
                            }
                        }
                        "resources" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    resources.push(val.to_string());
                                }
                            }
                        }
                        "resource_files" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    resource_files.push(val.to_string());
                                }
                            }
                        }
                        "javacopts" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    javacopts.push(val.to_string());
                                }
                            }
                        }
                        "plugins" => {
                            for item in child.children().filter(|c| c.is_element()) {
                                if let Some(val) = item.attribute("value") {
                                    plugins.push(TargetLabel::parse(val));
                                }
                            }
                        }
                        _ => {}
                    },
                    "string" => {
                        if attr_name == "main_class" {
                            if let Some(val) = child.attribute("value") {
                                main_class = Some(val.to_string());
                            }
                        } else if attr_name == "custom_package" {
                            if let Some(val) = child.attribute("value") {
                                custom_package = Some(val.to_string());
                            }
                        } else if attr_name == "manifest" {
                            if let Some(val) = child.attribute("value") {
                                manifest = Some(val.to_string());
                            }
                        } else if attr_name == "generator_function" {
                            if let Some(val) = child.attribute("value") {
                                if val == "kt_android_library" {
                                    kind = RuleKind::KotlinAndroidLibrary;
                                }
                            }
                        }
                    }
                    "label" => {
                        if attr_name == "manifest" {
                            if let Some(val) = child.attribute("value") {
                                manifest = Some(val.to_string());
                            }
                        }
                    }
                    "dict" => {
                        if attr_name == "manifest_values" {
                            for pair in child.children().filter(|c| c.is_element() && c.tag_name().name() == "pair") {
                                let strings: Vec<String> = pair
                                    .children()
                                    .filter(|c| c.is_element() && c.tag_name().name() == "string")
                                    .filter_map(|c| c.attribute("value").map(|s| s.to_string()))
                                    .collect();
                                if strings.len() == 2 {
                                    manifest_values.insert(strings[0].clone(), strings[1].clone());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            targets.insert(
                label.clone(),
                TargetRule {
                    label,
                    kind,
                    srcs,
                    deps,
                    runtime_deps,
                    exports,
                    resources,
                    javacopts,
                    main_class,
                    manifest,
                    custom_package,
                    resource_files,
                    manifest_values,
                    plugins,
                },
            );
        }
    }

    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sample_xml() {
        let xml = r#"
        <query version="2">
            <rule class="java_library" name="//java/com/example:core">
                <list name="srcs">
                    <label value="//java/com/example:Core.java"/>
                </list>
                <list name="deps">
                    <label value="//java/com/example/util:util"/>
                </list>
            </rule>
            <rule class="java_library" name="//java/com/example/util:util">
                <list name="srcs">
                    <label value="//java/com/example/util:Util.java"/>
                </list>
            </rule>
        </query>
        "#;

        let targets = parse_bazel_query_xml(xml).expect("XML should parse successfully");
        assert_eq!(targets.len(), 2);
        let core_label = TargetLabel::parse("//java/com/example:core");
        let core = targets.get(&core_label).expect("Core target should exist");
        assert_eq!(core.srcs, vec!["//java/com/example:Core.java"]);
        assert_eq!(core.deps, vec![TargetLabel::parse("//java/com/example/util:util")]);
    }
}
