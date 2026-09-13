use anyhow::Result;
use clap::Parser;
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

fn main() -> Result<()> {
    let args = Cli::parse();

    let log_level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Running gv (Gradle View generator)");
    info!("Output Directory: {:?}", args.output_dir);
    info!("Requested Targets: {:?}", args.targets);

    // 1. Initialize Bazel runner & workspace
    let bazel_runner = BazelRunner::new(args.bazel_bin, args.workspace)?;
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
    let generator = GradleGenerator::new(args.output_dir);
    generator.generate(&sliced_view)?;

    info!("Done!");
    Ok(())
}
