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
    pub path_absolute: bool,
}

impl GradleGenerator {
    pub fn new(output_dir: PathBuf, workspace_root: PathBuf, path_absolute: bool) -> Self {
        Self {
            output_dir,
            workspace_root,
            path_absolute,
        }
    }

    /// Generate the overlay Gradle project structure
    pub fn generate(&self, sliced: &SlicedView) -> Result<()> {
        fs::create_dir_all(&self.output_dir)
            .with_context(|| format!("Failed to create output dir {:?}", self.output_dir))?;

        self.generate_settings_gradle(sliced)?;
        self.generate_root_build_gradle()?;
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

    /// Generate Gradle wrapper files
    fn generate_wrapper(&self) -> Result<()> {
        let _ = std::process::Command::new("gradle")
            .arg("wrapper")
            .current_dir(&self.output_dir)
            .output();

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

        Ok(())
    }

    /// Generate settings.gradle.kts with all active module includes matching package paths
    fn generate_settings_gradle(&self, sliced: &SlicedView) -> Result<()> {
        let settings_path = self.output_dir.join("settings.gradle.kts");
        let mut file = File::create(&settings_path)?;

        writeln!(file, "rootProject.name = \"bazel-gradle-view\"\n")?;

        for module_label in &sliced.modules {
            let project_path = module_label.gradle_project_path();
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
            let pkg_overlay_dir = if mod_label.package_dir().is_empty() {
                self.output_dir.clone()
            } else {
                self.output_dir.join(mod_label.package_dir())
            };
            fs::create_dir_all(&pkg_overlay_dir)?;

            let build_file_path = pkg_overlay_dir.join("build.gradle.kts");
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
            let plugin_str = match rule.map(|r| &r.kind) {
                Some(RuleKind::JavaBinary) => "plugins {\n    application\n    `java-library`\n}",
                Some(RuleKind::KotlinJvmLibrary) => {
                    "plugins {\n    `java-library`\n    kotlin(\"jvm\") version \"1.9.22\"\n}"
                }
                Some(RuleKind::KotlinJvmTest) => {
                    "plugins {\n    `java`\n    kotlin(\"jvm\") version \"1.9.22\"\n}"
                }
                _ => "plugins {\n    `java-library`\n}",
            };
            writeln!(file, "{plugin_str}\n")?;

            // 2. Overlay source configuration: package directory contains the sources
            let src_dir = if mod_label.package_dir().is_empty() {
                self.workspace_root.clone()
            } else {
                self.workspace_root.join(mod_label.package_dir())
            };

            let src_dir_str = if self.path_absolute {
                src_dir.to_string_lossy().replace('\\', "/")
            } else {
                let rel = relative_path(&pkg_overlay_dir, &src_dir);
                rel.to_string_lossy().replace('\\', "/")
            };

            writeln!(
                file,
                r#"sourceSets {{
    named("main") {{
        java {{
            srcDirs(listOf(file("{src_dir_str}")))
            include("**/*.java")
            exclude("build/**")
        }}
    }}
}}
"#
            )?;

            // 3. Generate Bazel boundary build tasks if any dependencies are boundary targets
            let mut boundary_task_names = Vec::new();

            if let Some(r) = rule {
                let ws_root_str = if self.path_absolute {
                    self.workspace_root.to_string_lossy().replace('\\', "/")
                } else {
                    let rel = relative_path(&pkg_overlay_dir, &self.workspace_root);
                    rel.to_string_lossy().replace('\\', "/")
                };

                for dep in &r.deps {
                    if sliced.boundary_targets.contains(dep) {
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
    commandLine("bazel", "build", "{canonical_target}")
    outputs.file(layout.buildDirectory.file("bazel-outputs/{sanitized_boundary}.jar"))
    doLast {{
        val outDir = layout.buildDirectory.dir("bazel-outputs").get().asFile
        outDir.mkdirs()
        val dest = File(outDir, "{sanitized_boundary}.jar")
        val bazelBin = File(workingDir, "bazel-bin")
        val pkg = "{dep_pkg}"
        val targetName = "{dep_target_name}"

        val directCandidates = listOf(
            File(bazelBin, "$pkg/lib$targetName.jar"),
            File(bazelBin, "$pkg/$targetName.jar")
        )
        val direct = directCandidates.firstOrNull {{ it.exists() }}
        if (direct != null) {{
            direct.copyTo(dest, overwrite = true)
        }} else {{
            val matches = bazelBin.walkTopDown().filter {{
                it.isFile && it.extension == "jar" &&
                (it.name == "lib$targetName.jar" || it.name == "$targetName.jar")
            }}.toList()
            if (matches.isNotEmpty()) {{
                matches.first().copyTo(dest, overwrite = true)
            }} else if (!dest.exists()) {{
                dest.createNewFile()
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

            if let Some(r) = rule {
                for dep in &r.deps {
                    if sliced.modules.contains(dep) {
                        // Project dependency to sibling Gradle module matching package path
                        writeln!(file, "    implementation(project(\"{}\"))", dep.gradle_project_path())?;
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
                    "tasks.named(\"compileJava\") {{\n    dependsOn({task_var})\n}}\n"
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
            false,
        );
        gen_rel.generate(&sliced).unwrap();

        let pkg_a_build_rel = fs::read_to_string(
            out_dir_rel
                .path()
                .join("java/com/example/pkg_a/build.gradle.kts"),
        )
        .unwrap();
        assert!(pkg_a_build_rel.contains("srcDirs(listOf(file("));
        assert!(pkg_a_build_rel.contains("workingDir = file("));
        assert!(!pkg_a_build_rel.contains(&ws_dir.path().to_string_lossy().to_string()));

        // 2. Absolute generation
        let gen_abs = GradleGenerator::new(
            out_dir_abs.path().to_path_buf(),
            ws_dir.path().to_path_buf(),
            true,
        );
        gen_abs.generate(&sliced).unwrap();

        let pkg_a_build_abs = fs::read_to_string(
            out_dir_abs
                .path()
                .join("java/com/example/pkg_a/build.gradle.kts"),
        )
        .unwrap();
        assert!(pkg_a_build_abs.contains(&ws_dir.path().to_string_lossy().to_string()));
    }
}

