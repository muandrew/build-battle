use crate::bazel::config::WorkspaceConfig;
use std::cmp::Ordering;

/// One row in the IDE ↔ AGP ↔ Gradle ↔ JDK ↔ compileSdk compatibility matrix.
///
/// Each profile represents a specific Android Studio release and the toolchain
/// versions it bundles / supports.
#[derive(Debug, Clone)]
pub struct IdeProfile {
    /// Short key used on the CLI (e.g. "k1", "p2", "q4")
    pub ide_key: &'static str,
    /// Human-readable name (e.g. "Panda 2")
    pub ide_name: &'static str,
    /// Android Studio version number (e.g. "2025.3.2")
    pub ide_version: &'static str,
    /// Default / bundled AGP version for this IDE release
    pub agp_version: &'static str,
    /// Minimum Gradle version required by this AGP
    pub min_gradle: &'static str,
    /// Minimum JDK version required by this AGP
    pub min_jdk: u32,
    /// Maximum compileSdk (API level) this AGP supports
    pub max_compile_sdk: u32,
    /// Maximum AGP version this IDE supports
    pub max_agp: &'static str,
}

/// The full compatibility table, ordered oldest → newest.
static PROFILES: &[IdeProfile] = &[
    IdeProfile { ide_key: "k1",  ide_name: "Koala",     ide_version: "2024.1.1", agp_version: "8.5.2",  min_gradle: "8.6",  min_jdk: 17, max_compile_sdk: 34, max_agp: "8.5"  },
    IdeProfile { ide_key: "k2",  ide_name: "Koala 2",   ide_version: "2024.1.2", agp_version: "8.6.1",  min_gradle: "8.7",  min_jdk: 17, max_compile_sdk: 35, max_agp: "8.6"  },
    IdeProfile { ide_key: "l1",  ide_name: "Ladybug",   ide_version: "2024.2.1", agp_version: "8.7.3",  min_gradle: "8.9",  min_jdk: 17, max_compile_sdk: 35, max_agp: "8.7"  },
    IdeProfile { ide_key: "l2",  ide_name: "Ladybug 2", ide_version: "2024.2.2", agp_version: "8.8.0",  min_gradle: "8.9",  min_jdk: 17, max_compile_sdk: 35, max_agp: "8.8"  },
    IdeProfile { ide_key: "m1",  ide_name: "Meerkat",   ide_version: "2024.3.1", agp_version: "8.9.1",  min_gradle: "8.11", min_jdk: 17, max_compile_sdk: 35, max_agp: "8.9"  },
    IdeProfile { ide_key: "m2",  ide_name: "Meerkat 2", ide_version: "2024.3.2", agp_version: "8.10.0", min_gradle: "8.11", min_jdk: 17, max_compile_sdk: 35, max_agp: "8.10" },
    IdeProfile { ide_key: "n1",  ide_name: "Narwhal",   ide_version: "2025.1.1", agp_version: "8.11.1", min_gradle: "8.11", min_jdk: 17, max_compile_sdk: 36, max_agp: "8.11" },
    IdeProfile { ide_key: "n2",  ide_name: "Narwhal 2", ide_version: "2025.1.2", agp_version: "8.12.1", min_gradle: "8.12", min_jdk: 17, max_compile_sdk: 36, max_agp: "8.13" },
    IdeProfile { ide_key: "o1",  ide_name: "Otter",     ide_version: "2025.2.1", agp_version: "8.13.0", min_gradle: "8.12", min_jdk: 17, max_compile_sdk: 36, max_agp: "8.13" },
    IdeProfile { ide_key: "o2",  ide_name: "Otter 2",   ide_version: "2025.2.2", agp_version: "9.0.0",  min_gradle: "9.1",  min_jdk: 17, max_compile_sdk: 36, max_agp: "9.0"  },
    IdeProfile { ide_key: "o3",  ide_name: "Otter 3",   ide_version: "2025.2.3", agp_version: "9.0.1",  min_gradle: "9.1",  min_jdk: 17, max_compile_sdk: 36, max_agp: "9.0"  },
    IdeProfile { ide_key: "p1",  ide_name: "Panda",     ide_version: "2025.3.1", agp_version: "9.1.0",  min_gradle: "9.1",  min_jdk: 17, max_compile_sdk: 36, max_agp: "9.1"  },
    IdeProfile { ide_key: "p2",  ide_name: "Panda 2",   ide_version: "2025.3.2", agp_version: "9.1.1",  min_gradle: "9.2",  min_jdk: 17, max_compile_sdk: 36, max_agp: "9.1"  },
    IdeProfile { ide_key: "p3",  ide_name: "Panda 3",   ide_version: "2025.3.3", agp_version: "9.2.0",  min_gradle: "9.3",  min_jdk: 17, max_compile_sdk: 36, max_agp: "9.2"  },
    IdeProfile { ide_key: "p4",  ide_name: "Panda 4",   ide_version: "2025.3.4", agp_version: "9.2.1",  min_gradle: "9.3",  min_jdk: 17, max_compile_sdk: 36, max_agp: "9.2"  },
    IdeProfile { ide_key: "q1",  ide_name: "Quail",     ide_version: "2026.1.1", agp_version: "9.3.0",  min_gradle: "9.5",  min_jdk: 17, max_compile_sdk: 37, max_agp: "9.3"  },
    IdeProfile { ide_key: "q2",  ide_name: "Quail 2",   ide_version: "2026.1.2", agp_version: "9.3.1",  min_gradle: "9.5",  min_jdk: 17, max_compile_sdk: 37, max_agp: "9.3"  },
    IdeProfile { ide_key: "q3",  ide_name: "Quail 3",   ide_version: "2026.1.3", agp_version: "9.4.0",  min_gradle: "9.6",  min_jdk: 17, max_compile_sdk: 37, max_agp: "9.4"  },
    IdeProfile { ide_key: "q4",  ide_name: "Quail 4",   ide_version: "2026.1.4", agp_version: "9.4.1",  min_gradle: "9.6",  min_jdk: 17, max_compile_sdk: 37, max_agp: "9.4"  },
];

// ---------------------------------------------------------------------------
// Semver-style version comparison
// ---------------------------------------------------------------------------

/// Parse a dotted version string into numeric components.
/// e.g. "8.9.1" → [8, 9, 1],  "8.11" → [8, 11]
fn parse_version(v: &str) -> Vec<u32> {
    v.split('.')
        .filter_map(|seg| seg.parse::<u32>().ok())
        .collect()
}

/// Compare two dotted version strings with semver-style ordering.
/// Missing trailing components are treated as 0 (e.g. "8.5" == "8.5.0").
#[allow(dead_code)]
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let va = parse_version(a);
    let vb = parse_version(b);
    let len = va.len().max(vb.len());
    for i in 0..len {
        let ca = va.get(i).copied().unwrap_or(0);
        let cb = vb.get(i).copied().unwrap_or(0);
        match ca.cmp(&cb) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// Compare version `a` against version `b`, truncating `a` to the same
/// number of components as `b`. This allows `max_agp: "8.5"` to match
/// any patch version like "8.5.2" (compared as "8.5" vs "8.5" = Equal).
pub fn version_cmp_truncated(a: &str, b: &str) -> Ordering {
    let va = parse_version(a);
    let vb = parse_version(b);
    let len = vb.len(); // truncate to b's precision
    for i in 0..len {
        let ca = va.get(i).copied().unwrap_or(0);
        let cb = vb.get(i).copied().unwrap_or(0);
        match ca.cmp(&cb) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return the full list of profiles, ordered oldest → newest.
pub fn list_profiles() -> &'static [IdeProfile] {
    PROFILES
}

/// Look up a profile by its short key (e.g. "p2", "q4").
/// Does **not** handle the virtual keys "min" / "max".
pub fn resolve_ide_profile(key: &str) -> Option<&'static IdeProfile> {
    let lower = key.to_ascii_lowercase();
    PROFILES.iter().find(|p| p.ide_key == lower)
}

/// Check compatibility between a workspace config and an IDE profile.
/// Returns a list of human-readable conflict descriptions.
/// An empty vec means the profile is fully compatible.
pub fn check_conflicts(config: &WorkspaceConfig, profile: &IdeProfile) -> Vec<String> {
    let mut conflicts = Vec::new();

    // compileSdk too high for this profile's AGP
    if config.compile_sdk > profile.max_compile_sdk {
        conflicts.push(format!(
            "compileSdk {} exceeds {} (AS {}) max of {}",
            config.compile_sdk, profile.ide_name, profile.ide_version, profile.max_compile_sdk
        ));
    }

    // targetSdk too high for this profile's AGP
    if config.target_sdk > profile.max_compile_sdk {
        conflicts.push(format!(
            "targetSdk {} exceeds {} (AS {}) max of {}",
            config.target_sdk, profile.ide_name, profile.ide_version, profile.max_compile_sdk
        ));
    }

    // Workspace AGP is newer than this IDE supports.
    // Compare only up to the precision of max_agp (e.g. "8.5" means all of 8.5.x).
    if version_cmp_truncated(&config.agp_version, profile.max_agp) == Ordering::Greater {
        conflicts.push(format!(
            "AGP {} exceeds {} (AS {}) max of {}",
            config.agp_version, profile.ide_name, profile.ide_version, profile.max_agp
        ));
    }

    // JDK too low for this profile
    if config.java_version < profile.min_jdk {
        conflicts.push(format!(
            "JDK {} is below {} (AS {}) minimum of {}",
            config.java_version, profile.ide_name, profile.ide_version, profile.min_jdk
        ));
    }

    conflicts
}

/// Find the **oldest** IDE profile that is compatible with the given workspace config.
pub fn find_min_compatible(config: &WorkspaceConfig) -> Option<&'static IdeProfile> {
    PROFILES.iter().find(|p| check_conflicts(config, p).is_empty())
}

/// Find the **newest** IDE profile that is compatible with the given workspace config.
pub fn find_max_compatible(config: &WorkspaceConfig) -> Option<&'static IdeProfile> {
    PROFILES.iter().rev().find(|p| check_conflicts(config, p).is_empty())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_config(compile_sdk: u32, agp: &str, java: u32) -> WorkspaceConfig {
        WorkspaceConfig {
            java_version: java,
            java_installations: vec![],
            kotlin_version: "1.9.22".to_string(),
            compose_compiler_version: None,
            agp_version: agp.to_string(),
            compile_sdk,
            min_sdk: 21,
            target_sdk: 30,
            android_sdk_dir: None,
            maven_artifacts: HashMap::new(),
        }
    }

    #[test]
    fn test_version_cmp() {
        assert_eq!(version_cmp("8.5", "8.5.0"), Ordering::Equal);
        assert_eq!(version_cmp("8.9", "8.10"), Ordering::Less);
        assert_eq!(version_cmp("8.10.0", "8.9.1"), Ordering::Greater);
        assert_eq!(version_cmp("9.0.0", "8.13.0"), Ordering::Greater);
        assert_eq!(version_cmp("9.4.1", "9.4.1"), Ordering::Equal);
        assert_eq!(version_cmp("8.5.2", "8.6"), Ordering::Less);
    }

    #[test]
    fn test_version_cmp_truncated() {
        // "8.5.2" truncated to 2 components = "8.5" vs "8.5" → Equal
        assert_eq!(version_cmp_truncated("8.5.2", "8.5"), Ordering::Equal);
        // "8.6.1" truncated to 2 components = "8.6" vs "8.5" → Greater
        assert_eq!(version_cmp_truncated("8.6.1", "8.5"), Ordering::Greater);
        // "8.4.0" truncated to 2 components = "8.4" vs "8.5" → Less
        assert_eq!(version_cmp_truncated("8.4.0", "8.5"), Ordering::Less);
        // Full precision match
        assert_eq!(version_cmp_truncated("9.4.1", "9.4"), Ordering::Equal);
        assert_eq!(version_cmp_truncated("9.5.0", "9.4"), Ordering::Greater);
    }

    #[test]
    fn test_resolve_profile_by_key() {
        let p2 = resolve_ide_profile("p2").unwrap();
        assert_eq!(p2.ide_name, "Panda 2");
        assert_eq!(p2.agp_version, "9.1.1");

        let q4 = resolve_ide_profile("q4").unwrap();
        assert_eq!(q4.ide_name, "Quail 4");
        assert_eq!(q4.agp_version, "9.4.1");

        let k1 = resolve_ide_profile("K1").unwrap(); // case insensitive
        assert_eq!(k1.ide_name, "Koala");
    }

    #[test]
    fn test_resolve_unknown_key() {
        assert!(resolve_ide_profile("z9").is_none());
        assert!(resolve_ide_profile("min").is_none());
        assert!(resolve_ide_profile("max").is_none());
    }

    #[test]
    fn test_check_conflicts_none() {
        let config = make_config(34, "8.5.2", 17);
        let k1 = resolve_ide_profile("k1").unwrap();
        assert!(check_conflicts(&config, k1).is_empty());
    }

    #[test]
    fn test_check_conflicts_compile_sdk_too_high() {
        let config = make_config(36, "8.5.2", 17);
        let k1 = resolve_ide_profile("k1").unwrap();
        let conflicts = check_conflicts(&config, k1);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("compileSdk 36"));
        assert!(conflicts[0].contains("max of 34"));
    }

    #[test]
    fn test_check_conflicts_agp_too_new() {
        let config = make_config(35, "9.4.0", 17);
        let m1 = resolve_ide_profile("m1").unwrap();
        let conflicts = check_conflicts(&config, m1);
        assert!(conflicts.iter().any(|c| c.contains("AGP 9.4.0") && c.contains("max of 8.9")));
    }

    #[test]
    fn test_check_conflicts_jdk_too_low() {
        let config = make_config(34, "8.5.2", 11);
        let k1 = resolve_ide_profile("k1").unwrap();
        let conflicts = check_conflicts(&config, k1);
        assert!(conflicts.iter().any(|c| c.contains("JDK 11") && c.contains("minimum of 17")));
    }

    #[test]
    fn test_find_min_compatible() {
        // compileSdk 34, AGP 8.5.2 → first compatible is Koala (k1)
        let config = make_config(34, "8.5.2", 17);
        let profile = find_min_compatible(&config).unwrap();
        assert_eq!(profile.ide_key, "k1");

        // compileSdk 36, AGP 8.11.1 → Koala through Meerkat2 have max_compile_sdk < 36
        let config2 = make_config(36, "8.11.1", 17);
        let profile2 = find_min_compatible(&config2).unwrap();
        assert_eq!(profile2.ide_key, "n1"); // Narwhal is first with max_compile_sdk=36
    }

    #[test]
    fn test_find_max_compatible() {
        // compileSdk 34, AGP 8.5 → max compatible is any profile that supports
        // AGP ≤ max_agp. AGP 8.5 fits all profiles.
        // But does compileSdk 34 conflict? No — 34 ≤ all max_compile_sdk values.
        // So max compatible is the last profile (q4).
        let config = make_config(34, "8.5.2", 17);
        let profile = find_max_compatible(&config).unwrap();
        assert_eq!(profile.ide_key, "q4");

        // compileSdk 35, AGP 9.4.0 → AGP 9.4.0 exceeds max_agp for everything
        // except q3 (max_agp=9.4) and q4 (max_agp=9.4). Both have max_compile_sdk=37 ≥ 35.
        // So max compatible is q4.
        let config2 = make_config(35, "9.4.0", 17);
        let profile2 = find_max_compatible(&config2).unwrap();
        assert_eq!(profile2.ide_key, "q4");
    }

    #[test]
    fn test_find_no_compatible() {
        // compileSdk 99 — nothing supports this
        let config = make_config(99, "8.5.2", 17);
        assert!(find_min_compatible(&config).is_none());
        assert!(find_max_compatible(&config).is_none());
    }

    #[test]
    fn test_profiles_ordered() {
        let profiles = list_profiles();
        assert!(profiles.len() >= 19);
        assert_eq!(profiles.first().unwrap().ide_key, "k1");
        assert_eq!(profiles.last().unwrap().ide_key, "q4");
    }
}
