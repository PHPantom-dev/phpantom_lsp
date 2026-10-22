//! Publishing parsed files into the backend's indexes.
//!
//! [`super::ast_update`] parses a file into an [`AstIndexUpdate`]; this
//! module publishes a batch of them in one pass: class declarations,
//! standalone functions and `define()` constants, the reverse-inheritance
//! index, the resolved-class cache, each file's imports and namespaces,
//! and its symbol map, with the Laravel string spans whose meaning depends
//! on what the workspace declares refreshed against the result.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use crate::Backend;
use crate::ParseErrorEntry;
use crate::atom::Atom;
use crate::ci_map::CiMap;
use crate::class_lookup::Fqn;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::{LaravelStringDependency, SymbolMap};
use crate::types::{ClassInfo, DefineInfo, FunctionInfo, NamespaceSpan};

use super::ast_update::{AstIndexUpdate, BladeLowering};

/// An [`AstIndexUpdate`] alongside what the same file contributed on its
/// previous parse, which the publication replaces.
struct PreparedAstIndexUpdate {
    uri: String,
    parse_errors: Vec<ParseErrorEntry>,
    old_classes: Vec<Arc<ClassInfo>>,
    old_fqns: Vec<String>,
    new_fqns: Vec<String>,
    /// The anonymous classes this file declared on its *previous*
    /// parse, which `old_fqns` deliberately leaves out because they
    /// never reach the declaration index.  They do occupy the
    /// resolved-class cache, so they still have to be evicted from it.
    /// Empty on a first parse, where nothing can be cached yet.
    old_anon_fqns: Vec<String>,
    classes: Vec<Arc<ClassInfo>>,
    use_map: HashMap<String, String>,
    symbol_imports: crate::types::SymbolImports,
    resolved_names: Arc<OwnedResolvedNames>,
    pest_uses: Option<Arc<[String]>>,
    namespace_spans: Vec<NamespaceSpan>,
    functions: Vec<FunctionInfo>,
    defines: Vec<(String, DefineInfo)>,
    symbol_map: Arc<SymbolMap>,
    blade: Option<BladeLowering>,
    old_function_fqns: Vec<String>,
    old_define_names: Vec<String>,
    new_function_fqns: Vec<String>,
    new_define_names: Vec<String>,
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
    /// Whether the workspace declares the symbol that keeps a dormant Laravel
    /// span from meaning what Laravel would make of it.
    fn laravel_string_dependency_exists(&self, dependency: LaravelStringDependency) -> bool {
        match dependency {
            LaravelStringDependency::Function(name) => self.has_indexed_function(name.as_str()),
            LaravelStringDependency::Class(name) => self.has_indexed_class(name.as_str()),
        }
    }

    /// Clone and reconcile only published maps whose dormant Laravel spans
    /// depend on one of `affected`. Passing `None` is reserved for rare
    /// workspace purges where several declaration indexes changed together.
    fn refreshed_laravel_candidate_maps(
        &self,
        affected: Option<&HashSet<LaravelStringDependency>>,
        excluded_uris: &HashSet<&str>,
    ) -> Vec<(String, Arc<SymbolMap>)> {
        let candidates: Vec<(String, Arc<SymbolMap>)> = self
            .files
            .read()
            .iter()
            .filter(|(uri, _)| !excluded_uris.contains(uri.as_str()))
            .filter_map(|(uri, state)| Some((uri, state.symbol_map.as_ref()?)))
            .filter(|(_, map)| map.has_conditional_laravel_dependency(affected))
            .map(|(uri, map)| (uri.clone(), Arc::clone(map)))
            .collect();
        let mut dependency_presence = HashMap::new();
        candidates
            .into_iter()
            .filter_map(|(uri, map)| {
                let mut dependency_exists = |dependency| {
                    *dependency_presence
                        .entry(dependency)
                        .or_insert_with(|| self.laravel_string_dependency_exists(dependency))
                };
                if !map.conditional_laravel_spans_need_refresh(affected, &mut dependency_exists) {
                    return None;
                }
                let mut refreshed = map.as_ref().clone();
                let changed =
                    refreshed.refresh_conditional_laravel_spans(affected, dependency_exists);
                debug_assert!(changed);
                Some((uri, Arc::new(refreshed)))
            })
            .collect()
    }

    /// Publish refreshed conditional maps and keep their reference-index
    /// entries in the same generation. Used after discovery-index rebuilds;
    /// normal AST batches fold refreshed maps into their existing reindex.
    pub(crate) fn refresh_all_published_laravel_candidates(&self) -> bool {
        let refreshed = self.refreshed_laravel_candidate_maps(None, &HashSet::new());
        if refreshed.is_empty() {
            return false;
        }
        self.reindex_references_for_symbol_maps_batch(refreshed.clone());
        self.refresh_laravel_config_writes(
            refreshed
                .iter()
                .map(|(uri, map)| (uri.as_str(), map.as_ref())),
        );
        self.publish_symbol_maps(refreshed);
        // The refreshed maps decide which string keys those files check.
        self.diag.epoch.note_change();
        true
    }

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

    /// Publish a batch of parsed files into every index.
    ///
    /// Returns whether anything other files depend on changed: a class or
    /// function signature, or the meaning of another file's Laravel string
    /// spans.
    pub(crate) fn apply_ast_index_updates_batch(&self, updates: Vec<AstIndexUpdate>) -> bool {
        if updates.is_empty() {
            return false;
        }

        // Held until every index below is written, so that a slow pass
        // running alongside never keeps a result read partly before this
        // publication and partly after it.
        let _publication = self
            .diag
            .epoch
            .begin_publication(updates.iter().map(|update| update.uri.as_str()));

        let old_classes_by_update: Vec<Vec<Arc<ClassInfo>>> = {
            let uri_classes = self.symbols.uri_classes_index.read();
            updates
                .iter()
                .map(|update| uri_classes.get(&update.uri).cloned().unwrap_or_default())
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
                .map(|class| class_info_fqn(class))
                .collect();
            let classes: Vec<Arc<ClassInfo>> = crate::types::into_arcs(update.classes);
            let new_fqns: Vec<String> = classes
                .iter()
                .filter(|class| !class.name.starts_with("__anonymous@"))
                .map(|class| class.fqn().to_string())
                .collect();
            let old_anon_fqns: Vec<String> = old_classes
                .iter()
                .filter(|class| class.name.starts_with("__anonymous@"))
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
                old_anon_fqns,
                classes,
                use_map: update.use_map,
                symbol_imports: update.symbol_imports,
                resolved_names: update.resolved_names,
                pest_uses: update.pest_uses,
                namespace_spans: update.namespace_spans,
                functions: update.functions,
                defines: update.defines,
                symbol_map: update.symbol_map,
                blade: update.blade,
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

        // The names whose declarations this batch changed, for the caches
        // that can be invalidated by dependency rather than wholesale.
        let mut changed_names: Vec<Atom> = Vec::new();
        let any_function_changed = self.publish_batch_globals(&mut prepared, &mut changed_names);

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

        self.symbols.withdraw_inheritance(&evict_fqns);
        self.symbols.index_inheritance(&winning_classes);

        let (any_signature_changed, evicted_fqns) =
            self.evict_batch_resolved_classes(&prepared, &mut changed_names);

        {
            let mut uri_classes = self.symbols.uri_classes_index.write();
            let mut parsed_uris = self.parsed_uris.write();
            for update in &mut prepared {
                uri_classes.insert(update.uri.clone(), std::mem::take(&mut update.classes));
                parsed_uris.insert(update.uri.clone());
            }
        }

        // The batch's own symbol maps have their prospective Laravel spans
        // activated against the declarations published above before they
        // go out with the rest of each file's state.
        let refreshed_existing =
            self.refresh_batch_laravel_candidates(&mut prepared, &all_old_fqns, &all_new_fqns);
        let reference_items: Vec<(String, Arc<SymbolMap>)> = prepared
            .iter()
            .map(|update| (update.uri.clone(), Arc::clone(&update.symbol_map)))
            .chain(refreshed_existing.iter().cloned())
            .collect();
        self.publish_file_states(&mut prepared);
        self.publish_symbol_maps(refreshed_existing.iter().cloned());

        self.repopulate_resolved_classes(&evicted_fqns);

        let structural_changed = any_signature_changed || any_function_changed;
        if structural_changed {
            self.invalidate_signature_dependents(&changed_names);
        } else {
            for update in &prepared {
                self.evict_typed_receiver_view_spans(&update.uri);
                self.evict_morph_column_spans(&update.uri);
            }
        }

        let changed = structural_changed || !refreshed_existing.is_empty();

        self.reindex_references_for_symbol_maps_batch(reference_items);

        // Runtime writes follow every published map, including unedited
        // files whose facade alias was shadowed or restored by this batch.
        self.refresh_laravel_config_writes(
            prepared
                .iter()
                .map(|update| (update.uri.as_str(), update.symbol_map.as_ref()))
                .chain(
                    refreshed_existing
                        .iter()
                        .map(|(uri, map)| (uri.as_str(), map.as_ref())),
                ),
        );

        changed
    }

    /// Replace each batch file's [`FileState`](crate::file_state::FileState)
    /// with what its parse produced, all of it at once.
    ///
    /// A parse that lowered no template keeps the lowering the file already
    /// had.
    fn publish_file_states(&self, prepared: &mut [PreparedAstIndexUpdate]) {
        let mut files = self.files.write();
        for update in prepared {
            let blade = match update.blade.take() {
                Some(blade) => Some(crate::file_state::BladeFileState {
                    virtual_php: blade.virtual_php,
                    source_map: Arc::new(blade.source_map),
                }),
                None => files.get(&update.uri).and_then(|old| old.blade.clone()),
            };
            let state = crate::file_state::FileState {
                symbol_map: Some(Arc::clone(&update.symbol_map)),
                parse_errors: std::mem::take(&mut update.parse_errors),
                use_map: std::mem::take(&mut update.use_map),
                symbol_imports: std::mem::take(&mut update.symbol_imports),
                resolved_names: Some(Arc::clone(&update.resolved_names)),
                namespaces: std::mem::take(&mut update.namespace_spans),
                pest_uses: update.pest_uses.take(),
                blade,
            };
            files.insert(update.uri.clone(), Arc::new(state));
        }
    }

    /// Swap refreshed symbol maps into the states of files that were not
    /// re-parsed, leaving the rest of each state as it was.
    pub(crate) fn publish_symbol_maps(
        &self,
        maps: impl IntoIterator<Item = (String, Arc<SymbolMap>)>,
    ) {
        let mut files = self.files.write();
        for (uri, map) in maps {
            let state = match files.get(&uri) {
                Some(state) => state.with_symbol_map(map),
                None => crate::file_state::FileState {
                    symbol_map: Some(map),
                    ..Default::default()
                },
            };
            files.insert(uri, Arc::new(state));
        }
    }

    /// Record a parse that failed before it produced anything but `errors`.
    ///
    /// Whatever an earlier parse of the file published stays, so features
    /// keep answering from it until a parse succeeds.
    pub(crate) fn publish_parse_failure(&self, uri: &str, errors: Vec<ParseErrorEntry>) {
        let mut files = self.files.write();
        let state = match files.get(uri) {
            Some(old) => crate::file_state::FileState {
                parse_errors: errors,
                ..old.as_ref().clone()
            },
            None => crate::file_state::FileState {
                parse_errors: errors,
                ..Default::default()
            },
        };
        files.insert(uri.to_string(), Arc::new(state));
    }

    /// Publish a batch's standalone functions and `define()` constants, and
    /// record what each file contributed so its next parse can evict what
    /// it removes.
    ///
    /// Adds the function names whose signature changed, or which appeared
    /// or went away, to `changed_names`, and returns whether there were any.
    fn publish_batch_globals(
        &self,
        prepared: &mut [PreparedAstIndexUpdate],
        changed_names: &mut Vec<Atom>,
    ) -> bool {
        // Only touch a file's function entries when this parse contributes
        // functions or the previous parse did (so removals can be evicted).
        // The common case — a class file with no standalone functions that
        // never had any — skips the snapshot scan below entirely.
        let mut any_function_changed = false;
        {
            let mut fmap = self.symbols.global_functions.write();
            let mut dupes = self.symbols.duplicate_functions.write();
            for update in prepared.iter_mut() {
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
                    if !old_functions.is_empty() {
                        let changed = match old_functions
                            .iter()
                            .find(|(f, _)| f.eq_ignore_ascii_case(&fqn))
                        {
                            Some((_, old_info)) => !old_info.signature_eq(&func_info),
                            // New function — may affect callers.
                            None => true,
                        };
                        if changed {
                            any_function_changed = true;
                            changed_names.push(crate::resolution_deps::dep_key(&fqn));
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
                for (old_fqn, _) in &old_functions {
                    if !update
                        .new_function_fqns
                        .iter()
                        .any(|fqn| fqn.eq_ignore_ascii_case(old_fqn))
                    {
                        any_function_changed = true;
                        changed_names.push(crate::resolution_deps::dep_key(old_fqn));
                    }
                }
            }
        }

        // A name that failed to resolve before this file declared it must
        // resolve from now on, so its recorded miss is retired, the same
        // un-caching the batch's class declarations get.
        {
            let nf_cache = self.symbols.function_not_found_cache.read();
            if !nf_cache.is_empty() {
                drop(nf_cache);
                let mut nf_cache = self.symbols.function_not_found_cache.write();
                for update in prepared.iter() {
                    for fqn in &update.new_function_fqns {
                        nf_cache.remove(fqn);
                    }
                }
            }
        }

        {
            let mut dmap = self.symbols.global_defines.write();
            for update in prepared.iter_mut() {
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
            for update in prepared.iter() {
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

        any_function_changed
    }

    /// Evict the classes a batch changed from the resolved-class cache,
    /// with signature-level granularity.  Full indexing usually hits the
    /// first-parse fast path (`old_fqns` is empty), so this stays cheap
    /// during background indexing while preserving edit-time semantics.
    ///
    /// Adds every changed and evicted name to `changed_names`.  Returns
    /// whether any class signature changed, and the evicted names, sorted.
    fn evict_batch_resolved_classes(
        &self,
        prepared: &[PreparedAstIndexUpdate],
        changed_names: &mut Vec<Atom>,
    ) -> (bool, Vec<String>) {
        let mut any_signature_changed = false;
        let mut evicted_fqns = Vec::new();
        {
            let mut cache = self.resolved_class_cache.write();
            for update in prepared {
                // An anonymous class keeps its FQN across an edit that leaves
                // its opening brace where it was, so a cache entry from the
                // previous parse would keep answering for a body that has
                // since changed.  Comparing signatures is not worth it here:
                // the name is already offset-specific, so an edit that reaches
                // one at all has almost certainly changed it.
                for fqn in &update.old_anon_fqns {
                    changed_names.push(crate::resolution_deps::dep_key(fqn));
                    evicted_fqns.extend(crate::virtual_members::evict_fqn(&mut cache, fqn));
                    any_signature_changed = true;
                }

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
                            changed_names.push(crate::resolution_deps::dep_key(fqn));
                            evicted_fqns.extend(crate::virtual_members::evict_fqn(&mut cache, fqn));
                            any_signature_changed = true;
                        }
                    }
                }

                for fqn in &update.new_fqns {
                    if !update.old_fqns.contains(fqn) {
                        changed_names.push(crate::resolution_deps::dep_key(fqn));
                        evicted_fqns.extend(crate::virtual_members::evict_fqn(&mut cache, fqn));
                        any_signature_changed = true;
                    }
                }
            }
        }
        evicted_fqns.sort();
        evicted_fqns.dedup();
        // A class whose own resolution was cached read its parents, traits,
        // and mixins from that cache rather than loading them, so it records
        // no dependency on the one that just changed.  The eviction above
        // already walked the cache's reverse-dependency graph to find every
        // such class; carrying its result into the changed set is what makes
        // the dependency-keyed invalidation of other files sound for
        // inheritance.
        changed_names.extend(
            evicted_fqns
                .iter()
                .map(|fqn| crate::resolution_deps::dep_key(fqn)),
        );
        changed_names.sort_unstable();
        changed_names.dedup();

        (any_signature_changed, evicted_fqns)
    }

    /// Resolve the classes [`Self::evict_batch_resolved_classes`] evicted
    /// again, dependencies first, so the next request finds them cached.
    fn repopulate_resolved_classes(&self, evicted_fqns: &[String]) {
        if evicted_fqns.is_empty() {
            return;
        }

        // Population loads each class through `find_or_load_class`, so
        // the declarations lookups resolve to are the ones whose
        // dependencies decide the order.
        let sorted = {
            let fqn_idx = self.symbols.fqn_class_index.read();
            let iter = evicted_fqns
                .iter()
                .filter_map(|fqn| fqn_idx.get(fqn))
                .map(|class| (class.fqn().to_string(), class.as_ref()));
            crate::toposort::toposort_classes(iter)
        };

        let class_loader =
            |name: Fqn<'_>| -> Option<Arc<ClassInfo>> { self.find_or_load_class(&name) };
        crate::virtual_members::populate_from_sorted(
            &sorted,
            &self.resolved_class_cache,
            &class_loader,
        );
    }

    /// Drop what other files cached about the signatures a batch changed.
    fn invalidate_signature_dependents(&self, changed_names: &[Atom]) {
        self.member_completion_cache.lock().clear();
        // Exact member targets in other files may depend on the return or
        // property type that changed here, but only in the files whose
        // resolution consulted one of the names this parse changed.  A
        // signature keystroke in a controller would otherwise throw away
        // the receiver layer for every candidate file in the workspace
        // and make the next search rebuild all of it.  The edited file
        // itself is evicted when the batch's references are reindexed.
        self.retain_resolved_member_files(changed_names);
        // For the same reason an access in a file nothing touched can
        // start belonging to a different declaration, which the
        // per-file invalidation the reindex does cannot see.
        if !self.member_ref_counts.is_empty() {
            self.member_ref_counts.invalidate_locations_all();
        }
        // A receiver's type is settled against the classes of the whole
        // workspace, so a signature change anywhere can turn a call that
        // was not a render into one, or the other way round.
        self.blade.typed_receiver_view_spans_cache.write().clear();
        self.laravel.morph_column_spans_cache.write().clear();
    }

    /// Activate a batch's prospective Laravel spans against the declarations
    /// it published, and refresh the published maps of other files whose
    /// dormant spans depend on a name the batch declared or dropped.
    ///
    /// Returns those other files' refreshed maps, still to be reindexed and
    /// published.
    fn refresh_batch_laravel_candidates(
        &self,
        prepared: &mut [PreparedAstIndexUpdate],
        all_old_fqns: &[String],
        all_new_fqns: &[String],
    ) -> Vec<(String, Arc<SymbolMap>)> {
        // The batch declarations are now visible. Activate its prospective
        // Laravel spans against that final state, not the incomplete index
        // that happened to exist while parallel workers parsed the files.
        {
            let mut dependency_presence = HashMap::new();
            for update in prepared.iter_mut() {
                Arc::make_mut(&mut update.symbol_map).refresh_conditional_laravel_spans(
                    None,
                    |dependency| {
                        *dependency_presence
                            .entry(dependency)
                            .or_insert_with(|| self.laravel_string_dependency_exists(dependency))
                    },
                );
            }
        }

        // Only declaration names that can gate a candidate trigger a scan of
        // the published maps. Function signatures and ordinary class edits
        // therefore retain the existing O(batch) publication cost.
        let mut affected_laravel_dependencies = HashSet::new();
        for fqn in all_old_fqns.iter().chain(all_new_fqns) {
            if let Some(dependency) = LaravelStringDependency::root_facade(fqn) {
                affected_laravel_dependencies.insert(dependency);
            }
        }
        for update in prepared.iter() {
            for fqn in update
                .old_function_fqns
                .iter()
                .chain(&update.new_function_fqns)
            {
                if let Some(dependency) = LaravelStringDependency::namespaced_auth(fqn) {
                    affected_laravel_dependencies.insert(dependency);
                }
            }
        }
        if affected_laravel_dependencies.is_empty() {
            return Vec::new();
        }
        let prepared_uris: HashSet<&str> =
            prepared.iter().map(|update| update.uri.as_str()).collect();
        self.refreshed_laravel_candidate_maps(Some(&affected_laravel_dependencies), &prepared_uris)
    }
}
