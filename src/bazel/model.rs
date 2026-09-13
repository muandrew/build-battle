use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Parsed Bazel target label representation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetLabel {
    pub raw: String,
    pub repository: String,
    pub package: String,
    pub target_name: String,
}

impl TargetLabel {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let (repo, remainder) = if let Some(stripped) = trimmed.strip_prefix("@@") {
            if let Some(idx) = stripped.find("//") {
                (format!("@@{}", &stripped[..idx]), &stripped[idx + 2..])
            } else {
                (format!("@@{stripped}"), "")
            }
        } else if let Some(stripped) = trimmed.strip_prefix('@') {
            if let Some(idx) = stripped.find("//") {
                (format!("@{}", &stripped[..idx]), &stripped[idx + 2..])
            } else {
                (format!("@{stripped}"), "")
            }
        } else {
            let stripped = trimmed.strip_prefix("//").unwrap_or(trimmed);
            (String::new(), stripped)
        };

        let (pkg, target) = if let Some((p, t)) = remainder.split_once(':') {
            (p.to_string(), t.to_string())
        } else if remainder.is_empty() {
            (String::new(), String::new())
        } else {
            let pkg = remainder;
            let name = pkg.rsplit('/').next().unwrap_or(pkg);
            (pkg.to_string(), name.to_string())
        };

        TargetLabel {
            raw: raw.to_string(),
            repository: repo,
            package: pkg,
            target_name: target,
        }
    }

    /// Normalized canonical Bazel label `//package:target` or `@repo//package:target`
    pub fn canonical(&self) -> String {
        if self.repository.is_empty() {
            format!("//{}:{}", self.package, self.target_name)
        } else {
            format!("{}//{}:{}", self.repository, self.package, self.target_name)
        }
    }

    /// Relative directory path for this target's Gradle module in an overlay structure
    pub fn module_dir(&self, workspace_prefix: &std::path::Path) -> std::path::PathBuf {
        let mut path = std::path::PathBuf::new();
        for comp in workspace_prefix.components() {
            let s = comp.as_os_str().to_string_lossy();
            if !s.is_empty() {
                path.push(s.as_ref());
            }
        }
        if !self.package.is_empty() {
            path.push(&self.package);
        }
        path.push(&self.target_name);
        path
    }

    /// Gradle project path matching the directory structure including target name
    /// (e.g. `:java:com:google:copybara:buildozer:buildozer` or `:android:jetpack-compose:app:src:main:app`)
    pub fn gradle_project_path(&self, workspace_prefix: &std::path::Path) -> String {
        let mut parts = Vec::new();
        for comp in workspace_prefix.components() {
            let s = comp.as_os_str().to_string_lossy();
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
        if !self.package.is_empty() {
            for part in self.package.split('/') {
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
            }
        }
        parts.push(self.target_name.clone());

        format!(":{}", parts.join(":"))
    }

    /// Relative directory path for this package in the source tree
    pub fn package_dir(&self) -> &str {
        &self.package
    }

    /// Sanitized name suitable for Gradle task identifiers
    pub fn sanitized_name(&self) -> String {
        let mut parts = Vec::new();
        if !self.repository.is_empty() {
            parts.push(self.repository.as_str());
        }
        if !self.package.is_empty() {
            parts.push(self.package.as_str());
        }
        if !self.target_name.is_empty() {
            parts.push(self.target_name.as_str());
        }
        let full = parts.join("_");
        let sanitized: String = full
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        let trimmed = sanitized.trim_matches('_');
        if trimmed.is_empty() {
            "target".to_string()
        } else {
            trimmed.to_string()
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
            "java_library" | "android_library" => RuleKind::JavaLibrary,
            "java_binary" | "android_binary" => RuleKind::JavaBinary,
            "java_test" | "android_test" | "android_local_test" => RuleKind::JavaTest,
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
