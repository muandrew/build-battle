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

    /// Directory where the generated Gradle project view will be written (defaults to <BAZEL_PROJECT_ROOT>/.gv)
    #[arg(long = "output-dir", short = 'o', value_name = "OUTPUT_DIR")]
    pub output_dir: Option<PathBuf>,

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

fn looks_like_target(s: &str) -> bool {
    s.starts_with("//")
        || s.starts_with('@')
        || s.starts_with(':')
        || s.contains(':')
        || s == "..."
        || s.ends_with("/...")
        || s.ends_with(":all")
        || s.ends_with(":*")
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
        let raw_args: Vec<OsString> = args.into_iter().map(|a| a.into()).collect();
        if raw_args.is_empty() {
            return Self::try_parse_from(raw_args);
        }

        let prog_name = raw_args[0].clone();
        let rest = &raw_args[1..];

        let mut transformed_args: Vec<OsString> = Vec::new();
        let mut positional_indices: Vec<usize> = Vec::new();

        let mut i = 0;
        while i < rest.len() {
            let arg_os = &rest[i];
            let arg_str = arg_os.to_string_lossy();

            if arg_str == "-op" {
                transformed_args.push(OsString::from("--op"));
                if i + 1 < rest.len() {
                    i += 1;
                    transformed_args.push(rest[i].clone());
                }
            } else if arg_str.starts_with("-op=") {
                transformed_args.push(OsString::from(format!("--op={}", &arg_str[4..])));
            } else if arg_str == "-o"
                || arg_str == "--output-dir"
                || arg_str == "--outputpath"
                || arg_str == "--output-path"
                || arg_str == "--op"
                || arg_str == "--bazel-bin"
            {
                transformed_args.push(arg_os.clone());
                if i + 1 < rest.len() {
                    i += 1;
                    transformed_args.push(rest[i].clone());
                }
            } else if arg_str.starts_with('-') {
                transformed_args.push(arg_os.clone());
            } else {
                positional_indices.push(transformed_args.len());
                transformed_args.push(arg_os.clone());
            }
            i += 1;
        }

        // Positional args: pos 0 is workspace_root.
        // If there are >= 2 positional args:
        // Check if pos 1 looks like a target. If NOT, treat it as explicit output_dir.
        let mut final_args: Vec<OsString> = Vec::new();
        final_args.push(prog_name);

        let has_two_or_more_pos = positional_indices.len() >= 2;
        let pos1_idx = if has_two_or_more_pos {
            Some(positional_indices[1])
        } else {
            None
        };

        let pos1_is_output_dir = if let Some(idx) = pos1_idx {
            let val = transformed_args[idx].to_string_lossy();
            !looks_like_target(&val)
        } else {
            false
        };

        for (idx, item) in transformed_args.into_iter().enumerate() {
            if Some(idx) == pos1_idx && pos1_is_output_dir {
                let mut opt = OsString::from("--output-dir=");
                opt.push(item);
                final_args.push(opt);
            } else {
                final_args.push(item);
            }
        }

        Self::try_parse_from(final_args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_3_pos_args() {
        let cli = Cli::try_parse_normalized_from(["gv", "/my/root", "/my/out", "//pkg:target"]).unwrap();
        assert_eq!(cli.workspace_root, PathBuf::from("/my/root"));
        assert_eq!(cli.output_dir, Some(PathBuf::from("/my/out")));
        assert_eq!(cli.targets, vec!["//pkg:target"]);
        assert_eq!(cli.output_path, PathMode::Absolute);
        assert!(!cli.verbose);
    }

    #[test]
    fn test_cli_parsing_2_pos_args_with_target() {
        let cli = Cli::try_parse_normalized_from(["gv", "/my/root", "//pkg:target"]).unwrap();
        assert_eq!(cli.workspace_root, PathBuf::from("/my/root"));
        assert_eq!(cli.output_dir, None);
        assert_eq!(cli.targets, vec!["//pkg:target"]);
    }

    #[test]
    fn test_cli_parsing_2_pos_args_with_path_target() {
        let cli = Cli::try_parse_normalized_from([
            "gv",
            "e2e/examples/android/jetpack-compose",
            "e2e/examples/android/jetpack-compose/app/src/main:all",
        ])
        .unwrap();
        assert_eq!(
            cli.workspace_root,
            PathBuf::from("e2e/examples/android/jetpack-compose")
        );
        assert_eq!(cli.output_dir, None);
        assert_eq!(
            cli.targets,
            vec!["e2e/examples/android/jetpack-compose/app/src/main:all"]
        );
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

