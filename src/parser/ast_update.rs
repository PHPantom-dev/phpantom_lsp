//! AST update orchestration.
//!
//! This module contains the `update_ast` method that parses a PHP file
//! into an [`AstIndexUpdate`], which [`super::ast_publish`] then publishes
//! to all the backend indexes (uri_classes_index, global_functions,
//! global_defines, fqn_uri_index, and the file's `FileState`) in a single
//! pass.  The names the parsed declarations spell are qualified by
//! [`super::name_resolution`].
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::ParseErrorEntry;
use crate::atom::atom;
use crate::blade::call_site_inference::BladeScope;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::{SymbolMap, extract_symbol_map_for_index};
use crate::types::{ClassInfo, DefineInfo, FunctionInfo, NamespaceSpan};

use mago_allocator::LocalArena;

use mago_span::HasSpan;
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

        self.laravel
            .string_key_cache
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
            self.update_ast_inner(&uri_owned, content_owned, blade)
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
                self.publish_parse_failure(
                    uri,
                    vec![("Parse failed (internal error)".to_string(), 0, 0)],
                );
                false
            }
        }
    }

    /// Inner implementation of [`update_ast`] that performs the actual parse
    /// and publishes the resulting single-file update.
    ///
    /// An open file's parse is kept for the diagnostic pass this edit
    /// schedules (see [`Backend::take_parsed_for_diagnostics`]).
    fn update_ast_inner(
        &self,
        uri: &str,
        content: Arc<String>,
        blade: Option<BladeLowering>,
    ) -> bool {
        let (mut update, parsed) = if self.open_files.read().contains_key(uri) {
            let mut parse_errors = Vec::new();
            let parsed = super::ParsedFile::new(content, |arena, text| {
                let (errors, program) = parse_with_errors(arena, text);
                parse_errors = errors;
                program
            });
            let update = with_reusable_arena(|arena| {
                self.ast_index_update_from(
                    arena,
                    uri,
                    parsed.content(),
                    parse_errors,
                    parsed.program(),
                )
            });
            (update, Some(parsed))
        } else {
            (self.build_ast_index_update(uri, &content), None)
        };
        update.blade = blade;
        let changed = self.apply_ast_index_updates_batch(vec![update]);
        if let Some(parsed) = parsed {
            self.keep_parsed_for_diagnostics(uri, parsed);
        }
        changed
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
            .blade
            .injected_vars
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let components = self.blade_component_resolver(&injected.components);
        // Read under the guard rather than cloned: the set is small but
        // this runs on every keystroke in a template, and nothing the
        // preprocessor does can write it back.
        let directives = self.blade.directives.read();
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
        let injected_vars = self.blade.injected_vars.read();

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

        for (uri, errors) in failures {
            self.publish_parse_failure(&uri, errors);
        }

        // Invalidate the reverse pivot index when a background-indexed file
        // declares a many-to-many relationship, so its target models pick up
        // its pivot accessors on the next class load.
        if !self.laravel.pivots.is_dirty()
            && updates.iter().any(|u| {
                u.classes
                    .iter()
                    .any(crate::virtual_members::laravel::class_declares_pivot_relationship)
            })
        {
            self.laravel.pivots.mark_dirty();
            self.clear_resolved_member_files();
        }

        self.apply_ast_index_updates_batch(updates)
    }

    fn build_ast_index_update(&self, uri: &str, content: &str) -> AstIndexUpdate {
        with_reusable_arena(|arena| {
            let (parse_errors, program) = parse_with_errors(arena, content);
            self.ast_index_update_from(arena, uri, content, parse_errors, program)
        })
    }

    /// The index update for `program`, the parse of `content`, with `arena`
    /// for scratch space.
    fn ast_index_update_from(
        &self,
        arena: &LocalArena,
        uri: &str,
        content: &str,
        parse_errors: Vec<ParseErrorEntry>,
        program: &mago_syntax::cst::Program<'_>,
    ) -> AstIndexUpdate {
        // Run mago-names resolver while the arena is still alive.
        // This produces a `ResolvedNames` that maps every identifier's
        // byte offset to its fully-qualified name.  We immediately copy
        // the data into an owned `OwnedResolvedNames` so it survives
        // the arena drop.
        let name_resolver = mago_names::resolver::NameResolver::new(arena);
        let mago_resolved = name_resolver.resolve(program);
        let owned_resolved = OwnedResolvedNames::from_resolved(&mago_resolved);

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
        // session.  A phpstorm-stubs checkout under a stub path has the
        // same gaps, so it is corrected the same way.
        if self.is_phpstorm_stubs_uri(uri) {
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
            Some(&doc_ctx),
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
        let from_stub_path = self.is_external_stub_uri(uri);
        let classes: Vec<ClassInfo> = decls
            .classes_with_ns
            .into_iter()
            .map(|(mut cls, ns, _)| {
                cls.file_namespace = ns.as_deref().map(atom);
                cls.cache_fqn_in_uri(uri);
                // Keyed by FQN, so this has to wait until the class
                // carries one.  A stub path says what its classes are,
                // so it is left as written.
                if !from_stub_path {
                    crate::stub_patches::apply_third_party_class_patches(&mut cls);
                }
                cls
            })
            .collect();

        // Build the precomputed symbol map while the AST is still alive.
        // This must happen before the `Program` (and its arena) are dropped.
        // A stub file is never analysed or searched for references, so the
        // symbol map only matters once it is opened, which parses it again.
        // Stub packages can run to megabytes, where the map is a third of
        // the parse.
        let is_open = self.open_files.read().contains_key(uri);
        let mut symbol_map = if from_stub_path && !is_open {
            SymbolMap::default()
        } else {
            extract_symbol_map_for_index(
                program,
                content,
                &owned_resolved,
                self.is_symfony_project(),
                self.is_twig_project(),
            )
        };
        // An open file's slow diagnostics are built on the previous
        // pass's, which is only sound if that pass read an index built
        // from exactly the text it diagnosed.
        if is_open {
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
    }
}

/// Parse `content`, returning the syntax errors of the text as written and
/// the parse everything else reads, which keeps the code the first one
/// dropped.
fn parse_with_errors<'a>(
    arena: &'a LocalArena,
    content: &'a str,
) -> (Vec<ParseErrorEntry>, &'a mago_syntax::cst::Program<'a>) {
    let file_id = mago_database::file::FileId::new(b"input.php");
    let program = parse_file_content(arena, file_id, content.as_bytes());

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

    let program = super::recovery::reparse_dropped_code(arena, program, content).unwrap_or(program);
    (parse_errors, program)
}

#[cfg(test)]
#[path = "ast_update_tests.rs"]
mod tests;
