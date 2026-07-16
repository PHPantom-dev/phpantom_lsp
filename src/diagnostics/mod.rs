//! Diagnostics — collect and deliver LSP diagnostics for PHP files.
//!
//! This module collects diagnostics from multiple providers and delivers
//! them to the editor.
//!
//! ## Diagnostic code naming convention
//!
//! Every diagnostic has a `code` string that identifies the rule. When adding
//! a new diagnostic, follow these rules:
//!
//! 1. **All `snake_case`, no dots or other separators.**
//! 2. **Codes read as noun phrases describing the problem**, not bare
//!    categories. Prefer `argument_count_mismatch` over `argument_count`.
//! 3. **`unknown_*`** — symbol could not be resolved (class, function,
//!    member, variable).
//! 4. **`unused_*`** — symbol is defined/imported but never referenced.
//! 5. **`type_mismatch_*`** — a value's type doesn't satisfy a constraint.
//! 6. **`missing_*`** — a required declaration is absent (e.g.
//!    `missing_implementation` for unimplemented interface methods).
//! 7. **`invalid_*`** — a structural/syntactic violation (e.g.
//!    `invalid_class_kind`).
//! 8. **`deprecated_usage`** — usage of a deprecated symbol.
//! 9. **`syntax_error`** — parser-level errors.
//! 10. **`unresolved_*`** — the analyser couldn't determine the type of an
//!     expression (opt-in coverage hints).
//!
//! Two delivery models are supported:
//!
//! - **Pull model** (`textDocument/diagnostic`, LSP 3.17) — the editor
//!   requests diagnostics when it needs them.  Only visible files are
//!   diagnosed.  Cross-file invalidation uses `workspace/diagnostic/refresh`.
//!   This is the preferred model when the client supports it.
//!
//! - **Push model** (`textDocument/publishDiagnostics`) — the server
//!   pushes diagnostics after every edit.  Used as a fallback for clients
//!   that do not advertise pull-diagnostic support.
//!
//! Providers are grouped into three phases so that cheap results appear
//! immediately and expensive external tools never block native feedback:
//!
//! ## Phase 1 — fast (no type resolution)
//!
//! - **Syntax error diagnostics** — surface parse errors from the Mago
//!   parser as Error-severity diagnostics.  The most fundamental
//!   diagnostic: without it, a user with a typo gets no feedback until
//!   they try to run the code.
//! - **`@deprecated` usage diagnostics** — report references to symbols
//!   marked `@deprecated` with `DiagnosticTag::Deprecated` (renders as
//!   strikethrough in most editors).
//! - **Unused `use` dimming** — dim `use` declarations that are not
//!   referenced anywhere in the file with `DiagnosticTag::Unnecessary`.
//!
//! ## Phase 2 — slow (require type resolution)
//!
//! - **Unknown class diagnostics** — report `ClassReference` spans that
//!   cannot be resolved through any resolution phase (use-map, local
//!   classes, same-namespace, fqn_uri_index, PSR-4, stubs).
//! - **Unknown member diagnostics** — report `MemberAccess` spans where
//!   the member does not exist on the resolved class after full
//!   resolution (inheritance + virtual member providers).  Suppressed
//!   when the class has `__call` / `__callStatic` / `__get` magic methods.
//! - **Unknown function diagnostics** — report function calls that
//!   cannot be resolved to any known function definition.
//! - **Undefined variable diagnostics** — report variable reads that
//!   have no prior definition (assignment, parameter, foreach binding,
//!   catch variable, `global`, `static`, `use()` clause, or `list()`
//!   destructuring) in the same scope.  Uses a conservative Phase 1
//!   approach: any assignment anywhere in the function counts as a
//!   definition.  Suppressed for superglobals, `isset()` / `empty()`
//!   guards, `compact()` references, `extract()` calls, variable
//!   variables (`$$`), `@` error suppression, and `@var` annotations.
//! - **Unresolved member access diagnostics** (opt-in) — report
//!   `MemberAccess` spans where the **subject type** cannot be resolved
//!   at all.  Off by default; enable via `[diagnostics]
//!   unresolved-member-access = true` in `.phpantom.toml`.  Uses
//!   `Severity::HINT` to surface type-coverage gaps without drowning
//!   the editor in warnings.
//! - **Argument count diagnostics** — report calls where the number of
//!   arguments does not match the function/method signature.
//! - **Implementation error diagnostics** — report concrete classes that
//!   fail to implement all required methods from their interfaces or
//!   abstract parents.  Reuses the same missing-method detection as the
//!   "Implement missing methods" code action.
//!
//! ## Phase 3 — heavy (external process, dedicated workers)
//!
//! - **PHPStan proxy diagnostics** — run PHPStan in editor mode
//!   (`--tmp-file` / `--instead-of`) and surface its errors as LSP
//!   diagnostics.  Auto-detected via `vendor/bin/phpstan` or `$PATH`;
//!   configurable in `.phpantom.toml` under `[phpstan]`.
//!
//!   PHPStan runs in a **dedicated worker task**, separate from the
//!   main diagnostic worker, because it is extremely slow and
//!   resource-intensive.  At most one PHPStan process runs at a time.
//!   If edits arrive while PHPStan is running, the pending URI is
//!   updated and the worker picks it up after the current run finishes.
//!   Native diagnostics (phases 1 and 2) are never blocked.
//!
//! - **PHPCS proxy diagnostics** — run PHP_CodeSniffer via
//!   `phpcs --report=json` and surface coding standard violations as
//!   LSP diagnostics.  Auto-detected when `squizlabs/php_codesniffer`
//!   is in `require-dev`; configurable under `[phpcs]`.
//!
//!   PHPCS runs in its own **dedicated worker task**, following the
//!   same pattern as the PHPStan worker.  At most one PHPCS process
//!   runs at a time, with the same debounce and pending-URI slot
//!   design.
//!
//! - **Mago lint proxy diagnostics** — run `mago lint --reporting-format
//!   json --stdin-input` and surface AST-level lint issues (style,
//!   naming, code smells) as LSP diagnostics.  Auto-detected when
//!   `mago.toml` exists at the workspace root and `vendor/bin/mago` or
//!   `mago` on `$PATH` is available; configurable under `[mago]`.
//!
//!   Mago lint runs in its own **dedicated worker task**, following the
//!   same pattern as the PHPCS worker.  Source: `"mago-lint"`.
//!
//! - **Mago analyze proxy diagnostics** — run `mago analyze
//!   --reporting-format json --stdin-input` and surface type-aware
//!   analysis issues (type mismatches, unreachable code, unused
//!   definitions) as LSP diagnostics.  Same auto-detection as Mago lint.
//!
//!   Mago analyze runs in its own **dedicated worker task**, following
//!   the same pattern as the PHPStan worker.  Source: `"mago-analyze"`.
//!
//! ## Publishing strategy
//!
//! Each diagnostic source has its own per-URI cache:
//!
//! | Cache                    | Source             |
//! | ------------------------ | ------------------ |
//! | `diag_last_fast`         | syntax, unused use |
//! | `diag_last_slow`         | type resolution    |
//! | `phpstan_last_diags`     | PHPStan            |
//! | `phpcs_last_diags`       | PHPCS              |
//! | `mago_lint_last_diags`   | Mago lint          |
//! | `mago_analyze_last_diags`| Mago analyze       |
//!
//! When any source finishes, [`Backend::assemble_and_push`] reads all
//! per-source caches for the URI, merges them into a single set,
//! deduplicates, and filters suppressions.
//!
//! **Push mode:** The merged set is published via
//! `textDocument/publishDiagnostics`.  As each source finishes, its
//! cache is updated and the full assembled set is pushed.  The user
//! sees results incrementally: fast diagnostics first, then slow,
//! then PHPStan/PHPCS/Mago as each completes.
//!
//! **Pull mode:** Only fast diagnostics (syntax errors, unused
//! imports, unused variables) are pushed via `publishDiagnostics` so
//! the editor sees them instantly.  The full merged set is cached in
//! `diag_last_full` with a bumped `resultId`.  The pull handler
//! (`textDocument/diagnostic`) returns this cached set.  If the
//! cache is missing (e.g. the file was just opened), the pull
//! handler triggers computation directly instead of returning empty
//! results.  Pushing the full set in pull mode would duplicate every
//! slow and external diagnostic because editors merge pushed and
//! pulled sets additively.
//!
//! External tool workers (PHPStan, PHPCS, Mago) use their own
//! debounce timers in both modes because they are expensive.

mod argument_count;
pub(crate) mod class_name_mismatch;
mod deprecated;
pub(crate) mod helpers;
pub(crate) mod ignore_rules;
mod implementation_errors;
mod invalid_class_kind;
pub(crate) mod namespace_mismatch;
mod syntax_errors;
mod type_errors;
pub(crate) mod undefined_variables;
pub(crate) mod unknown_classes;
pub(crate) mod unknown_functions;
pub(crate) mod unknown_members;
pub(crate) mod unresolved_member_access;
mod unused_imports;
pub(crate) mod unused_variables;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::mago;
use crate::phpcs;
use crate::phpstan;
use crate::util::ranges_overlap;

// ── Shared helpers ──────────────────────────────────────────────────────────

impl Backend {
    /// Returns `true` if the URI should be skipped for diagnostics
    /// (stub files only).  Vendor files are not skipped because
    /// diagnostics only run on files the user has open in the editor,
    /// and users working in monorepos or with `--prefer-source`
    /// packages legitimately edit vendor files.
    fn should_skip_diagnostics(&self, uri_str: &str) -> bool {
        uri_str.starts_with("phpantom-stub://") || uri_str.starts_with("phpantom-stub-fn://")
    }

    /// Collect Phase 1 (fast) diagnostics: syntax errors, unused
    /// imports.  These are cheap — no type resolution.
    pub(crate) fn collect_fast_diagnostics(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        self.collect_syntax_error_diagnostics(uri_str, content, out);
        self.collect_unused_import_diagnostics(uri_str, content, out);
        self.collect_unused_variable_diagnostics(uri_str, content, out);
        self.collect_namespace_mismatch_diagnostics(uri_str, content, out);
        self.collect_class_name_mismatch_diagnostics(uri_str, content, out);
    }

    /// Collect Phase 2 (slow) diagnostics: unknown class/member/function,
    /// argument count, implementation errors, deprecated usage.  These
    /// require type resolution and are expensive.
    pub fn collect_slow_diagnostics(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // Activate the chain resolution cache so that all slow
        // diagnostic collectors share cached intermediate chain
        // prefix results (e.g. `$model->where(...)` resolved once
        // and reused by `$model->where(...)->whereNotNull(...)`).
        // This eliminates O(depth²) re-resolution of shared chain
        // prefixes across unknown_member, argument_count, type_error,
        // and deprecated collectors.
        let _chain_guard = crate::completion::resolver::with_chain_resolution_cache();

        // Activate the callable target cache so that the same method
        // on the same class is resolved at most once across all
        // diagnostic collectors.  For example, `Builder::where` is
        // looked up once and reused for every `$q->where(...)`,
        // `$query->where(...)`, and `Product::query()->where(...)`
        // call site in the file.
        let _callable_guard = crate::completion::call_resolution::with_callable_target_cache();
        let _body_infer_guard = self.activate_body_return_inferrer();
        let _auth_user_guard = self.activate_auth_user_resolver();

        // ── Phase 2: forward-walked diagnostic scope cache ──────
        // Walk every function/method body in the file once with the
        // forward walker, recording scope snapshots at each statement
        // boundary.  All subsequent `resolve_variable_types` calls
        // from diagnostic collectors hit the cache (O(log N) lookup)
        // instead of doing a full backward scan per member-access
        // span.  This eliminates the O(N × depth × file_size) cost
        // that caused multi-minute analysis times on large files.
        let _scope_guard = crate::completion::variable::forward_walk::with_diagnostic_scope_cache();
        {
            let file_ctx = self.file_context(uri_str);
            let class_loader = self.class_loader(&file_ctx);
            let function_loader_cl = self.function_loader(&file_ctx);
            let constant_loader_cl = self.constant_loader();
            let loaders = crate::completion::resolver::Loaders {
                function_loader: Some(&function_loader_cl),
                constant_loader: Some(&constant_loader_cl),
            };
            crate::completion::variable::forward_walk::build_diagnostic_scopes(
                content,
                &file_ctx.classes,
                &class_loader,
                loaders,
                Some(&self.resolved_class_cache),
            );
        }

        self.collect_unknown_class_diagnostics(uri_str, content, out);
        self.collect_unknown_member_diagnostics(uri_str, content, out);
        self.collect_unknown_function_diagnostics(uri_str, content, out);
        // NOTE: unresolved_member_access diagnostics are now emitted
        // inside collect_unknown_member_diagnostics (in the Untyped arm)
        // to avoid a second full walk with duplicate type resolution.
        self.collect_argument_count_diagnostics(uri_str, content, out);
        self.collect_type_error_diagnostics(uri_str, content, out);
        self.collect_implementation_error_diagnostics(uri_str, content, out);
        self.collect_deprecated_diagnostics(uri_str, content, out);
        self.collect_undefined_variable_diagnostics(uri_str, content, out);
        self.collect_invalid_class_kind_diagnostics(uri_str, content, out);
    }
}

/// Check whether a cached PHPStan diagnostic is stale given the current
/// file content.
///
/// A diagnostic is stale when the user has already fixed the underlying
/// issue (via a code action or manual edit) but PHPStan hasn't re-run
/// yet to clear it:
///
/// - `throws.unusedType` / `throws.notThrowable`: the `@throws` tag
///   was removed — stale if the type no longer appears after `@throws`.
/// - `missingType.checkedException`: the `@throws` tag was added —
///   stale if the exception short name now appears after `@throws`.
/// - `method.missingOverride`: the `#[Override]` attribute was added —
///   stale if a `#[...]` line containing `Override` appears near the
///   diagnostic line.
/// - **Any identifier**: the line now contains a `@phpstan-ignore`
///   comment that covers the diagnostic's identifier.
fn is_stale_phpstan_diagnostic(diag: &Diagnostic, content: &str) -> bool {
    let identifier = match &diag.code {
        Some(NumberOrString::String(s)) => s.as_str(),
        _ => return false,
    };

    // ── @phpstan-ignore covers this diagnostic ──────────────────────
    // If the line where the diagnostic appears now has a
    // `@phpstan-ignore` comment listing this identifier, the user
    // already suppressed it and the diagnostic is stale.
    if !identifier.is_empty()
        && identifier != "phpstan"
        && !identifier.starts_with("ignore.unmatched")
        && line_has_ignore_for(content, diag.range.start.line, identifier)
    {
        return true;
    }

    // The per-identifier heuristics for `throws.unusedType`,
    // `missingType.checkedException`, and `method.missingOverride`
    // have been removed.  These diagnostics are now cleared eagerly
    // by `codeAction/resolve` when the user picks a PHPStan quickfix
    // (see `clear_phpstan_diagnostics_after_resolve` in code_actions).
    // The `@phpstan-ignore` check above still covers manual edits.

    // ── method.override / property.override / property.overrideAttribute ─
    // The user may remove the attribute by hand, so check whether
    // `#[Override]` is still present near the diagnostic line.
    if identifier == "method.override"
        || identifier == "property.override"
        || identifier == "property.overrideAttribute"
    {
        return crate::code_actions::phpstan::remove_override::is_remove_override_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── method.tentativeReturnType — #[\ReturnTypeWillChange] added ─
    // The user may add the attribute by hand, so check whether it is
    // now present near the diagnostic line.
    if identifier == "method.tentativeReturnType" {
        return crate::code_actions::phpstan::add_return_type_will_change::is_add_return_type_will_change_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── PHPDoc type mismatch (return.phpDocType, parameter.phpDocType,
    //    property.phpDocType) — tag removed or type changed ──────────
    if identifier == "return.phpDocType"
        || identifier == "parameter.phpDocType"
        || identifier == "property.phpDocType"
    {
        return crate::code_actions::phpstan::fix_phpdoc_type::is_fix_phpdoc_type_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
            identifier,
        );
    }

    // ── new.static — check if the user manually fixed the class ─────
    // Unlike the actions above, `new.static` fixes are commonly applied
    // by hand (adding `final` to the class or constructor), so we keep
    // a content-based heuristic here.
    if identifier == "new.static" {
        return crate::code_actions::phpstan::new_static::is_new_static_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── class.prefixed — prefixed class name fixed ──────────────────
    // The user may fix the leading backslash by hand, so check whether
    // the prefixed name still appears on the diagnostic line.
    if identifier == "class.prefixed" {
        return crate::code_actions::phpstan::fix_prefixed_class::is_fix_prefixed_class_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
        );
    }

    // ── function.alreadyNarrowedType — always-true assert() removed ─
    // Only for `assert()` calls (not other functions sharing the same
    // identifier).  The diagnostic is stale when `assert(` no longer
    // appears on the diagnostic line.
    if identifier == "function.alreadyNarrowedType"
        && diag.message.starts_with("Call to function assert()")
    {
        return crate::code_actions::phpstan::remove_assert::is_remove_assert_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── return.void / return.empty / missingType.return ──────────────
    // Note: `return.type` is deliberately excluded — no content
    // heuristic can tell whether the right fix is to change the type
    // or change the code.  It is cleared eagerly by codeAction/resolve.
    if identifier == "return.void"
        || identifier == "return.empty"
        || identifier == "missingType.return"
    {
        return crate::code_actions::phpstan::fix_return_type::is_fix_return_type_stale(
            content,
            diag.range.start.line as usize,
            identifier,
        );
    }

    // ── deadCode.unreachable — unreachable statement removed ────────
    if identifier == "deadCode.unreachable" {
        return crate::code_actions::phpstan::remove_unreachable::is_remove_unreachable_stale(
            content,
            diag.range.start.line as usize,
        );
    }

    // ── missingType.iterableValue — @return with generic type added ─
    if identifier == "missingType.iterableValue" {
        return crate::code_actions::phpstan::add_iterable_type::is_add_iterable_type_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
        );
    }

    // ── return.unusedType — unused type removed from return type ─────
    if identifier == "return.unusedType" {
        return crate::code_actions::phpstan::remove_unused_return_type::is_remove_unused_return_type_stale(
            content,
            diag.range.start.line as usize,
            &diag.message,
        );
    }

    false
}

// The following helpers were used by the per-identifier stale detection
// branches that have been removed.  They are kept under `#[cfg(test)]`
// because existing tests exercise them directly.

#[cfg(test)]
#[allow(dead_code)]
/// Extract the type name from a `throws.unusedType` or
/// `throws.notThrowable` message.
fn extract_throws_diag_type(message: &str, identifier: &str) -> Option<String> {
    if identifier == "throws.unusedType" {
        let start = message.find(" has ")? + 5;
        let rest = &message[start..];
        let end = rest.find(" in PHPDoc @throws tag")?;
        Some(rest[..end].trim().to_string())
    } else {
        let start = message.find("@throws with type ")? + 18;
        let rest = &message[start..];
        let end = rest.find(" is not subtype")?;
        Some(rest[..end].trim().to_string())
    }
}

#[cfg(test)]
#[allow(dead_code)]
/// Extract the exception FQN from a `missingType.checkedException` message.
fn extract_checked_exception_fqn(message: &str) -> Option<String> {
    let marker = "throws checked exception ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find(" but")?;
    let fqn = crate::util::strip_fqn_prefix(rest[..end].trim());
    if fqn.is_empty() {
        return None;
    }
    Some(fqn.to_string())
}

/// Check whether the diagnostic's line (or the line before it) has a
/// `@phpstan-ignore` comment that lists the given identifier.
///
/// PHPStan ignore comments can appear:
/// - On the same line as the code: `$x = foo(); // @phpstan-ignore id`
/// - On the line before: `// @phpstan-ignore id`
///
/// Only the per-identifier form (`@phpstan-ignore id1, id2`) is
/// checked.  The blanket `@phpstan-ignore-line` and
/// `@phpstan-ignore-next-line` variants are **not** treated as a
/// match — our code action only produces per-identifier ignores, so
/// we should not eagerly clear diagnostics that happen to sit on a
/// line with a blanket suppression the user added independently.
fn line_has_ignore_for(content: &str, diag_line: u32, identifier: &str) -> bool {
    let line_idx = diag_line as usize;

    // Check the diagnostic line itself and the line before it.
    for idx in [line_idx, line_idx.wrapping_sub(1)] {
        if let Some(line) = content.split('\n').nth(idx)
            && let Some(ignore_pos) = line.find("@phpstan-ignore")
        {
            let after = &line[ignore_pos + "@phpstan-ignore".len()..];
            // `@phpstan-ignore-line` and `@phpstan-ignore-next-line`
            // suppress everything — we can't attribute them to any
            // single identifier, so skip them.
            if after.starts_with("-line") || after.starts_with("-next-line") {
                continue;
            }
            // Parse the identifier list.  Each entry may have a
            // parenthesized reason whose text can contain commas:
            //   @phpstan-ignore id1 (reason, with commas), id2 (reason)
            // A naive `split(',')` would break inside reasons, so we
            // walk the string and only split on commas at paren depth 0.
            let ids_text = after.trim_start();
            let ids_end = ids_text.find("*/").unwrap_or(ids_text.len());
            let ids_region = &ids_text[..ids_end];

            let mut depth: u32 = 0;
            let mut start = 0;
            for (i, ch) in ids_region.char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => depth = depth.saturating_sub(1),
                    ',' if depth == 0 => {
                        let entry = ids_region[start..i].trim();
                        let id = match entry.find(" (") {
                            Some(pos) => &entry[..pos],
                            None => entry,
                        };
                        if id == identifier {
                            return true;
                        }
                        start = i + 1;
                    }
                    _ => {}
                }
            }
            // Last (or only) entry after the final comma.
            let entry = ids_region[start..].trim();
            let id = match entry.find(" (") {
                Some(pos) => &entry[..pos],
                None => entry,
            };
            if id == identifier {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
#[allow(dead_code)]
/// Find the docblock text for the function/method enclosing `diag_line`.
///
/// Searches backward from `diag_line` to find the nearest `function`
/// keyword (which may be on the diagnostic line itself, e.g. on the
/// signature, or on a preceding line when the diagnostic is inside the
/// body or in the docblock above).  Then looks for a preceding
/// `/** ... */` block.  Returns the raw docblock text (from `/**` to
/// `*/` inclusive) if found, or an empty string if no docblock exists.
fn enclosing_docblock_text(content: &str, diag_line: usize) -> String {
    use crate::util::{contains_function_keyword, strip_trailing_modifiers};

    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return String::new();
    }

    // Scan backward from `diag_line` looking for a line that contains
    // the `function` keyword.  This handles three cases:
    //   1. Diagnostic inside the function body → walks up to the
    //      signature line.
    //   2. Diagnostic on the signature line → matches immediately.
    //   3. Diagnostic on the docblock above → walks down would be
    //      needed, but PHPStan diagnostics land on the signature or
    //      body, not the docblock lines.  If we reach the docblock
    //      line we still need to find the function below it.  As a
    //      pragmatic fallback we also scan forward a few lines.
    let mut func_line: Option<usize> = None;
    for idx in (0..=diag_line).rev() {
        if contains_function_keyword(lines[idx]) {
            func_line = Some(idx);
            break;
        }
    }

    // Fallback: if the diagnostic is on a docblock line above the
    // function, scan forward a few lines to find the signature.
    if func_line.is_none() {
        let start = diag_line + 1;
        let limit = (diag_line + 10).min(lines.len());
        for (i, line) in lines[start..limit].iter().enumerate() {
            if contains_function_keyword(line) {
                func_line = Some(start + i);
                break;
            }
        }
    }

    let func_line = match func_line {
        Some(l) => l,
        None => return String::new(),
    };

    // Compute the byte offset of the `function` keyword on that line.
    let line_byte_start: usize = lines.iter().take(func_line).map(|l| l.len() + 1).sum();
    let func_kw_rel = match lines[func_line].find("function") {
        Some(p) => p,
        None => return String::new(),
    };
    let func_kw_pos = line_byte_start + func_kw_rel;

    // Look for a `/** ... */` block before the function keyword
    // (skipping modifiers and whitespace).
    let before_func = &content[..func_kw_pos];
    let trimmed = before_func.trim_end();

    let after_mods = strip_trailing_modifiers(trimmed);
    if after_mods.ends_with("*/")
        && let Some(open) = after_mods.rfind("/**")
    {
        return after_mods[open..].to_string();
    }

    String::new()
}

#[cfg(test)]
#[allow(dead_code)]
/// Check whether `scope` (typically a single docblock) contains
/// `@throws <short_name>` (case-insensitive).
fn scope_has_throws_tag(scope: &str, short_name: &str) -> bool {
    let lower = short_name.to_lowercase();
    crate::docblock::extract_throws_tags(scope)
        .iter()
        .any(|ty| {
            ty.base_name()
                .map(crate::util::short_name)
                .is_some_and(|s| s.eq_ignore_ascii_case(&lower))
        })
}

/// How long to wait after the last keystroke before publishing diagnostics.
const DIAGNOSTIC_DEBOUNCE_MS: u64 = 500;

impl Backend {
    /// Deliver diagnostics for a single file.
    ///
    /// Called from the background diagnostic worker after debouncing.
    ///
    /// **Phase 1 (instant, both modes):** Run fast collectors (syntax
    /// errors, deprecated, unused imports), merge with *cached* slow
    /// and PHPStan results, and push via `publishDiagnostics`.  The
    /// editor shows strikethrough and dimming within milliseconds.
    ///
    /// **Phase 2 (background, mode-dependent):**
    ///
    /// - **Pull mode:** Compute slow diagnostics, build the full set
    ///   (fast + fresh slow + cached PHPStan), cache it in
    ///   `diag_last_full`, bump the `resultId`, and send
    ///   `workspace/diagnostic/refresh`.  The editor re-pulls and
    ///   gets the complete set.  Push always serves cached slow, so
    ///   no second push is needed.
    ///
    /// - **Push mode (fallback):** Compute slow diagnostics, then
    ///   push the full set (fast + fresh slow + cached PHPStan),
    ///   replacing the Phase 1 snapshot.
    pub(crate) async fn publish_diagnostics_for_file(&self, uri_str: &str, content: &str) {
        if self.should_skip_diagnostics(uri_str) {
            return;
        }

        // The native collectors are expensive (a full-file forward walk and
        // type resolution — hundreds of ms to several seconds on a large
        // file). They are pure CPU work and must never run inline on an async
        // worker: editors pull both `textDocument/diagnostic` and
        // `workspace/diagnostic` on every keystroke, and a handful of
        // concurrent inline computes will occupy every async runtime thread,
        // stalling delivery of completion and hover responses even though
        // those handlers themselves finished. Run them on the blocking pool.

        // ── Phase 1: collect and cache fast diagnostics ─────────────
        let fast_diagnostics = {
            let backend = self.clone_for_blocking();
            let uri = uri_str.to_string();
            let content = content.to_string();
            crate::server::run_blocking_cancel_safe(move || {
                let mut out = Vec::new();
                let effective_owned = backend.blade_virtual_content.read().get(&uri).cloned();
                let effective = effective_owned.as_deref().unwrap_or(&content);
                backend.collect_fast_diagnostics(&uri, effective, &mut out);
                out
            })
            .await
            .unwrap_or_default()
        };

        {
            let mut cache = self.diag_last_fast.lock();
            cache.insert(uri_str.to_string(), fast_diagnostics.clone());
        }

        // Update the assembled cache immediately so pull-capable editors
        // can see fast diagnostics before the slower native passes finish.
        self.assemble_and_push(uri_str).await;
        if self.supports_pull_diagnostics.load(Ordering::Acquire)
            && let Some(client) = &self.client
        {
            let _ = client.workspace_diagnostic_refresh().await;
        }

        // ── Phase 2: compute and cache slow diagnostics ─────────────
        let slow_diagnostics = {
            let backend = self.clone_for_blocking();
            let uri = uri_str.to_string();
            let content = content.to_string();
            crate::server::run_blocking_cancel_safe(move || {
                // The resolved-class cache guard contains a thread-local raw
                // pointer; activate it on this blocking thread for the
                // duration of the synchronous collection only.
                let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                    &backend.resolved_class_cache,
                );
                let mut out = Vec::new();
                let effective_owned = backend.blade_virtual_content.read().get(&uri).cloned();
                let effective = effective_owned.as_deref().unwrap_or(&content);
                backend.collect_slow_diagnostics(&uri, effective, &mut out);
                out
            })
            .await
            .unwrap_or_default()
        };

        {
            let mut cache = self.diag_last_slow.lock();
            cache.insert(uri_str.to_string(), slow_diagnostics);
        }

        // Update again with fresh slow results merged in.
        self.assemble_and_push(uri_str).await;
        if self.supports_pull_diagnostics.load(Ordering::Acquire)
            && let Some(client) = &self.client
        {
            let _ = client.workspace_diagnostic_refresh().await;
        }
    }

    /// Assemble diagnostics from all per-source caches for a URI and
    /// deliver them to the editor.
    ///
    /// Every source (fast, slow, PHPStan, PHPCS, Mago lint, Mago
    /// analyze) caches its results independently.  This helper merges
    /// them into one set, deduplicates, and filters suppressions.
    ///
    /// **Push mode:** The merged set is published via
    /// `textDocument/publishDiagnostics`.
    ///
    /// **Pull mode:** Only fast diagnostics (syntax errors, unused
    /// imports, unused variables) are pushed so the editor sees them
    /// instantly.  The full merged set is cached in `diag_last_full`
    /// with a bumped `resultId` so the next pull response returns it.
    /// Editors that support pull diagnostics merge pushed and pulled
    /// sets additively, so pushing the full set would duplicate every
    /// slow and external diagnostic.
    pub(crate) async fn assemble_and_push(&self, uri_str: &str) {
        let client = match &self.client {
            Some(c) => c,
            None => return,
        };

        let uri = match uri_str.parse::<Url>() {
            Ok(u) => u,
            Err(_) => return,
        };

        // ── Read all per-source caches ──────────────────────────────
        let mut full = Vec::new();

        {
            let cache = self.diag_last_fast.lock();
            if let Some(fast) = cache.get(uri_str) {
                full.extend(fast.iter().cloned());
            }
        }
        {
            let cache = self.diag_last_slow.lock();
            if let Some(slow) = cache.get(uri_str) {
                full.extend(slow.iter().cloned());
            }
        }

        let phpstan_before: Vec<Diagnostic> = {
            let cache = self.phpstan_last_diags.lock();
            cache.get(uri_str).cloned().unwrap_or_default()
        };

        // Eagerly prune stale PHPStan diagnostics against current
        // file content (e.g. @throws tag added/removed, @phpstan-ignore
        // comment added).
        if !phpstan_before.is_empty() {
            let content: Option<Arc<String>> = self.open_files.read().get(uri_str).cloned();
            let filtered: Vec<Diagnostic> = phpstan_before
                .iter()
                .filter(|d| {
                    if let Some(ref text) = content {
                        !is_stale_phpstan_diagnostic(d, text)
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
            if filtered.len() != phpstan_before.len() {
                let mut cache = self.phpstan_last_diags.lock();
                cache.insert(uri_str.to_string(), filtered.clone());
            }
            full.extend(filtered);
        }

        {
            let cache = self.phpcs_last_diags.lock();
            if let Some(phpcs_diags) = cache.get(uri_str) {
                full.extend(phpcs_diags.iter().cloned());
            }
        }
        {
            let cache = self.mago_lint_last_diags.lock();
            if let Some(mago_diags) = cache.get(uri_str) {
                full.extend(mago_diags.iter().cloned());
            }
        }
        {
            let cache = self.mago_analyze_last_diags.lock();
            if let Some(mago_diags) = cache.get(uri_str) {
                full.extend(mago_diags.iter().cloned());
            }
        }

        // ── Suppress imprecise overlaps and filter ──────────────────
        suppress_imprecise_overlaps(&mut full);
        let mut full = self.filter_suppressed(full);

        // ── Apply @phpantom-ignore comment suppression ─────────────
        {
            let content: Option<Arc<String>> = self.open_files.read().get(uri_str).cloned();
            if let Some(ref text) = content {
                filter_ignored_by_comment(&mut full, text);
            }
        }

        // ── Apply [[diagnostics.ignore]] config rules ────────────────
        {
            let rules = ignore_rules::compile_ignore_rules(&self.config.lock().diagnostics.ignore);
            if !rules.is_empty()
                && let Ok(file_path) = uri.to_file_path()
                && let Some(root) = self.workspace_root.read().clone()
            {
                let relative_path = file_path
                    .strip_prefix(&root)
                    .unwrap_or(&file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                ignore_rules::filter_ignored_by_config(&mut full, &relative_path, &rules);
            }
        }

        // If suppression removed any full-line PHPStan diagnostics
        // (because a precise native diagnostic covers the same line),
        // prune them from the PHPStan cache too so they don't resurface.
        if !phpstan_before.is_empty() {
            let pruned: Vec<Diagnostic> = phpstan_before
                .into_iter()
                .filter(|d| full.iter().any(|f| f.range == d.range))
                .collect();
            let mut cache = self.phpstan_last_diags.lock();
            cache.insert(uri_str.to_string(), pruned);
        }

        let pull_mode = self.supports_pull_diagnostics.load(Ordering::Acquire);

        if pull_mode {
            // ── Pull mode ───────────────────────────────────────────
            // Pull-capable clients use `textDocument/diagnostic` as the
            // single source of truth for native diagnostics.  We only cache
            // the merged set here; the caller triggers
            // `workspace/diagnostic/refresh` so the editor re-pulls.  This
            // avoids duplicate diagnostics in clients that keep pushed and
            // pulled diagnostics in separate namespaces.
            {
                let mut cache = self.diag_last_full.lock();
                cache.insert(uri_str.to_string(), full);
            }
            {
                let mut ids = self.diag_result_ids.lock();
                let id = ids.entry(uri_str.to_string()).or_insert(0);
                *id += 1;
            }
        } else {
            // ── Push mode ───────────────────────────────────────────
            client.publish_diagnostics(uri, full, None).await;
        }
    }

    /// Notify the diagnostic system that a file needs fresh diagnostics.
    ///
    /// **Push mode:** Queues the file for the debounced background
    /// diagnostic worker and schedules external tool runs.
    ///
    /// **Pull mode:** Only schedules external tool runs (PHPStan,
    /// PHPCS, Mago).  Native diagnostic computation is deferred until
    /// the editor sends a `textDocument/diagnostic` pull request, which
    /// triggers [`trigger_diagnostics_for_pull`].
    ///
    /// This returns immediately — all diagnostic computation happens
    /// in the background so that completion, hover, and signature help
    /// are never blocked.
    pub(crate) fn schedule_diagnostics(&self, uri: String) {
        // Don't schedule diagnostics before initialization is complete.
        // Files opened during startup will be diagnosed once
        // `initialized` sets `init_complete` and re-schedules them.
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        // In pull mode we deliberately KEEP the previously cached full set.
        // The pull handler no longer computes on the request path (that
        // wedged the transport — see `trigger_diagnostics_for_pull`), so the
        // editor would otherwise flicker to empty between an edit and the
        // background recompute. Keeping the stale set means a pull returns
        // the old diagnostics until the worker recomputes, bumps the
        // resultId, and requests a refresh, which makes the editor re-pull
        // the fresh set. The resultId is bumped only by `assemble_and_push`,
        // so the "unchanged" fast-path stays correct.

        // Both modes: queue for the debounced background worker.
        // In push mode the worker pushes the full assembled set.
        // In pull mode the worker pushes only fast diagnostics and
        // caches the full set in `diag_last_full` for pull responses.
        {
            let mut pending = self.diag_pending_uris.lock();
            pending.insert(uri.clone());
        }
        self.diag_version.fetch_add(1, Ordering::Release);
        self.diag_notify.notify_one();

        // External tools (PHPStan, PHPCS, Mago) are NOT scheduled here.
        // They are expensive (seconds per run) and would block save-
        // triggered runs due to the serialization guarantee (one process
        // at a time).
    }

    /// Schedule all external tool runs (PHPStan, PHPCS, Mago) for a
    /// single file.
    ///
    /// External tools are expensive (seconds per run) and at most one
    /// process runs at a time per tool, so triggering them on every
    /// keystroke would block save-triggered runs.
    pub(crate) fn schedule_external_diagnostics(&self, uri: String) {
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }
        self.schedule_phpstan(uri.clone());
        self.schedule_phpcs(uri.clone());
        self.schedule_mago_lint(uri.clone());
        self.schedule_mago_analyze(uri);
    }

    /// Invalidate diagnostics for all open files after a cross-file change.
    ///
    /// Called when a class signature changes in one file, because
    /// diagnostics in other open files (unknown member, unknown class,
    /// deprecated usage) may depend on the changed class.  The edited
    /// file itself is excluded (it is already scheduled by the caller).
    ///
    /// **Push mode:** Queues all open files for the background worker.
    ///
    /// **Pull mode:** Invalidates cached full diagnostics and sends
    /// `workspace/diagnostic/refresh` so the editor re-pulls.
    pub(crate) fn schedule_diagnostics_for_open_files(&self, exclude_uri: &str) {
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        let pull_mode = self.supports_pull_diagnostics.load(Ordering::Acquire);

        let uris: Vec<String> = self
            .open_files
            .read()
            .keys()
            .filter(|u| u.as_str() != exclude_uri)
            .cloned()
            .collect();
        if uris.is_empty() {
            return;
        }

        if pull_mode {
            // Invalidate cached full diagnostics so the next pull
            // triggers a fresh computation.  Do NOT remove resultIds —
            // see the comment in schedule_diagnostics for why.
            let mut cache = self.diag_last_full.lock();
            for uri in &uris {
                cache.remove(uri);
            }
        }

        // Both modes: queue all files for the debounced background worker.
        // In push mode the worker pushes the full assembled set.
        // In pull mode the worker pushes only fast diagnostics and
        // caches the full set in `diag_last_full` for pull responses.
        {
            let mut pending = self.diag_pending_uris.lock();
            pending.extend(uris);
        }
        self.diag_version.fetch_add(1, Ordering::Release);
        self.diag_notify.notify_one();
    }

    /// Compute native diagnostics for a single file (pull-mode path).
    ///
    /// Called directly from the pull handler (`textDocument/diagnostic`)
    /// when the cached full diagnostics are stale or missing.  Runs
    /// both fast and slow collectors synchronously (no debounce) and
    /// caches the results.  The pull handler reads `diag_last_full`
    /// after this returns.
    pub(crate) fn trigger_diagnostics_for_pull(&self, uri_str: &str) {
        // Don't compute diagnostics before initialization is complete.
        // The pull handler will return empty results; once `initialized`
        // finishes it schedules all open files which populates the cache.
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        if self.should_skip_diagnostics(uri_str) {
            return;
        }

        // NEVER compute on the request path. The native pass takes seconds on
        // a large file, and editors pull both `textDocument/diagnostic` and
        // `workspace/diagnostic` on every keystroke — far faster than the
        // compute drains. A synchronous pull holds a tower-lsp concurrency
        // slot for the whole compute, so a typing burst fills every slot and
        // wedges the transport (it can no longer even read `$/cancelRequest`).
        //
        // Instead, ensure the debounced background worker will (re)compute
        // this file off the async runtime; it caches the full set and
        // requests a workspace diagnostic refresh when done. This pull
        // returns whatever is currently cached (stale results are kept until
        // the refresh, so the editor never flickers to empty).
        {
            let mut pending = self.diag_pending_uris.lock();
            pending.insert(uri_str.to_string());
        }
        self.diag_notify.notify_one();
    }

    /// Long-lived background task that processes diagnostic requests.
    ///
    /// Active in both push and pull modes.  In push mode, the worker
    /// pushes the full assembled diagnostic set via
    /// `publishDiagnostics`.  In pull mode, it pushes only fast
    /// diagnostics and caches the full set in `diag_last_full` for
    /// the next pull response (see [`assemble_and_push`]).
    ///
    /// Spawned once during `initialized`.  Loops forever, waiting for
    /// [`schedule_diagnostics`](Self::schedule_diagnostics) to signal
    /// new work.  On each iteration:
    ///
    /// 1. Wait for a notification (new edit arrived).
    /// 2. Debounce: sleep [`DIAGNOSTIC_DEBOUNCE_MS`], then check
    ///    whether the version counter moved (more edits).  If so,
    ///    loop back to step 2.
    /// 3. Snapshot the pending URI and current file content.
    /// 4. Run the diagnostic collectors and publish results.
    /// 5. Loop back to step 1.
    ///
    /// Because there is exactly one instance of this task, at most one
    /// diagnostic pass runs at a time.  If edits arrive during step 4
    /// the version counter will have moved, and step 1 picks up
    /// immediately after step 4 finishes — giving the two-slot
    /// (one running + one pending) behaviour.
    pub(crate) async fn diagnostic_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.diag_notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 2: debounce ────────────────────────────────────
            loop {
                let version_before = self.diag_version.load(Ordering::Acquire);
                tokio::time::sleep(std::time::Duration::from_millis(DIAGNOSTIC_DEBOUNCE_MS)).await;
                let version_after = self.diag_version.load(Ordering::Acquire);
                if version_before == version_after {
                    // No new edits during the sleep — proceed.
                    break;
                }
                // More edits arrived — loop and debounce again.
            }

            // ── Step 3: snapshot all pending URIs ────────────────────
            let uris: std::collections::HashSet<String> = {
                let mut pending = self.diag_pending_uris.lock();
                std::mem::take(&mut *pending)
            };
            if uris.is_empty() {
                continue;
            }

            // ── Step 4: collect and publish for each URI ────────────
            // Snapshot content for each URI individually, releasing the
            // read lock before each async publish call so that
            // `did_change` is never blocked.
            for uri in &uris {
                let content = {
                    let files = self.open_files.read();
                    match files.get(uri) {
                        Some(c) => c.clone(),
                        None => continue,
                    }
                };
                self.publish_diagnostics_for_file(uri, &content).await;
            }
        }
    }

    // ── PHPStan worker ──────────────────────────────────────────────

    /// Schedule a PHPStan run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// saves rapidly, earlier requests are superseded.
    fn schedule_phpstan(&self, uri: String) {
        *self.phpstan_pending_uri.lock() = Some(uri);
        self.phpstan_notify.notify_one();
    }

    /// Long-lived background task that runs PHPStan on pending files.
    ///
    /// Spawned once during `initialized`, alongside the main diagnostic
    /// worker.  This task is completely independent: native diagnostics
    /// (phases 1 and 2) are never blocked by PHPStan.
    ///
    /// ## Serialization guarantee
    ///
    /// At most one PHPStan process runs at a time.  The worker loop:
    ///
    /// 1. Wait for a notification (new edit arrived).
    /// 2. Debounce: sleep [`PHPSTAN_DEBOUNCE_MS`], checking whether new
    ///    edits arrived.  If so, restart the debounce.
    /// 3. Snapshot the pending URI and file content.
    /// 4. Resolve the PHPStan binary (skip if not found / disabled).
    /// 5. Run PHPStan (blocking — this is the slow part).
    /// 6. Cache the results and re-publish diagnostics for the file.
    /// 7. Loop back to step 1.
    ///
    /// If the user edits while step 5 is in progress, the pending URI
    /// is updated.  When step 5 finishes, the worker sees the new
    /// notification and loops back to step 1, starting a fresh run
    /// with the latest content.
    pub(crate) async fn phpstan_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.phpstan_notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // Drain any extra stored permits so that notifications
            // that arrived between the last run finishing and this
            // `notified()` call don't cause an immediate second run.
            let _ = tokio::time::timeout(std::time::Duration::ZERO, self.phpstan_notify.notified())
                .await;

            // ── Step 2: snapshot the pending URI ────────────────────
            let uri = match self.phpstan_pending_uri.lock().take() {
                Some(u) => u,
                None => continue,
            };

            // Snapshot the file content.
            let content = {
                let files = self.open_files.read();
                match files.get(&uri) {
                    Some(c) => c.clone(),
                    None => continue,
                }
            };

            // ── Step 4: resolve PHPStan binary ──────────────────────
            let config = self.config();
            if config.phpstan.is_disabled() {
                continue;
            }

            let file_path = match uri.parse::<Url>().ok().and_then(|u| u.to_file_path().ok()) {
                Some(p) => p,
                None => continue,
            };

            let workspace_root = self.workspace_root.read().clone();
            let workspace_root = match workspace_root {
                Some(root) => root,
                None => continue,
            };

            let bin_dir: Option<String> = crate::composer::read_composer_package(&workspace_root)
                .map(|pkg| crate::composer::get_bin_dir(&pkg));

            let resolved = match phpstan::resolve_phpstan(
                Some(&workspace_root),
                &config.phpstan,
                bin_dir.as_deref(),
            ) {
                Some(r) => r,
                None => continue,
            };

            // ── Step 5: run PHPStan (the slow part) ─────────────────
            // Move the blocking PHPStan execution onto a dedicated
            // OS thread via `spawn_blocking`.  This is critical:
            // `run_phpstan` contains a poll loop that blocks the
            // thread.  If we ran it inline, the tokio runtime could
            // schedule other futures (including a second iteration
            // of this very worker) on other threads, breaking the
            // "at most one PHPStan process" guarantee.  By awaiting
            // the `spawn_blocking` handle, this task is suspended
            // (not occupying a runtime thread) and no re-entry can
            // happen until the handle resolves.
            let phpstan_config = config.phpstan.clone();
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let phpstan_diags = {
                let result = tokio::task::spawn_blocking(move || {
                    phpstan::run_phpstan(
                        &resolved,
                        &content,
                        &file_path,
                        &workspace_root,
                        &phpstan_config,
                        &shutdown_flag,
                    )
                })
                .await;

                match result {
                    Ok(Ok(diags)) => diags,
                    Ok(Err(_e)) => {
                        // PHPStan failures are silently ignored to
                        // avoid flooding the editor with errors when
                        // PHPStan is misconfigured or the project
                        // doesn't use it.
                        continue;
                    }
                    Err(_join_err) => {
                        // The blocking task panicked or was cancelled.
                        continue;
                    }
                }
            };

            // ── Step 6: cache results and re-publish ────────────────
            // Verify the file is still open *before* writing to the
            // cache.  If the file was closed while PHPStan was running,
            // `clear_diagnostics_for_file` already purged the cache
            // entry — writing it back would leave stale diagnostics
            // that resurface on the next `did_open`.
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            {
                let mut cache = self.phpstan_last_diags.lock();
                cache.insert(uri.clone(), phpstan_diags);
            }

            // Assemble and push so the editor sees fresh PHPStan
            // results merged with cached native diagnostics.
            self.assemble_and_push(&uri).await;

            // In pull mode the editor must be told to re-pull so it
            // picks up the new PHPStan results.
            if self.supports_pull_diagnostics.load(Ordering::Acquire)
                && let Some(client) = &self.client
            {
                let _ = client.workspace_diagnostic_refresh().await;
            }
        }
    }

    // ── PHPCS worker ────────────────────────────────────────────────

    /// Schedule a PHPCS run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// types rapidly, earlier requests are superseded.  This is
    /// intentional — PHPCS is too slow to queue up multiple files.
    fn schedule_phpcs(&self, uri: String) {
        *self.phpcs_pending_uri.lock() = Some(uri);
        self.phpcs_notify.notify_one();
    }

    /// Long-lived background task that runs PHPCS on pending files.
    ///
    /// Spawned once during `initialized`, alongside the main diagnostic
    /// worker and the PHPStan worker.  This task is completely
    /// independent: native diagnostics and PHPStan are never blocked.
    ///
    /// ## Serialization guarantee
    ///
    /// At most one PHPCS process runs at a time.  The worker loop:
    ///
    /// 1. Wait for a notification (new edit arrived).
    /// 2. Debounce: sleep [`PHPCS_DEBOUNCE_MS`], checking whether new
    ///    edits arrived.  If so, restart the debounce.
    /// 3. Snapshot the pending URI and file content.
    /// 4. Resolve the PHPCS binary (skip if not found / disabled).
    /// 5. Run PHPCS (blocking — this is the slow part).
    /// 6. Cache the results and re-publish diagnostics for the file.
    /// 7. Loop back to step 1.
    ///
    /// If the user edits while step 5 is in progress, the pending URI
    /// is updated.  When step 5 finishes, the worker sees the new
    /// notification and loops back to step 1, starting a fresh run
    /// with the latest content.
    pub(crate) async fn phpcs_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.phpcs_notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // Drain any extra stored permits (same rationale as the
            // PHPStan worker).
            let _ =
                tokio::time::timeout(std::time::Duration::ZERO, self.phpcs_notify.notified()).await;

            // ── Step 2: snapshot the pending URI ────────────────────
            let uri = match self.phpcs_pending_uri.lock().take() {
                Some(u) => u,
                None => continue,
            };

            // Snapshot the file content.
            let content = {
                let files = self.open_files.read();
                match files.get(&uri) {
                    Some(c) => c.clone(),
                    None => continue,
                }
            };

            // ── Step 4: resolve PHPCS binary ────────────────────────
            let config = self.config();
            if config.phpcs.is_disabled() {
                continue;
            }

            let file_path = match uri.parse::<Url>().ok().and_then(|u| u.to_file_path().ok()) {
                Some(p) => p,
                None => continue,
            };

            let workspace_root = self.workspace_root.read().clone();
            let workspace_root = match workspace_root {
                Some(root) => root,
                None => continue,
            };

            let bin_dir: Option<String> = crate::composer::read_composer_package(&workspace_root)
                .map(|pkg| crate::composer::get_bin_dir(&pkg));

            let resolved = match phpcs::resolve_phpcs(
                Some(&workspace_root),
                &config.phpcs,
                bin_dir.as_deref(),
            ) {
                Some(r) => r,
                None => continue,
            };

            // ── Step 5: run PHPCS (the slow part) ───────────────────
            let phpcs_config = config.phpcs.clone();
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let phpcs_diags = {
                let result = tokio::task::spawn_blocking(move || {
                    phpcs::run_phpcs(
                        &resolved,
                        &content,
                        &file_path,
                        &workspace_root,
                        &phpcs_config,
                        &shutdown_flag,
                    )
                })
                .await;

                match result {
                    Ok(Ok(diags)) => diags,
                    Ok(Err(_e)) => {
                        // PHPCS failures are silently ignored to
                        // avoid flooding the editor with errors when
                        // PHPCS is misconfigured or the project
                        // doesn't use it.
                        continue;
                    }
                    Err(_join_err) => {
                        // The blocking task panicked or was cancelled.
                        continue;
                    }
                }
            };

            // ── Step 6: cache results and re-publish ────────────────
            // Verify the file is still open before caching (same
            // rationale as the PHPStan worker).
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            {
                let mut cache = self.phpcs_last_diags.lock();
                cache.insert(uri.clone(), phpcs_diags);
            }

            // Assemble and push so the editor sees fresh PHPCS
            // results merged with cached native diagnostics.
            self.assemble_and_push(&uri).await;

            // In pull mode the editor must be told to re-pull so it
            // picks up the new PHPCS results.
            if self.supports_pull_diagnostics.load(Ordering::Acquire)
                && let Some(client) = &self.client
            {
                let _ = client.workspace_diagnostic_refresh().await;
            }
        }
    }

    // ── Mago lint worker ────────────────────────────────────────────

    /// Schedule a Mago lint run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// types rapidly, earlier requests are superseded.
    fn schedule_mago_lint(&self, uri: String) {
        *self.mago_lint_pending_uri.lock() = Some(uri);
        self.mago_lint_notify.notify_one();
    }

    /// Long-lived background task that runs `mago lint` on pending files.
    ///
    /// Spawned once during `initialized`.  This task is completely
    /// independent: native diagnostics, PHPStan, PHPCS, and Mago
    /// analyze are never blocked.
    ///
    /// At most one `mago lint` process runs at a time.  The worker
    /// loop follows the same pattern as the PHPCS worker.
    pub(crate) async fn mago_lint_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.mago_lint_notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // Drain any extra stored permits.
            let _ =
                tokio::time::timeout(std::time::Duration::ZERO, self.mago_lint_notify.notified())
                    .await;

            // ── Step 2: snapshot the pending URI ────────────────────
            let uri = match self.mago_lint_pending_uri.lock().take() {
                Some(u) => u,
                None => continue,
            };

            let content = {
                let files = self.open_files.read();
                match files.get(&uri) {
                    Some(c) => c.clone(),
                    None => continue,
                }
            };

            // ── Step 4: resolve Mago binary ─────────────────────────
            let config = self.config();
            if config.mago.is_disabled() {
                continue;
            }

            let workspace_root = self.workspace_root.read().clone();
            let workspace_root = match workspace_root {
                Some(root) => root,
                None => continue,
            };

            // Mago requires mago.toml to operate.
            if !mago::has_mago_config(&workspace_root) {
                continue;
            }

            let file_path = match uri.parse::<Url>().ok().and_then(|u| u.to_file_path().ok()) {
                Some(p) => p,
                None => continue,
            };

            let bin_dir: Option<String> = crate::composer::read_composer_package(&workspace_root)
                .map(|pkg| crate::composer::get_bin_dir(&pkg));

            let resolved =
                match mago::resolve_mago(Some(&workspace_root), &config.mago, bin_dir.as_deref()) {
                    Some(r) => r,
                    None => continue,
                };

            // ── Step 5: run mago lint (the slow part) ───────────────
            let mago_config = config.mago.clone();
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let mago_diags = {
                let result = tokio::task::spawn_blocking(move || {
                    mago::run_mago_lint(
                        &resolved,
                        &content,
                        &file_path,
                        &workspace_root,
                        &mago_config,
                        &shutdown_flag,
                    )
                })
                .await;

                match result {
                    Ok(Ok(diags)) => diags,
                    Ok(Err(_e)) => continue,
                    Err(_join_err) => continue,
                }
            };

            // ── Step 6: cache results and re-publish ────────────────
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            {
                let mut cache = self.mago_lint_last_diags.lock();
                cache.insert(uri.clone(), mago_diags);
            }

            self.assemble_and_push(&uri).await;

            // In pull mode the editor must be told to re-pull so it
            // picks up the new Mago lint results.
            if self.supports_pull_diagnostics.load(Ordering::Acquire)
                && let Some(client) = &self.client
            {
                let _ = client.workspace_diagnostic_refresh().await;
            }
        }
    }

    // ── Mago analyze worker ─────────────────────────────────────────

    /// Schedule a Mago analyze run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// types rapidly, earlier requests are superseded.
    fn schedule_mago_analyze(&self, uri: String) {
        *self.mago_analyze_pending_uri.lock() = Some(uri);
        self.mago_analyze_notify.notify_one();
    }

    /// Long-lived background task that runs `mago analyze` on pending files.
    ///
    /// Spawned once during `initialized`.  This task is completely
    /// independent: native diagnostics, PHPStan, PHPCS, and Mago lint
    /// are never blocked.
    ///
    /// At most one `mago analyze` process runs at a time.  The worker
    /// loop follows the same pattern as the PHPStan worker.
    pub(crate) async fn mago_analyze_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.mago_analyze_notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // Drain any extra stored permits.
            let _ = tokio::time::timeout(
                std::time::Duration::ZERO,
                self.mago_analyze_notify.notified(),
            )
            .await;

            // ── Step 2: snapshot the pending URI ────────────────────
            let uri = match self.mago_analyze_pending_uri.lock().take() {
                Some(u) => u,
                None => continue,
            };

            let content = {
                let files = self.open_files.read();
                match files.get(&uri) {
                    Some(c) => c.clone(),
                    None => continue,
                }
            };

            // ── Step 4: resolve Mago binary ─────────────────────────
            let config = self.config();
            if config.mago.is_disabled() {
                continue;
            }

            let workspace_root = self.workspace_root.read().clone();
            let workspace_root = match workspace_root {
                Some(root) => root,
                None => continue,
            };

            // Mago requires mago.toml to operate.
            if !mago::has_mago_config(&workspace_root) {
                continue;
            }

            let file_path = match uri.parse::<Url>().ok().and_then(|u| u.to_file_path().ok()) {
                Some(p) => p,
                None => continue,
            };

            let bin_dir: Option<String> = crate::composer::read_composer_package(&workspace_root)
                .map(|pkg| crate::composer::get_bin_dir(&pkg));

            let resolved =
                match mago::resolve_mago(Some(&workspace_root), &config.mago, bin_dir.as_deref()) {
                    Some(r) => r,
                    None => continue,
                };

            // ── Step 5: run mago analyze (the slow part) ────────────
            let mago_config = config.mago.clone();
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let mago_diags = {
                let result = tokio::task::spawn_blocking(move || {
                    mago::run_mago_analyze(
                        &resolved,
                        &content,
                        &file_path,
                        &workspace_root,
                        &mago_config,
                        &shutdown_flag,
                    )
                })
                .await;

                match result {
                    Ok(Ok(diags)) => diags,
                    Ok(Err(_e)) => continue,
                    Err(_join_err) => continue,
                }
            };

            // ── Step 6: cache results and re-publish ────────────────
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            {
                let mut cache = self.mago_analyze_last_diags.lock();
                cache.insert(uri.clone(), mago_diags);
            }

            self.assemble_and_push(&uri).await;

            // In pull mode the editor must be told to re-pull so it
            // picks up the new Mago analyze results.
            if self.supports_pull_diagnostics.load(Ordering::Acquire)
                && let Some(client) = &self.client
            {
                let _ = client.workspace_diagnostic_refresh().await;
            }
        }
    }

    /// Clear diagnostics for a file (e.g. on `did_close`).
    pub(crate) async fn clear_diagnostics_for_file(&self, uri_str: &str) {
        // Remove all per-source caches so we don't leak memory.
        self.diag_last_fast.lock().remove(uri_str);
        self.diag_last_slow.lock().remove(uri_str);
        // Remove cached PHPStan, PHPCS, and Mago diagnostics too.
        self.phpstan_last_diags.lock().remove(uri_str);
        self.phpcs_last_diags.lock().remove(uri_str);
        self.mago_lint_last_diags.lock().remove(uri_str);
        self.mago_analyze_last_diags.lock().remove(uri_str);
        // Remove pull-diagnostic caches.
        self.diag_result_ids.lock().remove(uri_str);
        self.diag_last_full.lock().remove(uri_str);

        let client = match &self.client {
            Some(c) => c,
            None => return,
        };

        let uri = match uri_str.parse::<Url>() {
            Ok(u) => u,
            Err(_) => return,
        };

        // Always push empty diagnostics to clear any Phase 1 snapshot.
        client.publish_diagnostics(uri, Vec::new(), None).await;

        if self.supports_pull_diagnostics.load(Ordering::Acquire) {
            // Tell the editor to re-pull diagnostics.  We spawn this
            // as a detached task instead of awaiting it because
            // workspace_diagnostic_refresh is a server-to-client
            // *request* that blocks until the client responds.  When
            // the editor closes many files in a burst, each didClose
            // handler would await a response while the client is busy
            // sending more messages, deadlocking the tower-lsp
            // service loop.
            let client = client.clone();
            tokio::spawn(async move {
                let _ = client.workspace_diagnostic_refresh().await;
            });
        }
    }
}

// ── Deduplication ───────────────────────────────────────────────────────────

/// Suppress lower-priority diagnostics when a higher-priority one covers
/// an overlapping range.
///
/// Rules (in precedence order):
/// 1. `unknown_class` trumps `unresolved_member_access`
/// 2. `unknown_member` trumps `unresolved_member_access`
/// 3. `scalar_member_access` trumps `unresolved_member_access`
/// 4. Full-line diagnostics are suppressed when any precise (sub-line)
///    diagnostic exists on the same line.
///
/// **Why rule 4 exists.** Diagnostics arrive from multiple independent
/// sources (Mago parser, PHPStan, native PHPantom checks) that use
/// completely different error codes and descriptions.  There is no
/// reliable way to determine whether two diagnostics from different
/// sources describe the same issue.  What we *can* determine is
/// precision: tools like PHPStan only report a line number, so their
/// diagnostics span the entire line (character 0 to a very large end
/// character).  Native diagnostics and parser errors pinpoint the exact
/// token.  A full-line underline obscures the precise location, making
/// it harder for the developer to spot the problem.  Suppressing it
/// unconditionally when any precise diagnostic exists on the same line
/// keeps the pinpointed one visible without losing information.  Once
/// the precise diagnostic is resolved, the full-line one reappears
/// automatically (if the underlying issue persists).
///
/// Each source's diagnostics are authoritative: if PHPStan reports five
/// issues on a line, all five are shown; if PHPantom reports two issues
/// on the same span, both are shown.  Cross-source overlap is handled
/// by rule 4 above, not by collapsing identical ranges.
impl Backend {
    /// Remove diagnostics that were eagerly suppressed by a
    /// `codeAction/resolve` handler and drain the suppression list.
    ///
    /// This is called during `assemble_and_push` so that the squiggly
    /// line disappears before the text edit is applied.
    fn filter_suppressed(&self, mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let mut suppressed = self.diag_suppressed.lock();
        if suppressed.is_empty() {
            return diagnostics;
        }
        diagnostics.retain(|d| {
            !suppressed
                .iter()
                .any(|s| d.range == s.range && d.message == s.message && d.code == s.code)
        });
        suppressed.clear();
        diagnostics
    }
}

/// Remove diagnostics that are redundant given more precise or
/// higher-priority diagnostics on the same line or range.
///
/// Two suppression rules:
///
/// 1. **`unresolved_member_access` vs priority diagnostics.**  When a
///    priority diagnostic (`unknown_class`, `unknown_member`,
///    `scalar_member_access`, `unknown_function`) overlaps an
///    `unresolved_member_access` hint, the hint is dropped because the
///    root cause is already surfaced by the priority diagnostic.
///
/// 2. **Full-line vs precise diagnostics.**  External tools (PHPStan,
///    PHPCS, Mago) sometimes report only a line number, producing a
///    diagnostic that spans the entire line (`char 0..1000+`).  When
///    any precise (sub-line) diagnostic exists on the same line, the
///    full-line diagnostic is suppressed because it obscures the more
///    useful precise marker.  Once the precise diagnostic is fixed,
///    the full-line one reappears on the next external tool run.
///
/// This is **not** deduplication in the traditional sense (removing
/// identical entries).  Each diagnostic source fully replaces its own
/// cache on every run, so true duplicates across sources do not occur.
fn suppress_imprecise_overlaps(diagnostics: &mut Vec<Diagnostic>) {
    if diagnostics.is_empty() {
        return;
    }

    // Collect the ranges of "priority" diagnostics that should
    // suppress `unresolved_member_access` hints.
    let priority_codes: &[&str] = &[
        "unknown_class",
        "unknown_member",
        "scalar_member_access",
        "unknown_function",
    ];

    let priority_ranges: Vec<Range> = diagnostics
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .map(|c| match c {
                    NumberOrString::String(s) => priority_codes.contains(&s.as_str()),
                    _ => false,
                })
                .unwrap_or(false)
        })
        .map(|d| d.range)
        .collect();

    // Collect lines that have at least one precise (sub-line)
    // diagnostic.  A diagnostic is "precise" when it does not span the
    // entire line, i.e. it has a meaningful character range rather than
    // `0..MAX`.  External tools like PHPStan only report a line number,
    // so their diagnostics stretch the full line.  A full-line underline
    // obscures the precise location and makes it harder for the
    // developer to spot the problem, so we suppress it unconditionally
    // when any precise diagnostic exists on the same line.
    let mut lines_with_precise: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for d in diagnostics.iter() {
        if !is_full_line_range(&d.range) {
            lines_with_precise.insert(d.range.start.line);
        }
    }

    diagnostics.retain(|d| {
        let is_unresolved = d
            .code
            .as_ref()
            .map(|c| match c {
                NumberOrString::String(s) => s == "unresolved_member_access",
                _ => false,
            })
            .unwrap_or(false);

        if is_unresolved {
            // Suppress if any priority diagnostic overlaps this range.
            return !priority_ranges
                .iter()
                .any(|pr| ranges_overlap(pr, &d.range));
        }

        // Suppress full-line diagnostics when any precise diagnostic
        // exists on the same line.  See the doc comment on this
        // function for the rationale.
        if is_full_line_range(&d.range) && lines_with_precise.contains(&d.range.start.line) {
            return false;
        }

        true
    });

    // Sort by range for stable output order.
    diagnostics.sort_by(|a, b| {
        a.range
            .start
            .line
            .cmp(&b.range.start.line)
            .then_with(|| a.range.start.character.cmp(&b.range.start.character))
            .then_with(|| a.range.end.line.cmp(&b.range.end.line))
            .then_with(|| a.range.end.character.cmp(&b.range.end.character))
    });
}

/// Returns `true` if the range spans a full line (character 0 to a
/// very large end character).  PHPStan and other line-only tools
/// produce these ranges because they don't report column information.
fn is_full_line_range(range: &Range) -> bool {
    range.start.line == range.end.line && range.start.character == 0 && range.end.character >= 1000
}

/// Remove diagnostics suppressed by `@phpantom-ignore` comments.
///
/// Supports two forms:
/// - **Same-line:** `$x->foo; // @phpantom-ignore unknown_member`
/// - **Next-line:** `// @phpantom-ignore unused_variable` on the line above.
///
/// Multiple codes can be comma-separated:
/// `// @phpantom-ignore unknown_member, unused_variable`
///
/// A bare `@phpantom-ignore` (no codes) suppresses ALL diagnostics on
/// that line.
pub(crate) fn filter_ignored_by_comment(diagnostics: &mut Vec<Diagnostic>, content: &str) {
    if diagnostics.is_empty() {
        return;
    }

    // Pre-compute per-line ignore sets.  A `None` value means "ignore all".
    // A `Some(set)` means only ignore those specific codes.
    let lines: Vec<&str> = content.lines().collect();
    let mut ignore_map: std::collections::HashMap<u32, Option<Vec<&str>>> =
        std::collections::HashMap::new();

    for (line_idx, line_text) in lines.iter().enumerate() {
        if let Some(ignore_pos) = line_text.find("@phpantom-ignore") {
            let after = &line_text[ignore_pos + "@phpantom-ignore".len()..];

            // Check this isn't `@phpantom-ignore-` (future extensions).
            if after.starts_with('-') {
                continue;
            }

            let codes: Option<Vec<&str>> = {
                let trimmed = after.trim();
                if trimmed.is_empty() || trimmed.starts_with("*/") {
                    None // bare ignore = suppress all
                } else {
                    // Strip trailing */ for block comments
                    let trimmed = trimmed.trim_end_matches("*/").trim();
                    Some(
                        trimmed
                            .split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    )
                }
            };

            // Determine whether this is a same-line or next-line ignore.
            // If the comment is the only non-whitespace on the line
            // (after stripping the `//` or `/*` prefix), it applies to
            // the next line.  Otherwise it applies to the same line.
            let before_comment = &line_text[..ignore_pos];
            let is_standalone = before_comment
                .trim()
                .trim_start_matches("//")
                .trim_start_matches("/*")
                .trim_start_matches('*')
                .trim()
                .is_empty();

            let target_line = if is_standalone {
                line_idx as u32 + 1 // next line
            } else {
                line_idx as u32 // same line
            };

            ignore_map.insert(target_line, codes);
        }
    }

    if ignore_map.is_empty() {
        return;
    }

    diagnostics.retain(|d| {
        let line = d.range.start.line;
        if let Some(codes) = ignore_map.get(&line) {
            match codes {
                None => false, // suppress all
                Some(code_list) => {
                    // Check if this diagnostic's code is in the list.
                    let diag_code = d.code.as_ref().and_then(|c| match c {
                        NumberOrString::String(s) => Some(s.as_str()),
                        _ => None,
                    });
                    if let Some(dc) = diag_code {
                        !code_list.contains(&dc)
                    } else {
                        true // no code = can't suppress
                    }
                }
            }
        } else {
            true
        }
    });
}

// ── Helpers ─────────────────────────────────────────────────────────────────

impl Backend {
    /// Convert a byte range from the preprocessed (virtual) content back to
    /// an LSP range in the original source file.
    ///
    /// For standard PHP files, this is a straight conversion.  For Blade
    /// files, it converts the bytes to positions in the virtual PHP, then
    /// translates those positions back to original Blade coordinates using
    /// the source map.
    pub(crate) fn offset_range_to_lsp_range(
        &self,
        uri: &str,
        content: &str,
        start_byte: usize,
        end_byte: usize,
    ) -> Option<Range> {
        let virtual_php_handle = self.blade_virtual_content.read();
        if let Some(virtual_php) = virtual_php_handle.get(uri)
            && let Some(map) = self.blade_source_maps.read().get(uri)
        {
            if start_byte > virtual_php.len() || end_byte > virtual_php.len() {
                return None;
            }

            let mut range = crate::util::byte_range_to_lsp_range(virtual_php, start_byte, end_byte);

            if range.start.line < crate::blade::PROLOGUE_LINES {
                // Diagnostic originates from the prologue (injected headers).
                // We skip these to avoid false positives on line 1 of Blade.
                return None;
            }

            range.start = map.php_to_blade(range.start);
            range.end = map.php_to_blade(range.end);

            return Some(range);
        }

        // Fallback for standard PHP or if map is missing
        if start_byte > content.len() || end_byte > content.len() {
            return None;
        }

        Some(crate::util::byte_range_to_lsp_range(
            content, start_byte, end_byte,
        ))
    }
}

/// Build a diagnostic range from byte offsets, returning `None` if either
/// offset is past the end of `content`.
///
/// This thin wrapper around [`crate::util::byte_range_to_lsp_range`] adds
/// a bounds check so that stale byte offsets (e.g. from a previous AST
/// after an edit) are rejected instead of silently clamped to EOF.
pub(crate) fn offset_range_to_lsp_range(
    content: &str,
    start_byte: usize,
    end_byte: usize,
) -> Option<Range> {
    if start_byte > content.len() || end_byte > content.len() {
        return None;
    }
    Some(crate::util::byte_range_to_lsp_range(
        content, start_byte, end_byte,
    ))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ─────────────────────────────────────────────────────

    fn make_range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Range {
        Range {
            start: Position {
                line: start_line,
                character: start_char,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        }
    }

    fn make_diagnostic(
        range: Range,
        severity: DiagnosticSeverity,
        code: &str,
        message: &str,
    ) -> Diagnostic {
        Diagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String(code.to_string())),
            code_description: None,
            source: Some("phpantom".to_string()),
            message: message.to_string(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    // ── ranges_overlap ──────────────────────────────────────────────

    #[test]
    fn overlapping_ranges_on_same_line() {
        let a = make_range(5, 0, 5, 10);
        let b = make_range(5, 5, 5, 15);
        assert!(ranges_overlap(&a, &b));
        assert!(ranges_overlap(&b, &a));
    }

    #[test]
    fn non_overlapping_ranges_on_same_line() {
        let a = make_range(5, 0, 5, 5);
        let b = make_range(5, 5, 5, 10);
        assert!(!ranges_overlap(&a, &b));
        assert!(!ranges_overlap(&b, &a));
    }

    #[test]
    fn non_overlapping_ranges_on_different_lines() {
        let a = make_range(1, 0, 1, 10);
        let b = make_range(2, 0, 2, 10);
        assert!(!ranges_overlap(&a, &b));
    }

    #[test]
    fn identical_ranges_overlap() {
        let r = make_range(3, 5, 3, 10);
        assert!(ranges_overlap(&r, &r));
    }

    #[test]
    fn contained_range_overlaps() {
        let outer = make_range(1, 0, 10, 0);
        let inner = make_range(5, 5, 5, 10);
        assert!(ranges_overlap(&outer, &inner));
        assert!(ranges_overlap(&inner, &outer));
    }

    // ── suppress_imprecise_overlaps ─────────────────────────────────

    #[test]
    fn suppresses_unresolved_member_when_unknown_class_overlaps() {
        let range = make_range(5, 0, 5, 15);
        let mut diags = vec![
            make_diagnostic(
                range,
                DiagnosticSeverity::WARNING,
                "unknown_class",
                "Unknown class X",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::HINT,
                "unresolved_member_access",
                "Unresolved member access on X",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("unknown_class".to_string()))
        );
    }

    #[test]
    fn suppresses_unresolved_member_when_unknown_member_overlaps() {
        let range = make_range(10, 0, 10, 20);
        let mut diags = vec![
            make_diagnostic(
                range,
                DiagnosticSeverity::WARNING,
                "unknown_member",
                "Unknown member foo",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::HINT,
                "unresolved_member_access",
                "Unresolved member access",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("unknown_member".to_string()))
        );
    }

    #[test]
    fn suppresses_unresolved_member_when_scalar_member_access_overlaps() {
        let range_outer = make_range(3, 0, 3, 20);
        let range_inner = make_range(3, 5, 3, 15);
        let mut diags = vec![
            make_diagnostic(
                range_outer,
                DiagnosticSeverity::ERROR,
                "scalar_member_access",
                "Cannot access member on scalar",
            ),
            make_diagnostic(
                range_inner,
                DiagnosticSeverity::HINT,
                "unresolved_member_access",
                "Unresolved member access",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("scalar_member_access".to_string()))
        );
    }

    #[test]
    fn keeps_unresolved_member_when_no_priority_diagnostic() {
        let range = make_range(5, 0, 5, 15);
        let mut diags = vec![make_diagnostic(
            range,
            DiagnosticSeverity::HINT,
            "unresolved_member_access",
            "Unresolved member access",
        )];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn keeps_unresolved_member_on_different_range() {
        let mut diags = vec![
            make_diagnostic(
                make_range(5, 0, 5, 10),
                DiagnosticSeverity::WARNING,
                "unknown_class",
                "Unknown class X",
            ),
            make_diagnostic(
                make_range(10, 0, 10, 10),
                DiagnosticSeverity::HINT,
                "unresolved_member_access",
                "Unresolved member access on Y",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn suppresses_multiple_unresolved_members_with_priority_overlap() {
        let range = make_range(5, 0, 5, 15);
        let mut diags = vec![
            make_diagnostic(
                range,
                DiagnosticSeverity::WARNING,
                "unknown_class",
                "Unknown class X",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::HINT,
                "unresolved_member_access",
                "Unresolved 1",
            ),
            make_diagnostic(
                range,
                DiagnosticSeverity::HINT,
                "unresolved_member_access",
                "Unresolved 2",
            ),
            make_diagnostic(
                make_range(20, 0, 20, 10),
                DiagnosticSeverity::HINT,
                "unresolved_member_access",
                "Unresolved 3 (different range)",
            ),
        ];
        suppress_imprecise_overlaps(&mut diags);
        // Only the unknown_class + the one on a different range should survive.
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn no_op_when_no_diagnostics() {
        let mut diags: Vec<Diagnostic> = vec![];
        suppress_imprecise_overlaps(&mut diags);
        assert!(diags.is_empty());
    }

    #[test]
    fn suppresses_full_line_phpstan_when_precise_diagnostic_on_same_line() {
        // A full-line diagnostic (from a tool that only reports line
        // numbers) is suppressed when any precise diagnostic exists on
        // the same line, regardless of error codes.  The precise
        // diagnostic pinpoints the exact location; the full-line
        // underline just adds noise.
        let phpstan = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Parameter #1 $x expects int, string given.".to_string(),
            ..Default::default()
        };
        let precise = Diagnostic {
            range: make_range(5, 10, 5, 20),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("unknown_class".to_string())),
            source: Some("phpantom".to_string()),
            message: "Class 'Foo' not found".to_string(),
            ..Default::default()
        };
        let mut diags = vec![phpstan, precise.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, precise.message);
    }

    #[test]
    fn suppresses_full_line_regardless_of_code() {
        // Suppression is unconditional — we cannot reliably determine
        // whether diagnostics from different tools (Mago parser,
        // PHPStan, native PHPantom) describe the same issue because
        // they use completely different error codes and descriptions.
        // Any precise diagnostic on the same line is enough.
        let phpstan = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("class.prefixed".to_string())),
            source: Some("phpstan".to_string()),
            message: "Class prefixed with vendor namespace.".to_string(),
            ..Default::default()
        };
        let syntax_error = Diagnostic {
            range: make_range(5, 3, 5, 10),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("syntax_error".to_string())),
            source: Some("phpantom".to_string()),
            message: "Syntax error: unexpected token `->`".to_string(),
            ..Default::default()
        };
        let mut diags = vec![phpstan, syntax_error.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, syntax_error.message);
    }

    #[test]
    fn keeps_full_line_phpstan_when_no_precise_diagnostic_on_line() {
        let phpstan = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Parameter #1 $x expects int, string given.".to_string(),
            ..Default::default()
        };
        let precise_other_line = Diagnostic {
            range: make_range(10, 3, 10, 15),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("unknown_class".to_string())),
            source: Some("phpantom".to_string()),
            message: "Class 'Bar' not found".to_string(),
            ..Default::default()
        };
        let mut diags = vec![phpstan.clone(), precise_other_line.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn keeps_precise_phpstan_diagnostic_on_same_line() {
        // If a future PHPStan version provides column info, don't suppress it.
        let phpstan_precise = Diagnostic {
            range: make_range(5, 8, 5, 20),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Parameter #1 $x expects int, string given.".to_string(),
            ..Default::default()
        };
        let native_precise = Diagnostic {
            range: make_range(5, 3, 5, 10),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("unknown_class".to_string())),
            source: Some("phpantom".to_string()),
            message: "Class 'Foo' not found".to_string(),
            ..Default::default()
        };
        let mut diags = vec![phpstan_precise.clone(), native_precise.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn suppresses_multiple_full_line_diags_when_precise_exists() {
        let phpstan1 = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("argument.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Error one".to_string(),
            ..Default::default()
        };
        let phpstan2 = Diagnostic {
            range: make_range(5, 0, 5, u32::MAX),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("return.type".to_string())),
            source: Some("phpstan".to_string()),
            message: "Error two".to_string(),
            ..Default::default()
        };
        let precise = Diagnostic {
            range: make_range(5, 2, 5, 8),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("unknown_member".to_string())),
            source: Some("phpantom".to_string()),
            message: "Method 'foo' not found".to_string(),
            ..Default::default()
        };
        let mut diags = vec![phpstan1, phpstan2, precise.clone()];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, precise.message);
    }

    #[test]
    fn keeps_multiple_diagnostics_on_same_range() {
        // Each source is authoritative — two PHPantom diagnostics on
        // the same span are both shown.
        let range = make_range(7, 3, 7, 12);
        let diag1 = make_diagnostic(
            range,
            DiagnosticSeverity::WARNING,
            "unknown_member",
            "Method 'foo' not found on class Bar",
        );
        let diag2 = make_diagnostic(
            range,
            DiagnosticSeverity::HINT,
            "deprecated_usage",
            "Method 'foo' is deprecated",
        );
        let mut diags = vec![diag1, diag2];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn keeps_multiple_phpstan_diagnostics_on_same_line() {
        // If PHPStan reports five issues on a line and no precise
        // diagnostic exists, all five survive.
        let make_phpstan = |code: &str, msg: &str| Diagnostic {
            range: make_range(10, 0, 10, u32::MAX),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(code.to_string())),
            source: Some("phpstan".to_string()),
            message: msg.to_string(),
            ..Default::default()
        };
        let mut diags = vec![
            make_phpstan("argument.type", "Parameter #1 expects int, string given."),
            make_phpstan("return.type", "Should return int but returns string."),
            make_phpstan("missingType.return", "Method has no return type."),
        ];
        suppress_imprecise_overlaps(&mut diags);
        assert_eq!(diags.len(), 3);
    }

    // ── is_stale_phpstan_diagnostic ─────────────────────────────────

    /// Helper: build a PHPStan-style full-line diagnostic.
    fn make_phpstan_diag(line: u32, code: &str, message: &str) -> Diagnostic {
        Diagnostic {
            range: make_range(line, 0, line, 200),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(code.to_string())),
            source: Some("PHPStan".to_string()),
            message: message.to_string(),
            ..Default::default()
        }
    }

    // ── Per-identifier heuristics removed ───────────────────────────
    //
    // The throws.unusedType, missingType.checkedException, and
    // method.missingOverride stale-detection branches have been
    // removed.  These diagnostics are now cleared eagerly by
    // `codeAction/resolve` (see `clear_phpstan_diagnostics_after_resolve`).
    // The tests below verify they are no longer considered stale by
    // `is_stale_phpstan_diagnostic` alone.

    #[test]
    fn throws_unused_type_not_stale_via_heuristic() {
        // Previously this was detected as stale because the @throws
        // tag was removed.  Now only codeAction/resolve clears it.
        let content = "<?php\nclass Foo {\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            2,
            "throws.unusedType",
            "Method App\\Foo::bar() has App\\Exceptions\\FooException in PHPDoc @throws tag but it's not thrown.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "throws.unusedType should NOT be stale via heuristic (cleared by resolve instead)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_mixed_reason_and_bare() {
        // First identifier has no reason, second has a reason.
        let content =
            "<?php\n$x = foo(); // @phpstan-ignore return.type, argument.type (not a real error)\n";
        let return_diag = make_phpstan_diag(1, "return.type", "...");
        let arg_diag = make_phpstan_diag(1, "argument.type", "...");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "bare identifier (no reason) should be stale"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "identifier with reason should be stale"
        );
    }

    #[test]
    fn stale_phpstan_ignore_reason_then_bare() {
        // First identifier has a reason, second is bare.
        let content =
            "<?php\n$x = foo(); // @phpstan-ignore return.type (some reason), argument.type\n";
        let return_diag = make_phpstan_diag(1, "return.type", "...");
        let arg_diag = make_phpstan_diag(1, "argument.type", "...");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "identifier with reason should be stale"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "bare identifier (no reason) after one with reason should be stale"
        );
    }

    #[test]
    fn missing_checked_exception_not_stale_via_heuristic() {
        // Previously this was detected as stale because a @throws
        // tag was added.  Now only codeAction/resolve clears it.
        let content = "<?php\nclass Foo {\n    /**\n     * @throws FooException\n     */\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            5,
            "missingType.checkedException",
            "Method App\\Foo::bar() throws checked exception App\\Exceptions\\FooException but it's missing from the PHPDoc @throws tag.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "missingType.checkedException should NOT be stale via heuristic (cleared by resolve instead)"
        );
    }

    #[test]
    fn stale_when_phpstan_ignore_covers_identifier() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore return.type\n}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale when @phpstan-ignore lists the identifier"
        );
    }

    #[test]
    fn not_stale_when_phpstan_ignore_covers_different_identifier() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore argument.type\n}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "should NOT be stale when @phpstan-ignore lists a different identifier"
        );
    }

    #[test]
    fn not_stale_for_phpstan_ignore_line_blanket() {
        // @phpstan-ignore-line suppresses everything, but we don't
        // eagerly prune for it — only per-identifier ignores count.
        let content =
            "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore-line\n}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "should NOT be stale for blanket @phpstan-ignore-line"
        );
    }

    // ── method.missingOverride stale detection ──────────────────────

    // ── method.missingOverride — heuristic removed ──────────────────

    #[test]
    fn missing_override_not_stale_via_heuristic() {
        // Previously this was detected as stale because #[Override]
        // was found above the method.  Now only codeAction/resolve
        // clears it.
        let content = "<?php\nclass Foo extends Bar {\n    #[\\Override]\n    public function baz(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "method.missingOverride",
            "Method Foo::baz() overrides method Bar::baz() but is missing the #[\\Override] attribute.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "method.missingOverride should NOT be stale via heuristic (cleared by resolve instead)"
        );
    }

    #[test]
    fn not_stale_for_phpstan_ignore_next_line_blanket() {
        let content = "<?php\nclass Foo {\n    // @phpstan-ignore-next-line\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "should NOT be stale for blanket @phpstan-ignore-next-line"
        );
    }

    #[test]
    fn stale_when_phpstan_ignore_on_previous_line() {
        let content = "<?php\nclass Foo {\n    // @phpstan-ignore return.type\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale when @phpstan-ignore on previous line lists the identifier"
        );
    }

    #[test]
    fn stale_phpstan_ignore_with_multiple_ids() {
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore return.type, argument.type\n}\n";
        let return_diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        let arg_diag = make_phpstan_diag(
            2,
            "argument.type",
            "Parameter #1 $x expects string, int given.",
        );
        let other_diag = make_phpstan_diag(2, "method.notFound", "Call to undefined method.");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "return.type should be stale (listed in ignore)"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "argument.type should be stale (listed in ignore)"
        );
        assert!(
            !is_stale_phpstan_diagnostic(&other_diag, content),
            "method.notFound should NOT be stale (not listed)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_with_reason_in_docblock() {
        // `/** @phpstan-ignore id (reason) */` — the `*/` comes after
        // the reason text, so the parser must not treat everything
        // up to `*/` as the identifier list.
        let content = "<?php\nclass Foo {\n    /** @phpstan-ignore return.type (not a real error) */\n    public function bar(): void {}\n}\n";
        let diag = make_phpstan_diag(
            3,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale when @phpstan-ignore in docblock has a (reason)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_with_per_id_reasons() {
        // Each identifier has its own `(reason)`:
        //   @phpstan-ignore return.type (reason1), argument.type (reason2)
        let content = "<?php\nclass Foo {\n    public function bar(): void {} // @phpstan-ignore return.type (not real), argument.type (also fine)\n}\n";
        let return_diag = make_phpstan_diag(
            2,
            "return.type",
            "Method App\\Foo::bar() should return string but returns void.",
        );
        let arg_diag = make_phpstan_diag(
            2,
            "argument.type",
            "Parameter #1 $x expects string, int given.",
        );
        let other_diag = make_phpstan_diag(2, "method.notFound", "Call to undefined method.");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "return.type should be stale (first in list with reason)"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "argument.type should be stale (second in list with reason)"
        );
        assert!(
            !is_stale_phpstan_diagnostic(&other_diag, content),
            "method.notFound should NOT be stale (not listed)"
        );
    }

    #[test]
    fn stale_phpstan_ignore_reason_with_commas() {
        // The reason text contains commas — must not confuse the parser.
        let content = "<?php\n/** @phpstan-ignore return.type (accepts A, B, and C but PHPDoc is narrower) */\nfunction foo(): void {}\n";
        let diag = make_phpstan_diag(
            2,
            "return.type",
            "Function foo() should return string but returns void.",
        );
        assert!(
            is_stale_phpstan_diagnostic(&diag, content),
            "should be stale even when reason text contains commas"
        );
    }

    #[test]
    fn stale_phpstan_ignore_multiple_ids_each_with_comma_reasons() {
        // Multiple identifiers, each with reason text that contains commas.
        // This is the trickiest case: the parser must not confuse commas
        // inside `(reason)` with the comma separating identifiers.
        let content = "<?php\n// @phpstan-ignore return.type (accepts A, B, C), argument.type (needs X, Y)\nfunction foo(): void {}\n";
        let return_diag = make_phpstan_diag(
            1,
            "return.type",
            "Function foo() should return string but returns void.",
        );
        let arg_diag = make_phpstan_diag(
            1,
            "argument.type",
            "Parameter #1 $x expects string, int given.",
        );
        let other_diag = make_phpstan_diag(1, "method.notFound", "Call to undefined method.");
        assert!(
            is_stale_phpstan_diagnostic(&return_diag, content),
            "return.type should be stale (first id, reason has commas)"
        );
        assert!(
            is_stale_phpstan_diagnostic(&arg_diag, content),
            "argument.type should be stale (second id, reason has commas)"
        );
        assert!(
            !is_stale_phpstan_diagnostic(&other_diag, content),
            "method.notFound should NOT be stale (not listed)"
        );
    }

    #[test]
    fn diag_with_no_code_is_never_stale() {
        let content = "<?php\n// @phpstan-ignore return.type\nfoo();";
        let diag = Diagnostic {
            range: make_range(1, 0, 1, 200),
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            source: Some("PHPStan".to_string()),
            message: "Some error.".to_string(),
            ..Default::default()
        };
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "diagnostic without a code should never be considered stale"
        );
    }

    #[test]
    fn ignore_unmatched_diag_is_never_stale_via_ignore_check() {
        // ignore.unmatched diagnostics should not be pruned by the
        // @phpstan-ignore check (they ARE the ignore comment).
        let content = "<?php\n$x = 1; // @phpstan-ignore ignore.unmatchedIdentifier\n";
        let diag = make_phpstan_diag(
            1,
            "ignore.unmatchedIdentifier",
            "No error with identifier foo is reported on line 2.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "ignore.unmatched* diagnostics must not be pruned by the ignore check"
        );
    }

    // ── Scoped docblock checks ──────────────────────────────────────
    //
    // The scoped docblock heuristics have been removed alongside the
    // per-identifier stale detection.  These tests verify the new
    // behaviour: throws/override diagnostics are never stale via
    // heuristic (they are cleared by codeAction/resolve instead).

    #[test]
    fn throws_not_stale_even_when_tag_on_same_function() {
        // Previously this was stale because @throws FooException was
        // found on baz()'s own docblock.  Now it's not — resolve
        // handles clearing.
        let content = concat!(
            "<?php\nclass Foo {\n",
            "    public function bar(): void {}\n",
            "    /**\n",
            "     * @throws FooException\n",
            "     */\n",
            "    public function baz(): void {\n",
            "        throw new FooException();\n",
            "    }\n",
            "}\n",
        );
        let diag = make_phpstan_diag(
            7,
            "missingType.checkedException",
            "Method App\\Foo::baz() throws checked exception App\\Exceptions\\FooException but it's missing from the PHPDoc @throws tag.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&diag, content),
            "missingType.checkedException should NOT be stale via heuristic"
        );
    }

    #[test]
    fn unused_throws_not_stale_via_heuristic_even_when_tag_removed() {
        // Previously baz()'s diagnostic was stale because the tag was
        // removed.  Now neither is stale via heuristic.
        let content = concat!(
            "<?php\nclass Foo {\n",
            "    /**\n",
            "     * @throws FooException\n",
            "     */\n",
            "    public function bar(): void {\n",
            "    }\n",
            "    public function baz(): void {\n",
            "    }\n",
            "}\n",
        );
        let bar_diag = make_phpstan_diag(
            5,
            "throws.unusedType",
            "Method App\\Foo::bar() has App\\Exceptions\\FooException in PHPDoc @throws tag but it's not thrown.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&bar_diag, content),
            "bar()'s throws.unusedType should NOT be stale via heuristic"
        );

        let baz_diag = make_phpstan_diag(
            7,
            "throws.unusedType",
            "Method App\\Foo::baz() has App\\Exceptions\\FooException in PHPDoc @throws tag but it's not thrown.",
        );
        assert!(
            !is_stale_phpstan_diagnostic(&baz_diag, content),
            "baz()'s throws.unusedType should NOT be stale via heuristic"
        );
    }

    #[test]
    fn enclosing_docblock_text_finds_correct_docblock() {
        let content = concat!(
            "<?php\nclass Foo {\n",
            "    /**\n",
            "     * @throws BarException\n",
            "     */\n",
            "    public function bar(): void {\n",
            "        // line 6\n",
            "    }\n",
            "    /**\n",
            "     * @throws BazException\n",
            "     */\n",
            "    public function baz(): void {\n",
            "        // line 12\n",
            "    }\n",
            "}\n",
        );
        let bar_doc = enclosing_docblock_text(content, 6);
        assert!(
            bar_doc.contains("BarException"),
            "bar()'s docblock should mention BarException, got: {}",
            bar_doc
        );
        assert!(
            !bar_doc.contains("BazException"),
            "bar()'s docblock should NOT mention BazException, got: {}",
            bar_doc
        );

        let baz_doc = enclosing_docblock_text(content, 12);
        assert!(
            baz_doc.contains("BazException"),
            "baz()'s docblock should mention BazException, got: {}",
            baz_doc
        );
        assert!(
            !baz_doc.contains("BarException"),
            "baz()'s docblock should NOT mention BarException, got: {}",
            baz_doc
        );
    }

    #[test]
    fn enclosing_docblock_text_returns_empty_when_no_docblock() {
        let content = "<?php\nfunction foo(): void {\n    // line 2\n}\n";
        let doc = enclosing_docblock_text(content, 2);
        assert!(
            doc.is_empty(),
            "should return empty when no docblock exists, got: {}",
            doc
        );
    }

    #[test]
    fn test_offset_range_to_lsp_range_ignores_prologue() {
        let backend = Backend::new_test();
        let uri = "file:///test.blade.php";
        let content = "Hello World";
        backend.update_ast(uri, content);

        // First 6 lines are prologue (including wrapper function declaration).
        let virtual_php = {
            let vc_handle = backend.blade_virtual_content.read();
            vc_handle
                .get(uri)
                .cloned()
                .expect("Virtual content should exist")
        };

        // Find start of line 5 (0-indexed).
        let mut offset = 0;
        let mut lines_seen = 0;
        for (i, ch) in virtual_php.char_indices() {
            if lines_seen == 5 {
                offset = i;
                break;
            }
            if ch == '\n' {
                lines_seen += 1;
            }
        }

        // byte range in prologue
        let range = backend.offset_range_to_lsp_range(uri, content, offset, offset + 5);
        assert!(range.is_none(), "Diagnostic in prologue should be ignored");

        // byte range after prologue (line 6+)
        let mut after_offset = 0;
        let mut lines_seen = 0;
        for (i, ch) in virtual_php.char_indices() {
            if lines_seen == 6 {
                after_offset = i;
                break;
            }
            if ch == '\n' {
                lines_seen += 1;
            }
        }

        let range_after =
            backend.offset_range_to_lsp_range(uri, content, after_offset, after_offset + 5);
        assert!(
            range_after.is_some(),
            "Diagnostic after prologue should be kept"
        );
    }

    /// A diagnostic pull must defer to the debounced background worker and
    /// never run the expensive native pass on the request path. Computing
    /// inline holds a tower-lsp concurrency slot for the full multi-second
    /// pass, and editors pull on every keystroke — so a typing burst would
    /// fill every slot and wedge the transport (the regression this guards).
    #[test]
    fn pull_diagnostics_defer_to_worker_and_never_compute_inline() {
        use std::sync::atomic::Ordering;

        let backend = crate::Backend::new_test();
        backend.init_complete.store(true, Ordering::Release);
        backend
            .supports_pull_diagnostics
            .store(true, Ordering::Release);

        let uri = "file:///defer.php";
        backend.update_ast(uri, "<?php\nclass A { public function f(): void {} }\n");

        backend.trigger_diagnostics_for_pull(uri);

        assert!(
            backend.diag_pending_uris.lock().contains(uri),
            "pull must schedule the file for the background worker"
        );
        assert!(
            !backend.diag_last_full.lock().contains_key(uri),
            "pull must not compute diagnostics inline on the request path"
        );
    }
}
