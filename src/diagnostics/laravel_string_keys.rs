//! The Laravel checks that judge a string key, and the command-signature
//! check that shares their shape.
//!
//! Both read the spans the symbol map recorded for a file, resolve each name
//! against what the project actually registers (routes, config keys,
//! views, translation keys, console commands, morph aliases, gate
//! abilities), and report the ones nothing declares.

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use super::helpers;
use crate::Backend;

/// The [`crate::symbol_map::LaravelStringKind`]s
/// [`Backend::collect_invalid_laravel_string_key_diagnostics`] can judge.
///
/// The kinds it cannot are dropped when the spans are gathered rather than
/// carried to the check and skipped there, so the reason each one is left
/// alone is written once: a Blade section or stack name is judged by the
/// Blade pass, which knows the templates around the one it is written in,
/// and a container binding key is judged by nothing at all, since anything
/// can be bound at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckedStringKind {
    Route,
    Config,
    View,
    Trans,
    Command,
    MorphAlias,
    GateAbility,
}

impl Backend {
    /// Emit a warning for each `$this->argument('x')` / `$this->option('x')`
    /// whose name is not a parameter of the enclosing command's `$signature`.
    pub(super) fn collect_invalid_command_param_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        use crate::symbol_map::SymbolKind;

        // (name, is_option, start, end) for each own-param span.
        let spans: Vec<(String, bool, u32, u32)> = {
            let maps = self.symbol_maps.read();
            let Some(symbol_map) = maps.get(uri) else {
                return;
            };
            symbol_map
                .spans
                .iter()
                .filter_map(|span| {
                    if let SymbolKind::CommandOwnParam { name, is_option } = &span.kind {
                        Some((name.clone(), *is_option, span.start, span.end))
                    } else {
                        None
                    }
                })
                .collect()
        };
        if spans.is_empty() {
            return;
        }

        // The spans of one command class all share its signature, so it is
        // resolved on the first of them and reused until a span falls outside
        // that class' body.
        let mut enclosing: Option<crate::virtual_members::laravel::EnclosingCommand> = None;

        for (name, is_option, start, end) in &spans {
            if !enclosing
                .as_ref()
                .is_some_and(|command| command.body.contains(start))
            {
                enclosing = crate::virtual_members::laravel::command_enclosing_signature(
                    content,
                    *start as usize,
                );
            }
            // A class that declares no `$signature` (e.g. a `$name`-only or
            // dynamically-built command) has nothing to validate against.
            let Some(signature) = enclosing.as_ref().and_then(|c| c.signature.as_ref()) else {
                continue;
            };
            let known = if *is_option {
                signature.option(name).is_some()
            } else {
                signature.argument(name).is_some()
            };
            if !known
                && let Some(range) =
                    self.offset_range_to_lsp_range(uri, content, *start as usize, *end as usize)
            {
                let label = if *is_option { "option" } else { "argument" };
                out.push(helpers::make_diagnostic(
                    range,
                    DiagnosticSeverity::WARNING,
                    "invalid_command_parameter",
                    format!("Unknown command {}: '{}'", label, name),
                ));
            }
        }
    }

    /// Emit a warning for each `LaravelStringKey` span whose key does
    /// not resolve to any declaration (typo in route name, config key,
    /// view name, or translation key).
    ///
    /// Only the kinds this pass can judge reach the check itself; see
    /// [`CheckedStringKind`].
    pub(super) fn collect_invalid_laravel_string_key_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        use crate::symbol_map::{LaravelStringKind, SymbolKind};

        // Extract the LaravelStringKey spans we need and determine which
        // kinds are present, then DROP the read lock before calling
        // enumeration functions.  Those functions call
        // `user_file_symbol_maps()` → `ensure_workspace_indexed()` →
        // `parse_files_parallel()` → `update_ast()` which acquires a
        // WRITE lock on `symbol_maps`.  Holding a read lock here while
        // that write is attempted would deadlock.
        let mut has_route = false;
        let mut has_config = false;
        let mut has_view = false;
        let mut has_trans = false;
        let mut has_command = false;
        let mut has_morph_alias = false;
        let mut has_gate_ability = false;
        let key_spans: Vec<(CheckedStringKind, String, u32, u32)> = {
            let Some(symbol_map) = self.symbol_maps.read().get(uri).cloned() else {
                return;
            };
            let extra = self.typed_receiver_view_spans_for(uri, &symbol_map);
            symbol_map
                .spans
                .iter()
                .chain(extra.iter())
                .filter_map(|span| {
                    if let SymbolKind::LaravelStringKey {
                        kind,
                        key,
                        is_write,
                        is_optional,
                    } = &span.kind
                    {
                        // A write declares the key it names, so there is
                        // nothing to check it against, and an optional key
                        // is one the call is written to do without: an
                        // `@includeFirst` candidate that names nothing is
                        // why the directive takes a list at all.
                        if *is_write || *is_optional {
                            return None;
                        }
                        let checked = match kind {
                            LaravelStringKind::Route => {
                                has_route = true;
                                CheckedStringKind::Route
                            }
                            LaravelStringKind::Config => {
                                has_config = true;
                                CheckedStringKind::Config
                            }
                            LaravelStringKind::View => {
                                has_view = true;
                                CheckedStringKind::View
                            }
                            LaravelStringKind::Trans => {
                                has_trans = true;
                                CheckedStringKind::Trans
                            }
                            LaravelStringKind::Command => {
                                has_command = true;
                                CheckedStringKind::Command
                            }
                            LaravelStringKind::MorphAlias => {
                                has_morph_alias = true;
                                CheckedStringKind::MorphAlias
                            }
                            LaravelStringKind::GateAbility => {
                                has_gate_ability = true;
                                CheckedStringKind::GateAbility
                            }
                            // A section or stack name is judged against the
                            // templates that render the one it is written
                            // in, which the Blade pass below has and this
                            // one does not.  And anything at all can be bound
                            // at runtime, so an unrecognised container key
                            // proves nothing — nor does an environment
                            // variable absent from `.env`, since the
                            // environment a process runs with is not on disk.
                            LaravelStringKind::Section
                            | LaravelStringKind::Stack
                            | LaravelStringKind::ContainerBinding
                            | LaravelStringKind::Env => return None,
                        };
                        Some((checked, key.clone(), span.start, span.end))
                    } else {
                        None
                    }
                })
                .collect()
        };

        if !has_route
            && !has_config
            && !has_view
            && !has_trans
            && !has_command
            && !has_morph_alias
            && !has_gate_ability
        {
            return;
        }

        // Enumerate valid keys once per kind (lazy), using the cached
        // enumerations.  Safe to call now that the `symbol_maps` read
        // lock has been released.  The enumerations are shared with every
        // other consumer, so they are read in place rather than copied
        // into a set per pass.
        let routes = has_route.then(|| self.cached_routes());
        // A package with no routes of its own, whose names are registered
        // by the host application, cannot be judged: the valid set is
        // unknown, not empty.  Installed packages register routes of their
        // own, so the question is whether *this project* contributed any,
        // not whether the set is empty.
        let project_registers_routes = routes
            .as_ref()
            .is_some_and(|discovery| discovery.routes.iter().any(|route| !route.from_vendor));
        // A library is installed into an application that declares the
        // configuration it reads, and that application is a file we never
        // see, so none of its keys can be judged.  Only an application owns
        // the whole of its configuration.
        let has_config = has_config && self.is_application_project();
        let declared_config_keys: Arc<[String]> = if has_config {
            self.cached_config_keys()
        } else {
            Arc::default()
        };
        let view_keys: Arc<[String]> = if has_view {
            self.cached_view_names()
        } else {
            Arc::default()
        };
        let trans_keys: Arc<[String]> = if has_trans {
            self.cached_trans_keys()
        } else {
            Arc::default()
        };
        // An application whose strings live in a database still has `vendor/`'s
        // own `lang/` files on disk, so the enumerated set is non-empty while
        // covering none of the application's own keys.  Once a provider has
        // rebound the translator away from Laravel's file loader, what is valid
        // is unknowable.
        let trans_source_is_unknowable = has_trans
            && self
                .laravel_provider_resources
                .read()
                .custom_translation_loader;
        // When no commands were indexed at all, skip command diagnostics
        // entirely.  The scan is heuristic (it relies on the `*Command`
        // naming convention), so an empty index likely means discovery
        // failed rather than that every referenced command is invalid.
        let commands_indexed = has_command && !self.laravel_commands.read().is_empty();
        // A morph alias is only checkable when the project calls
        // `Relation::enforceMorphMap()` / `requireMorphMap()`.  Without that,
        // an unmapped model still morphs under its class name, so the set of
        // valid `*_type` values is open and an unknown alias proves nothing.
        let morph_map_enforced = has_morph_alias && self.laravel_morph_map.read().is_enforced();

        // Abilities are only checkable when the project defines some: an
        // empty set means gate discovery found nothing (a project that
        // authorizes entirely through runtime-registered callbacks, or one
        // that is not really using Laravel's gate), not that every ability
        // referenced is wrong.
        //
        // A `Gate::before()` callback, or a package that answers checks from a
        // permission table, grants abilities that appear nowhere in source.  A
        // single unrelated `Gate::define()` call is enough to make the
        // enumerated set non-empty, so emptiness alone does not catch this: the
        // ability space is open and the whole check has to stand down —
        // including the walk of every policy class that would enumerate it.
        let gate_ability_space_is_open =
            has_gate_ability && self.laravel_gates.read().ability_space_is_open();
        let gate_abilities: Arc<[String]> = if has_gate_ability && !gate_ability_space_is_open {
            self.cached_gate_abilities()
        } else {
            Arc::default()
        };

        for (kind, key, start, end) in &key_spans {
            let (valid, label, code) = match kind {
                // An ability is judged against the model the check names, so
                // it reports which model rather than the shared
                // "Unknown <kind>: '<key>'" message the others share.
                CheckedStringKind::GateAbility => {
                    if !gate_ability_space_is_open
                        && !gate_abilities.is_empty()
                        && let Some(message) =
                            self.gate_ability_problem(uri, content, key, *start, &gate_abilities)
                        && let Some(range) = self.offset_range_to_lsp_range(
                            uri,
                            content,
                            *start as usize,
                            *end as usize,
                        )
                    {
                        out.push(helpers::make_diagnostic(
                            range,
                            DiagnosticSeverity::WARNING,
                            "invalid_laravel_ability",
                            message,
                        ));
                    }
                    continue;
                }
                CheckedStringKind::Route => {
                    let Some(discovery) = &routes else {
                        continue;
                    };
                    if !project_registers_routes {
                        continue;
                    }
                    // A group whose `->name()` argument was not a string
                    // literal (e.g. Filament's `Route::name($panelId . '.')`
                    // has children we cannot enumerate statically.  Any route
                    // that falls under such a prefix is unjudgeable, and so
                    // is any route ending in one of the names such a group
                    // registers, even when it recorded no known prefix at all
                    // (e.g. a group with no enclosing literal group whose own
                    // name is entirely a variable).
                    if discovery
                        .open_prefixes
                        .iter()
                        .any(|prefix| key.starts_with(prefix))
                        || discovery
                            .open_suffixes
                            .iter()
                            .any(|suffix| key.ends_with(suffix))
                    {
                        continue;
                    }
                    // A `Route::is('admin.*')` check names a pattern rather
                    // than one route, and matches whatever the project has
                    // under it.
                    let valid = if key.contains('*') {
                        discovery.names.iter().any(|name| {
                            crate::virtual_members::laravel::route_name_matches(key, name)
                        })
                    } else {
                        discovery.names.binary_search(key).is_ok()
                    };
                    (valid, "route", "invalid_laravel_route")
                }
                CheckedStringKind::Config => {
                    // Only judge a key whose config file we actually read.
                    // An unknown root means the file never reached us, so
                    // the key cannot be wrong as far as we can tell, while
                    // a typo inside a file we did read is still caught.  A
                    // runtime write is deliberately not a root of its own:
                    // the keys one file writes say nothing about what the
                    // rest of that namespace holds, least of all when the
                    // writes that established it were spelled dynamically.
                    let root = key.split('.').next().unwrap_or(key.as_str());
                    if !names_key_or_group(&declared_config_keys, root) {
                        continue;
                    }
                    // Config keys may be partial prefixes (e.g. `config('app')`)
                    // which are valid even without a direct match.  A key that
                    // `Config::set()` or the array form of the `config()`
                    // helper establishes is as real as one a `config/` file
                    // declares; a test that configures a disk in `setUp()`
                    // before exercising it is the common shape.
                    let valid = names_key_or_group(&declared_config_keys, key)
                        || self.runtime_config_key_covers(key);
                    (valid, "config key", "invalid_laravel_config")
                }
                CheckedStringKind::View => (
                    view_keys.binary_search(key).is_ok(),
                    "view",
                    "invalid_laravel_view",
                ),
                CheckedStringKind::Trans => {
                    // When no translation files are found at all, skip trans
                    // diagnostics entirely.  This avoids false positives in
                    // non-Laravel projects (WordPress, GetText) that also use
                    // `__()` or `trans()` as function names.
                    if trans_keys.is_empty() || trans_source_is_unknowable {
                        continue;
                    }
                    let valid = names_key_or_group(&trans_keys, key);
                    (valid, "translation key", "invalid_laravel_trans")
                }
                CheckedStringKind::Command => {
                    if !commands_indexed {
                        continue;
                    }
                    (
                        self.laravel_commands.read().contains_name(key),
                        "command",
                        "invalid_laravel_command",
                    )
                }
                CheckedStringKind::MorphAlias => {
                    if !morph_map_enforced {
                        continue;
                    }
                    (
                        self.laravel_morph_map.read().has_alias(key),
                        "morph type",
                        "invalid_laravel_morph_alias",
                    )
                }
            };
            if !valid
                && let Some(range) =
                    self.offset_range_to_lsp_range(uri, content, *start as usize, *end as usize)
            {
                out.push(helpers::make_diagnostic(
                    range,
                    DiagnosticSeverity::WARNING,
                    code,
                    format!("Unknown {}: '{}'", label, key),
                ));
            }
        }
    }

    /// Judge one authorization ability, returning the diagnostic message when
    /// it is wrong and `None` when it checks out.
    ///
    /// A check that names a model (`$user->can('update', $post)`,
    /// `Gate::allows('update', Post::class)`) is judged against *that model's*
    /// policy, so a real ability used on the wrong model is caught and named
    /// as such.  A `Gate::define()` registration applies to any subject, so it
    /// satisfies a model-bound check too.  When the model cannot be resolved —
    /// or the call names none — the ability only has to exist somewhere.
    fn gate_ability_problem(
        &self,
        uri: &str,
        content: &str,
        ability: &str,
        span_start: u32,
        known_abilities: &[String],
    ) -> Option<String> {
        let is_defined = self.laravel_gates.read().definition(ability).is_some();
        if is_defined {
            return None;
        }

        if let Some(model_fqn) = self.gate_subject_model(uri, content, span_start)
            && let Some((policy, abilities)) =
                crate::virtual_members::laravel::model_policy_abilities(self, &model_fqn)
        {
            if abilities
                .iter()
                .any(|name| name.eq_ignore_ascii_case(ability))
            {
                return None;
            }
            return Some(format!(
                "Ability '{}' is not defined for '{}' (policy {})",
                ability,
                model_fqn,
                policy.fqn()
            ));
        }

        if known_abilities
            .binary_search_by(|known| known.as_str().cmp(ability))
            .is_ok()
        {
            return None;
        }
        Some(format!("Unknown ability: '{}'", ability))
    }

    /// The FQN of the model a gate check named, when the symbol map recorded
    /// one and it resolves to a class.
    fn gate_subject_model(&self, uri: &str, content: &str, span_start: u32) -> Option<String> {
        // A Blade file's symbol map is built from the preprocessed virtual
        // PHP, so every offset in it — including the subject's — indexes that
        // text rather than the template the caller handed us.
        let virtual_php = self.blade_virtual_php_arc(uri);
        let content = virtual_php.as_ref().map_or(content, |php| php.as_str());

        let (subject_text, is_static) = {
            let maps = self.symbol_maps.read();
            let map = maps.get(uri)?;
            // The subject is stored as a range into the text the map was
            // built from, so a map built from older text would slice the
            // wrong bytes (or none at all).
            let source = map.source(content)?;
            let subject = map.gate_subject(span_start)?;
            (
                subject.subject_text.as_str(source).to_string(),
                subject.is_static,
            )
        };

        let ctx = self.file_context(uri);
        let class_loader = self.class_loader(&ctx);
        let function_loader = self.function_loader(&ctx);
        let resolution_ctx = crate::type_engine::subject_resolution::SubjectResolutionCtx {
            local_classes: &ctx.classes,
            use_map: &ctx.use_map,
            namespace: &ctx.namespace,
            content,
            class_loader: &class_loader,
            backend: Some(self),
            function_loader: &function_loader,
        };
        let name = crate::type_engine::subject_resolution::resolve_subject_type(
            &subject_text,
            is_static,
            span_start,
            &resolution_ctx,
        )?
        .top_level_class_names()
        .into_iter()
        .next()?;
        // The resolved type carries the name as written, so run it back
        // through the loader to canonicalize a short name against the file's
        // imports before looking up the model's policy.
        Some(class_loader(&name)?.fqn().to_string())
    }
}

/// Whether the sorted `keys` hold `key` itself or a key nested under it
/// (`app` for `app.name`), which is what makes a partial prefix such as
/// `config('app')` valid.
fn names_key_or_group(keys: &[String], key: &str) -> bool {
    if keys.binary_search_by(|k| k.as_str().cmp(key)).is_ok() {
        return true;
    }
    let group = format!("{key}.");
    let first_under = keys.partition_point(|k| k.as_str() < group.as_str());
    keys.get(first_under).is_some_and(|k| k.starts_with(&group))
}

#[cfg(test)]
mod tests {

    /// Regression test: `collect_invalid_laravel_string_key_diagnostics`
    /// must not hold a `symbol_maps` read lock while calling enumeration
    /// functions that reach `ensure_workspace_indexed()` →
    /// `parse_files_parallel()` → `update_ast()` → `symbol_maps.write()`.
    ///
    /// Before the fix, this deadlocked because the read lock was held
    /// for the entire function body.  The fix extracts the needed spans
    /// into an owned `Vec` and drops the lock before enumerating keys.
    ///
    /// To trigger the deadlock path, we create a workspace with an
    /// unindexed PHP file so `ensure_workspace_indexed` must parse it
    /// (acquiring a write lock).  A 5-second timeout catches the
    /// deadlock as a test failure instead of an infinite hang.
    #[test]
    fn laravel_string_key_diagnostics_no_deadlock() {
        // Set up a temp workspace with an unindexed PHP file so that
        // ensure_workspace_indexed() will call parse_files_parallel()
        // which needs a write lock on symbol_maps.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let tmp = dir.path().to_path_buf();
        let unindexed_file = tmp.join("Unindexed.php");
        std::fs::write(&unindexed_file, "<?php\nclass Unindexed {}\n").unwrap();

        let backend = crate::Backend::new_test_with_workspace(tmp.clone(), vec![]);

        // Register the unindexed file in fqn_uri_index so
        // ensure_workspace_indexed Phase 1 will try to parse it.
        let unindexed_uri = format!("file://{}", unindexed_file.to_str().unwrap());
        backend
            .symbols
            .fqn_uri_index
            .write()
            .insert("Unindexed".to_string(), unindexed_uri);

        // Parse a file with Laravel string key spans.
        let uri = "file:///app/Http/test.php";
        let php = "<?php\nconfig('app.name');\nroute('home');\n";
        backend.update_ast(uri, php);

        // Run the diagnostics in a thread with a timeout so a deadlock
        // is caught as a failure rather than an infinite hang.
        let (tx, rx) = std::sync::mpsc::channel();
        let backend = std::sync::Arc::new(backend);
        let bc = std::sync::Arc::clone(&backend);
        std::thread::spawn(move || {
            let mut out = Vec::new();
            bc.collect_slow_diagnostics(uri, php, &mut out);
            let _ = tx.send(out);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(_diags) => { /* success — no deadlock */ }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!(
                    "collect_slow_diagnostics deadlocked: symbol_maps read lock \
                     was likely held while enumeration functions tried to write"
                );
            }
            Err(e) => panic!("collect_slow_diagnostics failed: {:?}", e),
        }
    }
}
