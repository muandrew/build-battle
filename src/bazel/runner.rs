use crate::bazel::model::{TargetLabel, TargetRule};
use crate::bazel::query::parse_bazel_query_xml;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

/// Normalize user target pattern (e.g. workspace-relative path or path target) to canonical Bazel target pattern
pub fn normalize_target_pattern(pattern: &str, workspace_dir: &Path) -> String {
    let trimmed = pattern.trim();
    if trimmed.starts_with("//") || trimmed.starts_with('@') {
        return trimmed.to_string();
    }

    let canonical_ws = std::fs::canonicalize(workspace_dir).unwrap_or_else(|_| workspace_dir.to_path_buf());

    if let Some((pkg_part, target_part)) = trimmed.split_once(':') {
        let pkg_path = Path::new(pkg_part);
        // Try canonicalizing pkg_path directly, or relative to workspace_dir
        let abs_pkg = if let Ok(can) = std::fs::canonicalize(pkg_path) {
            Some(can)
        } else if let Ok(can) = std::fs::canonicalize(canonical_ws.join(pkg_path)) {
            Some(can)
        } else {
            None
        };

        if let Some(abs) = abs_pkg {
            if let Ok(rel) = abs.strip_prefix(&canonical_ws) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                let rel_str = rel_str.trim_start_matches('/');
                if rel_str.is_empty() {
                    return format!("//:{target_part}");
                } else {
                    return format!("//{rel_str}:{target_part}");
                }
            }
        }

        // Fallback string manipulation (e.g. for mock/non-existent paths in unit tests)
        let ws_str = workspace_dir.to_string_lossy();
        let without_ws = trimmed.strip_prefix(ws_str.as_ref()).unwrap_or(trimmed);
        let s = without_ws.strip_prefix('/').unwrap_or(without_ws);
        let s = s.strip_prefix("./").unwrap_or(s);
        if s.starts_with(':') {
            format!("//{s}")
        } else {
            format!("//{s}")
        }
    } else if trimmed == "..." || trimmed == ":all" || trimmed == ":*" {
        format!("//{trimmed}")
    } else {
        let ws_str = workspace_dir.to_string_lossy();
        let without_ws = trimmed.strip_prefix(ws_str.as_ref()).unwrap_or(trimmed);
        let s = without_ws.strip_prefix('/').unwrap_or(without_ws);
        let s = s.strip_prefix("./").unwrap_or(s);
        format!("//{s}")
    }
}

pub struct BazelRunner {
    pub bazel_bin: PathBuf,
    pub workspace_dir: PathBuf,
}

impl BazelRunner {
    pub fn new(bazel_bin: PathBuf, workspace_dir: PathBuf) -> Result<Self> {
        let canonical_ws = if workspace_dir.exists() {
            std::fs::canonicalize(&workspace_dir)
                .with_context(|| format!("Failed to canonicalize workspace path {:?}", workspace_dir))?
        } else {
            workspace_dir
        };

        Ok(Self {
            bazel_bin,
            workspace_dir: canonical_ws,
        })
    }

    fn has_bzlmod(&self) -> bool {
        self.workspace_dir.join("MODULE.bazel").exists()
    }

    /// Execute `bazel query` with transitive dependency closure and parse XML
    pub fn query_dependencies(
        &self,
        target_patterns: &[String],
    ) -> Result<HashMap<TargetLabel, TargetRule>> {
        if target_patterns.is_empty() {
            return Ok(HashMap::new());
        }

        // Normalize target patterns
        let normalized_patterns: Vec<String> = target_patterns
            .iter()
            .map(|p| normalize_target_pattern(p, &self.workspace_dir))
            .collect();

        // Construct query string: deps(target1 + target2 + ...)
        let query_expr = if normalized_patterns.len() == 1 {
            format!("deps({})", normalized_patterns[0])
        } else {
            let combined = normalized_patterns.join(" + ");
            format!("deps({})", combined)
        };

        info!("Executing Bazel query: {}", query_expr);

        let mut cmd = Command::new(&self.bazel_bin);
        cmd.current_dir(&self.workspace_dir);
        cmd.arg("query");

        if self.has_bzlmod() {
            cmd.arg("--enable_bzlmod");
        }
        cmd.arg("--keep_going");
        cmd.arg(&query_expr);
        cmd.arg("--output=xml");

        let output = cmd
            .output()
            .with_context(|| format!("Failed to execute Bazel binary at {:?}", self.bazel_bin))?;

        let xml_str = String::from_utf8_lossy(&output.stdout);
        if xml_str.trim().is_empty() && !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            bail!("Bazel query failed: {}", err_msg);
        }

        debug!("Bazel query returned {} bytes", xml_str.len());

        parse_bazel_query_xml(&xml_str)
    }

    /// Resolve target patterns to explicit target labels
    pub fn resolve_targets(&self, target_patterns: &[String]) -> Result<Vec<TargetLabel>> {
        let normalized_patterns: Vec<String> = target_patterns
            .iter()
            .map(|p| normalize_target_pattern(p, &self.workspace_dir))
            .collect();

        let pattern_str = normalized_patterns.join(" + ");
        let mut cmd = Command::new(&self.bazel_bin);
        cmd.current_dir(&self.workspace_dir);
        cmd.arg("query");

        if self.has_bzlmod() {
            cmd.arg("--enable_bzlmod");
        }
        cmd.arg("--keep_going");
        cmd.arg(&pattern_str);

        let output = cmd
            .output()
            .with_context(|| format!("Failed to resolve target patterns {:?}", target_patterns))?;

        let out_str = String::from_utf8_lossy(&output.stdout);
        if out_str.trim().is_empty() && !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            bail!("Bazel target pattern resolution failed: {}", err_msg);
        }

        let labels = out_str
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with("WARNING:") && !l.starts_with("ERROR:") && !l.starts_with("Loading:"))
            .map(TargetLabel::parse)
            .collect();

        Ok(labels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_target_pattern() {
        let ws = Path::new("/my/workspace");
        assert_eq!(
            normalize_target_pattern("//pkg:target", ws),
            "//pkg:target"
        );
        assert_eq!(
            normalize_target_pattern("@maven//:dep", ws),
            "@maven//:dep"
        );
        assert_eq!(
            normalize_target_pattern("/my/workspace/pkg:target", ws),
            "//pkg:target"
        );
        assert_eq!(
            normalize_target_pattern("/my/workspace/pkg/sub:all", ws),
            "//pkg/sub:all"
        );
        assert_eq!(
            normalize_target_pattern("pkg/sub:all", ws),
            "//pkg/sub:all"
        );
        assert_eq!(
            normalize_target_pattern(":all", ws),
            "//:all"
        );
    }
}
