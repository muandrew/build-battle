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

    let output_dir = expand_tilde(args.output_dir);
    let is_absolute = match args.output_path {
        cli::PathMode::Absolute => true,
        cli::PathMode::Relative => false,
    };

    info!("Running gv (Gradle View generator)");
    info!("Bazel Workspace Root: {:?}", workspace_root);
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
    let generator = GradleGenerator::new(output_dir, workspace_root, is_absolute);
    generator.generate(&sliced_view)?;

    info!("Done!");
    Ok(())
}
