use clap::{Parser, ValueEnum};
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[value(rename_all = "lowercase")]
pub enum PathMode {
    #[default]
    Absolute,
    Relative,
}

#[derive(Parser, Debug)]
#[command(
    name = "gv",
    about = "Generate a Gradle project view from Bazel targets with selective graph slicing",
    version
)]
pub struct Cli {
    /// Path to the Bazel workspace root directory
    #[arg(value_name = "BAZEL_PROJECT_ROOT")]
    pub workspace_root: PathBuf,

    /// Directory where the generated Gradle project view will be written
    #[arg(value_name = "OUTPUT_DIR")]
    pub output_dir: PathBuf,

    /// One or more Bazel targets (e.g. //pkg:target, pkg:all, pkg:...)
    #[arg(value_name = "TARGETS", required = true)]
    pub targets: Vec<String>,

    /// Path mode for emitted build files [relative|absolute] (defaults to absolute)
    #[arg(
        long = "outputpath",
        visible_alias = "output-path",
        visible_alias = "op",
        value_name = "MODE",
        default_value = "absolute"
    )]
    pub output_path: PathMode,

    /// Path to the Bazel binary to use
    #[arg(long, default_value = "bazel", value_name = "PATH")]
    pub bazel_bin: PathBuf,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,
}

impl Cli {
    pub fn parse_normalized() -> Self {
        Self::try_parse_normalized_from(std::env::args_os()).unwrap_or_else(|e| e.exit())
    }

    pub fn try_parse_normalized_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString>,
    {
        let normalized: Vec<OsString> = args
            .into_iter()
            .map(|arg| {
                let os_str: OsString = arg.into();
                if let Some(s) = os_str.to_str() {
                    if s == "-op" {
                        return OsString::from("--op");
                    } else if s.starts_with("-op=") {
                        return OsString::from(format!("--op={}", &s[4..]));
                    }
                }
                os_str
            })
            .collect();
        Self::try_parse_from(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_defaults() {
        let cli = Cli::try_parse_normalized_from(["gv", "/my/root", "/my/out", "//pkg:target"]).unwrap();
        assert_eq!(cli.workspace_root, PathBuf::from("/my/root"));
        assert_eq!(cli.output_dir, PathBuf::from("/my/out"));
        assert_eq!(cli.targets, vec!["//pkg:target"]);
        assert_eq!(cli.output_path, PathMode::Absolute);
        assert!(!cli.verbose);
    }

    #[test]
    fn test_cli_parsing_path_modes() {
        let cli_rel = Cli::try_parse_normalized_from([
            "gv", "/my/root", "/my/out", "//pkg:target", "-op", "relative",
        ])
        .unwrap();
        assert_eq!(cli_rel.output_path, PathMode::Relative);

        let cli_rel_eq = Cli::try_parse_normalized_from([
            "gv", "/my/root", "/my/out", "//pkg:target", "-op=relative",
        ])
        .unwrap();
        assert_eq!(cli_rel_eq.output_path, PathMode::Relative);

        let cli_abs = Cli::try_parse_normalized_from([
            "gv", "/my/root", "/my/out", "//pkg:target", "-op", "absolute",
        ])
        .unwrap();
        assert_eq!(cli_abs.output_path, PathMode::Absolute);

        let cli_long_rel = Cli::try_parse_normalized_from([
            "gv", "/my/root", "/my/out", "//pkg:target", "--outputpath", "relative",
        ])
        .unwrap();
        assert_eq!(cli_long_rel.output_path, PathMode::Relative);

        let cli_long_dash = Cli::try_parse_normalized_from([
            "gv", "/my/root", "/my/out", "//pkg:target", "--output-path", "relative",
        ])
        .unwrap();
        assert_eq!(cli_long_dash.output_path, PathMode::Relative);
    }
}

