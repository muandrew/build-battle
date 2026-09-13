use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Workspace-level configuration and toolchain versions derived from Bazel workspace files
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub java_version: u32,
    pub java_installations: Vec<PathBuf>,
    pub kotlin_version: String,
    pub compose_compiler_version: Option<String>,
    pub agp_version: String,
    pub compile_sdk: u32,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub android_sdk_dir: Option<PathBuf>,
    pub maven_artifacts: HashMap<String, String>,
}

impl WorkspaceConfig {
    pub fn detect(workspace_root: &Path) -> Self {
        let workspace_content = std::fs::read_to_string(workspace_root.join("WORKSPACE"))
            .or_else(|_| std::fs::read_to_string(workspace_root.join("WORKSPACE.bazel")))
            .unwrap_or_default();
        let module_content = std::fs::read_to_string(workspace_root.join("MODULE.bazel"))
            .unwrap_or_default();
        let bazelrc_content = std::fs::read_to_string(workspace_root.join(".bazelrc"))
            .unwrap_or_default();
        let bazelrc_user = std::fs::read_to_string(workspace_root.join(".bazelrc.user"))
            .unwrap_or_default();

        let all_content = format!(
            "{}\n{}\n{}\n{}",
            workspace_content, module_content, bazelrc_content, bazelrc_user
        );

        // 1. Detect Java version and installations
        let java_version = Self::extract_java_version(&all_content).unwrap_or_else(|| {
            warn!("Unspecified Java language version in Bazel workspace, defaulting to 17");
            17
        });
        let java_installations = Self::detect_java_installations();

        // 2. Detect Kotlin version
        let kotlin_version = Self::extract_kotlin_version(&all_content).unwrap_or_else(|| {
            warn!("Unspecified Kotlin version in Bazel workspace, defaulting to 1.9.22");
            "1.9.22".to_string()
        });

        // 3. Detect Compose compiler version
        let compose_compiler_version = Self::extract_compose_compiler_version(&all_content);

        // 4. Detect AGP version
        let agp_version = Self::extract_agp_version(&all_content).unwrap_or_else(|| {
            warn!("Unspecified Android Gradle Plugin version, defaulting to 8.2.2");
            "8.2.2".to_string()
        });

        // 5. Detect compileSdk
        let compile_sdk = Self::extract_compile_sdk(&all_content).unwrap_or_else(|| {
            warn!("Unspecified compileSdk in Bazel workspace, defaulting to 34");
            34
        });

        // 6. Detect minSdk and targetSdk
        let min_sdk = 21;
        let target_sdk = 30;

        // 7. Detect Android SDK location
        let android_sdk_dir = Self::detect_android_sdk();

        // 8. Extract Maven artifact coordinates
        let maven_artifacts = Self::extract_maven_artifacts(&all_content);

        info!(
            "Workspace config detected: java={}, kotlin={}, compose={:?}, agp={}, compileSdk={}, maven_artifacts={}",
            java_version, kotlin_version, compose_compiler_version, agp_version, compile_sdk, maven_artifacts.len()
        );

        Self {
            java_version,
            java_installations,
            kotlin_version,
            compose_compiler_version,
            agp_version,
            compile_sdk,
            min_sdk,
            target_sdk,
            android_sdk_dir,
            maven_artifacts,
        }
    }

    fn extract_maven_artifacts(content: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let mut compose_version = None;
        for line in content.lines() {
            let trimmed = line
                .trim()
                .trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c.is_whitespace());
            if let Some((group, rest)) = trimmed.split_once(':') {
                if let Some((artifact, ver)) = rest.split_once(':') {
                    if group.starts_with("androidx.compose") && compose_version.is_none() && !group.contains("compiler") {
                        let clean_ver: String = ver.chars().take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '-').collect();
                        if !clean_ver.is_empty() {
                            compose_version = Some(clean_ver);
                        }
                    }
                    let sanitized_group = group.replace('.', "_").replace('-', "_");
                    let sanitized_art = artifact.replace('.', "_").replace('-', "_");
                    let key = format!("{}_{}", sanitized_group, sanitized_art);
                    map.insert(key.clone(), trimmed.to_string());
                    map.insert(format!("@maven//:{key}"), trimmed.to_string());
                    map.insert(format!("//:{key}"), trimmed.to_string());
                }
            }
        }
        if let Some(ref cv) = compose_version {
            let foundation = format!("androidx.compose.foundation:foundation:{cv}");
            let layout = format!("androidx.compose.foundation:foundation-layout:{cv}");
            for key in ["androidx_compose_foundation_foundation", "@maven//:androidx_compose_foundation_foundation", "//:androidx_compose_foundation_foundation"] {
                map.entry(key.to_string()).or_insert_with(|| foundation.clone());
            }
            for key in ["androidx_compose_foundation_foundation_layout", "@maven//:androidx_compose_foundation_foundation_layout", "//:androidx_compose_foundation_foundation_layout"] {
                map.entry(key.to_string()).or_insert_with(|| layout.clone());
            }
        }
        map
    }

    fn extract_kotlin_version(content: &str) -> Option<String> {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("_KOTLIN_COMPILER_VERSION") || trimmed.starts_with("KOTLIN_VERSION") {
                if let Some((_, val)) = trimmed.split_once('=') {
                    let clean = val.trim().trim_matches('"').trim_matches('\'').trim();
                    if !clean.is_empty() {
                        return Some(clean.to_string());
                    }
                }
            }
            if trimmed.contains("kotlinc_version") && trimmed.contains("release =") {
                if let Some((_, after)) = trimmed.split_once("release =") {
                    let clean = after.trim().trim_matches('"').trim_matches('\'').trim_matches(',').trim();
                    if !clean.is_empty() && !clean.starts_with('_') {
                        return Some(clean.to_string());
                    }
                }
            }
        }
        None
    }

    fn extract_compose_compiler_version(content: &str) -> Option<String> {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains("androidx.compose.compiler:compiler:") {
                if let Some((_, ver_part)) = trimmed.split_once("androidx.compose.compiler:compiler:") {
                    let clean: String = ver_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '-')
                        .collect();
                    if !clean.is_empty() {
                        return Some(clean);
                    }
                }
            }
        }
        None
    }

    fn extract_agp_version(_content: &str) -> Option<String> {
        // Can be extended if rules_android includes AGP version metadata
        None
    }

    fn extract_compile_sdk(content: &str) -> Option<u32> {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains("api_level =") {
                if let Some((_, val)) = trimmed.split_once("api_level =") {
                    let clean: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(num) = clean.parse::<u32>() {
                        return Some(num);
                    }
                }
            }
        }
        None
    }

    fn extract_java_version(content: &str) -> Option<u32> {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains("java_language_version=") || trimmed.contains("java_language_version =") {
                if let Some((_, val)) = trimmed.split_once("java_language_version=") {
                    let clean: String = val.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(num) = clean.parse::<u32>() {
                        return Some(num);
                    }
                } else if let Some((_, val)) = trimmed.split_once("java_language_version =") {
                    let clean: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(num) = clean.parse::<u32>() {
                        return Some(num);
                    }
                }
            }
            if trimmed.contains("remotejdk_") || trimmed.contains("remotejdk") {
                if let Some(pos) = trimmed.find("remotejdk") {
                    let after = &trimmed[pos + "remotejdk".len()..];
                    let after = after.trim_start_matches('_');
                    let clean: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(num) = clean.parse::<u32>() {
                        return Some(num);
                    }
                }
            }
        }
        None
    }

    fn detect_java_installations() -> Vec<PathBuf> {
        let mut installations = Vec::new();
        let mut seen = std::collections::HashSet::new();

        let mut add_if_jdk = |candidate: PathBuf| {
            let home = if candidate.join("Contents/Home/bin/javac").exists() {
                candidate.join("Contents/Home")
            } else if candidate.join("bin/javac").exists() {
                candidate
            } else {
                return;
            };
            if let Ok(canonical) = home.canonicalize() {
                if seen.insert(canonical.clone()) {
                    installations.push(canonical);
                }
            }
        };

        // 1. JAVA_HOME
        if let Ok(jh) = std::env::var("JAVA_HOME") {
            add_if_jdk(PathBuf::from(jh));
        }

        // 2. SDKMAN
        if let Ok(home) = std::env::var("HOME") {
            let sdkman_java = PathBuf::from(&home).join(".sdkman/candidates/java");
            if let Ok(entries) = std::fs::read_dir(sdkman_java) {
                for entry in entries.flatten() {
                    add_if_jdk(entry.path());
                }
            }
            // User JVMs
            let user_jvms = PathBuf::from(&home).join("Library/Java/JavaVirtualMachines");
            if let Ok(entries) = std::fs::read_dir(user_jvms) {
                for entry in entries.flatten() {
                    add_if_jdk(entry.path());
                }
            }
        }

        // 3. System JVMs (macOS / Linux)
        for base in ["/Library/Java/JavaVirtualMachines", "/usr/lib/jvm"] {
            if let Ok(entries) = std::fs::read_dir(base) {
                for entry in entries.flatten() {
                    add_if_jdk(entry.path());
                }
            }
        }

        // 4. Bazel output base JDKs
        let mut bazel_roots = vec![PathBuf::from("/var/tmp"), PathBuf::from("/private/var/tmp")];
        if let Ok(home) = std::env::var("HOME") {
            bazel_roots.push(PathBuf::from(home).join(".cache/bazel"));
        }

        for root in bazel_roots {
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name = file_name.to_string_lossy();
                    if name.starts_with("_bazel_") {
                        let bazel_user_dir = entry.path();
                        if let Ok(sub_entries) = std::fs::read_dir(&bazel_user_dir) {
                            for sub in sub_entries.flatten() {
                                let ext_dir = sub.path().join("external");
                                if let Ok(ext_entries) = std::fs::read_dir(ext_dir) {
                                    for ext in ext_entries.flatten() {
                                        let ext_path = ext.path();
                                        let ext_name = ext.file_name();
                                        let ext_name_str = ext_name.to_string_lossy();
                                        if ext_name_str.contains("remotejdk") || ext_name_str.contains("rules_java") {
                                            // Look for zulu or jdk inside
                                            if let Ok(inner) = std::fs::read_dir(&ext_path) {
                                                for in_entry in inner.flatten() {
                                                    add_if_jdk(in_entry.path());
                                                }
                                            }
                                            add_if_jdk(ext_path);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        installations
    }

    fn detect_android_sdk() -> Option<PathBuf> {
        if let Ok(home) = std::env::var("ANDROID_HOME") {
            let p = PathBuf::from(home);
            if p.exists() {
                return Some(p);
            }
        }
        if let Ok(root) = std::env::var("ANDROID_SDK_ROOT") {
            let p = PathBuf::from(root);
            if p.exists() {
                return Some(p);
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let mac_path = PathBuf::from(home).join("Library/Android/sdk");
            if mac_path.exists() {
                return Some(mac_path);
            }
        }
        None
    }
}
