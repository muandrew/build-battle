use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Parsed Bazel target label representation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetLabel {
    pub raw: String,
    pub package: String,
    pub target_name: String,
}

impl TargetLabel {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let stripped = trimmed.strip_prefix("//").unwrap_or(trimmed);
        if let Some((pkg, name)) = stripped.split_once(':') {
            TargetLabel {
                raw: raw.to_string(),
                package: pkg.to_string(),
                target_name: name.to_string(),
            }
        } else {
            // Target name defaults to last component of package path
            let pkg = stripped;
            let name = pkg.rsplit('/').next().unwrap_or(pkg);
            TargetLabel {
                raw: raw.to_string(),
                package: pkg.to_string(),
                target_name: name.to_string(),
            }
        }
    }

    /// Normalized canonical Bazel label `//package:target`
    pub fn canonical(&self) -> String {
        format!("//{}:{}", self.package, self.target_name)
    }

    /// Gradle project path matching the directory structure (e.g. `:java:com:google:copybara:util`)
    pub fn gradle_project_path(&self) -> String {
        let pkg_part = self.package.replace('/', ":");
        if pkg_part.is_empty() {
            format!(":{}", self.target_name)
        } else {
            format!(":{pkg_part}")
        }
    }

    /// Relative directory path for this package in an overlay structure
    pub fn package_dir(&self) -> &str {
        &self.package
    }

    /// Sanitized name suitable for Gradle task identifiers
    pub fn sanitized_name(&self) -> String {
        let pkg_part = self.package.replace(['/', '-', '.'], "_");
        if pkg_part.is_empty() {
            self.target_name.replace(['/', '-', '.'], "_")
        } else {
            format!("{}_{}", pkg_part, self.target_name.replace(['/', '-', '.'], "_"))
        }
    }
}

/// Known Bazel rule types mapped to Gradle plugins
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleKind {
    JavaLibrary,
    JavaBinary,
    JavaTest,
    KotlinJvmLibrary,
    KotlinJvmTest,
    ProtoLibrary,
    Other(String),
}

impl RuleKind {
    pub fn from_rule_class(class_name: &str) -> Self {
        match class_name {
            "java_library" => RuleKind::JavaLibrary,
            "java_binary" => RuleKind::JavaBinary,
            "java_test" => RuleKind::JavaTest,
            "kt_jvm_library" | "kt_android_library" => RuleKind::KotlinJvmLibrary,
            "kt_jvm_test" => RuleKind::KotlinJvmTest,
            "proto_library" | "java_proto_library" => RuleKind::ProtoLibrary,
            other => RuleKind::Other(other.to_string()),
        }
    }
}

/// Detailed target rule metadata extracted from Bazel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetRule {
    pub label: TargetLabel,
    pub kind: RuleKind,
    pub srcs: Vec<String>,
    pub deps: Vec<TargetLabel>,
    pub runtime_deps: Vec<TargetLabel>,
    pub exports: Vec<TargetLabel>,
    pub resources: Vec<String>,
    pub javacopts: Vec<String>,
    pub main_class: Option<String>,
}

impl TargetRule {
    pub fn all_dependencies(&self) -> HashSet<TargetLabel> {
        let mut all = HashSet::new();
        all.extend(self.deps.clone());
        all.extend(self.runtime_deps.clone());
        all.extend(self.exports.clone());
        all
    }
}
