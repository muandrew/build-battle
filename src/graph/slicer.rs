use crate::bazel::model::{TargetLabel, TargetRule};
use petgraph::algo::has_path_connecting;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{HashMap, HashSet};

/// Sliced target view model
#[derive(Debug, Clone)]
pub struct SlicedView {
    /// Targets that become active Gradle modules
    pub modules: HashSet<TargetLabel>,
    /// Targets that are direct dependencies of modules, but resolved via Bazel output
    pub boundary_targets: HashSet<TargetLabel>,
    /// Underlying metadata for all targets in scope
    pub target_rules: HashMap<TargetLabel, TargetRule>,
}

pub struct GraphSlicer;

impl GraphSlicer {
    /// Slice the dependency graph based on selected targets and reachability rules
    pub fn slice_graph(
        all_rules: &HashMap<TargetLabel, TargetRule>,
        selected_targets: &[TargetLabel],
    ) -> SlicedView {
        let mut graph = DiGraph::<TargetLabel, ()>::new();
        let mut label_to_node: HashMap<TargetLabel, NodeIndex> = HashMap::new();

        // 1. Insert all nodes into petgraph
        for label in all_rules.keys() {
            let idx = graph.add_node(label.clone());
            label_to_node.insert(label.clone(), idx);
        }

        // Also add any selected targets if not already in graph
        for sel in selected_targets {
            if !label_to_node.contains_key(sel) {
                let idx = graph.add_node(sel.clone());
                label_to_node.insert(sel.clone(), idx);
            }
        }

        // 2. Insert edges (target -> dep)
        for (label, rule) in all_rules {
            if let Some(&from_idx) = label_to_node.get(label) {
                for dep in rule.all_dependencies() {
                    if let Some(&to_idx) = label_to_node.get(&dep) {
                        graph.add_edge(from_idx, to_idx, ());
                    }
                }
            }
        }

        let selected_set: HashSet<TargetLabel> = selected_targets.iter().cloned().collect();
        let mut active_modules: HashSet<TargetLabel> = selected_set.clone();

        // 3. Find intermediate nodes:
        // Any node w that lies on a directed path between two nodes u, v in selected_set
        // i.e., has_path(u, w) AND has_path(w, v) for some u, v in selected_set
        let all_labels: Vec<TargetLabel> = all_rules.keys().cloned().collect();

        for w_label in &all_labels {
            if active_modules.contains(w_label) {
                continue;
            }
            if let Some(&w_idx) = label_to_node.get(w_label) {
                let mut is_intermediate = false;

                for u_label in &selected_set {
                    if is_intermediate {
                        break;
                    }
                    if let Some(&u_idx) = label_to_node.get(u_label) {
                        if u_idx == w_idx {
                            continue;
                        }
                        if has_path_connecting(&graph, u_idx, w_idx, None) {
                            for v_label in &selected_set {
                                if let Some(&v_idx) = label_to_node.get(v_label) {
                                    if v_idx == w_idx || v_idx == u_idx {
                                        continue;
                                    }
                                    if has_path_connecting(&graph, w_idx, v_idx, None) {
                                        is_intermediate = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                if is_intermediate {
                    active_modules.insert(w_label.clone());
                }
            }
        }

        // 4. Determine boundary targets:
        // Any direct dependency of an active module that is NOT itself an active module
        let mut boundary_targets: HashSet<TargetLabel> = HashSet::new();

        for mod_label in &active_modules {
            if let Some(rule) = all_rules.get(mod_label) {
                for dep in rule.all_dependencies() {
                    if !active_modules.contains(&dep) {
                        boundary_targets.insert(dep);
                    }
                }
            }
        }

        SlicedView {
            modules: active_modules,
            boundary_targets,
            target_rules: all_rules.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bazel::model::RuleKind;

    fn make_dummy_rule(label_str: &str, deps_strs: &[&str]) -> (TargetLabel, TargetRule) {
        let label = TargetLabel::parse(label_str);
        let deps = deps_strs.iter().map(|s| TargetLabel::parse(s)).collect();
        let rule = TargetRule {
            label: label.clone(),
            kind: RuleKind::JavaLibrary,
            srcs: vec![],
            deps,
            runtime_deps: vec![],
            exports: vec![],
            resources: vec![],
            javacopts: vec![],
            main_class: None,
        };
        (label, rule)
    }

    /// Setup graph:
    /// a -> aa
    /// a -> ab
    /// aa -> aaa
    /// aa -> aab
    /// ab -> aba
    /// ab -> abb
    fn build_test_graph() -> HashMap<TargetLabel, TargetRule> {
        let mut map = HashMap::new();
        let rules = vec![
            make_dummy_rule("//pkg:a", &["//pkg:aa", "//pkg:ab"]),
            make_dummy_rule("//pkg:aa", &["//pkg:aaa", "//pkg:aab"]),
            make_dummy_rule("//pkg:ab", &["//pkg:aba", "//pkg:abb"]),
            make_dummy_rule("//pkg:aaa", &[]),
            make_dummy_rule("//pkg:aab", &[]),
            make_dummy_rule("//pkg:aba", &[]),
            make_dummy_rule("//pkg:abb", &[]),
        ];
        for (label, rule) in rules {
            map.insert(label, rule);
        }
        map
    }

    #[test]
    fn test_scenario_aa_aab() {
        let graph = build_test_graph();
        let selected = vec![
            TargetLabel::parse("//pkg:aa"),
            TargetLabel::parse("//pkg:aab"),
        ];

        let sliced = GraphSlicer::slice_graph(&graph, &selected);

        // Modules should only be aa, aab
        assert_eq!(sliced.modules.len(), 2);
        assert!(sliced.modules.contains(&TargetLabel::parse("//pkg:aa")));
        assert!(sliced.modules.contains(&TargetLabel::parse("//pkg:aab")));

        // aaa is a direct dependency of aa, but not selected -> BazelBoundary
        assert!(sliced.boundary_targets.contains(&TargetLabel::parse("//pkg:aaa")));
        // a, ab, aba, abb should NOT be modules or boundary
        assert!(!sliced.modules.contains(&TargetLabel::parse("//pkg:a")));
        assert!(!sliced.modules.contains(&TargetLabel::parse("//pkg:ab")));
        assert!(!sliced.boundary_targets.contains(&TargetLabel::parse("//pkg:aba")));
    }

    #[test]
    fn test_scenario_a_abb() {
        let graph = build_test_graph();
        let selected = vec![
            TargetLabel::parse("//pkg:a"),
            TargetLabel::parse("//pkg:abb"),
        ];

        let sliced = GraphSlicer::slice_graph(&graph, &selected);

        // Path a -> ab -> abb means ab must be intermediate module
        assert_eq!(sliced.modules.len(), 3);
        assert!(sliced.modules.contains(&TargetLabel::parse("//pkg:a")));
        assert!(sliced.modules.contains(&TargetLabel::parse("//pkg:ab")));
        assert!(sliced.modules.contains(&TargetLabel::parse("//pkg:abb")));

        // Boundary targets should include aa (from a) and aba (from ab)
        assert!(sliced.boundary_targets.contains(&TargetLabel::parse("//pkg:aa")));
        assert!(sliced.boundary_targets.contains(&TargetLabel::parse("//pkg:aba")));

        // aaa, aab should NOT be in modules or boundary targets
        assert!(!sliced.modules.contains(&TargetLabel::parse("//pkg:aaa")));
        assert!(!sliced.boundary_targets.contains(&TargetLabel::parse("//pkg:aaa")));
    }
}
