//! [`Backend::find_or_load_function`], and resolving a function or class
//! name written in a file against the index.

use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::Fqn;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionInfo};
use crate::util::short_name;

impl Backend {
    /// Try to find a standalone function by name, checking user-defined
    /// functions first, then falling back to embedded PHP stubs.
    ///
    /// The lookup order is:
    ///   1. `global_functions` — functions from Composer autoload files and
    ///      opened/changed files.
    ///   2. `stub_function_index` — built-in PHP functions embedded from
    ///      phpstorm-stubs.  Parsed lazily on first access and cached in
    ///      `global_functions` under a `phpantom-stub-fn://` URI so
    ///      subsequent lookups hit step 1.
    ///
    /// `candidates` is a list of names to try (e.g. the bare name, the
    /// FQN via use-map, the namespace-qualified name).  The first match
    /// wins.
    pub fn find_or_load_function(&self, candidates: &[&str]) -> Option<FunctionInfo> {
        let mut function = self.find_or_load_declared_function(candidates)?;
        self.verify_function_doc_types(&mut function);
        Some(function)
    }

    /// Keep each docblock type `function` settled in place of a native hint
    /// naming another class only where it narrows the hint (see
    /// [`docblock_narrowing`](crate::type_engine::type_ops::docblock_narrowing)).
    ///
    /// Every lookup clones the function out of the index, so the check runs
    /// on that copy rather than being cached.
    pub(crate) fn verify_function_doc_types(&self, function: &mut FunctionInfo) {
        if function.unverified_doc_types.is_empty() {
            return;
        }
        let loader = |name: Fqn<'_>| self.find_indexed_class(&PhpType::parse(&name));
        crate::type_engine::type_ops::docblock_narrowing::verify_function(function, &loader);
    }

    /// [`find_or_load_function`](Self::find_or_load_function) with the
    /// function's types as its parse settled them.
    fn find_or_load_declared_function(&self, candidates: &[&str]) -> Option<FunctionInfo> {
        // Every spelling tried is a name this resolution depends on, whether
        // or not it is the one that answers: a function declared under one
        // of the others later would win instead.
        crate::resolution_deps::record_all(candidates);

        let index_generation = self.symbols.function_index_generation();

        // ── Stub paths: a declaration there wins over every other ──
        if let Some(function) = self.load_external_stub_function(candidates) {
            return Some(function);
        }

        // ── Phase 1: Check global_functions (user code + already-cached stubs) ──
        {
            let fmap = self.symbols.global_functions.read();
            for &name in candidates {
                if let Some((_, info)) = fmap.get(name) {
                    return Some(info.clone());
                }
            }
        }

        // ── Negative cache: skip the fallback phases ──
        // A call to a function no index carries is re-resolved every time
        // the forward walker passes it, and Phase 1.75 below walks every
        // known autoload file per attempt.  Once every candidate spelling
        // is a recorded miss, the whole chain is skipped.
        {
            let nf_cache = self.symbols.function_not_found_cache.read();
            if !nf_cache.is_empty() && candidates.iter().all(|name| nf_cache.contains(name)) {
                return None;
            }
        }

        // ── Phase 1.5: Check autoload_function_index (byte-level scan) ──
        // The lightweight `find_symbols` byte-level scan discovers
        // function names at startup without a full AST parse, for both
        // non-Composer projects (workspace scan) and Composer projects
        // (autoload_files.php scan).  When a candidate matches here, we
        // lazily call `update_ast` on the file to get a complete
        // `FunctionInfo` and cache it in global_functions so subsequent
        // lookups hit Phase 1.
        //
        // Note: the lazy parse is a full AST parse (`update_ast`), which
        // is the same cost as opening the file.  This is acceptable
        // because it only happens once per function, on first access.
        {
            let idx = self.symbols.autoload_function_index.read();
            for &name in candidates {
                if let Some(path) = idx.get(name) {
                    let path = path.clone();
                    drop(idx); // release read lock before parsing

                    if let Some(content) = crate::util::read_php_source(&path) {
                        let uri = crate::util::path_to_uri(&path);
                        self.update_ast(&uri, &content);

                        // Re-check global_functions after parsing.
                        let fmap = self.symbols.global_functions.read();
                        for &retry_name in candidates {
                            if let Some((_, info)) = fmap.get(retry_name) {
                                return Some(info.clone());
                            }
                        }
                    }
                    break; // Only try one file per lookup
                }
            }
        }

        // ── Phase 1.75: Last-resort lazy parse of known autoload files ──
        // The byte-level scanner misses functions wrapped in
        // `if (! function_exists(...))` guards (brace depth > 0).
        // These are common in Laravel helpers and similar packages.
        // As a safety net, lazily parse each known autoload file via
        // `update_ast` until the function is found.  Each file is
        // parsed at most once: subsequent lookups hit Phase 1
        // (`global_functions`).
        {
            let paths = self.symbols.autoload_file_paths.read().clone();
            for path in &paths {
                // Skip files that have already been fully parsed (their
                // functions are already in global_functions via Phase 1).
                let uri = crate::util::path_to_uri(path);
                if self.parsed_uris.read().contains(&uri) {
                    continue;
                }

                if let Some(content) = crate::util::read_php_source(path) {
                    self.update_ast(&uri, &content);

                    let fmap = self.symbols.global_functions.read();
                    for &name in candidates {
                        if let Some((_, info)) = fmap.get(name) {
                            return Some(info.clone());
                        }
                    }
                }
            }
        }

        // ── Phase 2: Try embedded PHP stubs ──
        // The stub_function_index maps function names (including namespaced
        // ones like "Brotli\\compress") to the raw PHP source of the file
        // that defines them.
        for &name in candidates {
            // Copy the `&'static str` out: the guard must not be held across
            // the `global_functions` write the load makes.
            let stub_content = self.stub_function_index.read().get(name).copied();
            if let Some(stub_content) = stub_content
                && let Some(function) = self.load_stub_function(name, stub_content)
            {
                return Some(function);
            }
        }

        // Cache the negative result so subsequent lookups for the same
        // unknown function skip the fallback phases.  Another thread may
        // have parsed a file declaring one of the candidates since Phase 1
        // (Phase 1.75 skips files it did not parse itself), so re-check
        // under the write lock: `update_ast` inserts into
        // `global_functions` before it retires negatives, so either the
        // declaration is visible here or its retirement runs after us.
        {
            let mut nf_cache = self.symbols.function_not_found_cache.write();
            let fmap = self.symbols.global_functions.read();
            if let Some((_, info)) = candidates.iter().find_map(|&name| fmap.get(name)) {
                return Some(info.clone());
            }
            drop(fmap);
            // An index insert since this lookup began may have added the
            // function after the lookup read the index.
            if self.symbols.function_index_generation() == index_generation {
                for &name in candidates {
                    nf_cache.insert(name);
                }
            }
        }
        None
    }

    /// Parse the embedded stub file that declares the function `name`,
    /// cache everything it declares, and return that function.
    ///
    /// Every function in the file goes into `global_functions` under the
    /// file's stub URI, corrected by [`crate::stub_patches`], so a later
    /// lookup of any of them is answered by Phase 1.  The classes the file
    /// declares are cached in `uri_classes_index` under the same URI, so a
    /// type one of its functions names (a return type, say) resolves too.
    ///
    /// `None` when the file declares no function spelled `name`.
    fn load_stub_function(&self, name: &str, stub_content: &str) -> Option<FunctionInfo> {
        // The functions, the namespace and the classes below are all read
        // from this one parse.
        let _parse_guard = crate::parser::with_parse_cache(stub_content);
        let ver = Some(self.php_version());
        let mut functions = self.parse_functions_versioned(stub_content, ver);
        if functions.is_empty() {
            return None;
        }

        // Apply stub patches for phpstorm-stubs deficiencies (e.g.
        // array_reduce returning `mixed` instead of a template-based type).
        for func in &mut functions {
            crate::stub_patches::apply_function_stub_patches(func);
        }

        let stub_uri = format!("{}{}", crate::stubs::STUB_FUNCTION_URI_PREFIX, name);
        let mut result: Option<FunctionInfo> = None;
        {
            let mut fmap = self.symbols.global_functions.write();
            for func in &functions {
                let fqn = if let Some(ref ns) = func.namespace {
                    format!("{}\\{}", ns, func.name)
                } else {
                    func.name.to_string()
                };

                if result.is_none()
                    && (fqn.eq_ignore_ascii_case(name) || func.name.eq_ignore_ascii_case(name))
                {
                    result = Some(func.clone());
                }

                // Cache the FQN so future lookups hit Phase 1.  No short-name
                // fallback: `resolve_function_name` already builds
                // namespace-qualified candidates.
                fmap.or_insert_with(fqn, || (stub_uri.clone(), func.clone()));
            }
        }

        let mut classes = Self::parse_php_versioned(stub_content, ver);
        if !classes.is_empty() {
            let empty_use_map = HashMap::new();
            let stub_namespace = self.parse_namespace(stub_content);
            Self::resolve_parent_class_names(
                &mut classes,
                &empty_use_map,
                &empty_use_map,
                &stub_namespace,
            );
            let arc_classes: Vec<Arc<ClassInfo>> = crate::types::into_arcs(classes);
            self.symbols
                .uri_classes_index
                .write()
                .insert(stub_uri, arc_classes);
        }

        result
    }

    // ─── Shared Name Resolution ─────────────────────────────────────────────

    /// Find the class an unqualified `name` means in `file_namespace`
    /// itself: a class of the same file first, then the
    /// namespace-qualified FQN.
    ///
    /// Only a local class in the current namespace counts: in a file with
    /// several `namespace` blocks, a same-named class of another block is
    /// a different class.  In the global namespace the qualified lookup is
    /// left to the caller.
    pub(super) fn find_in_own_namespace(
        &self,
        name: &str,
        local_classes: &[Arc<ClassInfo>],
        file_namespace: &Option<String>,
    ) -> Option<Arc<ClassInfo>> {
        let lookup = short_name(name);
        let ns_matches = |c: &ClassInfo| match (&c.file_namespace, file_namespace) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (None, None) => true,
            _ => false,
        };
        if let Some(cls) = local_classes
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(lookup) && ns_matches(c))
        {
            return Some(Arc::clone(cls));
        }
        let ns = file_namespace.as_ref()?;
        self.find_or_load_class(&format!("{}\\{}", ns, name))
    }

    /// Resolve a function name using the file's imports and namespace.
    ///
    /// Equivalent to [`resolve_function_name_at`](Self::resolve_function_name_at)
    /// with no per-offset name resolution.  Prefer the offset-aware
    /// variant whenever the call expression's byte offset is available,
    /// so functions declared in a different `namespace` block of the same
    /// file resolve correctly.
    pub(crate) fn resolve_function_name(
        &self,
        name: &str,
        use_map: &HashMap<String, String>,
        function_imports: &HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<FunctionInfo> {
        self.resolve_function_name_at(name, None, 0, use_map, function_imports, file_namespace)
    }

    /// Whether discovery or full parsing has seen this exact function FQN.
    ///
    /// This is the allocation-free membership form used when a caller only
    /// needs to know whether PHP's namespace-local function shadows a global
    /// fallback. It deliberately does not trigger a lazy parse.
    pub(crate) fn has_indexed_function(&self, fqn: &str) -> bool {
        self.symbols.global_functions.read().get(fqn).is_some()
            || self
                .symbols
                .autoload_function_index
                .read()
                .get(fqn)
                .is_some()
    }

    /// Whether discovery or full parsing has seen this exact class FQN.
    ///
    /// Laravel's optional root facade aliases are used only when no real
    /// global class owns the same name. This membership check does not lazy
    /// load the class, keeping completion on that fallback allocation-free.
    pub(crate) fn has_indexed_class(&self, fqn: &str) -> bool {
        self.symbols.fqn_uri_index.read().get(fqn).is_some()
    }

    /// Resolve a function name, consulting mago-names' per-offset
    /// resolution for the authoritative fully-qualified name.
    ///
    /// A file may declare more than one `namespace` block.  The single
    /// `file_namespace` describes only one of them, so a call to a
    /// function declared in a *different* block would never build the
    /// right candidate from the use-map / namespace guess alone.
    /// `resolved_names` understands which block `offset` falls in and
    /// qualifies the name accordingly (e.g. `foo()` inside `namespace A`
    /// resolves to `A\foo`), so the FQN under which the function is
    /// already stored in `global_functions` becomes a candidate.
    ///
    /// `offset` is the starting byte offset of the function-name
    /// identifier in the source file.  Pass `0` (and/or `None`
    /// `resolved_names`) when no offset is available; the name is then
    /// qualified from the imports and `file_namespace`: the first segment
    /// of a qualified name through the class imports (`use_map`), a bare
    /// name through `function_imports`.
    ///
    /// Either way the lookup follows PHP's rule (see
    /// [`function_lookup_names`](crate::names::function_lookup_names)): only
    /// a bare name that is not imported falls back to the global function.
    pub(crate) fn resolve_function_name_at(
        &self,
        name: &str,
        resolved_names: Option<&crate::names::OwnedResolvedNames>,
        offset: u32,
        use_map: &HashMap<String, String>,
        function_imports: &HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<FunctionInfo> {
        // Offset-aware, multi-namespace-correct name from mago-names.
        // `offset == 0` is the "no offset available" sentinel (no PHP
        // function-call identifier ever starts at byte 0, before `<?php`).
        let offset_resolved = (offset != 0)
            .then(|| resolved_names.and_then(|rn| rn.get(offset)))
            .flatten();
        let (fqn, fallback) =
            crate::names::function_lookup_names(name, use_map, function_imports, file_namespace);
        let fqn = offset_resolved.unwrap_or(&fqn);
        match fallback {
            Some(global) if global != fqn => self.find_or_load_function(&[fqn, global]),
            _ => self.find_or_load_function(&[fqn]),
        }
    }
}
