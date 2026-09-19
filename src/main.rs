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

    let ws_expanded = expand_tilde(args.workspace_root.clone());
    let workspace_root = if ws_expanded.exists() {
        std::fs::canonicalize(&ws_expanded).unwrap_or(ws_expanded)
    } else {
        ws_expanded
    };

    let raw_output_dir = match args.output_dir.clone() {
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

    // 1. Resolve IDE profile (if requested) early to validate flags fast
    let (gradle_version, profile_action) = resolve_ide_and_gradle_version(&args, &workspace_root)?;

    // 2. Initialize Bazel runner & workspace
    let bazel_runner = BazelRunner::new(args.bazel_bin.clone(), workspace_root.clone())?;
    info!("Bazel Workspace: {:?}", bazel_runner.workspace_dir);

    // 3. Resolve requested target patterns into specific labels
    let resolved_targets = bazel_runner.resolve_targets(&args.targets)?;
    info!("Resolved {} explicit targets", resolved_targets.len());

    // 4. Query full transitive dependencies of targets
    let all_rules = bazel_runner.query_dependencies(&args.targets)?;
    info!("Found {} total rules in transitive dependency closure", all_rules.len());

    // 5. Slice dependency graph according to selection rules
    let sliced_view = GraphSlicer::slice_graph(&all_rules, &resolved_targets);
    info!(
        "Graph slicing complete: {} active Gradle modules, {} Bazel boundary dependencies",
        sliced_view.modules.len(),
        sliced_view.boundary_targets.len()
    );

    // 6. Generate Gradle project overlay in output directory
    let mut generator = GradleGenerator::new(output_dir, workspace_root, workspace_prefix, is_absolute, gradle_version);

    match profile_action {
        ProfileAction::Apply(profile) => generator.config.apply_ide_profile(profile),
        ProfileAction::Force(profile) => generator.config.force_ide_profile(profile),
        ProfileAction::None => {}
    }

    generator.generate(&sliced_view)?;

    info!("Done!");
    Ok(())
}

#[derive(Debug)]
enum ProfileAction {
    Apply(&'static bazel::compat::IdeProfile),
    Force(&'static bazel::compat::IdeProfile),
    None,
}

/// Resolve --ide / --fide flags, apply profile overrides, and return the
/// Gradle version to use for the wrapper and an action to apply to config.
fn resolve_ide_and_gradle_version(
    args: &Cli,
    workspace_root: &std::path::Path,
) -> Result<(Option<String>, ProfileAction)> {
    use bazel::compat::{check_conflicts, find_max_compatible, find_min_compatible, list_profiles, resolve_ide_profile};
    use bazel::config::WorkspaceConfig;
    use tracing::warn;

    let format_unknown_key = |key: &str| -> anyhow::Error {
        let available = list_profiles()
            .iter()
            .map(|p| p.ide_key)
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::anyhow!("Unknown IDE profile key \"{key}\". Available keys: min, max, {available}")
    };

    if let Some(ref key) = args.fide {
        // --fide: force mode — reject min/max, override workspace values
        let lower = key.to_ascii_lowercase();
        if lower == "min" || lower == "max" {
            anyhow::bail!(
                "--fide does not accept \"{key}\". Use --ide {key} instead \
                 (min/max are computed from workspace context)"
            );
        }
        let profile = resolve_ide_profile(&lower).ok_or_else(|| format_unknown_key(key))?;
        info!(
            "Forcing IDE profile: {} (AS {}) — AGP {}, Gradle ≥{}, JDK ≥{}, compileSdk ≤{}",
            profile.ide_name, profile.ide_version, profile.agp_version,
            profile.min_gradle, profile.min_jdk, profile.max_compile_sdk
        );
        Ok((Some(profile.min_gradle.to_string()), ProfileAction::Force(profile)))
    } else {
        let is_default = args.ide.is_none();
        let key = args.ide.as_deref().unwrap_or("max");
        let lower = key.to_ascii_lowercase();
        let config = WorkspaceConfig::detect(workspace_root);

        if lower == "min" {
            let profile = find_min_compatible(&config).ok_or_else(|| {
                anyhow::anyhow!(
                    "No compatible IDE profile found for workspace \
                     (compileSdk={}, AGP={}, JDK={})",
                    config.compile_sdk, config.agp_version, config.java_version
                )
            })?;
            info!(
                "Computed minimum compatible IDE: {} (AS {}) — AGP {}, Gradle ≥{}",
                profile.ide_name, profile.ide_version, profile.agp_version, profile.min_gradle
            );
            Ok((Some(profile.min_gradle.to_string()), ProfileAction::Apply(profile)))
        } else if lower == "max" {
            let profile = find_max_compatible(&config).ok_or_else(|| {
                anyhow::anyhow!(
                    "No compatible IDE profile found for workspace \
                     (compileSdk={}, AGP={}, JDK={})",
                    config.compile_sdk, config.agp_version, config.java_version
                )
            })?;
            if is_default {
                info!(
                    "Defaulting to maximum compatible IDE: {} (AS {}) — AGP {}, Gradle ≥{}",
                    profile.ide_name, profile.ide_version, profile.agp_version, profile.min_gradle
                );
            } else {
                info!(
                    "Computed maximum compatible IDE: {} (AS {}) — AGP {}, Gradle ≥{}",
                    profile.ide_name, profile.ide_version, profile.agp_version, profile.min_gradle
                );
            }
            Ok((Some(profile.min_gradle.to_string()), ProfileAction::Apply(profile)))
        } else {
            let profile = resolve_ide_profile(&lower).ok_or_else(|| format_unknown_key(key))?;
            let conflicts = check_conflicts(&config, profile);
            if !conflicts.is_empty() {
                for c in &conflicts {
                    warn!("IDE compatibility conflict: {c}");
                }
                warn!(
                    "Use --fide {key} to force this profile and override workspace values"
                );
                info!(
                    "Selected IDE profile (unapplied due to conflicts): {} (AS {}) — AGP {}, Gradle ≥{}",
                    profile.ide_name, profile.ide_version, profile.agp_version, profile.min_gradle
                );
                Ok((Some(profile.min_gradle.to_string()), ProfileAction::None))
            } else {
                info!(
                    "Selected IDE profile: {} (AS {}) — AGP {}, Gradle ≥{}",
                    profile.ide_name, profile.ide_version, profile.agp_version, profile.min_gradle
                );
                Ok((Some(profile.min_gradle.to_string()), ProfileAction::Apply(profile)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_ide_default_is_max() {
        let ws = tempdir().unwrap();
        let args = Cli::try_parse_normalized_from(["gv", "/root", "//pkg:target"]).unwrap();
        assert_eq!(args.ide, None);
        assert_eq!(args.fide, None);

        let (gradle_ver, action) = resolve_ide_and_gradle_version(&args, ws.path()).unwrap();
        assert!(gradle_ver.is_some());
        match action {
            ProfileAction::Apply(p) => {
                // Default should find the max compatible IDE (e.g. Quail 4)
                assert_eq!(p.ide_key, "q4");
            }
            _ => panic!("Expected ProfileAction::Apply"),
        }
    }

    #[test]
    fn test_resolve_ide_fide_min_rejected() {
        let ws = tempdir().unwrap();
        let args = Cli::try_parse_normalized_from(["gv", "/root", "//pkg:target", "--fide", "min"]).unwrap();
        let err = resolve_ide_and_gradle_version(&args, ws.path()).unwrap_err();
        assert!(err.to_string().contains("--fide does not accept \"min\""));
    }

    #[test]
    fn test_resolve_ide_fide_max_rejected() {
        let ws = tempdir().unwrap();
        let args = Cli::try_parse_normalized_from(["gv", "/root", "//pkg:target", "--fide", "max"]).unwrap();
        let err = resolve_ide_and_gradle_version(&args, ws.path()).unwrap_err();
        assert!(err.to_string().contains("--fide does not accept \"max\""));
    }
}

