//! Topological sort of class-like declarations.
//!
//! Produces a dependency-ordered list of class FQNs such that every
//! class appears after all of its dependencies (parent class, used
//! traits, implemented interfaces, and generic-argument classes).
//!
//! The sort uses iterative DFS with an explicit stack to avoid
//! recursion (and therefore stack overflow) regardless of hierarchy
//! depth.  Cycles are detected via a `visiting` set and silently
//! broken — the class that closes the cycle is processed without
//! the cyclic dependency's contributions.
//!
//! This is the foundation for eager iterative class population
//! (ER1 in `docs/todo/eager-resolution.md`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::types::ClassInfo;

/// Collect dependency edges for a single class.
///
/// Returns the FQNs of all classes that `class` directly depends on
/// for inheritance resolution: parent class, used traits, implemented
/// interfaces, and class names referenced in `@extends`, `@implements`,
/// and `@use` generic arguments.
fn class_dependencies(class: &ClassInfo) -> Vec<String> {
    let mut deps = Vec::new();

    if let Some(parent) = class.parent_class {
        deps.push(parent.to_string());
    }

    for trait_name in &class.used_traits {
        deps.push(trait_name.to_string());
    }

    for iface in &class.interfaces {
        deps.push(iface.to_string());
    }

    // Generic argument class names from @extends, @implements, @use.
    // These reference classes whose template parameters need to be
    // resolved before the current class can substitute them.
    for (name, _) in &class.extends_generics {
        deps.push(name.to_string());
    }
    for (name, _) in &class.implements_generics {
        deps.push(name.to_string());
    }
    for (name, _) in &class.use_generics {
        deps.push(name.to_string());
    }

    // Mixin classes (from @mixin tags).  Needed for Phase 2 (ER3)
    // so that mixin classes are populated before the classes that
    // reference them.  Including them here from the start means the
    // sort order is correct for both inheritance and virtual-member
    // passes.
    for mixin in &class.mixins {
        deps.push(mixin.to_string());
    }

    deps
}

/// State for a single frame in the iterative DFS stack.
///
/// Each frame represents a node being visited.  `dep_index` tracks
/// which dependency we process next, allowing us to resume after
/// pushing a child frame.
struct DfsFrame {
    /// The FQN of the class being visited.
    fqn: String,
    /// Index into the dependency list — how far we've gotten.
    dep_index: usize,
    /// Cached dependency list for this class.
    deps: Vec<String>,
}

/// Topologically sort class-like declarations by their dependencies.
///
/// # Arguments
///
/// * `classes` — iterator of `(FQN, &ClassInfo)` pairs covering every
///   known class-like declaration (from `uri_classes_index`, stubs, etc.).
///
/// # Returns
///
/// A `Vec<String>` of FQNs in dependency order: every class appears
/// after all of its dependencies.  Classes involved in cycles appear
/// in an unspecified but safe order (the cycle is silently broken).
///
/// Classes referenced as dependencies but not present in the input
/// (e.g. external classes not yet loaded) are silently skipped.
///
/// # Algorithm
///
/// Iterative DFS with an explicit stack.  Three sets track state:
///
/// - `visited`: nodes whose subtree is fully processed (already in
///   `sorted`).
/// - `visiting`: nodes currently on the DFS stack (ancestors of the
///   current node).  Re-encountering a node in this set means a
///   cycle — the edge is skipped.
/// - The DFS stack itself (`stack: Vec<DfsFrame>`).
///
/// This is equivalent to the recursive DFS in mago's
/// `populator::sorter::sort_class_likes`, but uses O(1) stack space
/// per class regardless of hierarchy depth.
pub(crate) fn toposort_classes<'a>(
    classes: impl Iterator<Item = (String, &'a ClassInfo)>,
) -> Vec<String> {
    // Build a map from FQN → dependency list.
    let mut dep_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_fqns: Vec<String> = Vec::new();

    for (fqn, class) in classes {
        let deps = class_dependencies(class);
        all_fqns.push(fqn.clone());
        dep_map.insert(fqn, deps);
    }

    // Sort the starting FQNs so that the DFS visitation order is
    // deterministic regardless of HashMap iteration order in the
    // caller (e.g. `toposort_from_uri_classes_index` iterates a HashMap whose
    // order varies between runs due to random hashing seeds).
    // Without this, classes at the same topological level can be
    // processed in different orders across runs, which causes the
    // recursion guard in `resolve_class_fully_inner` to break
    // implicit cycles differently — leading to non-deterministic
    // cache contents and flaky diagnostics.
    all_fqns.sort();

    let mut visited: HashSet<String> = HashSet::with_capacity(all_fqns.len());
    let mut visiting: HashSet<String> = HashSet::new();
    let mut sorted: Vec<String> = Vec::with_capacity(all_fqns.len());

    for start_fqn in &all_fqns {
        if visited.contains(start_fqn) {
            continue;
        }

        // Iterative DFS starting from `start_fqn`.
        let start_deps = dep_map.get(start_fqn).cloned().unwrap_or_default();

        visiting.insert(start_fqn.clone());

        let mut stack = vec![DfsFrame {
            fqn: start_fqn.clone(),
            dep_index: 0,
            deps: start_deps,
        }];

        while let Some(frame) = stack.last_mut() {
            if frame.dep_index < frame.deps.len() {
                let dep = frame.deps[frame.dep_index].clone();
                frame.dep_index += 1;

                // Skip dependencies not in our input set (external/unloaded classes).
                if !dep_map.contains_key(&dep) {
                    continue;
                }

                // Already fully processed — nothing to do.
                if visited.contains(&dep) {
                    continue;
                }

                // Cycle detected — skip this edge.
                if visiting.contains(&dep) {
                    continue;
                }

                // Push a new frame for this dependency.
                let child_deps = dep_map.get(&dep).cloned().unwrap_or_default();

                visiting.insert(dep.clone());

                stack.push(DfsFrame {
                    fqn: dep,
                    dep_index: 0,
                    deps: child_deps,
                });
            } else {
                // All dependencies processed — emit this node.
                let frame = stack.pop().unwrap();
                visiting.remove(&frame.fqn);
                visited.insert(frame.fqn.clone());
                sorted.push(frame.fqn);
            }
        }
    }

    sorted
}

/// Convenience wrapper that extracts `(FQN, &ClassInfo)` pairs from
/// the `uri_classes_index` structure used by `Backend`.
///
/// Flattens the per-file `Vec<Arc<ClassInfo>>` into a single iterator
/// of `(fqn, &ClassInfo)` pairs suitable for [`toposort_classes`].
pub(crate) fn toposort_from_uri_classes_index(
    uri_classes_index: &HashMap<String, Vec<Arc<ClassInfo>>>,
) -> Vec<String> {
    let iter = uri_classes_index
        .values()
        .flat_map(|classes| classes.iter())
        .map(|class| (class.fqn().to_string(), class.as_ref()));
    toposort_classes(iter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClassInfo, ClassLikeKind};

    /// Create a minimal `ClassInfo` for testing.
    fn make_class(name: &str) -> ClassInfo {
        ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom(name),
            methods: Default::default(),
            method_index: Default::default(),
            indexed_method_count: 0,
            properties: Default::default(),
            constants: Default::default(),
            start_offset: 0,
            end_offset: 0,
            keyword_offset: 0,
            parent_class: None,
            interfaces: Vec::new(),
            used_traits: Vec::new(),
            mixins: Vec::new(),
            mixin_generics: Vec::new(),
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            template_params: Vec::new(),
            template_param_bounds: Default::default(),
            template_param_defaults: Default::default(),
            extends_generics: Vec::new(),
            implements_generics: Vec::new(),
            use_generics: Vec::new(),
            type_aliases: Default::default(),
            trait_precedences: Vec::new(),
            trait_aliases: Vec::new(),
            class_docblock: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            laravel: None,
        }
    }

    #[test]
    fn linear_chain() {
        // C extends B extends A
        let a = make_class("A");
        let mut b = make_class("B");
        b.parent_class = Some(crate::atom::atom("A"));
        let mut c = make_class("C");
        c.parent_class = Some(crate::atom::atom("B"));

        let classes = vec![
            ("C".to_string(), &c),
            ("B".to_string(), &b),
            ("A".to_string(), &a),
        ];

        let sorted = toposort_classes(classes.into_iter());

        let pos_a = sorted.iter().position(|s| s == "A").unwrap();
        let pos_b = sorted.iter().position(|s| s == "B").unwrap();
        let pos_c = sorted.iter().position(|s| s == "C").unwrap();

        assert!(pos_a < pos_b, "A must come before B");
        assert!(pos_b < pos_c, "B must come before C");
    }

    #[test]
    fn trait_dependency() {
        let t = make_class("MyTrait");
        let mut c = make_class("MyClass");
        c.used_traits = vec![crate::atom::atom("MyTrait")];

        let classes = vec![("MyClass".to_string(), &c), ("MyTrait".to_string(), &t)];

        let sorted = toposort_classes(classes.into_iter());

        let pos_t = sorted.iter().position(|s| s == "MyTrait").unwrap();
        let pos_c = sorted.iter().position(|s| s == "MyClass").unwrap();

        assert!(pos_t < pos_c, "MyTrait must come before MyClass");
    }

    #[test]
    fn interface_dependency() {
        let iface = make_class("MyInterface");
        let mut c = make_class("MyClass");
        c.interfaces = vec![crate::atom::atom("MyInterface")];

        let classes = vec![
            ("MyClass".to_string(), &c),
            ("MyInterface".to_string(), &iface),
        ];

        let sorted = toposort_classes(classes.into_iter());

        let pos_i = sorted.iter().position(|s| s == "MyInterface").unwrap();
        let pos_c = sorted.iter().position(|s| s == "MyClass").unwrap();

        assert!(pos_i < pos_c, "MyInterface must come before MyClass");
    }

    #[test]
    fn mixin_dependency() {
        let builder = make_class("Builder");
        let mut model = make_class("Model");
        model.mixins = vec![crate::atom::atom("Builder")];

        let classes = vec![
            ("Model".to_string(), &model),
            ("Builder".to_string(), &builder),
        ];

        let sorted = toposort_classes(classes.into_iter());

        let pos_b = sorted.iter().position(|s| s == "Builder").unwrap();
        let pos_m = sorted.iter().position(|s| s == "Model").unwrap();

        assert!(pos_b < pos_m, "Builder must come before Model");
    }

    #[test]
    fn cycle_does_not_panic() {
        // A extends B, B extends A — a cycle.
        let mut a = make_class("A");
        a.parent_class = Some(crate::atom::atom("B"));
        let mut b = make_class("B");
        b.parent_class = Some(crate::atom::atom("A"));

        let classes = vec![("A".to_string(), &a), ("B".to_string(), &b)];

        // Should not panic — cycles are silently broken.
        let sorted = toposort_classes(classes.into_iter());

        assert_eq!(sorted.len(), 2);
        assert!(sorted.contains(&"A".to_string()));
        assert!(sorted.contains(&"B".to_string()));
    }

    #[test]
    fn missing_dependency_is_skipped() {
        // B extends A, but A is not in the input.
        let mut b = make_class("B");
        b.parent_class = Some(crate::atom::atom("A"));

        let classes = vec![("B".to_string(), &b)];

        let sorted = toposort_classes(classes.into_iter());

        assert_eq!(sorted, vec!["B".to_string()]);
    }

    #[test]
    fn diamond_inheritance() {
        // D extends B and C, both extend A.
        let a = make_class("A");
        let mut b = make_class("B");
        b.parent_class = Some(crate::atom::atom("A"));
        let mut c = make_class("C");
        c.parent_class = Some(crate::atom::atom("A"));
        let mut d = make_class("D");
        d.parent_class = Some(crate::atom::atom("B"));
        d.interfaces = vec![crate::atom::atom("C")];

        let classes = vec![
            ("D".to_string(), &d),
            ("C".to_string(), &c),
            ("B".to_string(), &b),
            ("A".to_string(), &a),
        ];

        let sorted = toposort_classes(classes.into_iter());

        let pos_a = sorted.iter().position(|s| s == "A").unwrap();
        let pos_b = sorted.iter().position(|s| s == "B").unwrap();
        let pos_c = sorted.iter().position(|s| s == "C").unwrap();
        let pos_d = sorted.iter().position(|s| s == "D").unwrap();

        assert!(pos_a < pos_b, "A must come before B");
        assert!(pos_a < pos_c, "A must come before C");
        assert!(pos_b < pos_d, "B must come before D");
        assert!(pos_c < pos_d, "C must come before D");
    }

    #[test]
    fn extends_generics_dependency() {
        // Collection<int, Language> — Collection should come before the child.
        let collection = make_class("Collection");
        let mut child = make_class("LanguageCollection");
        child.parent_class = Some(crate::atom::atom("Collection"));
        child.extends_generics = vec![(crate::atom::atom("Collection"), vec![])];

        let classes = vec![
            ("LanguageCollection".to_string(), &child),
            ("Collection".to_string(), &collection),
        ];

        let sorted = toposort_classes(classes.into_iter());

        let pos_col = sorted.iter().position(|s| s == "Collection").unwrap();
        let pos_child = sorted
            .iter()
            .position(|s| s == "LanguageCollection")
            .unwrap();

        assert!(pos_col < pos_child);
    }

    #[test]
    fn all_classes_appear_exactly_once() {
        let a = make_class("A");
        let mut b = make_class("B");
        b.parent_class = Some(crate::atom::atom("A"));
        let mut c = make_class("C");
        c.parent_class = Some(crate::atom::atom("A"));
        c.used_traits = vec![crate::atom::atom("B")];

        let classes = vec![
            ("A".to_string(), &a),
            ("B".to_string(), &b),
            ("C".to_string(), &c),
        ];

        let sorted = toposort_classes(classes.into_iter());

        assert_eq!(sorted.len(), 3);
        let mut deduped = sorted.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(deduped.len(), 3, "no duplicates");
    }

    #[test]
    fn empty_input() {
        let sorted = toposort_classes(std::iter::empty());
        assert!(sorted.is_empty());
    }

    #[test]
    fn single_class_no_deps() {
        let a = make_class("A");
        let classes = vec![("A".to_string(), &a)];

        let sorted = toposort_classes(classes.into_iter());

        assert_eq!(sorted, vec!["A".to_string()]);
    }

    #[test]
    fn self_referencing_class() {
        // A class that lists itself as a mixin (degenerate case).
        let mut a = make_class("A");
        a.mixins = vec![crate::atom::atom("A")];

        let classes = vec![("A".to_string(), &a)];

        let sorted = toposort_classes(classes.into_iter());

        assert_eq!(sorted, vec!["A".to_string()]);
    }

    #[test]
    fn complex_graph_with_multiple_dep_types() {
        // Interface I
        // Trait T uses I (interface dep)
        // Abstract class A implements I, uses T
        // Class B extends A, mixin M
        // Class M (the mixin)
        let i = make_class("I");
        let mut t = make_class("T");
        t.interfaces = vec![crate::atom::atom("I")];
        let mut a = make_class("A");
        a.interfaces = vec![crate::atom::atom("I")];
        a.used_traits = vec![crate::atom::atom("T")];
        let m = make_class("M");
        let mut b = make_class("B");
        b.parent_class = Some(crate::atom::atom("A"));
        b.mixins = vec![crate::atom::atom("M")];

        let classes = vec![
            ("B".to_string(), &b),
            ("M".to_string(), &m),
            ("A".to_string(), &a),
            ("T".to_string(), &t),
            ("I".to_string(), &i),
        ];

        let sorted = toposort_classes(classes.into_iter());

        let pos = |name: &str| sorted.iter().position(|s| s == name).unwrap();

        assert!(pos("I") < pos("T"), "I before T");
        assert!(pos("I") < pos("A"), "I before A");
        assert!(pos("T") < pos("A"), "T before A");
        assert!(pos("A") < pos("B"), "A before B");
        assert!(pos("M") < pos("B"), "M before B");
    }
}
