//! AST update orchestration.
//!
//! This module contains the `update_ast` method that parses a PHP file
//! into an [`AstIndexUpdate`], which [`super::ast_publish`] then publishes
//! to all the backend maps (uri_classes_index, use_map, namespace_map,
//! global_functions, global_defines, fqn_uri_index, symbol_maps) in a
//! single pass.  The names the parsed declarations spell are qualified by
//! [`super::name_resolution`].
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::ParseErrorEntry;
use crate::atom::{atom, bytes_to_str};
use crate::blade::call_site_inference::BladeScope;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::{SymbolMap, extract_symbol_map_for_index};
use crate::types::{ClassInfo, DefineInfo, FunctionInfo, NamespaceSpan};

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
    Update(Box<AstIndexUpdate>),
    ParseFailed {
        uri: String,
        errors: Vec<ParseErrorEntry>,
    },
}

pub(crate) struct AstIndexUpdate {
    pub(super) uri: String,
    pub(super) parse_errors: Vec<ParseErrorEntry>,
    pub(super) classes: Vec<ClassInfo>,
    pub(super) use_map: HashMap<String, String>,
    pub(super) symbol_imports: crate::types::SymbolImports,
    pub(super) resolved_names: Arc<OwnedResolvedNames>,
    /// What a Pest test file binds to itself (see [`crate::pest::file_uses`]).
    pub(super) pest_uses: Option<Arc<[String]>>,
    pub(super) namespace_spans: Vec<NamespaceSpan>,
    pub(super) functions: Vec<FunctionInfo>,
    pub(super) defines: Vec<(String, DefineInfo)>,
    pub(super) symbol_map: Arc<SymbolMap>,
    /// The lowering a template was parsed as, published together with
    /// `symbol_map` because the map's offsets index its virtual PHP.
    pub(super) blade: Option<BladeLowering>,
}

/// A Blade template lowered to the virtual PHP everything else reads it as,
/// with the source map that carries positions back to the template.
pub(crate) struct BladeLowering {
    pub(super) virtual_php: Arc<String>,
    pub(super) source_map: crate::blade::source_map::BladeSourceMap,
    /// The scope this lowering was seeded from, so a batch that outlives a
    /// newer scope written by call-site re-inference can be told apart from
    /// one still describing the current scope (see
    /// [`Backend::apply_ast_index_parse_results_batch`]).
    scope: BladeScope,
}

impl Backend {
    /// Update the uri_classes_index, use_map, and namespace_map for a given file URI
    /// by parsing its content.
    ///
    /// Returns `true` when at least one class signature in this file
    /// changed (or a class was added/removed), meaning other open files
    /// that reference those classes may have stale diagnostics.
    pub fn update_ast(&self, uri: &str, content: &str) -> bool {
        let blade = self.lower_blade_template(uri, content);
        let content_to_parse = blade.as_ref().map_or_else(
            || Arc::new(content.to_string()),
            |lowering| Arc::clone(&lowering.virtual_php),
        );

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
            self.update_ast_inner(&uri_owned, &content_owned, blade)
        });

        // The refreshes below that rebuild from the registered provider list
        // share one read of it.
        let providers = crate::backend::laravel::ProvidersOnce::new(self);

        // Keep the Laravel macro index coherent with edits to files that
        // register macros.  Cheap no-op for files without a `macro(` call.
        self.refresh_laravel_macros(uri, content, &providers);

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

        // Keep the container bindings, package config files, view and
        // translation directories, route files, and component namespaces
        // coherent with edits to the providers that register them.  Cheap
        // no-op for every file that is not a registered service provider.
        self.refresh_laravel_provider_resources(uri, content, &providers);

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
    fn update_ast_inner(&self, uri: &str, content: &str, blade: Option<BladeLowering>) -> bool {
        let mut update = self.build_ast_index_update(uri, content);
        update.blade = blade;
        self.apply_ast_index_updates_batch(vec![update])
    }

    /// Pull the imports out of the wrapper *method* a template whose
    /// `$this` is bound is wrapped in, for the same reason the wrapper
    /// function's are pulled out.  A no-op for every other class.
    fn extract_blade_wrapper_method_use_statements(
        statement: &Statement<'_>,
        use_map: &mut HashMap<String, String>,
        symbol_imports: &mut crate::types::SymbolImports,
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
                Self::extract_use_statements_from_statements(
                    block.statements.iter(),
                    use_map,
                    Some(&mut *symbol_imports),
                );
            }
        }
    }

    /// Preprocess a Blade template into the virtual PHP everything else
    /// reads it as, and the source map the position translation rides on.
    ///
    /// Nothing is published here: the lowering travels with the parse of
    /// its virtual PHP and is published alongside the symbol map in
    /// [`Self::apply_ast_index_updates_batch`], so the two always describe
    /// the same text.
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
    pub(crate) fn lower_blade_template(&self, uri: &str, content: &str) -> Option<BladeLowering> {
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
        let directives = self.blade_directives.read();
        let (virtual_php, source_map) = crate::blade::preprocessor::preprocess_with_vars(
            content,
            &injected.vars,
            crate::blade::template_kind(uri, content),
            injected.this_class.as_deref(),
            Some(&components),
            &directives,
        );
        Some(BladeLowering {
            virtual_php: Arc::new(virtual_php),
            source_map,
            scope: injected,
        })
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
        let blade = if self.is_blade_file(uri) && !self.open_files.read().contains_key(uri) {
            self.lower_blade_template(uri, content)
        } else {
            None
        };
        let content = blade
            .as_ref()
            .map_or(content, |lowering| lowering.virtual_php.as_str());

        match crate::util::catch_panic_unwind_safe("parse", uri, None, || {
            self.build_ast_index_update(uri, content)
        }) {
            Some(mut update) => {
                update.blade = blade;
                AstIndexParseResult::Update(Box::new(update))
            }
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
        // A template's lowering carries the scope it was built from. If
        // call-site re-inference has since written a newer scope for the
        // same uri (`reinfer_and_reparse_blade_with`, which re-lowers and
        // re-parses against it right away), this batch result is describing
        // variables the template no longer has -- drop it rather than
        // publish a lowering that is internally consistent but pinned to a
        // scope nothing points at anymore. The template stays on whatever
        // was last published (the fresh re-inference pass, if one already
        // landed) until its own next parse computes a lowering against the
        // current scope.
        let injected_vars = self.blade_injected_vars.read();

        let mut updates = Vec::new();
        let mut failures = Vec::new();
        for result in results {
            match result {
                AstIndexParseResult::Update(update) => {
                    if open_uris.contains_key(&update.uri) {
                        continue;
                    }
                    if let Some(blade) = &update.blade {
                        let current = injected_vars.get(&update.uri);
                        let stale = match current {
                            Some(current) => *current != blade.scope,
                            None => blade.scope != BladeScope::default(),
                        };
                        if stale {
                            continue;
                        }
                    }
                    updates.push(*update);
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
        drop(injected_vars);

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
            self.clear_resolved_member_files();
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

            let mut decls = Self::collect_file_declarations(program, &doc_ctx);
            let mut functions = Self::extract_file_functions(program, &decls, &doc_ctx);

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
                for (cls, _, _) in &mut decls.classes_with_ns {
                    crate::stub_patches::apply_class_stub_patches(cls);
                }
            }

            Self::qualify_function_names(
                &mut functions,
                &decls.namespace_spans,
                &decls.use_map,
                &decls.symbol_imports.constants,
                &decls.namespace,
            );

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
            // names so that cross-file inheritance resolution can find parent
            // classes via PSR-4.
            Self::resolve_parent_class_names_by_block(
                &mut decls.classes_with_ns,
                &decls.namespace_spans,
                &decls.use_map,
                &decls.symbol_imports.constants,
                &decls.namespace,
            );

            // Separate the classes from their namespace tags for storage,
            // stamping each ClassInfo with its namespace so that
            // `find_class_in_uri_classes_index` can distinguish classes with the same
            // short name in different namespace blocks.
            let classes: Vec<ClassInfo> = decls
                .classes_with_ns
                .into_iter()
                .map(|(mut cls, ns, _)| {
                    cls.file_namespace = ns.as_deref().map(atom);
                    cls.cache_fqn_in_uri(uri);
                    // Keyed by FQN, so this has to wait until the class
                    // carries one.
                    crate::stub_patches::apply_third_party_class_patches(&mut cls);
                    cls
                })
                .collect();

            // Build the precomputed symbol map while the AST is still alive.
            // This must happen before the `Program` (and its arena) are dropped.
            let mut symbol_map = extract_symbol_map_for_index(program, content, &owned_resolved);
            // An open file's slow diagnostics are built on the previous
            // pass's, which is only sound if that pass read an index built
            // from exactly the text it diagnosed.
            if self.open_files.read().contains_key(uri) {
                symbol_map.source_hash = Some(SymbolMap::hash_source(content));
            }
            let symbol_map = Arc::new(symbol_map);
            let pest_uses = crate::pest::file_uses(program.statements.iter(), &owned_resolved);

            // For files without any explicit namespace blocks, synthesize a
            // single span covering the entire file with the detected namespace
            // (which will be None for files without namespace declarations).
            let mut namespace_spans = decls.namespace_spans;
            if namespace_spans.is_empty() {
                namespace_spans.push(NamespaceSpan {
                    namespace: decls.namespace,
                    start: 0,
                    end: content.len() as u32,
                    use_map: HashMap::new(),
                    symbol_imports: Default::default(),
                });
            }

            AstIndexUpdate {
                uri: uri.to_string(),
                parse_errors,
                classes,
                use_map: decls.use_map,
                symbol_imports: decls.symbol_imports,
                resolved_names: Arc::new(owned_resolved),
                pest_uses,
                namespace_spans,
                functions,
                defines,
                symbol_map,
                blade: None,
            }
        })
    }

    /// Walk the file's statements once for its class-likes and imports,
    /// block by block.
    fn collect_file_declarations<'a>(
        program: &'a Program<'a>,
        doc_ctx: &DocblockCtx<'a>,
    ) -> FileDeclarations {
        let mut decls = FileDeclarations::default();
        for statement in program.statements.iter() {
            match statement {
                Statement::Use(use_stmt) => {
                    Self::extract_use_items(
                        &use_stmt.items,
                        &mut decls.use_map,
                        Some(&mut decls.symbol_imports),
                    );
                }
                Statement::Namespace(ns) => Self::collect_namespace_block(ns, doc_ctx, &mut decls),
                statement if Self::is_classlike_extraction_candidate(statement) => {
                    // A template whose `$this` is bound wraps its body
                    // in a method rather than a function, which buries
                    // the template's own imports just the same (see the
                    // wrapper-function arm below).
                    Self::extract_blade_wrapper_method_use_statements(
                        statement,
                        &mut decls.use_map,
                        &mut decls.symbol_imports,
                    );
                    let mut top_classes = Vec::new();
                    Self::extract_classes_from_statements(
                        std::iter::once(statement),
                        &mut top_classes,
                        Some(doc_ctx),
                    );
                    for cls in top_classes {
                        decls.classes_with_ns.push((cls, None, None));
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
                        &mut decls.use_map,
                        Some(&mut decls.symbol_imports),
                    );
                }
                _ => {
                    // Walk other top-level statements (expression statements,
                    // function declarations, etc.) for anonymous classes.
                    let mut anon_classes = Vec::new();
                    Self::find_anonymous_classes_in_statement(
                        statement,
                        &mut anon_classes,
                        Some(doc_ctx),
                    );
                    for cls in anon_classes {
                        decls.classes_with_ns.push((cls, None, None));
                    }
                }
            }
        }
        decls.fold_block_imports();

        // A class-like declared in two branches of a conditional yields
        // one entry per branch; keep the first so resolution is
        // deterministic (see `dedup_class_likes_first_wins`).
        Self::dedup_class_likes_first_wins(&mut decls.classes_with_ns, |(cls, ns, _)| (cls, ns));
        decls
    }

    /// Collect the class-likes and imports of one `namespace` block,
    /// tagging each class with the block's namespace.
    fn collect_namespace_block<'a>(
        ns: &'a Namespace<'a>,
        doc_ctx: &DocblockCtx<'a>,
        decls: &mut FileDeclarations,
    ) {
        let block_ns: Option<String> = ns
            .name
            .as_ref()
            .map(|ident| bytes_to_str(ident.value()).to_string())
            .filter(|n| !n.is_empty());

        // The file-level namespace is the FIRST non-empty one.
        if decls.namespace.is_none() {
            decls.namespace = block_ns.clone();
        }

        let mut block_classes = Vec::new();
        let mut block_use_map = HashMap::new();
        let mut block_symbol_imports = crate::types::SymbolImports::default();
        for inner in ns.statements().iter() {
            match inner {
                Statement::Use(use_stmt) => {
                    Self::extract_use_items(
                        &use_stmt.items,
                        &mut block_use_map,
                        Some(&mut block_symbol_imports),
                    );
                }
                inner if Self::is_classlike_extraction_candidate(inner) => {
                    Self::extract_classes_from_statements(
                        std::iter::once(inner),
                        &mut block_classes,
                        Some(doc_ctx),
                    );
                }
                Statement::Namespace(inner_ns) => {
                    // Nested namespaces (rare but valid)
                    Self::extract_use_statements_from_statements(
                        inner_ns.statements().iter(),
                        &mut block_use_map,
                        Some(&mut block_symbol_imports),
                    );
                    Self::extract_classes_from_statements(
                        inner_ns.statements().iter(),
                        &mut block_classes,
                        Some(doc_ctx),
                    );
                }
                _ => {
                    // Walk other statements (expression statements,
                    // control flow, etc.) for anonymous classes.
                    Self::find_anonymous_classes_in_statement(
                        inner,
                        &mut block_classes,
                        Some(doc_ctx),
                    );
                }
            }
        }

        let block_index = decls.namespace_spans.len();
        for cls in block_classes {
            decls
                .classes_with_ns
                .push((cls, block_ns.clone(), Some(block_index)));
        }

        let ns_span = ns.span();
        decls.namespace_spans.push(NamespaceSpan {
            namespace: block_ns,
            start: ns_span.start.offset,
            end: ns_span.end.offset,
            use_map: block_use_map,
            symbol_imports: block_symbol_imports,
        });
    }

    /// Extract standalone functions (including those inside if-guards
    /// like `if (! function_exists('...'))`) using the shared helper which
    /// recurses into if/block statements.
    fn extract_file_functions<'a>(
        program: &'a Program<'a>,
        decls: &FileDeclarations,
        doc_ctx: &DocblockCtx<'a>,
    ) -> Vec<FunctionInfo> {
        let mut functions = Vec::new();
        // Give the docblock context the use-map and namespace in force
        // where each function is declared so that parameter default values
        // (e.g. `Application::class`) can be resolved to FQNs during
        // extraction.
        let doc_ctx_with =
            |use_map: &HashMap<String, String>, namespace: &Option<String>| DocblockCtx {
                trivias: doc_ctx.trivias,
                content: doc_ctx.content,
                php_version: doc_ctx.php_version,
                use_map: use_map.clone(),
                namespace: namespace.clone(),
            };
        let func_doc_ctx = doc_ctx_with(&decls.use_map, &decls.namespace);
        let multi_block = decls.namespace_spans.len() > 1;
        let mut block_index = 0;
        for statement in program.statements.iter() {
            let block = match statement {
                Statement::Namespace(_) if multi_block => {
                    block_index += 1;
                    Some(&decls.namespace_spans[block_index - 1])
                }
                _ => None,
            };
            let block_doc_ctx = block.map(|span| doc_ctx_with(&span.use_map, &span.namespace));
            Self::extract_functions_from_statements(
                std::iter::once(statement),
                &mut functions,
                &decls.namespace,
                Some(block_doc_ctx.as_ref().unwrap_or(&func_doc_ctx)),
            );
        }
        functions
    }
}

/// The class-likes and imports a file declares, gathered in one walk of
/// its top level.
#[derive(Default)]
struct FileDeclarations {
    /// Each extracted class together with the namespace block it was
    /// declared in (its namespace, and its index in `namespace_spans`, or
    /// `None` outside any block).  This is critical for files that contain
    /// multiple `namespace { }` blocks, each declaring classes under a
    /// different namespace with its own imports.  The per-class namespace
    /// is used later when building the `fqn_uri_index`, and the block when
    /// resolving parent/trait names.
    classes_with_ns: Vec<(ClassInfo, Option<String>, Option<usize>)>,
    use_map: HashMap<String, String>,
    symbol_imports: crate::types::SymbolImports,
    /// The file-level namespace: the first non-empty one.
    namespace: Option<String>,
    namespace_spans: Vec<NamespaceSpan>,
}

impl FileDeclarations {
    /// The file-wide table holds every block's imports.  A file with one
    /// block keeps them only there (see `NamespaceSpan::use_map`).
    fn fold_block_imports(&mut self) {
        let multi_block = self.namespace_spans.len() > 1;
        for span in &mut self.namespace_spans {
            if multi_block {
                self.use_map.extend(
                    span.use_map
                        .iter()
                        .map(|(alias, fqn)| (alias.clone(), fqn.clone())),
                );
                let block = &span.symbol_imports;
                self.symbol_imports
                    .functions
                    .extend(block.functions.iter().map(|(a, f)| (a.clone(), f.clone())));
                self.symbol_imports
                    .constants
                    .extend(block.constants.iter().map(|(a, f)| (a.clone(), f.clone())));
            } else {
                self.use_map.extend(std::mem::take(&mut span.use_map));
                let block = std::mem::take(&mut span.symbol_imports);
                self.symbol_imports.functions.extend(block.functions);
                self.symbol_imports.constants.extend(block.constants);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::Backend;

    fn has_config_resource_span(
        backend: &Backend,
        uri: &str,
        key: &str,
        resource: crate::symbol_map::LaravelConfigResource,
    ) -> bool {
        backend.symbol_map_for(uri).is_some_and(|map| {
            map.spans.iter().any(|span| {
                matches!(
                    &span.kind,
                    crate::symbol_map::SymbolKind::LaravelStringKey {
                        kind: crate::symbol_map::LaravelStringKind::ConfigResource(found),
                        key: found_key,
                        ..
                    } if *found == resource && found_key == key
                )
            })
        })
    }

    fn reference_index_has_config_resource(
        backend: &Backend,
        uri: &str,
        key: &str,
        resource: crate::symbol_map::LaravelConfigResource,
    ) -> bool {
        let index_key = crate::reference_index::laravel_string_reference_key(
            crate::symbol_map::LaravelStringKind::ConfigResource(resource),
            key,
        );
        backend
            .reference_index
            .read()
            .get(&index_key)
            .is_some_and(|entries| entries.keys().any(|entry_uri| entry_uri.as_ref() == uri))
    }

    #[test]
    fn namespaced_auth_candidates_follow_cross_file_function_lifecycle() {
        use crate::symbol_map::LaravelConfigResource::AuthGuard;

        let backend = Backend::new_test();
        let consumer_uri = "file:///app/Consumer.php";
        let helper_uri = "file:///app/helpers.php";
        backend.update_ast(consumer_uri, "<?php\nnamespace App;\nauth('web');\n");
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));
        assert!(reference_index_has_config_resource(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));

        // An already-correct map must not be cloned or reindexed merely
        // because a workspace discovery refresh completed.
        assert!(!backend.refresh_all_published_laravel_candidates());

        let changed = backend.update_ast(
            helper_uri,
            "<?php\nnamespace App;\nfunction auth(?string $guard = null): object {}\n",
        );
        assert!(changed, "the consumer map changed even on a first parse");
        assert!(!has_config_resource_span(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));
        assert!(!reference_index_has_config_resource(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));

        let removed = HashSet::from([helper_uri.to_string()]);
        backend.withdraw_functions_for_uris(&removed);
        assert!(backend.refresh_all_published_laravel_candidates());
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));
        assert!(reference_index_has_config_resource(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));

        backend.update_ast(
            helper_uri,
            "<?php\nnamespace App;\nfunction auth(?string $guard = null): object {}\n",
        );
        assert!(!has_config_resource_span(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));

        backend.update_ast(helper_uri, "<?php\nnamespace App;\n");
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));
        assert!(reference_index_has_config_resource(
            &backend,
            consumer_uri,
            "web",
            AuthGuard
        ));
    }

    #[test]
    fn prospective_batch_functions_shadow_auth_candidates() {
        use crate::symbol_map::LaravelConfigResource::AuthGuard;

        let backend = Backend::new_test();
        let consumer_uri = "file:///app/Consumer.php";
        let helper_uri = "file:///app/helpers.php";
        let consumer = backend.parse_ast_index_update_for_index(
            consumer_uri,
            "<?php\nnamespace App;\nauth('admin');\n",
        );
        let helper = backend.parse_ast_index_update_for_index(
            helper_uri,
            "<?php\nnamespace App;\nfunction auth(?string $guard = null): object {}\n",
        );
        backend.apply_ast_index_parse_results_batch(vec![consumer, helper]);

        assert!(!has_config_resource_span(
            &backend,
            consumer_uri,
            "admin",
            AuthGuard
        ));
        assert!(!reference_index_has_config_resource(
            &backend,
            consumer_uri,
            "admin",
            AuthGuard
        ));
    }

    #[test]
    fn root_facade_candidates_follow_global_class_lifecycle() {
        use crate::symbol_map::LaravelConfigResource::{CacheStore, StorageDisk};

        let backend = Backend::new_test();
        let consumer_uri = "file:///app/Consumer.php";
        let class_uri = "file:///app/Cache.php";
        backend.update_ast(
            consumer_uri,
            "<?php\nCache::store('redis');\nStorage::disk('local');\nRoute::get('/')->where('id')->middleware('auth:route');\n",
        );
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "redis",
            CacheStore
        ));
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "local",
            StorageDisk
        ));
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "route",
            crate::symbol_map::LaravelConfigResource::AuthGuard
        ));

        backend.update_ast(
            class_uri,
            "<?php\nclass Cache { public static function store() {} }\nclass Route { public static function get() {} }\n",
        );
        assert!(!has_config_resource_span(
            &backend,
            consumer_uri,
            "redis",
            CacheStore
        ));
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "local",
            StorageDisk
        ));
        assert!(!has_config_resource_span(
            &backend,
            consumer_uri,
            "route",
            crate::symbol_map::LaravelConfigResource::AuthGuard
        ));
        let map = backend
            .symbol_map_for(consumer_uri)
            .expect("consumer map should remain published");
        let disk_indices = map.member_access_indices("disk");
        assert_eq!(disk_indices.len(), 1);
        assert!(matches!(
            &map.spans[disk_indices[0]].kind,
            crate::symbol_map::SymbolKind::MemberAccess { member_name, .. }
                if member_name == "disk"
        ));

        backend.update_ast(class_uri, "<?php\nclass Other {}\n");
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "redis",
            CacheStore
        ));
        assert!(has_config_resource_span(
            &backend,
            consumer_uri,
            "route",
            crate::symbol_map::LaravelConfigResource::AuthGuard
        ));
    }

    #[test]
    fn batch_publication_indexes_runtime_writes_without_opening_the_writer() {
        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(true);
        let writer_uri = "file:///project/tests/DiskTest.php";
        let reader_uri = "file:///project/app/Reader.php";
        let writer = backend.parse_ast_index_update_for_index(
            writer_uri,
            "<?php\nconfig(['cache.stores.scratch' => []]);\nConfig::set('filesystems.disks.temporary', []);\nStorage::fake('temporary');\nStorage::persistentFake('persistent');\n",
        );
        let reader = backend.parse_ast_index_update_for_index(
            reader_uri,
            "<?php\nStorage::disk('temporary');\nCache::store('scratch');\n",
        );

        backend.apply_ast_index_parse_results_batch(vec![reader, writer]);

        assert_eq!(
            backend.laravel_runtime_config_keys.read()[writer_uri],
            [
                "cache.stores.scratch",
                "filesystems.disks.persistent",
                "filesystems.disks.temporary",
            ]
        );
        let writes = backend.laravel_runtime_config_keys.read();
        assert_eq!(
            writes[writer_uri].len(),
            3,
            "duplicate writes share one key"
        );
        assert!(!writes.contains_key(reader_uri));
    }

    #[test]
    fn runtime_writes_follow_cross_file_facade_shadows() {
        for (facade, call, key) in [
            (
                "Storage",
                "Storage::fake('scratch')",
                "filesystems.disks.scratch",
            ),
            (
                "Config",
                "Config::set('cache.stores.scratch', [])",
                "cache.stores.scratch",
            ),
        ] {
            let backend = Backend::new_test();
            backend.resolved_class_cache.write().set_laravel(true);
            let writer_uri = "file:///project/tests/FixtureTest.php";
            let shadow_uri = "file:///project/app/LocalFacade.php";
            backend.update_ast(writer_uri, &format!("<?php\n{call};\n"));
            assert!(backend.runtime_config_key_covers(key), "{facade}");

            backend.update_ast(shadow_uri, &format!("<?php\nclass {facade} {{}}\n"));
            assert!(
                backend.laravel_runtime_config_keys.read().is_empty(),
                "a global {facade} class must withdraw the unedited file's write"
            );

            backend.update_ast(shadow_uri, "<?php\nclass Other {}\n");
            assert!(
                backend.runtime_config_key_covers(key),
                "removing the {facade} shadow must restore the unedited file's write"
            );
        }
    }

    #[test]
    fn discovery_refresh_reconciles_runtime_writes_with_facade_aliases() {
        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(true);
        let writer_uri = "file:///project/tests/DiskTest.php";
        let shadow_uri = "file:///project/app/Storage.php";
        let key = "filesystems.disks.scratch";
        backend.update_ast(writer_uri, "<?php\nStorage::fake('scratch');\n");
        assert!(backend.runtime_config_key_covers(key));

        backend.symbols.with_class_declarations(|declarations| {
            declarations.note_discovered("Storage", shadow_uri.to_string());
        });
        assert!(backend.refresh_all_published_laravel_candidates());
        assert!(backend.laravel_runtime_config_keys.read().is_empty());

        backend.symbols.with_class_declarations(|declarations| {
            declarations.withdraw_uris(&HashSet::from([shadow_uri.to_string()]));
        });
        assert!(backend.refresh_all_published_laravel_candidates());
        assert!(backend.runtime_config_key_covers(key));
        assert!(!backend.refresh_all_published_laravel_candidates());
    }

    #[test]
    fn controller_middleware_does_not_claim_auth_guards_without_a_typed_receiver() {
        use crate::symbol_map::LaravelConfigResource::AuthGuard;

        let backend = Backend::new_test();
        let uri = "file:///app/Controller.php";
        backend.update_ast(
            uri,
            "<?php\nclass Controller {\n    function boot() {\n        $this->middleware('auth:web');\n        $this->middleware('can:update,Post');\n    }\n}\nRoute::get('/')->middleware('auth:admin');\nRoute::get('/')->a()->b()->c()->d()->middleware('auth:too-deep');\n",
        );
        assert!(!has_config_resource_span(&backend, uri, "web", AuthGuard));
        assert!(has_config_resource_span(&backend, uri, "admin", AuthGuard));
        assert!(!has_config_resource_span(
            &backend, uri, "too-deep", AuthGuard
        ));
        assert!(backend.symbol_map_for(uri).is_some_and(|map| {
            map.spans.iter().any(|span| {
                matches!(
                    &span.kind,
                    crate::symbol_map::SymbolKind::LaravelStringKey {
                        kind: crate::symbol_map::LaravelStringKind::GateAbility,
                        key,
                        ..
                    } if key == "update"
                )
            })
        }));
    }

    /// Two parses of one template can race: an index worker lowers it with
    /// the scope it had, while call-site inference re-parses it with a
    /// larger one.  Whichever lands last, the virtual PHP, the source map
    /// and the symbol map must all come from the same parse, or every
    /// offset in the map points at the wrong text and every position
    /// translated back to the template is off by the prologue difference.
    #[test]
    fn racing_template_parses_leave_one_consistent_lowering() {
        let backend = Backend::new_test();
        let uri = "file:///resources/views/shop.blade.php";
        let template = "<p>{{ $a }}</p>\n{{ \\App\\Item::class }}\n";

        let stale = backend.parse_ast_index_update_for_index(uri, template);
        backend.blade_injected_vars.write().insert(
            uri.to_string(),
            crate::blade::call_site_inference::BladeScope {
                vars: vec![
                    ("a".to_string(), "int".to_string()),
                    ("b".to_string(), "string".to_string()),
                ],
                ..Default::default()
            },
        );
        backend.update_ast(uri, template);
        backend.apply_ast_index_parse_results_batch(vec![stale]);

        let virtual_php = backend.blade_virtual_php_arc(uri).unwrap();
        let symbol_map = backend.symbol_maps.read().get(uri).cloned().unwrap();
        assert!(
            symbol_map.matches_source(&virtual_php),
            "the symbol map must index the virtual PHP that was published with it"
        );

        let item = symbol_map
            .spans
            .iter()
            .find(|span| matches!(&span.kind, crate::symbol_map::SymbolKind::ClassReference { name, .. } if name.ends_with("Item")))
            .expect("the template names App\\Item");
        let position = crate::text_position::offset_to_position(&virtual_php, item.start as usize);
        assert_eq!(
            backend
                .try_translate_php_to_blade(uri, position)
                .map(|p| p.line),
            Some(1),
            "App\\Item is on the template's second line"
        );
    }

    /// A stale index parse must not pin a template back to the scope it
    /// started with. If call-site re-inference computes a new scope and
    /// re-parses the template before the index worker's older-scoped batch
    /// applies, the batch's lowering describes variables the template no
    /// longer has and must be dropped rather than published over the fresh
    /// state.
    #[test]
    fn stale_scope_batch_does_not_clobber_a_fresher_reinference() {
        let backend = Backend::new_test();
        let uri = "file:///resources/views/shop.blade.php";
        let template = "<p>{{ $a }}</p>\n{{ \\App\\Item::class }}\n";

        // An index worker lowers the template while the scope is still
        // empty (nothing has been cached for this uri yet).
        let stale = backend.parse_ast_index_update_for_index(uri, template);

        // Call-site re-inference computes the real scope and re-parses
        // against it before the index worker's batch is applied.
        backend.blade_injected_vars.write().insert(
            uri.to_string(),
            crate::blade::call_site_inference::BladeScope {
                vars: vec![("a".to_string(), "int".to_string())],
                ..Default::default()
            },
        );
        backend.update_ast(uri, template);
        let fresh_virtual_php = backend.blade_virtual_php_arc(uri).unwrap();

        // The index worker's batch, built from the empty scope, lands last.
        backend.apply_ast_index_parse_results_batch(vec![stale]);

        assert_eq!(
            backend.blade_virtual_php_arc(uri).unwrap(),
            fresh_virtual_php,
            "a batch built from a scope the template has moved past must not \
             overwrite the lowering built from the current scope"
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
    /// reverse-inheritance entries without putting them back, so
    /// go-to-implementation stopped listing the class.
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

    /// An inheritance edge is deduplicated through the class's own list of
    /// parents rather than through the parent's list of children, so a
    /// re-parse that leaves the edge unchanged must still leave exactly one
    /// entry for it.
    #[test]
    fn reparsing_a_class_does_not_duplicate_its_inheritance_edges() {
        let backend = Backend::new_test();
        let src =
            "<?php namespace Vendor; class Child extends Base implements Contract { use Helper; }";

        backend.update_ast("file:///child.php", src);
        backend.update_ast("file:///child.php", src);

        let gti = backend.symbols.gti_index.read();
        for parent in ["Vendor\\Base", "Vendor\\Contract", "Vendor\\Helper"] {
            let kids = gti
                .get(parent)
                .unwrap_or_else(|| panic!("{parent} should list Child as an implementor"));
            assert_eq!(
                kids.iter().filter(|k| *k == "Vendor\\Child").count(),
                1,
                "{parent} should list Child exactly once, got {kids:?}"
            );
        }
    }
}
