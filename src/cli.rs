use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "gv",
    about = "Generate a Gradle project view from Bazel targets with selective graph slicing",
    version
)]
pub struct Cli {
    /// Directory where the generated Gradle project view will be written
    #[arg(value_name = "OUTPUT_DIR")]
    pub output_dir: PathBuf,

    /// One or more Bazel targets (e.g. //pkg:target, pkg:all, pkg:...)
    #[arg(value_name = "TARGETS", required = true)]
    pub targets: Vec<String>,

    /// Path to the Bazel workspace root directory (defaults to auto-detection)
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    /// Path to the Bazel binary to use
    #[arg(long, default_value = "bazel", value_name = "PATH")]
    pub bazel_bin: PathBuf,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,
}
