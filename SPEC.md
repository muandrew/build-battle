# `gv` (Gradle View) — Specification & Requirements

## 1. Overview & Objectives

`gv` (*Gradle View*) is a CLI tool written in Rust designed to generate a functional, isolated Gradle project view from an existing Bazel-centric monorepo or project.

### Core Goals
- **IDE & Tooling Interoperability**: Allow developers and IDEs (IntelliJ IDEA, Android Studio, VS Code) to leverage Gradle's rich language server and indexing capabilities without modifying the canonical Bazel build rules in the source tree.
- **Selective Graph Slicing**: Given one or more target patterns, generate Gradle submodules *only* for the targets requested and intermediate connecting nodes. Direct dependencies outside this selected subgraph are fulfilled via delegated Bazel build outputs (e.g. jars/libraries produced by Bazel).
- **Overlay Model**: Generates an overlay directory structure in `<output-dir>` mirroring the Bazel workspace package hierarchy. The generated Gradle project structure overlays directly onto the codebase (or into an overlay target directory) rather than using external filesystem symlinks or synthetic absolute path linkages.
- **Reliable Metadata Extraction**: Query the Bazel dependency and rule graph using Bazel's native query engine (`bazel query` / `bazel cquery`) to determine build types, plugins, source sets, resources, dependencies, and toolchain constraints.

---

## 2. CLI Interface

### Usage Syntax
```bash
gv <output-dir> <target> [additional-targets...] [options]
```

### Positional Arguments
- `<output-dir>`: Directory where the generated Gradle project view (`settings.gradle.kts`, `build.gradle.kts`, helper tasks, etc.) will be written.
- `<target> [additional-targets...]`: One or more Bazel target patterns to include in the Gradle view.

### Target Pattern Support
`gv` supports standard Bazel target pattern syntax:
- Specific target: `//package-name:target-name` or `package-name:target-name`
- All targets in package: `//package-name:all` or `//package-name:*`
- All targets in package and subpackages: `//package-name/...` or `//package-name:__subpackages__`

### CLI Options
- `--workspace <path>`: Path to the Bazel workspace root (defaults to current working directory or nearest directory containing `WORKSPACE` / `MODULE.bazel`).
- `--bazel-bin <path>`: Path to the Bazel / Bazelisk binary (defaults to `bazel` found in `$PATH`).
- `--in-place`: Generate `build.gradle.kts` directly into the source packages instead of an isolated overlay (defaults to `false`).
- `--gradle-version <version>`: Gradle version to configure for the wrapper (defaults to `8.5`+).
- `--verbose`, `-v`: Enable debug logging and verbose Bazel query output.

---

## 3. Graph Slicing & Boundary Model

### 3.1 Graph Definitions
Let $G = (V, E)$ be the directed acyclic graph (DAG) of Bazel build targets, where an edge $(u, v) \in E$ denotes that target $u$ depends on target $v$ ($u \to v$).

Let $S \subseteq V$ be the set of resolved targets explicitly requested by the user via CLI arguments.

### 3.2 Intermediate Module Inclusion Rule
Any target $w \in V$ that lies on a directed path between two targets in $S$ must be promoted to a **Gradle Module**.
Formally:
$$M_{\text{intermediate}} = \{ w \in V \mid \exists u, v \in S \text{ such that } u \rightsquigarrow w \rightsquigarrow v \}$$
$$M = S \cup M_{\text{intermediate}}$$

Where $M$ is the set of all targets rendered as active Gradle submodules.

### 3.3 Boundary Dependencies (Bazel Delegation)
For any active Gradle module $m \in M$, let $\text{deps}(m)$ be its direct dependencies in $G$.
The set of **Boundary Dependencies** $B$ is defined as:
$$B = \{ b \in V \setminus M \mid \exists m \in M \text{ such that } (m, b) \in E \}$$

- Nodes in $B$ are **not** converted into Gradle submodules.
- Instead, $m$ consumes $b$ as a **Bazel Output**:
  - Gradle declares a dependency on the build artifact (e.g., `.jar`) produced by Bazel for $b$.
  - A corresponding Gradle task (e.g. `bazelBuild_<sanitized_label>`) invokes `bazel build <target_b>` to ensure the artifact is compiled and up-to-date.

### 3.4 Illustrative Scenarios

Given target dependency graph:
```
       a
      / \
     aa  ab
    / \  / \
  aaa aab aba abb
```

#### Scenario 1: `gv <output-dir> aa aab`
- **Selected ($S$)**: `{aa, aab}`
- **Active Gradle Modules ($M$)**:
  - `aa`
  - `aab`
- **Boundary Targets ($B$)**:
  - `aaa` (consumed by `aa` via Bazel build bridge)
- **Excluded**: `a`, `ab`, `aba`, `abb`

#### Scenario 2: `gv <output-dir> a abb`
- **Selected ($S$)**: `{a, abb}`
- **Paths from $a$ to $abb$**: $a \to ab \to abb$. Intermediate node $ab$ is promoted.
- **Active Gradle Modules ($M$)**:
  - `a`
  - `ab`
  - `abb`
- **Boundary Targets ($B$)**:
  - `aa` (consumed by `a` as Bazel output)
  - `aba` (consumed by `ab` as Bazel output)
- **Excluded**: `aaa`, `aab`

---

## 4. Bazel Query & Metadata Extraction

`gv` queries Bazel to inspect target properties.

### Query Strategy
1. **Target Resolution & Transitive Closure**:
   ```bash
   bazel query "deps(//target1 + //target2)" --output=xml
   ```
   or `jsonproto` / `proto`.
2. **Target Properties Extracted**:
   - `class` / `rule.class`: e.g. `java_library`, `java_binary`, `java_test`, `kt_jvm_library`, `proto_library`, etc.
   - `srcs`: Source files (.java, .kt, etc.).
   - `deps`: Direct compile-time dependencies.
   - `runtime_deps`: Runtime-only dependencies.
   - `exports`: Exported dependencies.
   - `resources`: Resource files or directories.
   - `javacopts`: Custom compiler flags, JVM target compatibility (e.g. `-source 11 -target 11`).
   - `main_class`: Main entry point for binaries.

### Rule to Plugin Mapping Table

| Bazel Rule Class | Gradle Plugin(s) | SourceSet / Configuration |
| :--- | :--- | :--- |
| `java_library` | `java-library` | `main` sourceSet |
| `java_binary` | `application` or `java` | `main` sourceSet, `mainClass` |
| `java_test` | `java`, `jvm-test-suite` | `test` sourceSet |
| `kt_jvm_library` | `org.jetbrains.kotlin.jvm` | `main` Kotlin sourceSet |
| `kt_jvm_test` | `org.jetbrains.kotlin.jvm` | `test` Kotlin sourceSet |
| `proto_library` / `java_proto_library` | Bazel delegated output / `com.google.protobuf` | Custom / Bazel bridge |

---

## 5. Generated Gradle Overlay Structure

The generated Gradle directory in `<output-dir>` is structured as an **overlay** mirroring the Bazel package tree:

```
<output-dir>/
├── settings.gradle.kts
├── build.gradle.kts (Root build configuration)
├── gradle/
│   └── wrapper/
│       ├── gradle-wrapper.jar
│       └── gradle-wrapper.properties
├── gradlew
├── gradlew.bat
└── java/
    └── com/
        └── example/
            ├── pkg_a/
            │   └── build.gradle.kts
            └── pkg_ab/
                └── build.gradle.kts
```

When overlayed onto the workspace (or when `<output-dir>` is set to the workspace root or an overlay mount), each `build.gradle.kts` aligns directly with its corresponding `BUILD` file and package sources.

### Module `build.gradle.kts` Pattern (Overlay)
```kotlin
plugins {
    `java-library`
}

// In an overlay, the package directory itself contains the sources
sourceSets {
    named("main") {
        java {
            srcDirs(".")
            include("**/*.java")
            exclude("build/**")
        }
    }
}

dependencies {
    // 1. Inter-module Gradle dependencies matching the package path
    implementation(project(":java:com:example:pkg_ab"))

    // 2. Bazel boundary dependencies
    implementation(files(layout.buildDirectory.file("bazel-outputs/boundary_aaa.jar")))
}

// 3. Task to compile boundary targets via Bazel
val buildBazelBoundaryAaa by tasks.registering(Exec::class) {
    workingDir = rootDir
    commandLine("bazel", "build", "//java/com/example/aaa:aaa")
}

tasks.named("compileJava") {
    dependsOn(buildBazelBoundaryAaa)
}
```

---

## 6. Testing & Validation Strategy (Google Copybara Reference)

### 6.1 Reference Project: `google/copybara`
- **Repo URL**: `https://github.com/google/copybara.git`
- **Isolation Policy**: No Copybara source code is ever committed into this repository.
- **Fixture Setup Script**: `scripts/setup_test_repo.sh`
  - Clones or shallow-fetches Copybara into a local `.test_fixtures/copybara` folder (which is added to `.gitignore`).
  - Provides sample target queries such as:
    - `//java/com/google/copybara:copybara`
    - `//java/com/google/copybara/util:util`
    - `//java/com/google/copybara/...`

### 6.2 Test Suites
1. **Unit Tests**:
   - Target pattern parsing (`//a/b:c`, `a/b:all`, `a/b/...`).
   - DAG path search and intermediate node inclusion algorithm.
   - Bazel query XML/JSON parser.
   - Gradle Kotlin DSL code generator.
2. **Mock / Golden Tests**:
   - Test against static XML/JSON query dumps from known Bazel graphs.
3. **End-to-End Integration Tests**:
   - Run `gv` against `.test_fixtures/copybara` outputting to temporary directories.
   - Verify generated Gradle build can run `gradle tasks` and compile classes.
