//! Which other open files a save can change the diagnostics of.
//!
//! Saving a file is the point at which PHPantom re-diagnoses the other
//! open files, because an edit to one file can invalidate diagnostics in
//! every file that uses it (unknown class, unknown member, argument
//! checks).  Re-running the slow pass for every open tab is the most
//! expensive thing the server does, so the fan-out is narrowed to the
//! files that can actually observe the change:
//!
//! 1. Diff the symbols the saved file declares against the baseline
//!    captured the last time it was opened or saved.  What comes out is
//!    the set of names whose meaning changed: class FQNs, global function
//!    and constant names, and individual member names.
//! 2. Ask the reference index which files mention any of those names.
//!    Only those files are queued.
//!
//! Anything the diff cannot answer falls back to "every open file", which
//! is what the server did unconditionally before: a file with no baseline
//! (first save after startup), a file that declares no symbols at all
//! (Laravel config, routes, translations, Blade views, plain scripts), or
//! a workspace whose reference index is not built yet.
//!
//! The diff reads the last committed parse, and `didChange` parses run in
//! the background, so a save that lands in the few milliseconds before the
//! newest keystroke has been parsed diffs against a marginally older
//! revision.  Nothing is lost: the new baseline is exactly what was
//! diffed, so whatever the save did not see is still part of the next
//! save's diff.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::Backend;
use crate::reference_index::ReferenceIndexKey;
use crate::types::ClassInfo;
use crate::util::{short_name, strip_fqn_prefix};

/// The symbols a file declared at a point in time.
///
/// Classes are kept as the `Arc<ClassInfo>` values the parse produced, so
/// capturing a baseline is a refcount bump rather than a deep copy, and
/// the diff can use the same `signature_eq` predicates the resolved-class
/// cache uses.  Global functions and constants are kept as names only:
/// `uri_globals_index` does not retain their signatures, so a file that
/// declares them always contributes those names to the affected set.
#[derive(Default)]
pub(crate) struct FileDeclarations {
    classes: Vec<Arc<ClassInfo>>,
    functions: Vec<String>,
    constants: Vec<String>,
}

impl FileDeclarations {
    fn is_empty(&self) -> bool {
        self.classes.is_empty() && self.functions.is_empty() && self.constants.is_empty()
    }
}

/// Per-file declaration baselines, keyed by document URI.
pub(crate) type DeclarationBaselines = HashMap<String, Arc<FileDeclarations>>;

impl Backend {
    /// Record what `uri` declares right now, to diff against on save.
    ///
    /// Called when a file is opened (and after each save) so the next save
    /// knows what changed.  Without a baseline the save falls back to
    /// re-diagnosing every open file.
    pub(crate) fn capture_declaration_baseline(&self, uri: &str) {
        let declarations = self.declarations_for_uri(uri);
        self.diag
            .decl_baselines
            .lock()
            .insert(uri.to_string(), Arc::new(declarations));
    }

    /// Drop the declaration baseline for a closed file.
    pub(crate) fn clear_declaration_baseline(&self, uri: &str) {
        self.diag.decl_baselines.lock().remove(uri);
    }

    /// The open files whose diagnostics can be affected by the save of
    /// `saved_uri` (which is itself excluded — the caller schedules it).
    pub(crate) fn open_files_affected_by_save(&self, saved_uri: &str) -> Vec<String> {
        let mut uris: Vec<String> = self
            .open_files
            .read()
            .keys()
            .filter(|u| u.as_str() != saved_uri)
            .cloned()
            .collect();
        if uris.is_empty() {
            return uris;
        }

        // Diff against the baseline and re-baseline for the next save.
        let current = Arc::new(self.declarations_for_uri(saved_uri));
        let baseline = self
            .diag
            .decl_baselines
            .lock()
            .insert(saved_uri.to_string(), Arc::clone(&current));

        let Some(baseline) = baseline else {
            return uris;
        };
        if baseline.is_empty() && current.is_empty() {
            // Nothing is declared here, so there is nothing to diff.  The
            // file may still be one whose *contents* other files depend on
            // (a Laravel config, translation, or route file), so keep the
            // conservative fan-out.
            return uris;
        }

        let mut keys = affected_reference_keys(&baseline, &current);
        if keys.is_empty() {
            uris.clear();
            return uris;
        }
        self.push_descendant_class_keys(&baseline, &current, &mut keys);

        self.retain_reference_candidates(&keys, &mut uris);
        uris
    }

    /// Add the class keys of every transitive descendant of a changed
    /// class to `keys`.
    ///
    /// A grandchild names only its direct parent, so the class keys of
    /// the changed class never reach it; what it inherits changed all the
    /// same.  The descendant's own key reaches both the files that extend
    /// it and the files that use it.
    fn push_descendant_class_keys(
        &self,
        baseline: &FileDeclarations,
        current: &FileDeclarations,
        keys: &mut Vec<ReferenceIndexKey>,
    ) {
        let mut queue: std::collections::VecDeque<String> = baseline
            .classes
            .iter()
            .chain(current.classes.iter())
            .map(|class| class.fqn().to_string())
            .filter(|fqn| keys.contains(&ReferenceIndexKey::class_owned(fqn.clone())))
            .collect();
        if queue.is_empty() {
            return;
        }
        let mut seen: HashSet<String> = queue.iter().cloned().collect();
        let mut descendant_keys = HashSet::new();
        let gti = self.symbols.gti_index.read();
        while let Some(fqn) = queue.pop_front() {
            for child in gti.get(&fqn).into_iter().flatten() {
                if seen.insert(child.clone()) {
                    push_class_keys(&mut descendant_keys, child);
                    queue.push_back(child.clone());
                }
            }
        }
        drop(gti);
        keys.extend(descendant_keys);
    }

    /// Collect the classes, functions, and constants `uri` currently
    /// contributes to the workspace indexes.
    fn declarations_for_uri(&self, uri: &str) -> FileDeclarations {
        let classes = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let (functions, constants) = self
            .symbols
            .uri_globals_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        FileDeclarations {
            classes,
            functions,
            constants,
        }
    }
}

/// The reference-index keys naming everything whose meaning may have
/// changed between `baseline` and `current`.
fn affected_reference_keys(
    baseline: &FileDeclarations,
    current: &FileDeclarations,
) -> Vec<ReferenceIndexKey> {
    let mut keys = HashSet::new();

    // Global functions and constants: names only, so any file that
    // declares them contributes them on every save.
    for name in baseline.functions.iter().chain(current.functions.iter()) {
        push_name_keys(&mut keys, name, ReferenceIndexKey::function_owned);
    }
    for name in baseline.constants.iter().chain(current.constants.iter()) {
        push_name_keys(&mut keys, name, ReferenceIndexKey::Constant);
    }

    let mut seen_fqns = HashSet::new();
    for class in baseline.classes.iter().chain(current.classes.iter()) {
        let fqn = class.fqn();
        if !seen_fqns.insert(fqn) {
            continue;
        }

        match (
            find_class(&baseline.classes, fqn.as_str()),
            find_class(&current.classes, fqn.as_str()),
        ) {
            // Declared before and after: only the difference matters.
            (Some(old), Some(new)) => {
                if old.signature_eq(new) {
                    // The declaration is unchanged, but a method body may
                    // not be: a method with no declared return type has
                    // its type inferred from its body, so callers of one
                    // can observe a body-only edit.  Constructors are
                    // exempt — nothing consults their return type.
                    push_body_inferred_member_keys(&mut keys, new);
                    continue;
                }
                push_class_keys(&mut keys, &fqn);
                if old.class_level_signature_eq(new) {
                    push_changed_member_keys(&mut keys, old, new);
                } else {
                    // A new parent, a different `@mixin`, an edited class
                    // docblock: any member's resolution can shift.
                    push_all_member_keys(&mut keys, old);
                    push_all_member_keys(&mut keys, new);
                }
            }
            // Added or removed outright.
            _ => {
                push_class_keys(&mut keys, &fqn);
                push_all_member_keys(&mut keys, class);
            }
        }
    }

    keys.into_iter().collect()
}

fn find_class<'a>(classes: &'a [Arc<ClassInfo>], fqn: &str) -> Option<&'a ClassInfo> {
    classes
        .iter()
        .find(|class| class.fqn().as_str() == fqn)
        .map(Arc::as_ref)
}

fn push_class_keys(keys: &mut HashSet<ReferenceIndexKey>, fqn: &str) {
    push_name_keys(keys, fqn, ReferenceIndexKey::class_owned);
}

/// Push both the fully-qualified and the short spelling of `name`.
///
/// The index stores a reference under every spelling it could be written
/// as, including the short name for an unresolved reference, so a class
/// that a file could not resolve before this save is still matched.
fn push_name_keys(
    keys: &mut HashSet<ReferenceIndexKey>,
    name: &str,
    make: fn(String) -> ReferenceIndexKey,
) {
    let fqn = strip_fqn_prefix(name);
    if fqn.is_empty() {
        return;
    }
    keys.insert(make(fqn.to_string()));
    let short = short_name(fqn);
    if short != fqn {
        keys.insert(make(short.to_string()));
    }
}

/// Push the member key for `name` in both its static and instance form.
///
/// The index distinguishes the two, and a call site can spell a member
/// either way (including incorrectly, which is exactly the case a
/// diagnostic reports).
fn push_member_keys(keys: &mut HashSet<ReferenceIndexKey>, name: &str) {
    if name.is_empty() {
        return;
    }
    for is_static in [false, true] {
        keys.insert(ReferenceIndexKey::Member {
            name: name.to_string(),
            is_static,
        });
    }
}

fn push_all_member_keys(keys: &mut HashSet<ReferenceIndexKey>, class: &ClassInfo) {
    for method in class.methods.iter() {
        push_member_keys(keys, &method.name);
    }
    for prop in class.properties.iter() {
        push_member_keys(keys, prop.name.strip_prefix('$').unwrap_or(&prop.name));
    }
    for constant in class.constants.iter() {
        push_member_keys(keys, &constant.name);
    }
    if let Some(ref doc) = class.doc_members {
        for method in &doc.methods {
            push_member_keys(keys, &method.name);
        }
        for (name, _) in &doc.properties {
            push_member_keys(keys, name);
        }
    }
}

/// Push the members that differ between two revisions of the same class.
///
/// Only valid when the class-level metadata is unchanged; a class-level
/// change can affect members that are identical on both sides.
fn push_changed_member_keys(
    keys: &mut HashSet<ReferenceIndexKey>,
    old: &ClassInfo,
    new: &ClassInfo,
) {
    for method in new.methods.iter() {
        match old.get_method(&method.name) {
            Some(previous) if previous.signature_eq(method) => {}
            _ => push_member_keys(keys, &method.name),
        }
    }
    for method in old.methods.iter() {
        if !new.has_method(&method.name) {
            push_member_keys(keys, &method.name);
        }
    }

    for prop in new.properties.iter() {
        let name = prop.name.strip_prefix('$').unwrap_or(&prop.name);
        match old.properties.iter().find(|p| p.name == prop.name) {
            Some(previous) if previous.signature_eq(prop) => {}
            _ => push_member_keys(keys, name),
        }
    }
    for prop in old.properties.iter() {
        if !new.properties.iter().any(|p| p.name == prop.name) {
            push_member_keys(keys, prop.name.strip_prefix('$').unwrap_or(&prop.name));
        }
    }

    for constant in new.constants.iter() {
        match old.constants.iter().find(|c| c.name == constant.name) {
            Some(previous) if previous.signature_eq(constant) => {}
            _ => push_member_keys(keys, &constant.name),
        }
    }
    for constant in old.constants.iter() {
        if !new.constants.iter().any(|c| c.name == constant.name) {
            push_member_keys(keys, &constant.name);
        }
    }

    // A body-only edit to an untyped method is invisible to the
    // comparisons above, but not to its callers.
    push_body_inferred_member_keys(keys, new);
}

/// Push the members whose type comes from a method body, and which a
/// signature comparison therefore cannot rule out as unchanged.
///
/// A `mixed` return (native or docblock) is treated the same as no
/// declared type: both fall through to body inference, so a body-only
/// edit to either can change the type callers see.
fn push_body_inferred_member_keys(keys: &mut HashSet<ReferenceIndexKey>, class: &ClassInfo) {
    for method in class.methods.iter() {
        if method.return_type.as_ref().is_none_or(|t| t.is_mixed())
            && !method.name.eq_ignore_ascii_case("__construct")
        {
            push_member_keys(keys, &method.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    /// A backend with two files open, the reference index built, and a
    /// declaration baseline captured for each.
    fn backend_with_open_files(files: &[(&str, &str)]) -> Backend {
        let backend = Backend::new_test();
        for (uri, content) in files {
            backend
                .open_files
                .write()
                .insert((*uri).to_string(), Arc::new((*content).to_string()));
            backend.update_ast(uri, content);
            backend.capture_declaration_baseline(uri);
        }
        backend.workspace_indexed.store(true, Ordering::Release);
        backend
    }

    fn save(backend: &Backend, uri: &str, content: &str) -> Vec<String> {
        backend
            .open_files
            .write()
            .insert(uri.to_string(), Arc::new(content.to_string()));
        backend.update_ast(uri, content);
        let mut affected = backend.open_files_affected_by_save(uri);
        affected.sort();
        affected
    }

    const CONSUMER: &str = "file:///project/Consumer.php";
    const BYSTANDER: &str = "file:///project/Bystander.php";
    const SERVICE: &str = "file:///project/Service.php";

    fn service(body: &str) -> String {
        format!("<?php\nnamespace App;\nclass Service {{\n{body}}}\n")
    }

    fn workspace(service_body: &str) -> Backend {
        backend_with_open_files(&[
            (SERVICE, &service(service_body)),
            (
                CONSUMER,
                "<?php\nnamespace App;\nclass Consumer {\n  public function run(Service $s): void { $s->handle(1); }\n}\n",
            ),
            (
                BYSTANDER,
                "<?php\nnamespace App;\nclass Bystander {\n  public function ping(): string { return 'pong'; }\n}\n",
            ),
        ])
    }

    #[test]
    fn only_the_affected_files_reach_the_diagnostic_queue() {
        let backend = workspace("  public function handle(int $n): void {}\n");
        backend.init_complete.store(true, Ordering::Release);
        backend.diag.pending_uris.lock().clear();

        backend
            .open_files
            .write()
            .insert(SERVICE.to_string(), Arc::new(service("")));
        backend.update_ast(SERVICE, &service(""));
        backend.schedule_diagnostics_for_open_files(SERVICE);

        let pending = backend.diag.pending_uris.lock().clone();
        assert_eq!(
            pending.into_iter().collect::<Vec<_>>(),
            vec![CONSUMER.to_string()]
        );
    }

    #[test]
    fn signature_change_queues_only_files_that_use_the_class() {
        let backend = workspace("  public function handle(int $n): void {}\n");

        let affected = save(
            &backend,
            SERVICE,
            &service("  public function handle(int $n, string $s): void {}\n"),
        );

        assert_eq!(affected, vec![CONSUMER.to_string()]);
    }

    #[test]
    fn naming_the_changed_class_is_enough_to_be_requeued() {
        let backend = workspace("  public function handle(int $n): void {}\n");

        // Adding a member is a change to the class as a whole: a subclass
        // can gain a missing-implementation diagnostic from it without
        // ever mentioning the new member.  So every file that names the
        // class is requeued, and only those.
        let affected = save(
            &backend,
            SERVICE,
            &service(
                "  public function handle(int $n): void {}\n  public function other(): int { return 1; }\n",
            ),
        );

        assert_eq!(affected, vec![CONSUMER.to_string()]);
    }

    #[test]
    fn body_only_edit_of_a_typed_method_affects_nobody() {
        let backend = workspace("  public function handle(int $n): void { $x = 1; }\n");

        let affected = save(
            &backend,
            SERVICE,
            &service("  public function handle(int $n): void { $x = 2; }\n"),
        );

        assert!(affected.is_empty(), "got {affected:?}");
    }

    #[test]
    fn body_only_edit_of_an_untyped_method_reaches_its_callers() {
        // Without a declared return type the type comes from the body, so
        // a body edit can change what a caller resolves.
        let backend = workspace("  public function handle($n) { return 1; }\n");

        let affected = save(
            &backend,
            SERVICE,
            &service("  public function handle($n) { return 'one'; }\n"),
        );

        assert_eq!(affected, vec![CONSUMER.to_string()]);
    }

    #[test]
    fn removing_a_class_reaches_the_files_that_referenced_it() {
        let backend = workspace("  public function handle(int $n): void {}\n");

        let affected = save(&backend, SERVICE, "<?php\nnamespace App;\n");

        assert_eq!(affected, vec![CONSUMER.to_string()]);
    }

    #[test]
    fn adding_a_class_reaches_the_files_that_could_not_resolve_it() {
        let backend = backend_with_open_files(&[
            (SERVICE, "<?php\nnamespace App;\n"),
            (
                CONSUMER,
                "<?php\nnamespace App;\nclass Consumer {\n  public function run(): Service { return new Service(); }\n}\n",
            ),
            (BYSTANDER, "<?php\nnamespace App;\nclass Bystander {}\n"),
        ]);

        let affected = save(
            &backend,
            SERVICE,
            "<?php\nnamespace App;\nclass Service {}\n",
        );

        assert_eq!(affected, vec![CONSUMER.to_string()]);
    }

    #[test]
    fn changing_a_parent_class_reaches_users_of_the_inherited_members() {
        let backend = backend_with_open_files(&[
            (
                SERVICE,
                "<?php\nnamespace App;\nclass Service {\n  public function handle(int $n): void {}\n}\n",
            ),
            (
                CONSUMER,
                // Uses `handle` without naming `Service` anywhere.
                "<?php\nnamespace App;\nclass Consumer {\n  public function run(Child $c): void { $c->handle(1); }\n}\n",
            ),
            (
                BYSTANDER,
                "<?php\nnamespace App;\nclass Child extends Service {}\n",
            ),
        ]);

        let affected = save(
            &backend,
            SERVICE,
            "<?php\nnamespace App;\nclass Service {\n  public function handle(int $n, int $m): void {}\n}\n",
        );

        assert_eq!(affected, vec![BYSTANDER.to_string(), CONSUMER.to_string()]);
    }

    // Cases adapted from laravel-lsp's MIT-licensed test suite.  A
    // grandchild names only its direct parent, so a change further up the
    // chain reaches it only through the inheritance graph.
    #[test]
    fn changing_a_grandparent_class_reaches_its_grandchildren() {
        let backend = backend_with_open_files(&[
            (
                SERVICE,
                "<?php\nnamespace App;\nabstract class Service {}\n",
            ),
            (
                CONSUMER,
                "<?php\nnamespace App;\nabstract class Middle extends Service {}\n",
            ),
            (
                BYSTANDER,
                "<?php\nnamespace App;\nclass Leaf extends Middle {}\n",
            ),
        ]);

        let affected = save(
            &backend,
            SERVICE,
            "<?php\nnamespace App;\nabstract class Service {\n  abstract public function run(): void;\n}\n",
        );

        assert_eq!(affected, vec![BYSTANDER.to_string(), CONSUMER.to_string()]);
    }

    #[test]
    fn changing_a_used_trait_reaches_the_users_of_the_trait_that_uses_it() {
        let backend = backend_with_open_files(&[
            (SERVICE, "<?php\nnamespace App;\ntrait Service {}\n"),
            (
                CONSUMER,
                "<?php\nnamespace App;\ntrait Middle {\n  use Service;\n}\n",
            ),
            (
                BYSTANDER,
                "<?php\nnamespace App;\nclass Leaf {\n  use Middle;\n}\n",
            ),
        ]);

        let affected = save(
            &backend,
            SERVICE,
            "<?php\nnamespace App;\ntrait Service {\n  abstract public function run(): void;\n}\n",
        );

        assert_eq!(affected, vec![BYSTANDER.to_string(), CONSUMER.to_string()]);
    }

    #[test]
    fn a_file_without_a_baseline_falls_back_to_every_open_file() {
        let backend = workspace("  public function handle(int $n): void {}\n");
        backend.clear_declaration_baseline(SERVICE);

        let affected = save(
            &backend,
            SERVICE,
            &service("  public function handle(int $n): void {}\n"),
        );

        assert_eq!(affected, vec![BYSTANDER.to_string(), CONSUMER.to_string()]);
    }

    #[test]
    fn a_file_that_declares_nothing_falls_back_to_every_open_file() {
        // Laravel config, translation, and route files declare no symbols
        // but other files still depend on their contents.
        let config = "file:///project/config/app.php";
        let backend = backend_with_open_files(&[
            (config, "<?php\nreturn ['name' => 'Acme'];\n"),
            (CONSUMER, "<?php\nnamespace App;\nclass Consumer {}\n"),
        ]);

        let affected = save(&backend, config, "<?php\nreturn ['name' => 'Other'];\n");

        assert_eq!(affected, vec![CONSUMER.to_string()]);
    }

    #[test]
    fn an_unindexed_workspace_falls_back_to_every_open_file() {
        let backend = workspace("  public function handle(int $n): void {}\n");
        backend.workspace_indexed.store(false, Ordering::Release);

        let affected = save(
            &backend,
            SERVICE,
            &service("  public function handle(int $n, int $m): void {}\n"),
        );

        assert_eq!(affected, vec![BYSTANDER.to_string(), CONSUMER.to_string()]);
    }

    #[test]
    fn a_changed_global_function_reaches_its_callers() {
        let helpers = "file:///project/helpers.php";
        let backend = backend_with_open_files(&[
            (
                helpers,
                "<?php\nnamespace App;\nfunction shout(string $s): string { return $s; }\n",
            ),
            (
                CONSUMER,
                "<?php\nnamespace App;\nclass Consumer {\n  public function run(): void { shout('hi'); }\n}\n",
            ),
            (BYSTANDER, "<?php\nnamespace App;\nclass Bystander {}\n"),
        ]);

        let affected = save(
            &backend,
            helpers,
            "<?php\nnamespace App;\nfunction shout(string $s, int $times): string { return $s; }\n",
        );

        assert_eq!(affected, vec![CONSUMER.to_string()]);
    }
}
