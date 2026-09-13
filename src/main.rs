use anyhow::Result;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod bazel;
mod cli;
mod generator;
mod graph;

use bazel::BazelRunner;
use cli::Cli;
use generator::GradleGenerator;
use graph::GraphSlicer;

fn expand_tilde(path: std::path::PathBuf) -> std::path::PathBuf {
    if let Ok(path_str) = path.clone().into_os_string().into_string() {
        if path_str.starts_with("~/") || path_str == "~" {
            if let Some(home) = std::env::var_os("HOME") {
                let home_path = std::path::PathBuf::from(home);
                if path_str == "~" {
                    return home_path;
                } else {
                    return home_path.join(&path_str[2..]);
                }
            }
        }
    }
    path
}

pub fn find_workspace_prefix(workspace_root: &std::path::Path) -> std::path::PathBuf {
    let canonical = match std::fs::canonicalize(workspace_root) {
        Ok(c) => c,
        Err(_) => workspace_root.to_path_buf(),
    };

    if canonical.join(".git").exists() {
        return std::path::PathBuf::new();
    }

    let mut current = canonical.parent();
    while let Some(parent) = current {
        if parent.join(".git").exists() {
            if let Ok(rel) = canonical.strip_prefix(parent) {
                return rel.to_path_buf();
            }
        }
        current = parent.parent();
    }

    std::path::PathBuf::new()
}

fn main() -> Result<()> {
    let args = Cli::parse_normalized();

    let log_level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let ws_expanded = expand_tilde(args.workspace_root);
    let workspace_root = if ws_expanded.exists() {
        std::fs::canonicalize(&ws_expanded).unwrap_or(ws_expanded)
    } else {
        ws_expanded
    };

    let raw_output_dir = match args.output_dir {
        Some(dir) => dir,
        None => workspace_root.join(".gv"),
    };
    let output_dir = expand_tilde(raw_output_dir);
    let is_absolute = match args.output_path {
        cli::PathMode::Absolute => true,
        cli::PathMode::Relative => false,
    };

    let workspace_prefix = find_workspace_prefix(&workspace_root);

    info!("Running gv (Gradle View generator)");
    info!("Bazel Workspace Root: {:?}", workspace_root);
    info!("Workspace Prefix: {:?}", workspace_prefix);
    info!("Output Directory: {:?}", output_dir);
    info!("Requested Targets: {:?}", args.targets);
    info!("Path Mode: {:?}", args.output_path);

    // 1. Initialize Bazel runner & workspace
    let bazel_runner = BazelRunner::new(args.bazel_bin, workspace_root.clone())?;
    info!("Bazel Workspace: {:?}", bazel_runner.workspace_dir);

    // 2. Resolve requested target patterns into specific labels
    let resolved_targets = bazel_runner.resolve_targets(&args.targets)?;
    info!("Resolved {} explicit targets", resolved_targets.len());

    // 3. Query full transitive dependencies of targets
    let all_rules = bazel_runner.query_dependencies(&args.targets)?;
    info!("Found {} total rules in transitive dependency closure", all_rules.len());

    // 4. Slice dependency graph according to selection rules
    let sliced_view = GraphSlicer::slice_graph(&all_rules, &resolved_targets);
    info!(
        "Graph slicing complete: {} active Gradle modules, {} Bazel boundary dependencies",
        sliced_view.modules.len(),
        sliced_view.boundary_targets.len()
    );

    // 5. Generate Gradle project overlay in output directory
    let generator = GradleGenerator::new(output_dir, workspace_root, workspace_prefix, is_absolute);
    generator.generate(&sliced_view)?;

    info!("Done!");
    Ok(())
}
