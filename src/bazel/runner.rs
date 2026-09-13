use crate::bazel::model::{TargetLabel, TargetRule};
use crate::bazel::query::parse_bazel_query_xml;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

pub struct BazelRunner {
    pub bazel_bin: PathBuf,
    pub workspace_dir: PathBuf,
}

impl BazelRunner {
    pub fn new(bazel_bin: PathBuf, workspace_override: Option<PathBuf>) -> Result<Self> {
        let workspace_dir = match workspace_override {
            Some(w) => w,
            None => Self::detect_workspace_dir(&bazel_bin)?,
        };

        Ok(Self {
            bazel_bin,
            workspace_dir,
        })
    }

    /// Detect workspace directory via `bazel info workspace` or file system traversal
    fn detect_workspace_dir(bazel_bin: &Path) -> Result<PathBuf> {
        // Try invoking `bazel info workspace` first
        let output = Command::new(bazel_bin)
            .arg("info")
            .arg("workspace")
            .output();

        if let Ok(out) = output {
            if out.status.success() {
                let ws_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !ws_str.is_empty() {
                    return Ok(PathBuf::from(ws_str));
                }
            }
        }

        // Fallback: search parent directories for WORKSPACE / MODULE.bazel
        let mut curr = std::env::current_dir()?;
        loop {
            if curr.join("WORKSPACE").exists()
                || curr.join("WORKSPACE.bazel").exists()
                || curr.join("MODULE.bazel").exists()
            {
                return Ok(curr);
            }
            if !curr.pop() {
                break;
            }
        }

        bail!("Could not detect a Bazel workspace root directory. Please provide --workspace <PATH>.")
    }

    /// Execute `bazel query` with transitive dependency closure and parse XML
    pub fn query_dependencies(
        &self,
        target_patterns: &[String],
    ) -> Result<HashMap<TargetLabel, TargetRule>> {
        if target_patterns.is_empty() {
            return Ok(HashMap::new());
        }

        // Construct query string: deps(target1 + target2 + ...)
        let query_expr = if target_patterns.len() == 1 {
            format!("deps({})", target_patterns[0])
        } else {
            let combined = target_patterns.join(" + ");
            format!("deps({})", combined)
        };

        info!("Executing Bazel query: {}", query_expr);

        let output = Command::new(&self.bazel_bin)
            .current_dir(&self.workspace_dir)
            .arg("query")
            .arg(&query_expr)
            .arg("--output=xml")
            .output()
            .with_context(|| format!("Failed to execute Bazel binary at {:?}", self.bazel_bin))?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            bail!("Bazel query failed: {}", err_msg);
        }

        let xml_str = String::from_utf8_lossy(&output.stdout);
        debug!("Bazel query returned {} bytes", xml_str.len());

        parse_bazel_query_xml(&xml_str)
    }

    /// Resolve target patterns to explicit target labels
    pub fn resolve_targets(&self, target_patterns: &[String]) -> Result<Vec<TargetLabel>> {
        let pattern_str = target_patterns.join(" + ");
        let output = Command::new(&self.bazel_bin)
            .current_dir(&self.workspace_dir)
            .arg("query")
            .arg(&pattern_str)
            .output()
            .with_context(|| format!("Failed to resolve target patterns {:?}", target_patterns))?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            bail!("Bazel target pattern resolution failed: {}", err_msg);
        }

        let out_str = String::from_utf8_lossy(&output.stdout);
        let labels = out_str
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(TargetLabel::parse)
            .collect();

        Ok(labels)
    }
}
