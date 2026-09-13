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
}

impl GradleGenerator {
    pub fn new(
        output_dir: PathBuf,
        workspace_root: PathBuf,
        workspace_prefix: PathBuf,
        path_absolute: bool,
    ) -> Self {
        Self {
            output_dir,
            workspace_root,
            workspace_prefix,
            path_absolute,
        }
    }

    /// Generate the overlay Gradle project structure
    pub fn generate(&self, sliced: &SlicedView) -> Result<()> {
        fs::create_dir_all(&self.output_dir)
            .with_context(|| format!("Failed to create output dir {:?}", self.output_dir))?;

        self.generate_settings_gradle(sliced)?;
        self.generate_root_build_gradle()?;
        self.generate_gradle_properties()?;
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

    /// Generate gradle.properties with configured Bazel JDK home if detected (only for Java >= 17)
    fn generate_gradle_properties(&self) -> Result<()> {
        let props_path = self.output_dir.join("gradle.properties");
        if let Some(jdk_home) = Self::detect_bazel_jdk(&self.workspace_root) {
            if let Some(ver) = Self::get_jdk_major_version(&jdk_home) {
                if ver >= 17 {
                    let mut file = File::create(props_path)?;
                    let jdk_str = jdk_home.to_string_lossy().replace('\\', "/");
                    writeln!(file, "org.gradle.java.home={jdk_str}")?;
                }
            }
        }
        Ok(())
    }

    fn get_jdk_major_version(jdk_home: &Path) -> Option<u32> {
        let java_bin = jdk_home.join("bin/java");
        let mut cmd = std::process::Command::new(java_bin);
        cmd.arg("-version");
        if let Ok(output) = cmd.output() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let full = format!("{}\n{}", stdout, stderr);
            for line in full.lines() {
                if line.contains("version") {
                    if let Some(start) = line.find('"') {
                        let rest = &line[start + 1..];
                        if let Some(end) = rest.find('"') {
                            let ver_str = &rest[..end];
                            let parts: Vec<&str> = ver_str.split('.').collect();
                            if parts[0] == "1" && parts.len() > 1 {
                                return parts[1].parse::<u32>().ok();
                            } else {
                                return parts[0].parse::<u32>().ok();
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn detect_bazel_jdk(workspace_root: &Path) -> Option<PathBuf> {
        let has_bzlmod = workspace_root.join("MODULE.bazel").exists();
        let mut cmd = std::process::Command::new("bazel");
        cmd.current_dir(workspace_root);
        cmd.arg("cquery");
        if has_bzlmod {
            cmd.arg("--enable_bzlmod");
        }
        cmd.arg("@rules_java//toolchains:current_java_toolchain");
        cmd.arg("--output=starlark");
        cmd.arg("--starlark:expr=getattr([p for k, p in providers(target).items() if 'JavaToolchainInfo' in str(k)][0].java_runtime, 'java_home', None)");
        cmd.arg("--noshow_progress");

        let java_home_rel = if let Ok(output) = cmd.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .map(|l| l.trim())
                .find(|t| t.starts_with("external/") || t.starts_with('/') || t.starts_with('@'))
                .map(|s| s.to_string())
        } else {
            None
        };

        if let Some(rel) = java_home_rel {
            if !rel.is_empty() && rel != "None" {
                let mut info_cmd = std::process::Command::new("bazel");
                info_cmd.current_dir(workspace_root);
                info_cmd.arg("info");
                info_cmd.arg("execution_root");
                if let Ok(info_out) = info_cmd.output() {
                    let exec_root = String::from_utf8_lossy(&info_out.stdout).trim().to_string();
                    let full_path = if rel.starts_with('/') {
                        PathBuf::from(&rel)
                    } else if let Some(stripped) = rel.strip_prefix("@@").or_else(|| rel.strip_prefix('@')) {
                        PathBuf::from(&exec_root).join("external").join(stripped)
                    } else {
                        PathBuf::from(&exec_root).join(&rel)
                    };
                    if let Some(jdk) = Self::resolve_jdk_home(&full_path) {
                        return Some(jdk);
                    }
                }
            }
        }

        let mut info_cmd = std::process::Command::new("bazel");
        info_cmd.current_dir(workspace_root);
        info_cmd.arg("info");
        info_cmd.arg("java-home");
        if let Ok(info_out) = info_cmd.output() {
            let jh = String::from_utf8_lossy(&info_out.stdout).trim().to_string();
            if !jh.is_empty() {
                let p = PathBuf::from(jh);
                if let Some(jdk) = Self::resolve_jdk_home(&p) {
                    return Some(jdk);
                }
            }
        }

        None
    }

    fn resolve_jdk_home(path: &Path) -> Option<PathBuf> {
        if path.join("bin/java").exists() {
            return Some(path.to_path_buf());
        }
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                let mac_home = p.join("Contents/Home");
                if mac_home.join("bin/java").exists() {
                    return Some(mac_home);
                }
            }
        }
        None
    }

    /// Generate Gradle wrapper files
    fn generate_wrapper(&self) -> Result<()> {
        let candidates = [
            "gradle",
            "/opt/homebrew/bin/gradle",
            "/usr/local/bin/gradle",
        ];
        for cand in candidates {
            if let Ok(output) = std::process::Command::new(cand)
                .arg("wrapper")
                .current_dir(&self.output_dir)
                .output()
            {
                if output.status.success() {
                    break;
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

        writeln!(file, "rootProject.name = \"bazel-gradle-view\"\n")?;

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

        writeln!(
            file,
            r#"plugins {{
    base
    kotlin("jvm") version "1.9.22" apply false
}}

allprojects {{
    repositories {{
        mavenCentral()
        google()
    }}
}}
"#
        )?;

        Ok(())
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

            // 1. Plugins
            let is_kotlin = rule.map(|r| {
                matches!(r.kind, RuleKind::KotlinJvmLibrary | RuleKind::KotlinJvmTest)
                    || r.srcs.iter().any(|s| s.ends_with(".kt"))
            }).unwrap_or(false);
            let is_binary = rule.map(|r| matches!(r.kind, RuleKind::JavaBinary)).unwrap_or(false);

            let plugin_str = if is_kotlin {
                "plugins {\n    `java-library`\n    kotlin(\"jvm\") version \"1.9.22\"\n}"
            } else if is_binary {
                "plugins {\n    application\n    `java-library`\n}"
            } else {
                "plugins {\n    `java-library`\n}"
            };
            writeln!(file, "{plugin_str}\n")?;

            // 2. Overlay source configuration: source dir is package dir in workspace
            let src_dir = if mod_label.package_dir().is_empty() {
                self.workspace_root.clone()
            } else {
                self.workspace_root.join(mod_label.package_dir())
            };

            let src_dir_str = if self.path_absolute {
                src_dir.to_string_lossy().replace('\\', "/")
            } else {
                let rel = relative_path(&module_overlay_dir, &src_dir);
                rel.to_string_lossy().replace('\\', "/")
            };

            let mut include_lines = String::new();
            if let Some(r) = rule {
                let rel_srcs: Vec<String> = r
                    .srcs
                    .iter()
                    .filter_map(|s| {
                        let trimmed = s.trim();
                        if let Some((pkg, file)) = trimmed.strip_prefix("//").and_then(|t| t.split_once(':')) {
                            if pkg == mod_label.package_dir() {
                                Some(file.to_string())
                            } else {
                                Some(format!("{pkg}/{file}"))
                            }
                        } else if let Some(stripped) = trimmed.strip_prefix(':') {
                            Some(stripped.to_string())
                        } else if !trimmed.is_empty() && !trimmed.starts_with('@') {
                            Some(trimmed.to_string())
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

            writeln!(
                file,
                r#"sourceSets {{
    named("main") {{
        java {{
            srcDirs(listOf(file("{src_dir_str}")))
{include_lines}            exclude("build/**", ".gv/**")
        }}
    }}
}}
"#
            )?;

            // 3. Boundary tasks
            let mut seen_boundary = std::collections::HashSet::new();
            let mut boundary_task_names = Vec::new();

            let ws_root_str = if self.path_absolute {
                self.workspace_root.to_string_lossy().replace('\\', "/")
            } else {
                let rel = relative_path(&module_overlay_dir, &self.workspace_root);
                rel.to_string_lossy().replace('\\', "/")
            };

            if let Some(r) = rule {
                for dep in &r.deps {
                    if sliced.boundary_targets.contains(dep) && seen_boundary.insert(dep) {
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
    commandLine("sh", "-c", "bazel query 'deps({canonical_target}, 1)' --noshow_progress 2>/dev/null | grep -E '^(@|//)' | xargs bazel build 2>/dev/null || bazel build {canonical_target}")
    outputs.file(layout.buildDirectory.file("bazel-outputs/{sanitized_boundary}.jar"))
    doLast {{
        val outDir = layout.buildDirectory.dir("bazel-outputs").get().asFile
        outDir.mkdirs()
        val dest = File(outDir, "{sanitized_boundary}.jar")
        val jarsToMerge = mutableListOf<File>()

        try {{
            val cqueryExpr = "'\\n'.join([f.path for p in (providers(target).values() if providers(target) else []) if hasattr(p, 'compile_jars') for f in (p.compile_jars.to_list() if hasattr(p.compile_jars, 'to_list') else p.compile_jars)] + [f.path for f in getattr(getattr(target, 'files', None), 'to_list', lambda: [])()])"
            val proc = ProcessBuilder("bazel", "cquery", "deps({canonical_target}, 1)", "--output=starlark", "--starlark:expr=$cqueryExpr", "--noshow_progress")
                .directory(workingDir)
                .redirectError(ProcessBuilder.Redirect.DISCARD)
                .start()
            val lines = proc.inputStream.bufferedReader().readLines()
            proc.waitFor()
            for (line in lines) {{
                val trimmed = line.trim()
                if (trimmed.endsWith(".jar")) {{
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
            val proc = ProcessBuilder("bazel", "cquery", "deps({canonical_target}, 1)", "--output=files", "--noshow_progress")
                .directory(workingDir)
                .redirectError(ProcessBuilder.Redirect.DISCARD)
                .start()
            val lines = proc.inputStream.bufferedReader().readLines()
            proc.waitFor()
            for (line in lines) {{
                val trimmed = line.trim()
                if (trimmed.endsWith(".jar")) {{
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
                for dep in &r.deps {
                    if sliced.modules.contains(dep) && dep != mod_label {
                        let proj_path = dep.gradle_project_path(&self.workspace_prefix);
                        if seen_mod_deps.insert(proj_path.clone()) {
                            writeln!(file, "    implementation(project(\"{proj_path}\"))")?;
                        }
                    }
                }
                for (_, task_var) in &boundary_task_names {
                    writeln!(file, "    implementation(files({task_var}))")?;
                }
            }
            writeln!(file, "}}\n")?;

            for (_, task_var) in &boundary_task_names {
                writeln!(
                    file,
                    "tasks.matching {{ it.name == \"compileJava\" || it.name == \"compileKotlin\" }}.configureEach {{\n    dependsOn({task_var})\n}}\n"
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
                srcs: vec![],
                deps: vec![target_b.clone(), boundary.clone()],
                runtime_deps: vec![],
                exports: vec![],
                resources: vec![],
                javacopts: vec![],
                main_class: None,
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

