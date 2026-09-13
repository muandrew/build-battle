use crate::bazel::config::WorkspaceConfig;
use crate::bazel::model::RuleKind;
use crate::graph::SlicedView;
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tracing::info;

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = components.last() {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            _ => components.push(component),
        }
    }
    components.into_iter().collect()
}

pub fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from_abs = if from.is_absolute() {
        normalize_path(from)
    } else {
        normalize_path(&std::env::current_dir().unwrap_or_default().join(from))
    };
    let to_abs = if to.is_absolute() {
        normalize_path(to)
    } else {
        normalize_path(&std::env::current_dir().unwrap_or_default().join(to))
    };

    let from_components: Vec<_> = from_abs.components().collect();
    let to_components: Vec<_> = to_abs.components().collect();

    let mut common_len = 0;
    while common_len < from_components.len()
        && common_len < to_components.len()
        && from_components[common_len] == to_components[common_len]
    {
        common_len += 1;
    }

    if common_len == 0 && (from_components.first() != to_components.first()) {
        return to_abs;
    }

    let mut result = PathBuf::new();
    for _ in common_len..from_components.len() {
        result.push("..");
    }
    for comp in &to_components[common_len..] {
        result.push(comp.as_os_str());
    }

    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

pub struct GradleGenerator {
    pub output_dir: PathBuf,
    pub workspace_root: PathBuf,
    pub workspace_prefix: PathBuf,
    pub path_absolute: bool,
    pub config: WorkspaceConfig,
}

impl GradleGenerator {
    pub fn new(
        output_dir: PathBuf,
        workspace_root: PathBuf,
        workspace_prefix: PathBuf,
        path_absolute: bool,
    ) -> Self {
        let config = WorkspaceConfig::detect(&workspace_root);
        Self {
            output_dir,
            workspace_root,
            workspace_prefix,
            path_absolute,
            config,
        }
    }

    /// Generate the overlay Gradle project structure
    pub fn generate(&self, sliced: &SlicedView) -> Result<()> {
        fs::create_dir_all(&self.output_dir)
            .with_context(|| format!("Failed to create output dir {:?}", self.output_dir))?;

        self.generate_settings_gradle(sliced)?;
        self.generate_root_build_gradle()?;
        self.generate_gradle_properties()?;
        self.generate_local_properties()?;
        self.generate_overlay_module_build_gradles(sliced)?;
        self.generate_wrapper()?;

        info!(
            "Successfully generated Gradle overlay in {:?} with {} modules and {} Bazel boundary targets.",
            self.output_dir,
            sliced.modules.len(),
            sliced.boundary_targets.len()
        );

        Ok(())
    }

    /// Generate local.properties with sdk.dir if Android SDK is available
    fn generate_local_properties(&self) -> Result<()> {
        if let Some(ref sdk_dir) = self.config.android_sdk_dir {
            let local_props_path = self.output_dir.join("local.properties");
            let mut file = File::create(local_props_path)?;
            let sdk_str = sdk_dir.to_string_lossy().replace('\\', "/");
            writeln!(file, "sdk.dir={sdk_str}")?;
        }
        Ok(())
    }

    /// Generate gradle.properties with JVM options and Android properties
    fn generate_gradle_properties(&self) -> Result<()> {
        let props_path = self.output_dir.join("gradle.properties");
        let mut file = File::create(props_path)?;
        writeln!(file, "org.gradle.jvmargs=-Xmx2048m -XX:MaxMetaspaceSize=512m")?;
        writeln!(file, "android.useAndroidX=true")?;
        writeln!(file, "android.nonTransitiveRClass=true")?;
        if !self.config.java_installations.is_empty() {
            let paths_str = self
                .config
                .java_installations
                .iter()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(file, "org.gradle.java.installations.paths={paths_str}")?;
        }
        Ok(())
    }


    /// Generate Gradle wrapper files
    fn generate_wrapper(&self) -> Result<()> {
        let temp_dir = tempfile::tempdir().ok();
        if let Some(ref tdir) = temp_dir {
            let _ = std::fs::write(tdir.path().join("settings.gradle.kts"), "rootProject.name = \"wrapper-init\"\n");
            let candidates = [
                "gradle",
                "/opt/homebrew/bin/gradle",
                "/usr/local/bin/gradle",
            ];
            for cand in candidates {
                match std::process::Command::new(cand)
                    .arg("wrapper")
                    .arg("--gradle-version")
                    .arg("8.5")
                    .current_dir(tdir.path())
                    .output()
                {
                    Ok(output) => {
                        if output.status.success() {
                            let _ = std::fs::copy(tdir.path().join("gradlew"), self.output_dir.join("gradlew"));
                            let _ = std::fs::copy(tdir.path().join("gradlew.bat"), self.output_dir.join("gradlew.bat"));
                            let t_wrapper = tdir.path().join("gradle/wrapper");
                            let out_wrapper = self.output_dir.join("gradle/wrapper");
                            let _ = std::fs::create_dir_all(&out_wrapper);
                            let _ = std::fs::copy(t_wrapper.join("gradle-wrapper.jar"), out_wrapper.join("gradle-wrapper.jar"));
                            let _ = std::fs::copy(t_wrapper.join("gradle-wrapper.properties"), out_wrapper.join("gradle-wrapper.properties"));
                            break;
                        } else {
                            tracing::warn!("gradle wrapper with {} failed: {}", cand, String::from_utf8_lossy(&output.stderr));
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to execute {}: {}", cand, e);
                    }
                }
            }
        }

        let wrapper_dir = self.output_dir.join("gradle/wrapper");
        fs::create_dir_all(&wrapper_dir)?;
        let props_path = wrapper_dir.join("gradle-wrapper.properties");
        if !props_path.exists() {
            let mut file = File::create(props_path)?;
            writeln!(
                file,
                "distributionBase=GRADLE_USER_HOME\n\
                 distributionPath=wrapper/dists\n\
                 distributionUrl=https\\://services.gradle.org/distributions/gradle-8.5-bin.zip\n\
                 zipStoreBase=GRADLE_USER_HOME\n\
                 zipStorePath=wrapper/dists"
            )?;
        }

        let gradlew_path = self.output_dir.join("gradlew");
        if gradlew_path.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&gradlew_path) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&gradlew_path, perms);
                }
            }
        }

        Ok(())
    }

    /// Generate settings.gradle.kts with all active module includes matching package paths
    fn generate_settings_gradle(&self, sliced: &SlicedView) -> Result<()> {
        let settings_path = self.output_dir.join("settings.gradle.kts");
        let mut file = File::create(&settings_path)?;

        writeln!(
            file,
            r#"pluginManagement {{
    repositories {{
        google()
        mavenCentral()
        gradlePluginPortal()
    }}
}}
dependencyResolutionManagement {{
    repositoriesMode.set(RepositoriesMode.PREFER_SETTINGS)
    repositories {{
        google()
        mavenCentral()
    }}
}}
rootProject.name = "bazel-gradle-view"
"#
        )?;

        let mut included_projects = std::collections::BTreeSet::new();
        for module_label in &sliced.modules {
            let project_path = module_label.gradle_project_path(&self.workspace_prefix);
            included_projects.insert(project_path);
        }

        for project_path in included_projects {
            writeln!(file, "include(\"{project_path}\")")?;
        }

        Ok(())
    }

    /// Generate root build.gradle.kts
    fn generate_root_build_gradle(&self) -> Result<()> {
        let root_build_path = self.output_dir.join("build.gradle.kts");
        let mut file = File::create(&root_build_path)?;

        let kotlin_ver = &self.config.kotlin_version;
        let agp_ver = &self.config.agp_version;

        writeln!(
            file,
            r#"plugins {{
    base
    kotlin("jvm") version "{kotlin_ver}" apply false
    kotlin("android") version "{kotlin_ver}" apply false
    id("com.android.application") version "{agp_ver}" apply false
    id("com.android.library") version "{agp_ver}" apply false
}}
"#
        )?;

        Ok(())
    }

    fn sanitize_and_write_manifest(src_path: &Path, dst_path: &Path, default_pkg: &str) -> std::io::Result<()> {
        if !src_path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(src_path)?;
        let pkg = if let Some(p) = content.split("package=\"").nth(1).and_then(|s| s.split('"').next()) {
            p.to_string()
        } else {
            default_pkg.to_string()
        };

        let mut modified = content;
        if !pkg.is_empty() {
            modified = modified.replace("android:name=\".", &format!("android:name=\"{pkg}."));
        }

        if let Some(start_idx) = modified.find("<manifest") {
            if let Some(end_idx) = modified[start_idx..].find('>') {
                let manifest_tag = &modified[start_idx..start_idx + end_idx];
                if let Some(pkg_start) = manifest_tag.find("package=\"") {
                    if let Some(pkg_end) = manifest_tag[pkg_start + 9..].find('"') {
                        let full_pkg_attr = &manifest_tag[pkg_start..pkg_start + 9 + pkg_end + 1];
                        modified = format!(
                            "{}{}{}",
                            &modified[..start_idx + pkg_start],
                            "",
                            &modified[start_idx + pkg_start + full_pkg_attr.len()..]
                        );
                    }
                }
            }
        }

        std::fs::write(dst_path, modified)
    }

    /// Generate build.gradle.kts in each overlay package directory
    fn generate_overlay_module_build_gradles(&self, sliced: &SlicedView) -> Result<()> {
        for mod_label in &sliced.modules {
            let module_rel_dir = mod_label.module_dir(&self.workspace_prefix);
            let module_overlay_dir = self.output_dir.join(&module_rel_dir);
            fs::create_dir_all(&module_overlay_dir)?;

            let build_file_path = module_overlay_dir.join("build.gradle.kts");
            let mut file = File::create(build_file_path)?;

            writeln!(
                file,
                "import java.io.File\n\
                 import java.io.FileOutputStream\n\
                 import java.util.zip.ZipFile\n\
                 import java.util.zip.ZipEntry\n\
                 import java.util.zip.ZipOutputStream\n"
            )?;

            let rule = sliced.target_rules.get(mod_label);

            let is_android_app = rule.map(|r| matches!(r.kind, RuleKind::AndroidApplication)).unwrap_or(false);
            let is_android_lib = rule.map(|r| matches!(r.kind, RuleKind::AndroidLibrary | RuleKind::KotlinAndroidLibrary)).unwrap_or(false);
            let is_android = rule.map(|r| r.kind.is_android()).unwrap_or(false);
            let is_kotlin = rule.map(|r| r.kind.is_kotlin() || r.srcs.iter().any(|s| s.ends_with(".kt"))).unwrap_or(false);
            let is_binary = rule.map(|r| matches!(r.kind, RuleKind::JavaBinary)).unwrap_or(false);
            let has_compose = rule.map(|r| r.has_compose()).unwrap_or(false) || (is_android && self.config.compose_compiler_version.is_some());

            // 1. Plugins
            let plugin_str = if is_android_app {
                if is_kotlin {
                    "plugins {\n    id(\"com.android.application\")\n    kotlin(\"android\")\n}".to_string()
                } else {
                    "plugins {\n    id(\"com.android.application\")\n}".to_string()
                }
            } else if is_android_lib {
                if is_kotlin {
                    "plugins {\n    id(\"com.android.library\")\n    kotlin(\"android\")\n}".to_string()
                } else {
                    "plugins {\n    id(\"com.android.library\")\n}".to_string()
                }
            } else if is_kotlin {
                format!("plugins {{\n    `java-library`\n    kotlin(\"jvm\") version \"{}\"\n}}", self.config.kotlin_version)
            } else if is_binary {
                "plugins {\n    application\n    `java-library`\n}".to_string()
            } else {
                "plugins {\n    `java-library`\n}".to_string()
            };
            writeln!(file, "{plugin_str}\n")?;

            // 2. Source configuration
            let src_dir = if mod_label.package_dir().is_empty() {
                self.workspace_root.clone()
            } else {
                self.workspace_root.join(mod_label.package_dir())
            };

            let has_srcs = rule.map(|r| !r.srcs.is_empty()).unwrap_or(true);
            let effective_src_dir = if let Some(r) = rule {
                if r.srcs.iter().any(|s| s.starts_with("java/") || s.contains("/java/")) && src_dir.join("java").exists() {
                    src_dir.join("java")
                } else if r.srcs.iter().any(|s| s.starts_with("src/main/java/") || s.contains("/src/main/java/")) && src_dir.join("src/main/java").exists() {
                    src_dir.join("src/main/java")
                } else {
                    src_dir.clone()
                }
            } else {
                src_dir.clone()
            };

            let effective_src_dir_str = if self.path_absolute {
                effective_src_dir.to_string_lossy().replace('\\', "/")
            } else {
                let rel = relative_path(&module_overlay_dir, &effective_src_dir);
                rel.to_string_lossy().replace('\\', "/")
            };

            let mut include_lines = String::new();
            if let Some(r) = rule {
                let rel_srcs: Vec<String> = r
                    .srcs
                    .iter()
                    .filter_map(|s| {
                        let trimmed = s.trim();
                        let without_java = if let Some(stripped) = trimmed.strip_prefix("java/") {
                            stripped
                        } else if let Some(stripped) = trimmed.strip_prefix("src/main/java/") {
                            stripped
                        } else {
                            trimmed
                        };

                        if let Some((pkg, file)) = without_java.strip_prefix("//").and_then(|t| t.split_once(':')) {
                            if pkg == mod_label.package_dir() {
                                Some(file.to_string())
                            } else {
                                Some(format!("{pkg}/{file}"))
                            }
                        } else if let Some(stripped) = without_java.strip_prefix(':') {
                            Some(stripped.to_string())
                        } else if !without_java.is_empty() && !without_java.starts_with('@') {
                            Some(without_java.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();

                if !rel_srcs.is_empty() {
                    let formatted_includes: Vec<String> = rel_srcs
                        .iter()
                        .map(|s| format!("                \"{s}\""))
                        .collect();
                    include_lines = format!("            include(\n{}\n            )\n", formatted_includes.join(",\n"));
                }
            }

            if is_android {
                let default_pkg = if mod_label.package_dir().is_empty() {
                    "com.example.app".to_string()
                } else {
                    mod_label.package_dir().replace('/', ".")
                };

                let base_ns = rule
                    .and_then(|r| r.custom_package.as_ref())
                    .cloned()
                    .or_else(|| rule.and_then(|r| r.manifest_values.get("applicationId")).cloned())
                    .unwrap_or(default_pkg);

                let namespace = if is_android_app {
                    base_ns.clone()
                } else {
                    format!("{}.{}", base_ns, mod_label.target_name.replace('-', "_"))
                };

                let app_id = rule
                    .and_then(|r| r.manifest_values.get("applicationId"))
                    .cloned()
                    .unwrap_or_else(|| base_ns.clone());

                let compile_sdk = self.config.compile_sdk;
                let min_sdk = self.config.min_sdk;
                let target_sdk = self.config.target_sdk;

                let compose_block = if has_compose {
                    let compose_compiler_ver = self.config.compose_compiler_version.as_deref().unwrap_or("1.5.8");
                    format!(
                        "    buildFeatures {{\n        compose = true\n    }}\n    composeOptions {{\n        kotlinCompilerExtensionVersion = \"{compose_compiler_ver}\"\n    }}\n"
                    )
                } else {
                    String::new()
                };

                let manifest_line = if is_android {
                    if let Some(r) = rule {
                        if let Some(ref m) = r.manifest {
                            let m_rel = m.trim_start_matches("//").split_once(':').map(|(_, name)| name).unwrap_or(m.as_str());
                            let src_manifest = src_dir.join(m_rel);
                            if src_manifest.exists() {
                                let overlay_manifest = module_overlay_dir.join("AndroidManifest.xml");
                                let _ = Self::sanitize_and_write_manifest(&src_manifest, &overlay_manifest, &base_ns);
                                format!("            manifest.srcFile(file(\"AndroidManifest.xml\"))\n")
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                let res_line = if src_dir.join("res").exists() || rule.map(|r| !r.resource_files.is_empty()).unwrap_or(false) {
                    let res_path = src_dir.join("res");
                    let res_str = if self.path_absolute {
                        res_path.to_string_lossy().replace('\\', "/")
                    } else {
                        let rel = relative_path(&module_overlay_dir, &res_path);
                        rel.to_string_lossy().replace('\\', "/")
                    };
                    format!("            res.srcDirs(listOf(file(\"{res_str}\")))\n")
                } else {
                    String::new()
                };

                let default_config_block = if is_android_app {
                    format!(
                        "    defaultConfig {{\n        applicationId = \"{app_id}\"\n        minSdk = {min_sdk}\n        targetSdk = {target_sdk}\n    }}\n"
                    )
                } else {
                    format!("    defaultConfig {{\n        minSdk = {min_sdk}\n    }}\n")
                };

                let src_dir_line = if has_srcs {
                    format!("            java.srcDirs(listOf(file(\"{effective_src_dir_str}\")))\n")
                } else {
                    String::new()
                };

                writeln!(
                    file,
                    r#"android {{
    namespace = "{namespace}"
    compileSdk = {compile_sdk}

{default_config_block}
    compileOptions {{
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }}
    lint {{
        abortOnError = false
        checkReleaseBuilds = false
    }}

{compose_block}
    sourceSets {{
        named("main") {{
{manifest_line}{res_line}{src_dir_line}        }}
    }}
}}
"#
                )?;
            } else {
                let java_version = self.config.java_version;
                let src_dir_line = if has_srcs {
                    format!("            srcDirs(listOf(file(\"{effective_src_dir_str}\")))\n")
                } else {
                    String::new()
                };
                writeln!(
                    file,
                    r#"java {{
    toolchain {{
        languageVersion.set(JavaLanguageVersion.of({java_version}))
    }}
}}

sourceSets {{
    named("main") {{
        java {{
{src_dir_line}{include_lines}            exclude("build/**", ".gv/**")
        }}
    }}
}}
"#
                )?;
            }

            // 3. Boundary tasks
            let mut seen_boundary = std::collections::HashSet::new();
            let mut boundary_task_names = Vec::new();

            let ws_root_str = if self.path_absolute {
                self.workspace_root.to_string_lossy().replace('\\', "/")
            } else {
                let rel = relative_path(&module_overlay_dir, &self.workspace_root);
                rel.to_string_lossy().replace('\\', "/")
            };

            let find_maven_coord = |dep: &crate::bazel::model::TargetLabel| -> Option<String> {
                self.config.maven_artifacts.get(&dep.target_name)
                    .or_else(|| self.config.maven_artifacts.get(&dep.raw))
                    .or_else(|| self.config.maven_artifacts.get(&dep.canonical()))
                    .cloned()
            };

            let is_toolchain_target = |dep: &crate::bazel::model::TargetLabel| -> bool {
                dep.raw.starts_with("@io_bazel_rules_kotlin//")
                    || dep.raw.starts_with("@build_bazel_rules_android//")
                    || dep.raw.starts_with("@bazel_tools//")
                    || dep.raw.starts_with("@androidsdk//")
                    || dep.raw.starts_with("@local_config_")
                    || dep.repository.starts_with("@io_bazel_rules_kotlin")
                    || dep.repository.starts_with("@build_bazel_rules_android")
                    || dep.repository.starts_with("@bazel_tools")
                    || dep.repository.starts_with("@androidsdk")
                    || dep.repository.starts_with("@local_config")
            };

            let bzlmod_flag = if self.workspace_root.join("MODULE.bazel").exists() { " --enable_bzlmod" } else { "" };
            let bzlmod_arg = if self.workspace_root.join("MODULE.bazel").exists() { ", \"--enable_bzlmod\"" } else { "" };

            if let Some(r) = rule {
                for dep in r.deps.iter().chain(r.exports.iter()) {
                    if sliced.boundary_targets.contains(dep) && find_maven_coord(dep).is_none() && !is_toolchain_target(dep) && seen_boundary.insert(dep) {
                        let sanitized_boundary = dep.sanitized_name();
                        let canonical_target = dep.canonical();
                        let dep_pkg = dep.package_dir();
                        let dep_target_name = &dep.target_name;
                        let task_var = format!("buildBazel_{sanitized_boundary}");
                        boundary_task_names.push((sanitized_boundary.clone(), task_var.clone()));

                        writeln!(
                            file,
                            r#"val {task_var} by tasks.registering(Exec::class) {{
    workingDir = file("{ws_root_str}")
    commandLine("sh", "-c", "bazel query{bzlmod_flag} 'deps({canonical_target})' --noshow_progress 2>/dev/null | grep -E '^(@|//)' | xargs bazel build{bzlmod_flag} 2>/dev/null || bazel build{bzlmod_flag} {canonical_target}")
    outputs.file(layout.buildDirectory.file("bazel-outputs/{sanitized_boundary}.jar"))
    doLast {{
        val outDir = layout.buildDirectory.dir("bazel-outputs").get().asFile
        outDir.mkdirs()
        val dest = File(outDir, "{sanitized_boundary}.jar")
        val jarsToMerge = mutableListOf<File>()

        try {{
            val cqueryExpr = "'\\n'.join([f.path for p in (providers(target).values() if providers(target) else []) if hasattr(p, 'compile_jars') for f in (p.compile_jars.to_list() if hasattr(p.compile_jars, 'to_list') else p.compile_jars)] + [f.path for f in getattr(getattr(target, 'files', None), 'to_list', lambda: [])()])"
            val proc = ProcessBuilder("bazel", "cquery"{bzlmod_arg}, "deps({canonical_target})", "--output=starlark", "--starlark:expr=$cqueryExpr", "--noshow_progress")
                .directory(workingDir)
                .redirectError(ProcessBuilder.Redirect.DISCARD)
                .start()
            val lines = proc.inputStream.bufferedReader().readLines()
            proc.waitFor()
            for (line in lines) {{
                val trimmed = line.trim()
                if (trimmed.endsWith(".jar") && !trimmed.endsWith("-sources.jar") && !trimmed.endsWith("d8_compat_dx.jar") && !trimmed.endsWith("platformclasspath.jar") && !trimmed.endsWith("proguard.jar") && !trimmed.endsWith("libr8.jar") && !trimmed.endsWith("android.jar") && !trimmed.endsWith("ImportDepsChecker_deploy.jar") && !trimmed.endsWith("all_android_tools_deploy.jar") && !trimmed.endsWith("apksigner.jar") && !trimmed.endsWith("generate_main_dex_list.jar") && !trimmed.endsWith("libauto_value_plugin.jar") && !trimmed.endsWith("libzip.jar") && !trimmed.endsWith("librules_jvm_external.jar")) {{
                    val f = if (File(trimmed).isAbsolute) File(trimmed) else File(workingDir, trimmed)
                    if (f.exists() && f.length() > 0 && !jarsToMerge.contains(f)) {{
                        jarsToMerge.add(f)
                    }}
                }}
            }}
        }} catch (e: Exception) {{
            // Fallback below
        }}

        try {{
            val proc = ProcessBuilder("bazel", "cquery"{bzlmod_arg}, "deps({canonical_target})", "--output=files", "--noshow_progress")
                .directory(workingDir)
                .redirectError(ProcessBuilder.Redirect.DISCARD)
                .start()
            val lines = proc.inputStream.bufferedReader().readLines()
            proc.waitFor()
            for (line in lines) {{
                val trimmed = line.trim()
                if (trimmed.endsWith(".jar") && !trimmed.endsWith("-sources.jar") && !trimmed.endsWith("d8_compat_dx.jar") && !trimmed.endsWith("platformclasspath.jar") && !trimmed.endsWith("proguard.jar") && !trimmed.endsWith("libr8.jar") && !trimmed.endsWith("android.jar") && !trimmed.endsWith("ImportDepsChecker_deploy.jar") && !trimmed.endsWith("all_android_tools_deploy.jar") && !trimmed.endsWith("apksigner.jar") && !trimmed.endsWith("generate_main_dex_list.jar") && !trimmed.endsWith("libauto_value_plugin.jar") && !trimmed.endsWith("libzip.jar") && !trimmed.endsWith("librules_jvm_external.jar")) {{
                    val f = if (File(trimmed).isAbsolute) File(trimmed) else File(workingDir, trimmed)
                    if (f.exists() && f.length() > 0 && !jarsToMerge.contains(f)) {{
                        jarsToMerge.add(f)
                    }}
                }}
            }}
        }} catch (e: Exception) {{
            // Fallback below
        }}

        val bazelBin = File(workingDir, "bazel-bin").canonicalFile
        val pkg = "{dep_pkg}"
        val targetName = "{dep_target_name}"
        val directCandidates = listOf(
            File(bazelBin, "$pkg/lib$targetName.jar"),
            File(bazelBin, "$pkg/lib$targetName-hjar.jar"),
            File(bazelBin, "$pkg/$targetName.jar")
        )
        for (c in directCandidates) {{
            if (c.exists() && c.length() > 0 && !jarsToMerge.contains(c)) {{
                jarsToMerge.add(c)
            }}
        }}

        val seenEntries = mutableSetOf<String>()
        ZipOutputStream(FileOutputStream(dest)).use {{ out ->
            for (jar in jarsToMerge) {{
                if (!jar.exists() || jar.length() == 0L) continue
                try {{
                    ZipFile(jar).use {{ zf ->
                        for (entry in zf.entries()) {{
                            if (entry.isDirectory || (entry.name.startsWith("META-INF/") && !entry.name.startsWith("META-INF/services/"))) continue
                            if (seenEntries.add(entry.name)) {{
                                out.putNextEntry(ZipEntry(entry.name))
                                zf.getInputStream(entry).copyTo(out)
                                out.closeEntry()
                            }}
                        }}
                    }}
                }} catch (e: Exception) {{
                    // Ignore unreadable jar
                }}
            }}
        }}
    }}
}}
"#
                        )?;
                    }
                }
            }

            // 4. Dependencies
            writeln!(file, "dependencies {{")?;
            let mut seen_mod_deps = std::collections::HashSet::new();

            if let Some(r) = rule {
                for exp in &r.exports {
                    if sliced.modules.contains(exp) && exp != mod_label {
                        let proj_path = exp.gradle_project_path(&self.workspace_prefix);
                        if seen_mod_deps.insert(proj_path.clone()) {
                            writeln!(file, "    api(project(\"{proj_path}\"))")?;
                        }
                    } else if let Some(coord) = find_maven_coord(exp) {
                        if seen_mod_deps.insert(coord.clone()) {
                            writeln!(file, "    api(\"{coord}\")")?;
                        }
                    }
                }
                for dep in &r.deps {
                    if sliced.modules.contains(dep) && dep != mod_label {
                        let proj_path = dep.gradle_project_path(&self.workspace_prefix);
                        if seen_mod_deps.insert(proj_path.clone()) {
                            writeln!(file, "    implementation(project(\"{proj_path}\"))")?;
                        }
                    } else if let Some(coord) = find_maven_coord(dep) {
                        if seen_mod_deps.insert(coord.clone()) {
                            writeln!(file, "    implementation(\"{coord}\")")?;
                        }
                    }
                }
                if is_android && has_compose {
                    if let Some(mat_coord) = self.config.maven_artifacts.get("androidx_compose_material_material") {
                        if seen_mod_deps.insert(mat_coord.clone()) {
                            writeln!(file, "    implementation(\"{mat_coord}\")")?;
                        }
                    }
                }
                for (sanitized_boundary, _) in &boundary_task_names {
                    writeln!(
                        file,
                        "    implementation(files(layout.buildDirectory.file(\"bazel-outputs/{sanitized_boundary}.jar\")))"
                    )?;
                }
            }
            writeln!(file, "}}\n")?;

            for (_, task_var) in &boundary_task_names {
                writeln!(
                    file,
                    "tasks.matching {{ it.name.contains(\"compile\") || it.name.contains(\"Compile\") || it.name.contains(\"Resources\") || it.name.contains(\"Manifest\") }}.configureEach {{\n    dependsOn({task_var})\n}}\n"
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bazel::model::{RuleKind, TargetLabel, TargetRule};
    use std::collections::{HashMap, HashSet};
    use tempfile::tempdir;

    #[test]
    fn test_relative_path_calc() {
        assert_eq!(
            relative_path(
                Path::new("/workspace/.gv/java/com/example"),
                Path::new("/workspace/java/com/example")
            ),
            PathBuf::from("../../../../java/com/example")
        );

        assert_eq!(
            relative_path(
                Path::new("/workspace/.gv/java/com/example"),
                Path::new("/workspace")
            ),
            PathBuf::from("../../../..")
        );

        assert_eq!(
            relative_path(
                Path::new("/workspace/java/com/example"),
                Path::new("/workspace/java/com/example")
            ),
            PathBuf::from(".")
        );
    }

    #[test]
    fn test_gradle_generation_relative_and_absolute() {
        let ws_dir = tempdir().unwrap();
        let out_dir_rel = tempdir().unwrap();
        let out_dir_abs = tempdir().unwrap();

        let target_a = TargetLabel::parse("//java/com/example/pkg_a:pkg_a");
        let target_b = TargetLabel::parse("//java/com/example/pkg_b:pkg_b");
        let boundary = TargetLabel::parse("//java/com/example/boundary:boundary");

        let mut modules = HashSet::new();
        modules.insert(target_a.clone());
        modules.insert(target_b.clone());

        let mut boundary_targets = HashSet::new();
        boundary_targets.insert(boundary.clone());

        let mut target_rules = HashMap::new();
        target_rules.insert(
            target_a.clone(),
            TargetRule {
                label: target_a.clone(),
                kind: RuleKind::JavaLibrary,
                srcs: vec!["A.java".to_string()],
                deps: vec![target_b.clone(), boundary.clone()],
                runtime_deps: vec![],
                exports: vec![],
                resources: vec![],
                javacopts: vec![],
                main_class: None,
                manifest: None,
                custom_package: None,
                resource_files: vec![],
                manifest_values: HashMap::new(),
                plugins: vec![],
            },
        );
        target_rules.insert(
            target_b.clone(),
            TargetRule {
                label: target_b.clone(),
                kind: RuleKind::JavaLibrary,
                srcs: vec![],
                deps: vec![],
                runtime_deps: vec![],
                exports: vec![],
                resources: vec![],
                javacopts: vec![],
                main_class: None,
                manifest: None,
                custom_package: None,
                resource_files: vec![],
                manifest_values: HashMap::new(),
                plugins: vec![],
            },
        );

        let sliced = SlicedView {
            modules,
            boundary_targets,
            target_rules,
        };

        // 1. Relative generation
        let gen_rel = GradleGenerator::new(
            out_dir_rel.path().to_path_buf(),
            ws_dir.path().to_path_buf(),
            PathBuf::new(),
            false,
        );
        gen_rel.generate(&sliced).unwrap();

        let pkg_a_build_rel = fs::read_to_string(
            out_dir_rel
                .path()
                .join("java/com/example/pkg_a/pkg_a/build.gradle.kts"),
        )
        .unwrap();
        assert!(pkg_a_build_rel.contains("srcDirs(listOf(file("));
        assert!(pkg_a_build_rel.contains("workingDir = file("));
        assert!(!pkg_a_build_rel.contains(&ws_dir.path().to_string_lossy().to_string()));

        // 2. Absolute generation
        let gen_abs = GradleGenerator::new(
            out_dir_abs.path().to_path_buf(),
            ws_dir.path().to_path_buf(),
            PathBuf::new(),
            true,
        );
        gen_abs.generate(&sliced).unwrap();

        let pkg_a_build_abs = fs::read_to_string(
            out_dir_abs
                .path()
                .join("java/com/example/pkg_a/pkg_a/build.gradle.kts"),
        )
        .unwrap();
        assert!(pkg_a_build_abs.contains(&ws_dir.path().to_string_lossy().to_string()));
    }
}

