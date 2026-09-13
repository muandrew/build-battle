use crate::bazel::model::RuleKind;
use crate::graph::SlicedView;
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tracing::info;

pub struct GradleGenerator {
    pub output_dir: PathBuf,
}

impl GradleGenerator {
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }

    /// Generate the overlay Gradle project structure
    pub fn generate(&self, sliced: &SlicedView) -> Result<()> {
        fs::create_dir_all(&self.output_dir)
            .with_context(|| format!("Failed to create output dir {:?}", self.output_dir))?;

        self.generate_settings_gradle(sliced)?;
        self.generate_root_build_gradle()?;
        self.generate_overlay_module_build_gradles(sliced)?;

        info!(
            "Successfully generated Gradle overlay in {:?} with {} modules and {} Bazel boundary targets.",
            self.output_dir,
            sliced.modules.len(),
            sliced.boundary_targets.len()
        );

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
            writeln!(
                file,
                r#"sourceSets {{
    named("main") {{
        java {{
            srcDirs(".")
            include("**/*.java")
            exclude("build/**")
        }}
    }}
}}
"#
            )?;

            // 3. Dependencies & Bazel Boundary tasks
            writeln!(file, "dependencies {{")?;

            if let Some(r) = rule {
                for dep in &r.deps {
                    if sliced.modules.contains(dep) {
                        // Project dependency to sibling Gradle module matching package path
                        writeln!(file, "    implementation(project(\"{}\"))", dep.gradle_project_path())?;
                    } else if sliced.boundary_targets.contains(dep) {
                        // Bazel boundary output jar
                        let sanitized_boundary = dep.sanitized_name();
                        writeln!(
                            file,
                            "    implementation(files(layout.buildDirectory.file(\"bazel-outputs/{sanitized_boundary}.jar\")))"
                        )?;
                    }
                }
            }
            writeln!(file, "}}\n")?;

            // 4. Generate Bazel boundary build tasks if any dependencies are boundary targets
            if let Some(r) = rule {
                for dep in &r.deps {
                    if sliced.boundary_targets.contains(dep) {
                        let sanitized_boundary = dep.sanitized_name();
                        let canonical_target = dep.canonical();

                        writeln!(
                            file,
                            r#"val buildBazel_{sanitized_boundary} by tasks.registering(Exec::class) {{
    workingDir = rootDir
    commandLine("bazel", "build", "{canonical_target}")
    doLast {{
        val outDir = layout.buildDirectory.dir("bazel-outputs").get().asFile
        outDir.mkdirs()
    }}
}}

tasks.named("compileJava") {{
    dependsOn(buildBazel_{sanitized_boundary})
}}
"#
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}
