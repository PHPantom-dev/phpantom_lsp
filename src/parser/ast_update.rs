/// AST update orchestration and name resolution.
///
/// This module contains the `update_ast` method that performs a full
/// parse of a PHP file and updates all the backend maps (uri_classes_index,
/// use_map, namespace_map, global_functions, global_defines, fqn_uri_index,
/// symbol_maps) in a single pass.  It also contains the name resolution
/// helpers (`resolve_parent_class_names`, `resolve_name`) used to convert
/// short class names to fully-qualified names.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::ParseErrorEntry;
use crate::atom::{Atom, atom, bytes_to_str};
use crate::ci_map::CiMap;
use crate::names::OwnedResolvedNames;
use crate::php_type::PhpType;
use crate::symbol_map::{SymbolMap, extract_symbol_map};
use crate::types::{
    ClassInfo, DefineInfo, DocblockMembers, FunctionInfo, MethodInfo, NamespaceSpan, TypeAliasDef,
};

use mago_allocator::LocalArena;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use crate::Backend;

use super::DocblockCtx;

/// Run `f` with a parsing arena, reusing a thread-local `LocalArena` across
/// calls instead of allocating a fresh one each time.
///
/// `update_ast_inner` is invoked on every keystroke (each `didChange`),
/// so a fresh `LocalArena::new()` per call returns its backing pages to the OS
/// via `munmap` on drop and re-acquires them via `mmap` on the next
/// parse. Reusing one arena and `reset()`ing it (an O(1) bump-pointer
/// rewind that keeps the pages allocated) eliminates those syscalls
/// during active editing.
///
/// Resolution can trigger a nested parse on the same thread (e.g.
/// `find_or_load_function` calls `update_ast` while the outer parse is
/// still using the arena). Such re-entrant calls fall back to a throwaway
/// `LocalArena` so the shared arena is never aliased — the borrow held for the
/// duration of `f` makes `try_borrow_mut` fail for the nested call.
fn with_reusable_arena<R>(f: impl FnOnce(&LocalArena) -> R) -> R {
    thread_local! {
        static ARENA: RefCell<LocalArena> = const { RefCell::new(LocalArena::new()) };
    }

    ARENA.with(|cell| match cell.try_borrow_mut() {
        Ok(mut arena) => {
            arena.reset();
            f(&arena)
        }
        Err(_) => f(&LocalArena::new()),
    })
}

pub(crate) enum AstIndexParseResult {
    Update(AstIndexUpdate),
    ParseFailed {
        uri: String,
        errors: Vec<ParseErrorEntry>,
    },
}

pub(crate) struct AstIndexUpdate {
    uri: String,
    parse_errors: Vec<ParseErrorEntry>,
    classes: Vec<ClassInfo>,
    use_map: HashMap<String, String>,
    resolved_names: Arc<OwnedResolvedNames>,
    namespace_spans: Vec<NamespaceSpan>,
    functions: Vec<FunctionInfo>,
    defines: Vec<(String, DefineInfo)>,
    symbol_map: Arc<SymbolMap>,
}

fn class_info_fqn(class: &ClassInfo) -> String {
    match &class.file_namespace {
        Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, class.name),
        _ => class.name.to_string(),
    }
}

/// The declaration of `fqn` that `uri` contributed, whether or not it is
/// the one currently serving lookups.
fn declared_function<'a>(
    fmap: &'a CiMap<(String, FunctionInfo)>,
    dupes: &'a CiMap<BTreeMap<String, FunctionInfo>>,
    fqn: &str,
    uri: &str,
) -> Option<&'a FunctionInfo> {
    if let Some(decls) = dupes.get(fqn) {
        return decls.get(uri);
    }
    fmap.get(fqn)
        .filter(|(owner, _)| owner == uri)
        .map(|(_, info)| info)
}

/// Mirror the lowest-sorting declaration of `fqn` into the lookup map,
/// dropping the name entirely once nothing declares it.
fn publish_winning_function(
    fmap: &mut CiMap<(String, FunctionInfo)>,
    dupes: &mut CiMap<BTreeMap<String, FunctionInfo>>,
    fqn: &str,
) {
    let Some(decls) = dupes.get(fqn) else { return };
    match decls.iter().next() {
        Some((uri, info)) => {
            let winner = (uri.clone(), info.clone());
            fmap.insert(fqn.to_string(), winner);
        }
        None => {
            fmap.remove(fqn);
        }
    }
    // One declarant left is not a duplicate; the lookup map alone
    // describes it, so stop paying for the second entry.
    if dupes.get(fqn).is_some_and(|d| d.len() <= 1) {
        dupes.remove(fqn);
    }
}

/// Record that `uri` declares `fqn`.
///
/// The lowest-sorting URI wins, so a name two files declare resolves to
/// the same one of them however the indexing workers were scheduled.
fn declare_function(
    fmap: &mut CiMap<(String, FunctionInfo)>,
    dupes: &mut CiMap<BTreeMap<String, FunctionInfo>>,
    fqn: String,
    uri: &str,
    info: FunctionInfo,
) {
    if let Some(decls) = dupes.get_mut(&fqn) {
        decls.insert(uri.to_string(), info);
        publish_winning_function(fmap, dupes, &fqn);
        return;
    }
    match fmap.get(fqn.as_str()) {
        // A second file declares a name another one already holds, which
        // is how a package ships a native helper alongside a
        // `function_exists`-guarded polyfill.  Start tracking both.
        Some((existing_uri, existing_info)) if existing_uri != uri => {
            let mut decls = BTreeMap::new();
            decls.insert(existing_uri.clone(), existing_info.clone());
            decls.insert(uri.to_string(), info);
            dupes.insert(fqn.clone(), decls);
            publish_winning_function(fmap, dupes, &fqn);
        }
        _ => {
            fmap.insert(fqn, (uri.to_string(), info));
        }
    }
}

/// Drop `uri`'s declaration of `fqn`, handing the name to the next-lowest
/// file that still declares it.
fn withdraw_function(
    fmap: &mut CiMap<(String, FunctionInfo)>,
    dupes: &mut CiMap<BTreeMap<String, FunctionInfo>>,
    fqn: &str,
    uri: &str,
) {
    if let Some(decls) = dupes.get_mut(fqn) {
        decls.remove(uri);
        publish_winning_function(fmap, dupes, fqn);
        return;
    }
    if fmap.get(fqn).is_some_and(|(owner, _)| owner == uri) {
        fmap.remove(fqn);
    }
}

impl Backend {
    /// Drop every function declaration contributed by `uris`, handing each
    /// name to the next-lowest file that still declares it.
    ///
    /// Used by the file-watcher purge, where a deleted file must not take a
    /// name another file also declares down with it.
    pub(crate) fn withdraw_functions_for_uris(&self, uris: &std::collections::HashSet<String>) {
        let mut fmap = self.symbols.global_functions.write();
        let mut dupes = self.symbols.duplicate_functions.write();

        let affected: Vec<String> = dupes
            .iter()
            .filter(|(_, decls)| decls.keys().any(|u| uris.contains(u)))
            .map(|(fqn, _)| fqn.to_string())
            .collect();
        for fqn in affected {
            if let Some(decls) = dupes.get_mut(&fqn) {
                decls.retain(|u, _| !uris.contains(u));
            }
            publish_winning_function(&mut fmap, &mut dupes, &fqn);
        }

        // Whatever the promotions above did not re-point still belongs to a
        // purged file.
        fmap.retain(|_, (u, _)| !uris.contains(u));
    }

    /// Update the uri_classes_index, use_map, and namespace_map for a given file URI
    /// by parsing its content.
    ///
    /// Returns `true` when at least one class signature in this file
    /// changed (or a class was added/removed), meaning other open files
    /// that reference those classes may have stale diagnostics.
    pub fn update_ast(&self, uri: &str, content: &str) -> bool {
        // Invalidate thread-local mixin cache so stale ClassInfo is not
        // served after a file changes.
        crate::virtual_members::phpdoc::bump_mixin_generation();

        let content_to_parse = self
            .record_blade_virtual_php(uri, content)
            .unwrap_or_else(|| content.to_string());

        self.laravel_string_key_cache
            .write()
            .invalidate_for_uri(uri, content);
        self.refresh_blade_discovery(uri);
        self.refresh_blade_block_index(uri, content);

        // The mago-syntax parser contains `unreachable!()` and `.expect()`
        // calls that can panic on malformed PHP (e.g. partially-written
        // heredocs/nowdocs, which are common while editing).  Wrap the
        // entire parse + extraction in `catch_unwind` so a parser panic
        // doesn't crash the LSP server and produce a zombie process.
        //
        // On panic the file is simply skipped — no maps are updated, and
        // the user gets stale (but not missing) completions until the
        // file is saved in a parseable state.
        let content_owned = content_to_parse;
        let uri_owned = uri.to_string();

        let result = crate::util::catch_panic_unwind_safe("parse", uri, None, || {
            self.update_ast_inner(&uri_owned, &content_owned)
        });

        // Keep the Laravel macro index coherent with edits to files that
        // register macros.  Cheap no-op for files without a `macro(` call.
        self.refresh_laravel_macros(uri, content);

        // Keep the filesystem disk type coherent with edits to `config/` and
        // to files that register a `Storage::extend()` driver.
        self.refresh_laravel_storage_drivers(uri, content);

        // Keep the reverse pivot index coherent with edits to files that
        // declare (or previously declared) a many-to-many relationship.
        self.refresh_laravel_pivots(uri, content);

        // Keep the Artisan command index coherent with edits to command
        // files.  Cheap no-op for files that are not (and were not) commands.
        self.refresh_laravel_command_index(uri);

        // Keep the Eloquent morph map coherent with edits to the provider that
        // registers it.  Cheap no-op for files without a `morphMap(` call.
        self.refresh_laravel_morph_map(uri, content);

        // Keep the authorization gate index coherent with edits to the
        // provider that registers abilities and policies.  Cheap no-op for
        // files that mention neither `Gate` nor `$policies`.
        self.refresh_laravel_gates(uri, content);

        // Keep the set of config keys the project declares at runtime
        // coherent with edits to the files that write them.  Reads the spans
        // the parse above just stored, so it runs after it rather than
        // alongside the token-gated refreshes.
        self.refresh_laravel_config_writes(uri);

        // Keep the container bindings, package config files, view and
        // translation directories, route files, and component namespaces
        // coherent with edits to the providers that register them.  Cheap
        // no-op for every file that is not a registered service provider.
        self.refresh_laravel_provider_resources(uri, content);

        match result {
            Some(changed) => changed,
            None => {
                // Parser panicked — store a single "Parse failed" error
                // so the syntax-error diagnostic collector can report it.
                self.parse_errors.write().insert(
                    uri.to_string(),
                    vec![("Parse failed (internal error)".to_string(), 0, 0)],
                );
                false
            }
        }
    }

    /// Inner implementation of [`update_ast`] that performs the actual parse
    /// and publishes the resulting single-file update.
    fn update_ast_inner(&self, uri: &str, content: &str) -> bool {
        let update = self.build_ast_index_update(uri, content);
        self.apply_ast_index_updates_batch(vec![update])
    }

    /// Pull the imports out of the wrapper *method* a template whose
    /// `$this` is bound is wrapped in, for the same reason the wrapper
    /// function's are pulled out.  A no-op for every other class.
    fn extract_blade_wrapper_method_use_statements(
        statement: &Statement<'_>,
        use_map: &mut HashMap<String, String>,
    ) {
        use mago_syntax::cst::class_like::member::ClassLikeMember;
        use mago_syntax::cst::class_like::method::MethodBody;

        let Statement::Class(class) = statement else {
            return;
        };
        if !crate::blade::is_scope_class(bytes_to_str(class.name.value)) {
            return;
        }
        for member in class.members.iter() {
            if let ClassLikeMember::Method(method) = member
                && bytes_to_str(method.name.value) == crate::blade::WRAPPER_FUNCTION
                && let MethodBody::Concrete(block) = &method.body
            {
                Self::extract_use_statements_from_statements(block.statements.iter(), use_map);
            }
        }
    }

    /// Preprocess a Blade template into the virtual PHP everything else
    /// reads it as, publishing the source map and the virtual content the
    /// position translation rides on.
    ///
    /// Returns `None` for a file that is not a template, which is parsed
    /// as it stands.
    ///
    /// Seeds the template scope with the set cached by the refresh passes
    /// (post-index refresh, Blade did_open, caller save): the members of
    /// the component class backing the view, then the types its call sites
    /// imply, plus the class the view's `$this` is bound to.  Both variable
    /// sources sit below the template's own declarations, which the
    /// preprocessor gives priority.
    ///
    /// Neither is computed here: this runs on the parallel index/analyse
    /// workers, where scanning call sites is wasted (callers may not be
    /// parsed yet) and resolving their expression types from many threads
    /// at once has deadlocked against the batch-publish locks.  The serial
    /// refresh passes own the cache; this path only reads it.
    pub(crate) fn record_blade_virtual_php(&self, uri: &str, content: &str) -> Option<String> {
        if !self.is_blade_file(uri) {
            return None;
        }

        let injected = self
            .blade_injected_vars
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let components = self.blade_component_resolver(&injected.components);
        // Read under the guard rather than cloned: the set is small but
        // this runs on every keystroke in a template, and nothing the
        // preprocessor does can write it back.
        let custom_directives = self.blade_custom_directives.read();
        let (virtual_php, source_map) = crate::blade::preprocessor::preprocess_with_vars(
            content,
            &injected.vars,
            crate::blade::template_kind(uri, content),
            injected.this_class.as_deref(),
            Some(&components),
            &custom_directives,
        );
        self.blade_source_maps
            .write()
            .insert(uri.to_string(), source_map);
        self.blade_virtual_content
            .write()
            .insert(uri.to_string(), virtual_php.clone());
        Some(virtual_php)
    }

    pub(crate) fn parse_ast_index_update_for_index(
        &self,
        uri: &str,
        content: &str,
    ) -> AstIndexParseResult {
        let uri_owned = uri.to_string();
        // A template is indexed as the virtual PHP it lowers to, the same
        // way an open one is.  Parsing its own bytes leaves everything
        // Blade-specific as inline HTML, so the symbol map holds none of
        // the class references the template makes and rename, references,
        // and diagnostics all read it as empty.
        //
        // An open buffer is left alone: this batch reads the file from
        // disk, and `apply_ast_index_parse_results_batch` drops its result
        // for that reason.  Publishing virtual content from disk here
        // would leave it describing different text than the symbol map
        // `did_change` published, which is exactly what the rename's
        // `matches_source` check refuses to plan against.
        let preprocessed = if self.is_blade_file(uri) && !self.open_files.read().contains_key(uri) {
            self.record_blade_virtual_php(uri, content)
        } else {
            None
        };
        let content = preprocessed.as_deref().unwrap_or(content);

        match crate::util::catch_panic_unwind_safe("parse", uri, None, || {
            self.build_ast_index_update(uri, content)
        }) {
            Some(update) => AstIndexParseResult::Update(update),
            None => AstIndexParseResult::ParseFailed {
                uri: uri_owned,
                errors: vec![("Parse failed (internal error)".to_string(), 0, 0)],
            },
        }
    }

    pub(crate) fn apply_ast_index_parse_results_batch(
        &self,
        results: Vec<AstIndexParseResult>,
    ) -> bool {
        if results.is_empty() {
            return false;
        }

        // Drop any result whose file is currently open in the editor. This
        // batch comes from a background/workspace parse that read the file
        // straight from disk (or from a pre-edit buffer snapshot); the open
        // buffer's own `did_change` -> `update_ast` has already published
        // newer state for it. Publishing the stale batch result here would
        // clobber that state and leave hover, diagnostics, and references
        // computed from pre-edit content until the next keystroke. Open
        // buffers are always kept fresh by `did_change`, so skipping them
        // here loses nothing.
        let open_uris = self.open_files.read();

        let mut updates = Vec::new();
        let mut failures = Vec::new();
        for result in results {
            match result {
                AstIndexParseResult::Update(update) => {
                    if open_uris.contains_key(&update.uri) {
                        continue;
                    }
                    updates.push(update);
                }
                AstIndexParseResult::ParseFailed { uri, errors } => {
                    if open_uris.contains_key(&uri) {
                        continue;
                    }
                    failures.push((uri, errors));
                }
            }
        }
        drop(open_uris);

        if !failures.is_empty() {
            let mut parse_errors = self.parse_errors.write();
            for (uri, errors) in failures {
                parse_errors.insert(uri, errors);
            }
        }

        // Invalidate the reverse pivot index when a background-indexed file
        // declares a many-to-many relationship, so its target models pick up
        // its pivot accessors on the next class load.
        if !self
            .laravel_pivots_dirty
            .load(std::sync::atomic::Ordering::Relaxed)
            && updates.iter().any(|u| {
                u.classes
                    .iter()
                    .any(crate::virtual_members::laravel::class_declares_pivot_relationship)
            })
        {
            self.laravel_pivots_dirty
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }

        self.apply_ast_index_updates_batch(updates)
    }

    fn build_ast_index_update(&self, uri: &str, content: &str) -> AstIndexUpdate {
        with_reusable_arena(|arena| {
            let file_id = mago_database::file::FileId::new(b"input.php");
            let program = parse_file_content(arena, file_id, content.as_bytes());

            // Run mago-names resolver while the arena is still alive.
            // This produces a `ResolvedNames` that maps every identifier's
            // byte offset to its fully-qualified name.  We immediately copy
            // the data into an owned `OwnedResolvedNames` so it survives
            // the arena drop.
            let name_resolver = mago_names::resolver::NameResolver::new(arena);
            let mago_resolved = name_resolver.resolve(program);
            let owned_resolved = OwnedResolvedNames::from_resolved(&mago_resolved);

            let parse_errors: Vec<ParseErrorEntry> = program
                .errors
                .iter()
                .map(|e| {
                    let span = e.span();
                    (
                        super::error_format::format_parse_error(e),
                        span.start.offset,
                        span.end.offset,
                    )
                })
                .collect();

            let doc_ctx = DocblockCtx {
                trivias: program.trivia.as_slice(),
                content,
                php_version: Some(self.php_version()),
                use_map: HashMap::new(),
                namespace: None,
            };

            // Extract all three in a single parse pass.
            //
            // `classes_with_ns` tracks each extracted class together with the
            // namespace block it was declared in.  This is critical for files
            // that contain multiple `namespace { }` blocks, each declaring
            // classes under a different namespace.  The per-class namespace is
            // used later when building the `fqn_uri_index` and when resolving
            // parent/trait names.
            let mut classes_with_ns: Vec<(ClassInfo, Option<String>)> = Vec::new();
            let mut use_map = HashMap::new();
            let mut namespace: Option<String> = None;
            let mut namespace_spans: Vec<NamespaceSpan> = Vec::new();

            for statement in program.statements.iter() {
                match statement {
                    Statement::Use(use_stmt) => {
                        Self::extract_use_items(&use_stmt.items, &mut use_map);
                    }
                    Statement::Namespace(ns) => {
                        let block_ns: Option<String> = ns
                            .name
                            .as_ref()
                            .map(|ident| bytes_to_str(ident.value()).to_string())
                            .filter(|n| !n.is_empty());

                        let ns_span = ns.span();
                        namespace_spans.push(NamespaceSpan {
                            namespace: block_ns.clone(),
                            start: ns_span.start.offset,
                            end: ns_span.end.offset,
                        });

                        // The file-level namespace is the FIRST non-empty one.
                        if namespace.is_none() {
                            namespace = block_ns.clone();
                        }

                        // Collect classes from this namespace block, tagging
                        // each with the block's namespace.
                        let mut block_classes = Vec::new();
                        for inner in ns.statements().iter() {
                            match inner {
                                Statement::Use(use_stmt) => {
                                    Self::extract_use_items(&use_stmt.items, &mut use_map);
                                }
                                Statement::Class(_)
                                | Statement::Interface(_)
                                | Statement::Trait(_)
                                | Statement::Enum(_)
                                // Class-likes declared inside conditional /
                                // control-flow blocks (e.g. Doctrine's
                                // `ServiceEntityRepository` version guard) —
                                // the extractor descends into the bodies.
                                | Statement::If(_)
                                | Statement::Block(_)
                                | Statement::Try(_)
                                | Statement::Switch(_)
                                | Statement::While(_)
                                | Statement::DoWhile(_)
                                | Statement::For(_)
                                | Statement::Foreach(_) => {
                                    Self::extract_classes_from_statements(
                                        std::iter::once(inner),
                                        &mut block_classes,
                                        Some(&doc_ctx),
                                    );
                                }
                                Statement::Namespace(inner_ns) => {
                                    // Nested namespaces (rare but valid)
                                    Self::extract_use_statements_from_statements(
                                        inner_ns.statements().iter(),
                                        &mut use_map,
                                    );
                                    Self::extract_classes_from_statements(
                                        inner_ns.statements().iter(),
                                        &mut block_classes,
                                        Some(&doc_ctx),
                                    );
                                }
                                _ => {
                                    // Walk other statements (expression statements,
                                    // control flow, etc.) for anonymous classes.
                                    Self::find_anonymous_classes_in_statement(
                                        inner,
                                        &mut block_classes,
                                        Some(&doc_ctx),
                                    );
                                }
                            }
                        }

                        for cls in block_classes {
                            classes_with_ns.push((cls, block_ns.clone()));
                        }
                    }
                    Statement::Class(_)
                    | Statement::Interface(_)
                    | Statement::Trait(_)
                    | Statement::Enum(_)
                    // Class-likes declared inside top-level conditional /
                    // control-flow blocks — the extractor descends into the
                    // bodies (and still collects anonymous classes within).
                    | Statement::If(_)
                    | Statement::Block(_)
                    | Statement::Try(_)
                    | Statement::Switch(_)
                    | Statement::While(_)
                    | Statement::DoWhile(_)
                    | Statement::For(_)
                    | Statement::Foreach(_) => {
                        // A template whose `$this` is bound wraps its body
                        // in a method rather than a function, which buries
                        // the template's own imports just the same (see the
                        // wrapper-function arm below).
                        Self::extract_blade_wrapper_method_use_statements(statement, &mut use_map);
                        let mut top_classes = Vec::new();
                        Self::extract_classes_from_statements(
                            std::iter::once(statement),
                            &mut top_classes,
                            Some(&doc_ctx),
                        );
                        for cls in top_classes {
                            classes_with_ns.push((cls, None));
                        }
                    }
                    // Laravel compiles a template's `@php` and `<?php`
                    // regions into the top level of the generated view
                    // file, so a `use` written in one imports for the whole
                    // template. Wrapping the body in a function to make it
                    // analysable buries those imports in a function body,
                    // where the file's use-map would never see them.
                    Statement::Function(func)
                        if bytes_to_str(func.name.value) == crate::blade::WRAPPER_FUNCTION =>
                    {
                        Self::extract_use_statements_from_statements(
                            func.body.statements.iter(),
                            &mut use_map,
                        );
                    }
                    _ => {
                        // Walk other top-level statements (expression statements,
                        // function declarations, etc.) for anonymous classes.
                        let mut anon_classes = Vec::new();
                        Self::find_anonymous_classes_in_statement(
                            statement,
                            &mut anon_classes,
                            Some(&doc_ctx),
                        );
                        for cls in anon_classes {
                            classes_with_ns.push((cls, None));
                        }
                    }
                }
            }

            // A class-like declared in two branches of a conditional yields
            // one entry per branch; keep the first so resolution is
            // deterministic (see `dedup_class_likes_first_wins`).
            Self::dedup_class_likes_first_wins(&mut classes_with_ns);

            // Extract standalone functions (including those inside if-guards
            // like `if (! function_exists('...'))`) using the shared helper
            // which recurses into if/block statements.
            let mut functions = Vec::new();
            // Update doc_ctx with the file's use-map and namespace so that
            // parameter default values (e.g. `Application::class`) can be
            // resolved to FQNs during extraction.
            let func_doc_ctx = DocblockCtx {
                trivias: doc_ctx.trivias,
                content: doc_ctx.content,
                php_version: doc_ctx.php_version,
                use_map: use_map.clone(),
                namespace: namespace.clone(),
            };
            Self::extract_functions_from_statements(
                program.statements.iter(),
                &mut functions,
                &namespace,
                Some(&func_doc_ctx),
            );

            // Drop the declarations the Blade lowering wrote itself: the
            // wrapper holding the template body and the prologue's marker
            // functions.  Every template lowers to the same ones, so
            // publishing them makes each template a redeclaration of the
            // last and puts boilerplate no file wrote into
            // workspace-symbol results.  They stay in the virtual PHP, so
            // the calls the lowering emits still resolve against them
            // within the template.
            if self.is_blade_file(uri) {
                functions.retain(|func| !crate::blade::is_synthetic_function(&func.name));
            }

            // Apply stub patches when parsing embedded stub content
            // (e.g. a constant lookup routes its stub source through
            // `update_ast` under a `phpantom-stub://const/…` URI).  The
            // same stub file often defines functions and classes too;
            // without patching here, those register with unpatched
            // signatures and overwrite (or preempt) the patched entries
            // from the stub-function and stub-class loaders — silently
            // dropping e.g. `array_map`'s template parameters and
            // breaking closure parameter inference for the rest of the
            // session.
            if uri.starts_with("phpantom-stub") {
                for func in &mut functions {
                    crate::stub_patches::apply_function_stub_patches(func);
                }
                for (cls, _) in &mut classes_with_ns {
                    crate::stub_patches::apply_class_stub_patches(cls);
                }
            }

            if !functions.is_empty() {
                // Resolve class-like names in function return types and
                // parameter type hints to FQNs so that cross-file consumers
                // can resolve them without the declaring file's use map.
                // This mirrors the resolution done for class method return
                // types and parameter hints in `resolve_parent_class_names`.
                for func in &mut functions {
                    let skip_names: Vec<String> =
                        func.template_params.iter().map(|a| a.to_string()).collect();
                    // Use the function's own namespace (not the file-level one)
                    // so that multi-namespace files resolve return types
                    // against the correct namespace block.
                    let func_ns = func.namespace.clone().or_else(|| namespace.clone());
                    let resolver = Self::build_type_resolver(&use_map, &func_ns, &skip_names);

                    if let Some(ref ret) = func.return_type {
                        let resolved = ret.resolve_names(&resolver);
                        if resolved != *ret {
                            func.return_type = Some(resolved);
                        }
                    }
                    if let Some(ref ret) = func.native_return_type {
                        let resolved = ret.resolve_names(&resolver);
                        if resolved != *ret {
                            func.native_return_type = Some(resolved);
                        }
                    }
                    if let Some(ref cond) = func.conditional_return {
                        let resolved = cond.resolve_names(&resolver);
                        if resolved != *cond {
                            func.conditional_return = Some(resolved);
                        }
                    }
                    for param in func.parameters.make_mut() {
                        if let Some(ref hint) = param.type_hint {
                            let resolved = hint.resolve_names(&resolver);
                            if resolved != *hint {
                                param.type_hint = Some(resolved);
                            }
                        }
                        // `@param-closure-this` names a class the same way
                        // the rest of the docblock does, so it is resolved
                        // against the declaring file's imports here.  The
                        // call site, which is where the closure's `$this` is
                        // read, has no access to this file's use-map.
                        if let Some(ref this_type) = param.closure_this_type {
                            let resolved = this_type.resolve_names(&resolver);
                            if resolved != *this_type {
                                param.closure_this_type = Some(resolved);
                            }
                        }
                    }
                    // Resolve exception class names in @throws tags.
                    for throw in &mut func.throws {
                        let resolved = throw.resolve_names(&resolver);
                        if resolved != *throw {
                            *throw = resolved;
                        }
                    }
                }
            }

            // Extract define() constants from the already-parsed AST and
            // store them in the global_defines map so they appear in
            // completions.  This reuses the parse pass above rather than
            // doing a separate regex scan over the raw content.
            let mut define_entries = Vec::new();
            Self::extract_defines_from_statements(
                program.statements.iter(),
                &mut define_entries,
                content,
                None,
            );
            let defines: Vec<(String, DefineInfo)> = define_entries
                .into_iter()
                .map(|(name, offset, value)| {
                    (
                        name,
                        DefineInfo {
                            file_uri: uri.to_string(),
                            name_offset: offset,
                            value,
                        },
                    )
                })
                .collect();

            // Post-process: resolve parent_class short names to fully-qualified
            // names using the file's use_map and each class's own namespace so
            // that cross-file inheritance resolution can find parent classes via
            // PSR-4.
            //
            // For files with multiple namespace blocks, each class's names are
            // resolved against its own namespace rather than the file-level
            // default.  This is done by grouping classes by namespace and
            // calling resolve_parent_class_names once per group.
            {
                // Gather distinct namespaces used in this file.
                let mut ns_groups: HashMap<Option<String>, Vec<usize>> = HashMap::new();
                for (i, (_cls, ns)) in classes_with_ns.iter().enumerate() {
                    ns_groups.entry(ns.clone()).or_default().push(i);
                }

                // When all classes share the same namespace, take the fast
                // path (single call, no extra allocation).
                if ns_groups.len() <= 1 {
                    let mut classes: Vec<ClassInfo> =
                        classes_with_ns.iter().map(|(c, _)| c.clone()).collect();
                    Self::resolve_parent_class_names(&mut classes, &use_map, &namespace);
                    // Write back
                    for (i, cls) in classes.into_iter().enumerate() {
                        classes_with_ns[i].0 = cls;
                    }
                } else {
                    // Multi-namespace file: resolve each group with its own
                    // namespace context.
                    for (group_ns, indices) in &ns_groups {
                        let mut group: Vec<ClassInfo> = indices
                            .iter()
                            .map(|&i| classes_with_ns[i].0.clone())
                            .collect();
                        Self::resolve_parent_class_names(&mut group, &use_map, group_ns);
                        for (j, &idx) in indices.iter().enumerate() {
                            classes_with_ns[idx].0 = group[j].clone();
                        }
                    }
                }
            }

            // Separate the classes from their namespace tags for storage,
            // stamping each ClassInfo with its namespace so that
            // `find_class_in_uri_classes_index` can distinguish classes with the same
            // short name in different namespace blocks.
            let classes: Vec<ClassInfo> = classes_with_ns
                .iter()
                .map(|(c, ns)| {
                    let mut cls = c.clone();
                    cls.file_namespace = ns.as_deref().map(atom);
                    cls.cache_fqn();
                    // Keyed by FQN, so this has to wait until the class
                    // carries one.
                    crate::stub_patches::apply_third_party_class_patches(&mut cls);
                    cls
                })
                .collect();

            // Build the precomputed symbol map while the AST is still alive.
            // This must happen before the `Program` (and its arena) are dropped.
            let symbol_map = Arc::new(extract_symbol_map(program, content));

            // For files without any explicit namespace blocks, synthesize a
            // single span covering the entire file with the detected namespace
            // (which will be None for files without namespace declarations).
            if namespace_spans.is_empty() {
                namespace_spans.push(NamespaceSpan {
                    namespace: namespace.clone(),
                    start: 0,
                    end: content.len() as u32,
                });
            }

            AstIndexUpdate {
                uri: uri.to_string(),
                parse_errors,
                classes,
                use_map,
                resolved_names: Arc::new(owned_resolved),
                namespace_spans,
                functions,
                defines,
                symbol_map,
            }
        })
    }

    pub(crate) fn apply_ast_index_updates_batch(&self, updates: Vec<AstIndexUpdate>) -> bool {
        if updates.is_empty() {
            return false;
        }

        struct PreparedAstIndexUpdate {
            uri: String,
            parse_errors: Vec<ParseErrorEntry>,
            old_classes: Vec<ClassInfo>,
            old_fqns: Vec<String>,
            new_fqns: Vec<String>,
            classes: Vec<Arc<ClassInfo>>,
            use_map: HashMap<String, String>,
            resolved_names: Arc<OwnedResolvedNames>,
            namespace_spans: Vec<NamespaceSpan>,
            functions: Vec<FunctionInfo>,
            defines: Vec<(String, DefineInfo)>,
            symbol_map: Arc<SymbolMap>,
            old_function_fqns: Vec<String>,
            old_define_names: Vec<String>,
            new_function_fqns: Vec<String>,
            new_define_names: Vec<String>,
        }

        let old_classes_by_update: Vec<Vec<ClassInfo>> = {
            let uri_classes = self.symbols.uri_classes_index.read();
            updates
                .iter()
                .map(|update| {
                    uri_classes
                        .get(&update.uri)
                        .map(|classes| {
                            classes
                                .iter()
                                .map(|class| ClassInfo::clone(class))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        };

        // Recall the standalone functions and defines each file contributed
        // on its previous parse so that symbols an edit deleted or renamed
        // can be evicted from the global maps.  Without this, deleting or
        // renaming a `function foo()` or `define('X', …)` leaves the old
        // entry behind for the whole session (stale completion, hover, and
        // go-to-definition).
        let old_globals_by_update: Vec<(Vec<String>, Vec<String>)> = {
            let uri_globals = self.symbols.uri_globals_index.read();
            updates
                .iter()
                .map(|update| uri_globals.get(&update.uri).cloned().unwrap_or_default())
                .collect()
        };

        let mut prepared = Vec::with_capacity(updates.len());
        let mut all_old_fqns = Vec::new();
        let mut all_new_fqns = Vec::new();
        let mut all_classes = Vec::new();

        for ((update, old_classes), (old_function_fqns, old_define_names)) in updates
            .into_iter()
            .zip(old_classes_by_update)
            .zip(old_globals_by_update)
        {
            let old_fqns: Vec<String> = old_classes
                .iter()
                .filter(|class| !class.name.starts_with("__anonymous@"))
                .map(class_info_fqn)
                .collect();
            let classes: Vec<Arc<ClassInfo>> = update.classes.into_iter().map(Arc::new).collect();
            let new_fqns: Vec<String> = classes
                .iter()
                .filter(|class| !class.name.starts_with("__anonymous@"))
                .map(|class| class.fqn().to_string())
                .collect();

            all_old_fqns.extend(old_fqns.iter().cloned());
            all_new_fqns.extend(new_fqns.iter().cloned());
            all_classes.extend(classes.iter().cloned());

            prepared.push(PreparedAstIndexUpdate {
                uri: update.uri,
                parse_errors: update.parse_errors,
                old_classes,
                old_fqns,
                new_fqns,
                classes,
                use_map: update.use_map,
                resolved_names: update.resolved_names,
                namespace_spans: update.namespace_spans,
                functions: update.functions,
                defines: update.defines,
                symbol_map: update.symbol_map,
                old_function_fqns,
                old_define_names,
                new_function_fqns: Vec::new(),
                new_define_names: Vec::new(),
            });
        }

        all_old_fqns.sort();
        all_old_fqns.dedup();
        all_new_fqns.sort();
        all_new_fqns.dedup();

        {
            let mut parse_errors = self.parse_errors.write();
            for update in &mut prepared {
                parse_errors.insert(update.uri.clone(), std::mem::take(&mut update.parse_errors));
            }
        }

        // Names this batch took over from a file that previously won them.
        // Their entries in the fqn-keyed derived indexes below belong to the
        // old owner and have to be cleared; every other name this batch
        // declares either had no owner (nothing to clear) or kept the one it
        // had.  Collecting them keeps first-parse indexing off the per-fqn
        // full-map scan that eviction costs.
        let mut reowned_fqns: Vec<String> = Vec::new();

        self.symbols.with_class_declarations(|decls| {
            // Withdraw the declarations this parse no longer makes.  A name
            // another file also declares is handed to that file rather than
            // disappearing with this one; `all_old_fqns` mixes together
            // every URI's prior declarations, so dropping by fqn alone would
            // also delete a different file's still-valid entry whenever a
            // losing duplicate gets reparsed on its own.
            for update in &prepared {
                for old_fqn in &update.old_fqns {
                    if update.new_fqns.contains(old_fqn) {
                        continue;
                    }
                    decls.withdraw(old_fqn, &update.uri);
                }
            }

            // A package may declare the same class in more than one file
            // behind a `class_exists` guard — Carbon ships an empty
            // `DatePeriodBase` alongside one that carries the pre-8.2
            // properties.  Both get declared, and the lowest-sorting URI
            // wins, instead of whichever parse worker happened to finish
            // last deciding the members the name resolves to (and the
            // diagnostics that follow) anew on every run.
            for update in &prepared {
                for class in &update.classes {
                    if class.name.starts_with("__anonymous@") {
                        continue;
                    }
                    let fqn = class.fqn();
                    if decls.declare(&fqn, &update.uri, class).reowned {
                        reowned_fqns.push(fqn.to_string());
                    }
                }
            }
        });
        {
            let nf_cache = self.symbols.class_not_found_cache.read();
            if !nf_cache.is_empty() {
                drop(nf_cache);
                let mut nf_cache = self.symbols.class_not_found_cache.write();
                for fqn in &all_new_fqns {
                    nf_cache.remove(fqn);
                }
            }
        }

        // Retire memoised lookups: this parse repointed every FQN the file
        // declares, dropped the ones it no longer does, and un-cached their
        // negatives.  It has to come after all three, or a lookup that read
        // the new generation could still see a negative about to be cleared
        // and memoise it as fresh.
        self.symbols.note_class_lookup_change();

        // Only touch a file's function entries when this parse contributes
        // functions or the previous parse did (so removals can be evicted).
        // The common case — a class file with no standalone functions that
        // never had any — skips the snapshot scan below entirely.
        let mut any_function_changed = false;
        {
            let mut fmap = self.symbols.global_functions.write();
            let mut dupes = self.symbols.duplicate_functions.write();
            for update in &mut prepared {
                if update.functions.is_empty() && update.old_function_fqns.is_empty() {
                    continue;
                }

                // Snapshot the functions this file contributed last parse,
                // so we can detect signature changes and trigger cross-file
                // diagnostic invalidation.  Reading the recorded name list
                // rather than scanning the whole map by URI also finds the
                // declarations this file was outvoted on.
                let old_functions: Vec<(String, FunctionInfo)> = update
                    .old_function_fqns
                    .iter()
                    .filter_map(|fqn| {
                        declared_function(&fmap, &dupes, fqn, &update.uri)
                            .map(|info| (fqn.clone(), info.clone()))
                    })
                    .collect();

                // Withdraw this file's previous declarations so that
                // renamed/deleted functions don't linger.  A name another
                // file also declares survives on that file's declaration.
                for (old_fqn, _) in &old_functions {
                    withdraw_function(&mut fmap, &mut dupes, old_fqn, &update.uri);
                }

                for func_info in std::mem::take(&mut update.functions) {
                    let fqn = if let Some(ref ns) = func_info.namespace {
                        format!("{}\\{}", ns, func_info.name)
                    } else {
                        func_info.name.to_string()
                    };

                    // Skip a declaration the embedded stubs already own, so
                    // the stub's signature, return type, generics and
                    // deprecation status are what hover, completion and
                    // diagnostics see.
                    //
                    // Two shapes reach here.  A polyfill — Laravel and
                    // friends wrap helpers such as `str_contains` in
                    // `if (! function_exists('…'))` — is dead code on a PHP
                    // version that ships the native function.  An
                    // *unguarded* redeclaration of a global function the
                    // stubs know is dead code on every version, because PHP
                    // fatals on redeclaring an internal function: the file
                    // is a signature stub written for tooling
                    // (`phpstan/php-8-stubs` ships one PHP file per
                    // builtin), and letting its bare `array` return type
                    // win would strip `array_keys()` of the `@template TKey`
                    // the whole key-type inference rests on.
                    //
                    // An unguarded declaration only loses out in the global
                    // namespace, where a name the stubs carry is
                    // necessarily a PHP internal.  A namespaced collision
                    // (`Brotli\compress`) can be a real implementation, so
                    // there it still takes a `function_exists` guard to
                    // stand down.
                    if (func_info.is_polyfill || !fqn.contains('\\'))
                        && self.stub_function_index.read().contains_key(fqn.as_str())
                    {
                        continue;
                    }

                    // Check whether this function's signature changed
                    // compared to the previous parse.  A change (or a new
                    // function) means other open files that call it may
                    // have stale diagnostics.
                    //
                    // **First-parse fast path**: when `old_functions` is
                    // empty the file has never been parsed before.  New
                    // functions appearing on first parse are not changes
                    // — they mirror the class first-parse fast path.
                    if !any_function_changed && !old_functions.is_empty() {
                        match old_functions
                            .iter()
                            .find(|(f, _)| f.eq_ignore_ascii_case(&fqn))
                        {
                            Some((_, old_info)) => {
                                if !old_info.signature_eq(&func_info) {
                                    any_function_changed = true;
                                }
                            }
                            None => {
                                // New function — may affect callers.
                                any_function_changed = true;
                            }
                        }
                    }

                    // Insert under the FQN only.  For namespaced functions
                    // the FQN is `Namespace\name`; for global functions it
                    // is just the bare name.  `resolve_function_name` already
                    // builds namespace-qualified candidates, so a short-name
                    // fallback entry is unnecessary and would cause collisions
                    // when two namespaces define the same short name.
                    update.new_function_fqns.push(fqn.clone());
                    declare_function(&mut fmap, &mut dupes, fqn, &update.uri, func_info);
                }

                // A function was removed from this file — callers may
                // now reference an unknown function.
                if !any_function_changed
                    && !old_functions.is_empty()
                    && update.new_function_fqns.len() != old_functions.len()
                {
                    any_function_changed = true;
                }
            }
        }

        {
            let mut dmap = self.symbols.global_defines.write();
            for update in &mut prepared {
                if update.defines.is_empty() && update.old_define_names.is_empty() {
                    continue;
                }

                for (name, define) in std::mem::take(&mut update.defines) {
                    // Overwrite rather than `or_insert_with` so edits to an
                    // existing `define`/`const` propagate: changing the value
                    // updates hover, and inserting lines above it updates the
                    // go-to-definition offset.
                    update.new_define_names.push(name.clone());
                    dmap.insert(name, define);
                }

                // Evict names this file used to contribute but no longer
                // does, guarding on the stored URI so a constant redefined
                // in another file is not clobbered.
                for old_name in &update.old_define_names {
                    if update.new_define_names.contains(old_name) {
                        continue;
                    }
                    if dmap.get(old_name).is_some_and(|d| d.file_uri == update.uri) {
                        dmap.remove(old_name);
                    }
                }
            }
        }

        // Record what each file contributed so the next parse can evict
        // whatever it removes.  Drop the entry entirely when the file has
        // no globals, to avoid accumulating empty records for class files.
        {
            let mut globals_index = self.symbols.uri_globals_index.write();
            for update in &prepared {
                if update.new_function_fqns.is_empty() && update.new_define_names.is_empty() {
                    globals_index.remove(&update.uri);
                } else {
                    globals_index.insert(
                        update.uri.clone(),
                        (
                            update.new_function_fqns.clone(),
                            update.new_define_names.clone(),
                        ),
                    );
                }
            }
        }

        // These two indexes are keyed by fqn, so they have to be refilled
        // from whichever declaration won the tie-break above rather than
        // from this batch's classes.  When a batch reparses (or drops) the
        // losing copy of a duplicated name, the winner is not in
        // `all_classes`, and populating from the batch would leave
        // go-to-implementation disagreeing with the class index about which
        // declaration the name refers to.
        let evict_fqns = if reowned_fqns.is_empty() {
            all_old_fqns.clone()
        } else {
            let mut fqns = all_old_fqns.clone();
            fqns.append(&mut reowned_fqns);
            fqns.sort();
            fqns.dedup();
            fqns
        };

        let winning_classes: Vec<Arc<ClassInfo>> = {
            let fqn_idx = self.symbols.fqn_class_index.read();
            all_new_fqns
                .iter()
                .chain(
                    evict_fqns
                        .iter()
                        .filter(|fqn| all_new_fqns.binary_search(fqn).is_err()),
                )
                .filter_map(|fqn| fqn_idx.get(fqn).map(Arc::clone))
                .chain(
                    all_classes
                        .iter()
                        .filter(|class| class.name.starts_with("__anonymous@"))
                        .map(Arc::clone),
                )
                .collect()
        };

        self.evict_methods_for_fqns(&evict_fqns);
        self.evict_gti_for_fqns(&evict_fqns);
        self.populate_method_store(&winning_classes);
        self.populate_gti_index(&winning_classes);

        // Selectively invalidate the resolved-class cache with
        // signature-level granularity.  Full indexing usually hits the
        // first-parse fast path (`old_fqns` is empty), so this stays cheap
        // during background indexing while preserving edit-time semantics.
        let mut any_signature_changed = false;
        let mut evicted_fqns = Vec::new();
        {
            let mut cache = self.resolved_class_cache.write();
            for update in &prepared {
                if update.old_fqns.is_empty() {
                    continue;
                }

                for fqn in &update.old_fqns {
                    let old_cls = update
                        .old_classes
                        .iter()
                        .find(|class| class_info_fqn(class) == *fqn);
                    let new_cls = update
                        .classes
                        .iter()
                        .find(|class| class.fqn().as_str() == fqn);

                    match (old_cls, new_cls) {
                        (Some(old), Some(new)) if old.signature_eq(new) => {}
                        _ => {
                            evicted_fqns.extend(crate::virtual_members::evict_fqn(&mut cache, fqn));
                            any_signature_changed = true;
                        }
                    }
                }

                for fqn in &update.new_fqns {
                    if !update.old_fqns.contains(fqn) {
                        evicted_fqns.extend(crate::virtual_members::evict_fqn(&mut cache, fqn));
                        any_signature_changed = true;
                    }
                }
            }
        }
        evicted_fqns.sort();
        evicted_fqns.dedup();

        {
            let mut uri_classes = self.symbols.uri_classes_index.write();
            let mut parsed_uris = self.parsed_uris.write();
            for update in &mut prepared {
                uri_classes.insert(update.uri.clone(), std::mem::take(&mut update.classes));
                parsed_uris.insert(update.uri.clone());
            }
        }

        {
            let mut imports = self.file_imports.write();
            let mut resolved_names = self.resolved_names.write();
            let mut namespaces = self.file_namespaces.write();
            for update in &mut prepared {
                imports.insert(update.uri.clone(), std::mem::take(&mut update.use_map));
                resolved_names.insert(update.uri.clone(), Arc::clone(&update.resolved_names));
                namespaces.insert(
                    update.uri.clone(),
                    std::mem::take(&mut update.namespace_spans),
                );
            }
        }

        if !evicted_fqns.is_empty() {
            let sorted = {
                let uri_classes = self.symbols.uri_classes_index.read();
                let iter = uri_classes
                    .values()
                    .flat_map(|classes| classes.iter())
                    .filter(|class| evicted_fqns.contains(&class.fqn().to_string()))
                    .map(|class| (class.fqn().to_string(), class.as_ref()));
                crate::toposort::toposort_classes(iter)
            };

            let class_loader =
                |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
            crate::virtual_members::populate_from_sorted(
                &sorted,
                &self.resolved_class_cache,
                &class_loader,
            );
        }

        let changed = any_signature_changed || any_function_changed;

        if changed {
            self.member_completion_cache.lock().clear();
            // Exact member targets in other files may depend on the return or
            // property type that changed here. Rebuild those files lazily;
            // the edited file itself is evicted by reference reindexing below.
            self.clear_resolved_member_files();
            // A receiver's type is settled against the classes of the whole
            // workspace, so a signature change anywhere can turn a call that
            // was not a render into one, or the other way round.
            self.typed_receiver_view_spans_cache.write().clear();
        } else {
            for update in &prepared {
                self.evict_typed_receiver_view_spans(&update.uri);
            }
        }

        let reference_items: Vec<(String, Arc<SymbolMap>)> = prepared
            .iter()
            .map(|update| (update.uri.clone(), Arc::clone(&update.symbol_map)))
            .collect();
        self.reindex_references_for_symbol_maps_batch(reference_items);

        {
            let mut symbol_maps = self.symbol_maps.write();
            for update in prepared {
                symbol_maps.insert(update.uri, update.symbol_map);
            }
        }

        changed
    }

    /// Resolve `parent_class` short names in a list of `ClassInfo` to
    /// fully-qualified names using the file's `use_map` and `namespace`.
    ///
    /// Rules (matching PHP name resolution):
    ///   1. Already fully-qualified (`\Foo\Bar`) → strip leading `\`
    ///   2. Qualified (`Foo\Bar`) → if first segment is in use_map, expand it;
    ///      otherwise prepend current namespace
    ///   3. Unqualified (`Bar`) → check use_map; otherwise prepend namespace
    ///   4. No namespace and not in use_map → keep as-is
    pub fn resolve_parent_class_names(
        classes: &mut [ClassInfo],
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
    ) {
        // Collect type alias names from ALL classes in the file up-front.
        // A type alias defined on one class can be referenced from methods
        // in a different class in the same file, so we must skip all of
        // them to avoid mangling alias names into FQN form.
        let all_alias_names: Vec<Atom> = classes
            .iter()
            .flat_map(|c| c.type_aliases.keys().copied())
            .collect();

        for class in classes.iter_mut() {
            if let Some(ref parent) = class.parent_class {
                let resolved = Self::resolve_name(parent, use_map, namespace);
                class.parent_class = Some(atom(&resolved));
            }
            class.used_traits = class
                .used_traits
                .iter()
                .map(|t| atom(&Self::resolve_name(t, use_map, namespace)))
                .collect();

            class.interfaces = class
                .interfaces
                .iter()
                .map(|i| atom(&Self::resolve_name(i, use_map, namespace)))
                .collect();

            // Resolve the `@phpstan-require-extends` base class to its
            // fully-qualified name so it is loadable cross-file.
            if let Some(ref required) = class.require_extends {
                class.require_extends =
                    Some(atom(&Self::resolve_name(required, use_map, namespace)));
            }

            class.require_implements = class
                .require_implements
                .iter()
                .map(|i| atom(&Self::resolve_name(i, use_map, namespace)))
                .collect();

            // Resolve trait names in `insteadof` precedence adaptations
            for prec in &mut class.trait_precedences {
                prec.trait_name = atom(&Self::resolve_name(&prec.trait_name, use_map, namespace));
                prec.insteadof = prec
                    .insteadof
                    .iter()
                    .map(|t| atom(&Self::resolve_name(t, use_map, namespace)))
                    .collect();
            }

            // Resolve trait names in `as` alias adaptations
            for alias in &mut class.trait_aliases {
                if let Some(ref t) = alias.trait_name {
                    alias.trait_name = Some(atom(&Self::resolve_name(t, use_map, namespace)));
                }
            }

            // Resolve mixin names to fully-qualified names.
            // Skip names that match a template parameter — these are
            // not class names but placeholders that will be substituted
            // with concrete types when the generic class is instantiated
            // (e.g. `@template TWraps` + `@mixin TWraps`).
            class.mixins = class
                .mixins
                .iter()
                .map(|m| {
                    if class.template_params.contains(m) {
                        *m
                    } else {
                        atom(&Self::resolve_name(m, use_map, namespace))
                    }
                })
                .collect();

            if let Some(laravel) = class.laravel.as_deref_mut() {
                let resolver =
                    |name: &str| -> String { Self::resolve_name(name, use_map, namespace) };

                // Resolve custom collection class name to FQN.
                if let Some(collection) = laravel.custom_collection.take() {
                    laravel.custom_collection = Some(collection.resolve_names(&resolver));
                }

                // Resolve custom builder class name to FQN.
                if let Some(builder) = laravel.custom_builder.take() {
                    laravel.custom_builder = Some(builder.resolve_names(&resolver));
                }

                // Resolve a Laravel factory's explicitly configured model
                // class to an FQN. Class-constant initializers follow the
                // file's imports and namespace; literal class strings were
                // marked absolute while being extracted and therefore remain
                // unchanged.
                if let Some(model) = laravel.factory_model.take() {
                    laravel.factory_model = Some(model.resolve_names(&resolver));
                }
            }

            // Resolve a facade's `getFacadeAccessor()` class reference to an
            // FQN so the concrete class it forwards to is loadable from any
            // file, not just the one that imported it.
            if let Some(crate::types::FacadeAccessor::Class(written)) =
                class.laravel().and_then(|l| l.facade_accessor)
            {
                let resolved = atom(&Self::resolve_name(&written, use_map, namespace));
                class.laravel_mut().facade_accessor =
                    Some(crate::types::FacadeAccessor::Class(resolved));
            }
            // Resolve the `#[UsePolicy]` class name to an FQN so the policy is
            // loadable cross-file.
            if let Some(policy) = class.laravel().and_then(|l| l.policy_class.clone()) {
                class.laravel_mut().policy_class =
                    Some(Self::resolve_name(&policy, use_map, namespace));
            }

            // Resolve custom pivot class names (`->using(X::class)`) to FQNs so
            // that hover shows the fully-qualified pivot class and it is
            // loadable cross-file.
            if class
                .laravel()
                .is_some_and(|l| l.belongs_to_many_pivots.iter().any(|p| p.using.is_some()))
            {
                for pivot in class.laravel_mut().belongs_to_many_pivots.iter_mut() {
                    if let Some(using) = &pivot.using {
                        pivot.using = Some(Self::resolve_name(using, use_map, namespace));
                    }
                }
            }

            // Resolve cast class names to FQN so that custom cast
            // classes like `DecimalCast` (imported via `use`) are
            // loadable cross-file when `cast_type_to_php_type` calls
            // the class loader.
            {
                let casts: Vec<(String, String)> = class
                    .laravel()
                    .map(|l| l.casts_definitions.clone())
                    .unwrap_or_default();
                if !casts.is_empty() {
                    let resolved: Vec<(String, String)> = casts
                        .into_iter()
                        .map(|(col, cast_type)| {
                            // Only resolve class-like cast types (not
                            // built-in strings like "boolean", "datetime",
                            // etc.).  A simple heuristic: if the value
                            // contains an uppercase letter and is not a
                            // known built-in, treat it as a class name.
                            //
                            // Skip names that already contain a `\` — they
                            // are already qualified (e.g. the string literal
                            // `'App\Casts\HtmlCast'`).  Passing them through
                            // `resolve_name` would prepend the file's
                            // namespace, producing a broken FQN like
                            // `App\Models\App\Casts\HtmlCast`.
                            let first_segment = cast_type.split(':').next().unwrap_or(&cast_type);
                            if first_segment.contains('\\') || first_segment.starts_with('\\') {
                                // Already qualified — strip leading `\` if present to produce canonical FQN.
                                let canonical = cast_type
                                    .strip_prefix('\\')
                                    .map_or(cast_type.clone(), |s| s.to_string());
                                (col, canonical)
                            } else if first_segment.chars().any(|c| c.is_ascii_uppercase()) {
                                let resolved_class =
                                    Self::resolve_name(first_segment, use_map, namespace);
                                if resolved_class != first_segment {
                                    // Re-attach any `:argument` suffix.
                                    let suffix = &cast_type[first_segment.len()..];
                                    (col, format!("{resolved_class}{suffix}"))
                                } else {
                                    (col, cast_type)
                                }
                            } else {
                                (col, cast_type)
                            }
                        })
                        .collect();
                    class.laravel_mut().casts_definitions = resolved;
                }
            }

            // Resolve type arguments in @extends, @implements, and @use
            // generics so that after generic substitution, return types
            // and property types are fully-qualified and can be resolved
            // across files via PSR-4.
            //
            // Template params of the current class must be skipped so
            // that forwarded params (e.g. `@use BuildsQueries<TModel>`
            // where TModel is a class-level template) remain as bare
            // names and match substitution map keys later.
            let tpl_params: Vec<String> = class
                .template_params
                .iter()
                .map(|a| a.to_string())
                .collect();
            Self::resolve_generics_type_args(
                &mut class.extends_generics,
                use_map,
                namespace,
                &tpl_params,
            );
            Self::resolve_generics_type_args(
                &mut class.implements_generics,
                use_map,
                namespace,
                &tpl_params,
            );
            Self::resolve_generics_type_args(
                &mut class.use_generics,
                use_map,
                namespace,
                &tpl_params,
            );
            Self::resolve_generics_type_args(
                &mut class.mixin_generics,
                use_map,
                namespace,
                &tpl_params,
            );

            // Resolve template parameter bounds (`@template T of Bound`)
            // so that short names like `PDependNode` become FQNs like
            // `PDepend\Source\AST\ASTNode`.  Without this, mixin
            // resolution that falls back to bounds gets unresolvable
            // short names.
            {
                let bound_resolver = Self::build_type_resolver(use_map, namespace, &tpl_params);
                for bound in class.template_param_bounds.values_mut() {
                    let resolved = bound.resolve_names(&bound_resolver);
                    if resolved != *bound {
                        *bound = resolved;
                    }
                }
            }

            // Resolve class-like names in method return types and property
            // type hints so that cross-file resolution works correctly.
            // For example, if a method returns `Country` and the file has
            // `use Acme\Core\Enums\Country`, the return type becomes
            // the FQN `Acme\Core\Enums\Country`.
            //
            // Template params and type alias names are excluded to avoid
            // mangling generic types and locally-defined type aliases.
            // We collect alias names from ALL classes in the file because
            // a type alias defined on one class may be referenced from a
            // method in a different class in the same file.
            let template_params = &class.template_params;
            let skip_names: Vec<String> = template_params
                .iter()
                .map(|a| a.to_string())
                .chain(all_alias_names.iter().map(|a| a.to_string()))
                .collect();
            let resolver = Self::build_type_resolver(use_map, namespace, &skip_names);

            // Also resolve class-like names inside type alias definitions
            // so that `@phpstan-type ActiveUser User` where `User` is
            // imported via `use App\Models\User` becomes `App\Models\User`.
            for def in class.type_aliases.values_mut() {
                match def {
                    TypeAliasDef::Import { source_class, .. } => {
                        // Imported alias — resolve the source class name.
                        let resolved_class = Self::resolve_name(source_class, use_map, namespace);
                        if resolved_class != *source_class {
                            *source_class = resolved_class;
                        }
                    }
                    TypeAliasDef::Local(php_type) => {
                        // Local alias — resolve class names within the type.
                        let resolved = php_type.resolve_names(&resolver);
                        *php_type = resolved;
                    }
                }
            }

            for method in class.methods.make_mut() {
                let method = Arc::make_mut(method);
                // Build a per-method skip list that includes both class-level
                // and method-level template params so that names like `T` in
                // `@return Collection<T>` are not namespace-resolved.
                //
                // When the method has its own template params, build a
                // per-method resolver that skips them in addition to the
                // class-level skip names.  Otherwise reuse the class-level
                // resolver.
                let method_skip: Vec<String>;
                let method_resolver: &dyn Fn(&str) -> String = if method.template_params.is_empty()
                {
                    &resolver
                } else {
                    method_skip = skip_names
                        .iter()
                        .cloned()
                        .chain(method.template_params.iter().map(|a| a.to_string()))
                        .collect();
                    // SAFETY: `method_skip` lives until end of this
                    // `for method` iteration, so the closure is valid.
                    &Self::build_type_resolver(use_map, namespace, &method_skip)
                };

                if let Some(ref ret) = method.return_type {
                    let resolved = ret.resolve_names(method_resolver);
                    if resolved != *ret {
                        method.return_type = Some(resolved);
                    }
                }
                if let Some(ref cond) = method.conditional_return {
                    let resolved = cond.resolve_names(method_resolver);
                    if resolved != *cond {
                        method.conditional_return = Some(resolved);
                    }
                }
                for param in method.parameters.make_mut() {
                    if let Some(ref hint) = param.type_hint {
                        let resolved = hint.resolve_names(method_resolver);
                        if resolved != *hint {
                            param.type_hint = Some(resolved);
                        }
                    }
                    // `@param-closure-this` names a class the same way the
                    // rest of the docblock does, so it is resolved against
                    // the declaring file's imports here.  The call site,
                    // which is where the closure's `$this` is read, has no
                    // access to this file's use-map.
                    if let Some(ref this_type) = param.closure_this_type {
                        let resolved = this_type.resolve_names(method_resolver);
                        if resolved != *this_type {
                            param.closure_this_type = Some(resolved);
                        }
                    }
                }
                // Resolve exception class names in @throws tags.
                for throw in &mut method.throws {
                    let resolved = throw.resolve_names(method_resolver);
                    if resolved != *throw {
                        *throw = resolved;
                    }
                }
                // `@psalm-if-this-is` and `@psalm-this-out` are matched
                // against the receiver's type, and the method's own
                // `@template T of Bound` bounds are matched against the
                // concrete types filling them.  Both sides of those
                // comparisons have to be spelled the same way, and the
                // receiver arrives fully qualified.
                if let Some(ref pattern) = method.if_this_is {
                    let resolved = pattern.resolve_names(method_resolver);
                    if resolved != *pattern {
                        method.if_this_is = Some(resolved);
                    }
                }
                if let Some(ref self_out) = method.self_out {
                    let resolved = self_out.resolve_names(method_resolver);
                    if resolved != *self_out {
                        method.self_out = Some(resolved);
                    }
                }
                for bound in method.template_param_bounds.values_mut() {
                    let resolved = bound.resolve_names(method_resolver);
                    if resolved != *bound {
                        *bound = resolved;
                    }
                }
            }
            for prop in class.properties.make_mut() {
                let Some(hint) = prop.type_hint.as_ref() else {
                    continue;
                };
                let resolved = hint.resolve_names(&resolver);
                if resolved != *hint {
                    Arc::make_mut(prop).type_hint = Some(resolved);
                }
            }

            // Resolve type names inside `@property` / `@property-read` /
            // `@property-write` and `@method` tags.  Their type strings use
            // short names relative to the declaring file's imports, and a
            // cross-file consumer has no access to this file's use-map.
            if let Some(doc) = class.doc_members.as_ref()
                && let Some(resolved) =
                    Self::resolve_doc_member_types(doc, use_map, namespace, &skip_names, &resolver)
            {
                class.doc_members = Some(resolved);
            }
        }
    }

    /// Resolve short class names in the `@property` and `@method` tags that
    /// were parsed out of a class-level docblock.
    ///
    /// The tag types are written against the declaring file's imports, so
    /// they are qualified here while that use-map is still in scope.  A
    /// consumer in another file only ever sees the resolved form.
    ///
    /// Returns `None` when nothing changed, so the common case keeps
    /// sharing the existing `Arc` rather than rebuilding it.
    fn resolve_doc_member_types(
        doc: &Arc<DocblockMembers>,
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
        skip_names: &[String],
        resolver: &dyn Fn(&str) -> String,
    ) -> Option<Arc<DocblockMembers>> {
        let mut changed = false;

        let properties: Vec<(Atom, Option<PhpType>)> = doc
            .properties
            .iter()
            .map(|(name, ty)| {
                let resolved = ty.as_ref().map(|t| {
                    let r = t.resolve_names(resolver);
                    if r != *t {
                        changed = true;
                    }
                    r
                });
                (*name, resolved)
            })
            .collect();

        let methods: Vec<Arc<MethodInfo>> = doc
            .methods
            .iter()
            .map(|m| {
                // `@method foo<T>(T $x): T` declares its own template
                // params, which must not be namespace-qualified.
                let method_skip: Vec<String>;
                let owned_resolver;
                let method_resolver: &dyn Fn(&str) -> String = if m.template_params.is_empty() {
                    resolver
                } else {
                    method_skip = skip_names
                        .iter()
                        .cloned()
                        .chain(m.template_params.iter().map(|a| a.to_string()))
                        .collect();
                    owned_resolver = Self::build_type_resolver(use_map, namespace, &method_skip);
                    &owned_resolver
                };

                let mut resolved = (**m).clone();
                let mut method_changed = false;

                if let Some(ref ret) = resolved.return_type {
                    let r = ret.resolve_names(method_resolver);
                    if r != *ret {
                        resolved.return_type = Some(r);
                        method_changed = true;
                    }
                }
                for param in resolved.parameters.make_mut() {
                    if let Some(ref hint) = param.type_hint {
                        let r = hint.resolve_names(method_resolver);
                        if r != *hint {
                            param.native_type_hint = Some(r.clone());
                            param.type_hint = Some(r);
                            method_changed = true;
                        }
                    }
                }
                for bound in resolved.template_param_bounds.values_mut() {
                    let r = bound.resolve_names(method_resolver);
                    if r != *bound {
                        *bound = r;
                        method_changed = true;
                    }
                }

                if method_changed {
                    changed = true;
                    Arc::new(resolved)
                } else {
                    Arc::clone(m)
                }
            })
            .collect();

        changed.then(|| {
            Arc::new(DocblockMembers {
                methods,
                properties,
            })
        })
    }

    /// Resolve type arguments in a generics list (e.g. `@extends`, `@implements`,
    /// `@use`) to fully-qualified names.
    ///
    /// Each entry is `(ClassName, [TypeArg1, TypeArg2, …])`.  The class name
    /// itself is resolved (e.g. `HasFactory` → `App\Concerns\HasFactory`),
    /// and each type argument that looks like a class name (i.e. not a scalar
    /// like `int`, `string`, etc.) is also resolved.
    ///
    /// `skip_names` contains template parameter names that must NOT be
    /// resolved.  Without this, a forwarded template param like `TModel`
    /// in `@use BuildsQueries<TModel>` would be namespace-qualified to
    /// e.g. `Illuminate\Database\Eloquent\TModel`, preventing it from
    /// matching substitution map keys during generic resolution.
    fn resolve_generics_type_args(
        generics: &mut [(Atom, Vec<PhpType>)],
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
        skip_names: &[String],
    ) {
        let resolver = Self::build_type_resolver(use_map, namespace, skip_names);
        for (class_name, type_args) in generics.iter_mut() {
            // Resolve the base class/trait/interface name
            let resolved: String = Self::resolve_name(class_name, use_map, namespace);
            *class_name = atom(&resolved);

            // Resolve each type argument (now PhpType) via resolve_names
            for arg in type_args.iter_mut() {
                let resolved = arg.resolve_names(&resolver);
                if resolved != *arg {
                    *arg = resolved;
                }
            }
        }
    }

    /// Build a resolver closure that resolves class-like names to FQNs,
    /// skipping template parameters, type aliases, and keyword types.
    ///
    /// The returned closure is suitable for passing to
    /// `PhpType::resolve_names()`.  `is_keyword_type` inside `resolve_names`
    /// already handles scalar and keyword types; this closure additionally
    /// skips names in `skip_names` (template params and type alias names).
    fn build_type_resolver<'a>(
        use_map: &'a HashMap<String, String>,
        namespace: &'a Option<String>,
        skip_names: &'a [String],
    ) -> impl Fn(&str) -> String + 'a {
        move |name: &str| {
            if skip_names.iter().any(|s| s == name) {
                return name.to_string();
            }
            Self::resolve_name(name, use_map, namespace)
        }
    }

    /// Resolve a class name to its fully-qualified form given a use_map and
    /// namespace context.
    ///
    /// The returned name is **always without a leading `\`**.  This is the
    /// canonical FQN representation used throughout the codebase.  For
    /// example, `\RuntimeException` is returned as `RuntimeException`, and
    /// `\App\Models\User` as `App\Models\User`.
    fn resolve_name(
        name: &str,
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
    ) -> String {
        // 1. Already fully-qualified — strip the leading `\`.
        if let Some(stripped) = name.strip_prefix('\\') {
            return stripped.to_string();
        }

        // 2/3. Check if the (first segment of the) name is in the use_map
        if let Some(pos) = name.find('\\') {
            // Qualified name — check first segment
            let first = &name[..pos];
            let rest = &name[pos..]; // includes leading '\'
            if let Some(fqn) = use_map.get(first) {
                return format!("{}{}", fqn, rest);
            }
        } else {
            // Unqualified name — check directly
            if let Some(fqn) = use_map.get(name) {
                return fqn.clone();
            }
        }

        // 4. Prepend current namespace if available.
        if let Some(ns) = namespace {
            format!("{}\\{}", ns, name)
        } else {
            name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Backend;

    /// Changing a function's parameter type should cause `update_ast` to
    /// return `true` (signature changed), triggering cross-file
    /// diagnostic invalidation.  This is the exact scenario from
    /// GitHub issue #123.
    #[test]
    fn update_ast_detects_function_param_type_change() {
        let backend = Backend::new_test();
        let uri = "file:///test2.php";

        let v1 = "<?php\nfunction bar(null $x) {\n    return $x;\n}\n";
        let changed = backend.update_ast(uri, v1);
        // First parse — no old functions to compare against.
        assert!(!changed, "First parse should not report a change");

        let v2 = "<?php\nfunction bar(string $x) {\n    return $x;\n}\n";
        let changed = backend.update_ast(uri, v2);
        assert!(
            changed,
            "Changing parameter type null→string must be detected"
        );
    }

    /// Changing a function's return type should be detected.
    #[test]
    fn update_ast_detects_function_return_type_change() {
        let backend = Backend::new_test();
        let uri = "file:///helpers.php";

        let v1 = "<?php\nfunction helper(): int {\n    return 42;\n}\n";
        backend.update_ast(uri, v1);

        let v2 = "<?php\nfunction helper(): string {\n    return 'hello';\n}\n";
        let changed = backend.update_ast(uri, v2);
        assert!(changed, "Changing return type int→string must be detected");
    }

    /// Changing only the function body (not the signature) should NOT
    /// trigger cross-file invalidation.
    #[test]
    fn update_ast_ignores_function_body_change() {
        let backend = Backend::new_test();
        let uri = "file:///helpers.php";

        let v1 = "<?php\nfunction helper(int $x): int {\n    return $x + 1;\n}\n";
        backend.update_ast(uri, v1);

        let v2 = "<?php\nfunction helper(int $x): int {\n    return $x + 2;\n}\n";
        let changed = backend.update_ast(uri, v2);
        assert!(
            !changed,
            "Body-only change should not report a signature change"
        );
    }

    /// Adding a new function should be detected as a change.
    #[test]
    fn update_ast_detects_new_function() {
        let backend = Backend::new_test();
        let uri = "file:///helpers.php";

        let v1 = "<?php\nfunction foo(): void {}\n";
        backend.update_ast(uri, v1);

        let v2 = "<?php\nfunction foo(): void {}\nfunction bar(): void {}\n";
        let changed = backend.update_ast(uri, v2);
        assert!(changed, "Adding a new function must be detected");
    }

    /// Removing a function should be detected as a change.
    #[test]
    fn update_ast_detects_removed_function() {
        let backend = Backend::new_test();
        let uri = "file:///helpers.php";

        let v1 = "<?php\nfunction foo(): void {}\nfunction bar(): void {}\n";
        backend.update_ast(uri, v1);

        let v2 = "<?php\nfunction foo(): void {}\n";
        let changed = backend.update_ast(uri, v2);
        assert!(changed, "Removing a function must be detected");
    }

    /// Adding a parameter to a function should be detected.
    #[test]
    fn update_ast_detects_added_parameter() {
        let backend = Backend::new_test();
        let uri = "file:///helpers.php";

        let v1 = "<?php\nfunction greet(string $name): string {\n    return $name;\n}\n";
        backend.update_ast(uri, v1);

        let v2 = "<?php\nfunction greet(string $name, string $greeting = 'Hello'): string {\n    return \"$greeting $name\";\n}\n";
        let changed = backend.update_ast(uri, v2);
        assert!(changed, "Adding a parameter must be detected");
    }

    /// Verify that stale function entries are cleaned up when a file
    /// is re-parsed without the function.
    #[test]
    fn update_ast_cleans_up_stale_functions() {
        let backend = Backend::new_test();
        let uri = "file:///helpers.php";

        let v1 = "<?php\nfunction old_helper(): void {}\n";
        backend.update_ast(uri, v1);
        assert!(
            backend
                .symbols
                .global_functions
                .read()
                .get("old_helper")
                .is_some(),
            "Function should be registered after first parse"
        );

        let v2 = "<?php\n// function removed\n";
        backend.update_ast(uri, v2);
        assert!(
            backend
                .symbols
                .global_functions
                .read()
                .get("old_helper")
                .is_none(),
            "Stale function should be removed after re-parse"
        );
    }

    /// Class signature changes should still be detected (regression guard).
    #[test]
    fn update_ast_still_detects_class_signature_change() {
        let backend = Backend::new_test();
        let uri = "file:///MyClass.php";

        let v1 = "<?php\nclass MyClass {\n    public function foo(): int { return 1; }\n}\n";
        backend.update_ast(uri, v1);

        let v2 = "<?php\nclass MyClass {\n    public function foo(): string { return 'a'; }\n}\n";
        let changed = backend.update_ast(uri, v2);
        assert!(
            changed,
            "Class method return type change must still be detected"
        );
    }

    /// A `belongsToMany` body with `->as(...)`, `->using(...)`, and
    /// `->withPivot(...)` must populate `belongs_to_many_pivots`, with the
    /// pivot class resolved to an FQN.
    #[test]
    fn parses_pivot_accessor_using_and_columns_from_relationship_body() {
        let backend = Backend::new_test();
        let uri = "file:///app/Models/User.php";
        let content = "<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany;
class User extends Model {
    /** @return BelongsToMany<Role, $this> */
    public function roles(): BelongsToMany {
        return $this->belongsToMany(Role::class)->as('participation')->using(RoleUser::class)->withPivot('expires_at', 'active');
    }
}
";
        backend.update_ast(uri, content);
        let classes = backend.symbols.uri_classes_index.read();
        let user = classes
            .get(uri)
            .and_then(|c| c.iter().find(|c| c.name == "User"))
            .expect("User class should be indexed");
        let pivots = &user
            .laravel()
            .expect("User should have Laravel metadata")
            .belongs_to_many_pivots;
        assert_eq!(pivots.len(), 1, "one pivot relation, got: {pivots:?}");
        assert_eq!(pivots[0].method, "roles");
        assert_eq!(
            pivots[0].accessor,
            crate::types::PivotAccessor::Custom(crate::atom::atom("participation"))
        );
        assert_eq!(
            pivots[0].using.as_deref(),
            Some("App\\Models\\RoleUser"),
            "using() class should be resolved to an FQN"
        );
        assert_eq!(pivots[0].columns, vec!["expires_at", "active"]);
    }

    #[test]
    fn marks_dynamic_pivot_accessor_as_unknown() {
        let backend = Backend::new_test();
        let uri = "file:///app/Models/User.php";
        let content = "<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany;
class User extends Model {
    /** @return BelongsToMany<Role, $this> */
    public function roles(string $accessor): BelongsToMany {
        return $this->belongsToMany(Role::class)->as($accessor);
    }
}
";
        backend.update_ast(uri, content);
        let classes = backend.symbols.uri_classes_index.read();
        let user = classes
            .get(uri)
            .and_then(|c| c.iter().find(|c| c.name == "User"))
            .expect("User class should be indexed");
        let pivot = user
            .laravel()
            .and_then(|laravel| laravel.belongs_to_many_pivots.first())
            .expect("dynamic as() should still record pivot metadata");
        assert_eq!(pivot.accessor, crate::types::PivotAccessor::Unknown);
    }

    /// A background/workspace parse batch must not clobber the state of a
    /// file that is currently open in the editor. The open buffer's own
    /// `update_ast` has already published newer (post-edit) state; the
    /// batch result was parsed from stale disk content and must be skipped.
    #[test]
    fn batch_publish_skips_open_files() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Widget.php";

        // The editor buffer has been edited: method `edited` exists.
        let edited = "<?php\nclass Widget {\n    public function edited(): int { return 1; }\n}\n";
        backend.update_ast(uri, edited);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(edited.to_string()));

        // A background index parses the file straight from disk, where it
        // still has the pre-edit method `stale`.
        let stale = "<?php\nclass Widget {\n    public function stale(): int { return 1; }\n}\n";
        let result = backend.parse_ast_index_update_for_index(uri, stale);
        backend.apply_ast_index_parse_results_batch(vec![result]);

        // The open buffer's state must survive: the class still exposes
        // `edited`, not the stale `stale`.
        let classes = backend.symbols.uri_classes_index.read();
        let widget = classes
            .get(uri)
            .and_then(|c| c.first())
            .expect("Widget class should still be indexed");
        assert!(
            widget.methods.iter().any(|m| m.name == "edited"),
            "open buffer's edited method must survive the batch publish"
        );
        assert!(
            !widget.methods.iter().any(|m| m.name == "stale"),
            "stale disk parse must not clobber the open buffer"
        );
    }

    /// The open-file skip must not affect files that are not open: a
    /// background parse of an unopened file still publishes normally.
    #[test]
    fn batch_publish_applies_unopened_files() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Gadget.php";

        let content = "<?php\nclass Gadget {\n    public function run(): int { return 1; }\n}\n";
        let result = backend.parse_ast_index_update_for_index(uri, content);
        backend.apply_ast_index_parse_results_batch(vec![result]);

        let classes = backend.symbols.uri_classes_index.read();
        let gadget = classes
            .get(uri)
            .and_then(|c| c.first())
            .expect("unopened Gadget class should be indexed by the batch");
        assert!(
            gadget.methods.iter().any(|m| m.name == "run"),
            "unopened file must publish normally"
        );
    }

    #[test]
    fn ast_index_parse_result_batch_records_failures_and_empty_noops() {
        let backend = Backend::new_test();
        assert!(!backend.apply_ast_index_parse_results_batch(Vec::new()));

        let uri = "file:///project/src/Broken.php";
        let changed =
            backend.apply_ast_index_parse_results_batch(vec![AstIndexParseResult::ParseFailed {
                uri: uri.to_string(),
                errors: vec![("Parse failed (internal error)".to_string(), 10, 20)],
            }]);

        assert!(!changed);
        assert_eq!(
            backend.parse_errors.read().get(uri).cloned(),
            Some(vec![("Parse failed (internal error)".to_string(), 10, 20)])
        );
    }

    /// When a class name is declared in two files, the fqn-keyed derived
    /// indexes must describe the same declaration the class index resolved
    /// the name to.  Re-parsing the losing copy used to clear the winner's
    /// method-store and reverse-inheritance entries without putting them
    /// back, so go-to-implementation stopped listing the class.
    #[test]
    fn duplicate_class_derived_indexes_follow_the_winner() {
        let backend = Backend::new_test();

        let rich =
            "<?php namespace Vendor; class Variant extends Base { public function rich() {} }";
        let bare = "<?php namespace Vendor; class Variant {}";

        backend.update_ast("file:///b_bare.php", bare);
        backend.update_ast("file:///a_rich.php", rich);

        // Re-parse only the losing file.
        backend.update_ast(
            "file:///b_bare.php",
            "<?php namespace Vendor; class Variant { /* edited */ }",
        );

        assert!(
            backend
                .symbols
                .method_store
                .read()
                .contains_key(&("Vendor\\Variant".to_string(), "rich".to_string())),
            "the winner's method must survive a re-parse of the losing copy"
        );
        assert!(
            backend
                .symbols
                .gti_index
                .read()
                .get("Vendor\\Base")
                .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Variant")),
            "the winner's parent must still list it as an implementor"
        );
    }

    /// A class the batch declares for the first time still reaches the
    /// derived indexes: restricting eviction to names whose owner changed
    /// must not also restrict what gets populated.
    #[test]
    fn newly_declared_class_reaches_derived_indexes() {
        let backend = Backend::new_test();

        backend.update_ast(
            "file:///fresh.php",
            "<?php namespace Vendor; class Fresh extends Base { public function hello() {} }",
        );

        assert!(
            backend
                .symbols
                .method_store
                .read()
                .contains_key(&("Vendor\\Fresh".to_string(), "hello".to_string())),
            "a first-parse class must populate the method store"
        );
        assert!(
            backend
                .symbols
                .gti_index
                .read()
                .get("Vendor\\Base")
                .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Fresh")),
            "a first-parse class must populate the reverse-inheritance index"
        );
    }

    /// When the winning file stops declaring a duplicated name, the
    /// derived indexes must be rebuilt from the declaration that takes
    /// over, not left describing the withdrawn one.
    #[test]
    fn promoted_class_declaration_refreshes_derived_indexes() {
        let backend = Backend::new_test();

        backend.update_ast(
            "file:///a_rich.php",
            "<?php namespace Vendor; class Variant extends Rich { public function rich() {} }",
        );
        backend.update_ast(
            "file:///b_bare.php",
            "<?php namespace Vendor; class Variant extends Bare { public function bare() {} }",
        );

        // The winner is renamed, so only b_bare.php declares the name.
        backend.update_ast(
            "file:///a_rich.php",
            "<?php namespace Vendor; class Renamed extends Rich { public function rich() {} }",
        );

        let store = backend.symbols.method_store.read();
        assert!(
            store.contains_key(&("Vendor\\Variant".to_string(), "bare".to_string())),
            "the promoted declaration's members must be indexed"
        );
        assert!(
            !store.contains_key(&("Vendor\\Variant".to_string(), "rich".to_string())),
            "the withdrawn declaration's members must be gone"
        );

        let gti = backend.symbols.gti_index.read();
        assert!(
            gti.get("Vendor\\Bare")
                .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Variant")),
            "the promoted declaration's parent must list it as an implementor"
        );
        assert!(
            !gti.get("Vendor\\Rich")
                .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Variant")),
            "the withdrawn declaration's parent must not still list it"
        );
    }
}
