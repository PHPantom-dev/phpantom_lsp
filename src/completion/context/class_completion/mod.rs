//! Class name completions.
//!
//! This module handles building completion items for class, interface,
//! trait, and enum names when no member-access operator (`->` or `::`)
//! is present.
//!
//! Also provides a Throwable-filtered variant for catch clause fallback
//! and `throw new` completion, which only suggests exception classes
//! from already-parsed sources and includes everything else (class index,
//! stubs) unfiltered.
//!
//! Constant, function, and namespace completions live in their own
//! sibling modules (`constant_completion`, `function_completion`,
//! `namespace_completion`).
use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::text_position::utf16_col_to_char_index;
use crate::util::{short_name, strip_fqn_prefix};

mod attributes;
mod context_detect;
mod ctor;
mod heuristics;
mod items;
mod name_context;
mod ranking;

#[cfg(test)]
pub(crate) use context_detect::detect_stub_class_kind;
pub(crate) use context_detect::{detect_class_name_context, is_class_declaration_name};
use items::ClassItemCtx;
pub(crate) use name_context::ClassNameContext;
pub(in crate::completion) use ranking::match_quality;
pub(crate) use ranking::{affinity_score, build_affinity_table};

/// Expand a namespace alias at the start of a typed prefix.
///
/// Given a prefix like `OA\Re` and a use-map containing
/// `OA → OpenApi\Attributes`, returns `Some("OpenApi\Attributes\Re")`.
/// Returns `None` when the first segment of the prefix is not a known
/// alias or the prefix contains no backslash.
pub(in crate::completion) fn expand_alias_prefix(
    normalized_prefix: &str,
    use_map: &HashMap<String, String>,
) -> Option<String> {
    let bs = normalized_prefix.find('\\')?;
    let first_segment = &normalized_prefix[..bs];
    let rest = &normalized_prefix[bs + 1..]; // may be empty
    let fqn_ns = use_map.get(first_segment)?;
    if rest.is_empty() {
        Some(format!("{}\\", fqn_ns))
    } else {
        Some(format!("{}\\{}", fqn_ns, rest))
    }
}

/// Check whether a class matches the typed prefix.
///
/// In FQN-prefix mode (`is_fqn` is `true`) both the short name and the
/// fully-qualified name are checked so that `App\Models\U` can surface
/// `App\Models\User`.  When `expanded_prefix_lower` is provided (the
/// alias-expanded form of the prefix, e.g. `openapi\attributes\re` for
/// a typed `OA\Re`), it is also checked against the FQN so that
/// classes under an aliased namespace are matched.
///
/// In non-FQN mode only the short name is checked to avoid flooding the
/// response with every class under a broad namespace prefix.
pub(in crate::completion) fn matches_class_prefix(
    short_name: &str,
    fqn: &str,
    prefix_lower: &str,
    is_fqn: bool,
    expanded_prefix_lower: Option<&str>,
) -> bool {
    if is_fqn {
        short_name.to_lowercase().contains(prefix_lower)
            || fqn.to_lowercase().contains(prefix_lower)
            || expanded_prefix_lower.is_some_and(|exp| fqn.to_lowercase().contains(exp))
    } else {
        short_name.to_lowercase().contains(prefix_lower)
    }
}

/// Parameters for [`Backend::build_class_name_completions`].
///
/// Groups the arguments that the three call sites assemble differently
/// (especially `affinity_table_override` for `UseImport` context).
pub(crate) struct ClassCompletionParams<'a> {
    pub(crate) file_use_map: &'a HashMap<String, String>,
    pub(crate) file_namespace: &'a Option<String>,
    pub(crate) prefix: &'a str,
    pub(crate) content: &'a str,
    pub(crate) context: ClassNameContext,
    pub(crate) position: Position,
    /// When `Some`, used for namespace affinity scoring instead of
    /// building a table from `file_use_map`.  Needed for `UseImport`
    /// context where the caller passes an empty use-map (to suppress
    /// bogus source-1 entries) but still wants affinity from the real
    /// imports.
    pub(crate) affinity_table_override: Option<HashMap<String, u32>>,
    /// The file URI where the completion was triggered, used for
    /// `completionItem/resolve` data.
    pub(crate) uri: &'a str,
}

impl Backend {
    /// Extract the partial identifier (class name fragment) that the user
    /// is currently typing at the given cursor position.
    ///
    /// Walks backward from the cursor through alphanumeric characters,
    /// underscores, and backslashes (namespace separators).  Returns
    /// `None` if the resulting text starts with `$` (variable context)
    /// or is empty.
    pub fn extract_partial_class_name(content: &str, position: Position) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();
        if position.line as usize >= lines.len() {
            return None;
        }

        let line = lines[position.line as usize];
        let chars: Vec<char> = line.chars().collect();
        let col = utf16_col_to_char_index(line, position.character);

        // Walk backwards through identifier characters (including `\`)
        let mut i = col;
        while i > 0
            && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\')
        {
            i -= 1;
        }

        if i == col {
            // Nothing typed — no partial identifier
            return None;
        }

        // If preceded by `$`, this is a variable, not a class name
        if i > 0 && chars[i - 1] == '$' {
            return None;
        }

        // If preceded by `->` or `::`, member completion handles this
        if i >= 2 && chars[i - 2] == '-' && chars[i - 1] == '>' {
            return None;
        }
        if i >= 2 && chars[i - 2] == ':' && chars[i - 1] == ':' {
            return None;
        }

        // If preceded by `<?`, this is the PHP open tag (`<?php`), not
        // a class/function name — suppress completion entirely.
        if i >= 2 && chars[i - 2] == '<' && chars[i - 1] == '?' {
            return None;
        }

        let partial: String = chars[i..col].iter().collect();
        if partial.is_empty() {
            return None;
        }

        Some(partial)
    }

    /// Maximum number of class name completions to return.
    ///
    /// After this limit the result is marked `is_incomplete = true` so
    /// the editor re-requests as the user types more characters.
    pub(in crate::completion) const MAX_CLASS_COMPLETIONS: usize = 100;

    /// Build completion items for class, interface, trait, and enum
    /// names.
    ///
    /// Searches five sources in priority order:
    ///   1. File's `use` imports (already imported)
    ///   2. Same-namespace classes (from `uri_classes_index`)
    ///   3. `fqn_uri_index` (discovered / interacted-with classes)
    ///   4. Class index (all autoloaded classes)
    ///   5. Built-in PHP stubs
    ///
    /// Returns `(items, is_incomplete)`.
    pub(crate) fn build_class_name_completions(
        &self,
        params: ClassCompletionParams<'_>,
    ) -> (Vec<CompletionItem>, bool) {
        let ClassCompletionParams {
            file_use_map,
            file_namespace,
            prefix,
            content,
            context,
            position,
            affinity_table_override,
            uri,
        } = params;
        let is_new = context.is_new();
        let is_attribute = context.is_attribute();
        let is_use_import = matches!(context, ClassNameContext::UseImport);
        // In FQN mode (except UseImport), try to shorten references
        // using the file's existing `use` imports.  E.g. if the user
        // has `use Cassandra\Exception;`, typing `Exception\Al` should
        // insert `Exception\AlreadyExistsException` rather than the
        // full FQN.
        let should_shorten_via_imports = !is_use_import;
        let has_leading_backslash = prefix.starts_with('\\');
        let normalized = strip_fqn_prefix(prefix);
        let prefix_lower = normalized.to_lowercase();
        // In UseImport context, always treat the prefix as FQN so that
        // the full qualified name is inserted (not the short name) and
        // no redundant `use` text-edit is generated.
        let is_fqn_prefix = has_leading_backslash || normalized.contains('\\') || is_use_import;

        // When the prefix starts with an alias (e.g. `OA\Re` where
        // `use OpenApi\Attributes as OA`), expand it to the FQN form
        // (`OpenApi\Attributes\Re`) so that `matches_class_prefix` can
        // find classes under the aliased namespace.
        let expanded = expand_alias_prefix(normalized, file_use_map);
        let expanded_lower = expanded.as_deref().map(|s| s.to_lowercase());
        let expanded_prefix_lower = expanded_lower.as_deref();

        // In UseImport context, suppress namespace-relative
        // simplification — `use User;` is wrong even when the cursor
        // file lives in the same namespace as `User`.  Passing `None`
        // makes `class_edit_texts` emit the full FQN.
        let no_namespace: Option<String> = None;
        let effective_namespace = if is_use_import {
            &no_namespace
        } else {
            file_namespace
        };

        // When the user is typing a namespace-qualified reference (e.g.
        // `http\En`, `\App\Models\U`, or `\Demo`), the editor may treat
        // `\` as a word boundary and only replace the text after the
        // last `\`.  Provide an explicit replacement range covering the
        // entire typed prefix so the editor replaces it in full.
        let fqn_replace_range = if is_fqn_prefix {
            Some(Range {
                start: Position {
                    line: position.line,
                    character: position
                        .character
                        .saturating_sub(prefix.chars().count() as u32),
                },
                end: position,
            })
        } else {
            None
        };
        let mut seen_fqns: HashSet<String> = HashSet::new();
        let mut items: Vec<CompletionItem> = Vec::new();

        // Build the affinity table from the file's use-map and namespace,
        // unless the caller provided a pre-built one (e.g. UseImport
        // context where the real use-map differs from file_use_map).
        let affinity_table = affinity_table_override
            .unwrap_or_else(|| build_affinity_table(file_use_map, file_namespace));

        // Extract the short-name portion of the typed prefix for match
        // quality classification.  E.g. `"Order"` from `"App\\Order"`.
        let quality_prefix = match normalized.rfind('\\') {
            Some(pos) => normalized[pos + 1..].to_string(),
            None => normalized.to_string(),
        };

        // Pre-compute the use-block info for alphabetical `use` insertion.
        // In UseImport context, always treat as namespace-qualified so
        // labels remain FQNs (the user is writing a fully-qualified
        // import statement like `use Foo\Bar`).  Also treat a leading
        // backslash (e.g. `\Cassa`) as namespace-qualified so labels
        // stay as FQNs in explicit-global-namespace mode.
        let prefix_has_namespace =
            normalized.contains('\\') || has_leading_backslash || is_use_import;

        let needs_ctor = is_new || is_attribute;
        let ctx = ClassItemCtx {
            is_fqn_prefix,
            has_leading_backslash,
            shorten_via_imports: should_shorten_via_imports,
            is_new,
            is_attribute,
            fqn_replace_range,
            file_use_map,
            use_block: self.use_block_for(
                uri,
                content,
                self.import_block_range_at(
                    uri,
                    crate::text_position::position_to_byte_offset(content, position),
                ),
            ),
            file_namespace: effective_namespace,
            affinity_table,
            quality_prefix,
            prefix_has_namespace,
            uri,
        };

        // Build a reverse lookup: FQN → use-import short name.
        let use_imported_fqns: HashSet<&str> = file_use_map.values().map(String::as_str).collect();

        // Namespace prefix for same/sub-namespace proximity check.
        let ns_prefix = file_namespace.as_ref().map(|ns| format!("{}\\{}", ns, ""));

        // ── Helper: classify a FQN into source tier and demotion ─────
        //
        // Returns `None` to exclude, or
        // `Some((tier, origin_tier, demoted, deprecated))`.
        //   '0' = use-imported (already in file's use-map)
        //   '1' = same/sub namespace
        //   '2' = everything else
        let classify = |fqn: &str, sn: &str| -> Option<(char, char, bool, bool)> {
            // Context-aware kind filtering (instantiability, Throwable, etc.)
            if context.is_class_only() {
                match self.check_context_match(fqn, context) {
                    Some(false) => return None,
                    Some(true) => { /* passed */ }
                    None => {
                        // Unloaded class — pass through with heuristic
                        // demotion below.  In narrow contexts, reject
                        // entirely since we can't verify.
                        if context.is_narrow_kind() {
                            return None;
                        }
                    }
                }
            }

            // Determine tier by proximity.
            let tier = if use_imported_fqns.contains(fqn) {
                '0'
            } else {
                let in_same_or_sub_ns = match &ns_prefix {
                    Some(pfx) => fqn.starts_with(pfx.as_str()),
                    None => !fqn.contains('\\'),
                };
                if in_same_or_sub_ns { '1' } else { '2' }
            };

            let origin_tier = self
                .symbols
                .fqn_origin_index
                .read()
                .get(fqn)
                .copied()
                .unwrap_or(crate::ClassCompletionOrigin::Project)
                .sort_tier();

            // Demotion: only for unloaded classes where context-match
            // was unknown.
            let demoted = context.is_class_only()
                && self.check_context_match(fqn, context).is_none()
                && context.likely_mismatch(sn);

            // Deprecation flag from ClassInfo if available.
            let deprecated = self
                .find_class_in_uri_classes_index(fqn)
                .as_ref()
                .is_some_and(|c| c.deprecation_message.is_some());

            Some((tier, origin_tier, demoted, deprecated))
        };

        // ── Pass 1: fqn_uri_index (project + vendor classes) ────────
        // Collect keys first to avoid holding the read lock while
        // calling classify/is_likely_namespace_not_class (which
        // re-acquire the same lock — parking_lot RwLock is not
        // reentrant).
        let fqn_keys: Vec<String> = self
            .symbols
            .fqn_uri_index
            .read()
            .keys()
            .map(str::to_owned)
            .collect();
        for fqn in &fqn_keys {
            let sn = short_name(fqn);
            if !matches_class_prefix(sn, fqn, &prefix_lower, is_fqn_prefix, expanded_prefix_lower) {
                continue;
            }
            if !seen_fqns.insert(fqn.clone()) {
                continue;
            }
            let Some((tier, origin_tier, demoted, deprecated)) = classify(fqn, sn) else {
                continue;
            };
            // Skip namespace aliases.
            if self.is_likely_namespace_not_class(fqn) {
                continue;
            }
            let ctor = if needs_ctor {
                self.ctor_params_for(fqn)
            } else {
                None
            };
            items.push(ctx.build_item(
                ctx.item_texts(fqn, tier),
                fqn,
                tier,
                origin_tier,
                demoted,
                ctor.as_deref(),
                deprecated,
            ));
        }

        // ── Pass 2: stub_index (built-in PHP classes) ───────────────
        // Collect keys first to avoid deadlock (same reason as pass 1).
        let stub_keys: Vec<String> = self.stub_index.read().keys().map(str::to_owned).collect();
        for fqn in &stub_keys {
            let sn = short_name(fqn);
            if !matches_class_prefix(sn, fqn, &prefix_lower, is_fqn_prefix, expanded_prefix_lower) {
                continue;
            }
            if !seen_fqns.insert(fqn.to_string()) {
                continue;
            }
            let Some((tier, _origin_tier, demoted, deprecated)) = classify(fqn, sn) else {
                continue;
            };
            let ctor = if needs_ctor {
                self.ctor_params_for(fqn)
            } else {
                None
            };
            items.push(ctx.build_item(
                ctx.item_texts(fqn, tier),
                fqn,
                tier,
                crate::ClassCompletionOrigin::CoreStub.sort_tier(),
                demoted,
                ctor.as_deref(),
                deprecated,
            ));
        }

        // ── Namespace segment items (FQN mode only) ─────────────────
        // When the user is typing a namespace-qualified reference (e.g.
        // `App\`, `\Illuminate\Database\`), inject the distinct
        // next-level namespace segments as MODULE-kind items so the
        // user can drill into the namespace tree incrementally instead
        // of being overwhelmed by hundreds of deeply-nested classes.
        if is_fqn_prefix {
            // Everything up to and including the last `\` in the
            // normalized (no leading `\`) prefix.  For `App\Models\U`
            // this is `App\Models\`; for `App\` it is `App\`.
            let ns_prefix_end = normalized.rfind('\\').map(|p| p + 1).unwrap_or(0);

            // Only inject segments when the prefix actually contains a
            // backslash.  A bare name like `User` in UseImport context
            // has `is_fqn_prefix` true but no namespace to browse.
            if ns_prefix_end > 0 {
                // When the prefix starts with an alias, use the
                // expanded FQN form for namespace segment matching so
                // that `OA\` finds segments under `OpenApi\Attributes\`.
                let raw_ns_prefix = &normalized[..ns_prefix_end];
                let expanded_ns = expand_alias_prefix(raw_ns_prefix, file_use_map);
                let ns_prefix_lower = expanded_ns
                    .as_deref()
                    .unwrap_or(raw_ns_prefix)
                    .to_lowercase();
                // Partial text after the last `\` that the user is
                // still typing (e.g. `U` from `App\Models\U`).  Used
                // to filter segments whose short name doesn't match.
                let after_ns_lower = normalized[ns_prefix_end..].to_lowercase();

                let mut seen_segments: HashSet<String> = HashSet::new();

                for fqn in &seen_fqns {
                    let fqn_lower = fqn.to_lowercase();
                    if !fqn_lower.starts_with(&ns_prefix_lower) {
                        continue;
                    }
                    // Portion of the FQN after the namespace prefix.
                    // PHP namespaces are ASCII so byte offsets match.
                    let rest = &fqn[ns_prefix_end..];
                    if let Some(next_bs) = rest.find('\\') {
                        let segment_short = &rest[..next_bs];
                        // Filter: the segment's short name must start
                        // with whatever the user typed after the last `\`.
                        if !after_ns_lower.is_empty()
                            && !segment_short.to_lowercase().starts_with(&after_ns_lower)
                        {
                            continue;
                        }
                        let segment = fqn[..ns_prefix_end + next_bs].to_string();
                        seen_segments.insert(segment);
                    }
                }

                for segment in &seen_segments {
                    let short = segment.rsplit('\\').next().unwrap_or(segment);

                    // Compute insert text the same way class_edit_texts
                    // does for FQN mode.
                    let (label, insert_ns) = if let Some(ns) = effective_namespace {
                        let ns_with_slash = format!("{}\\", ns);
                        if let Some(relative) = segment.strip_prefix(&ns_with_slash) {
                            (relative.to_string(), relative.to_string())
                        } else if has_leading_backslash {
                            (segment.clone(), format!("\\{}", segment))
                        } else {
                            (segment.clone(), segment.clone())
                        }
                    } else if has_leading_backslash {
                        (segment.clone(), format!("\\{}", segment))
                    } else {
                        (segment.clone(), segment.clone())
                    };

                    let filter = if has_leading_backslash {
                        format!("\\{}", segment)
                    } else {
                        segment.clone()
                    };

                    items.push(CompletionItem {
                        label,
                        kind: Some(CompletionItemKind::MODULE),
                        detail: Some(format!("namespace {}", segment)),
                        insert_text: Some(insert_ns.clone()),
                        filter_text: Some(filter),
                        sort_text: Some(format!("0!_{}", short.to_lowercase())),
                        text_edit: fqn_replace_range.map(|range| {
                            CompletionTextEdit::Edit(TextEdit {
                                range,
                                new_text: insert_ns,
                            })
                        }),
                        ..CompletionItem::default()
                    });
                }
            }
        }

        // Always sort by sort_text so the editor receives items in our
        // intended order.  Editors apply their own fuzzy scoring on top,
        // but many use sort_text as the primary or tie-breaking key when
        // items share the same fuzzy-match quality.  Sending items
        // unsorted lets the editor's internal ordering dominate, which
        // defeats our match-quality / tier / affinity scheme.
        items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));

        // Cap the result set so the client isn't overwhelmed.
        let is_incomplete = items.len() > Self::MAX_CLASS_COMPLETIONS;
        if is_incomplete {
            items.truncate(Self::MAX_CLASS_COMPLETIONS);
        }

        (items, is_incomplete)
    }

    /// Check whether a FQN is likely a namespace (not a class).
    ///
    /// Returns `true` only when we can *confirm* the FQN is a
    /// namespace — i.e. it is NOT a known class, AND we have positive
    /// evidence that it is a namespace (it appears as a namespace in
    /// `namespace_map`, or known classes exist under it as a prefix).
    ///
    /// When we have no information either way, returns `false` (benefit
    /// of the doubt — treat it as a potential class so undiscovered
    /// imports still appear in completions).
    fn is_likely_namespace_not_class(&self, fqn: &str) -> bool {
        // If the FQN is a known class, it's definitely not just a
        // namespace — even if classes also exist under it.
        if self.find_class_in_uri_classes_index(fqn).is_some() {
            return false;
        }
        if self.symbols.fqn_uri_index.read().contains_key(fqn) {
            return false;
        }

        if self.stub_index.read().contains_key(fqn) {
            return false;
        }

        // Not a known class. Check for positive namespace evidence.

        // 1. Some open file declares this FQN as its namespace.
        {
            let files = self.files.read();
            for state in files.values() {
                for span in &state.namespaces {
                    if span.namespace.as_deref() == Some(fqn) {
                        return true;
                    }
                }
            }
        }

        // 2. Known classes exist under this FQN as a namespace prefix.
        let prefix = format!("{}\\", fqn);
        if self
            .symbols
            .fqn_uri_index
            .read()
            .keys()
            .any(|k| k.starts_with(&prefix))
        {
            return true;
        }

        if self
            .stub_index
            .read()
            .keys()
            .any(|k| k.starts_with(&prefix))
        {
            return true;
        }

        // No evidence either way — benefit of the doubt.
        false
    }

    /// Check whether a class matches the given `ClassNameContext`, or
    /// allow it through if not loaded.
    ///
    /// Returns `Some(true)` when the class is loaded and passes
    /// `context.matches()`, `Some(false)` when loaded but rejected,
    /// and `None` when the class could not be loaded at all.
    ///
    /// Callers should hard-exclude on `Some(false)`, pass through on
    /// `None` (with optional heuristic demotion via
    /// [`ClassNameContext::likely_mismatch`]), and include on
    /// `Some(true)` without demotion.
    fn check_context_match(&self, class_name: &str, context: ClassNameContext) -> Option<bool> {
        // load_stub_class checks uri_classes_index first, then parses in-memory
        // stubs if needed.  No disk I/O.
        if let Some(cls) = self.load_stub_class(class_name) {
            let kind_ok = context.matches(&cls);
            if !kind_ok {
                return Some(false);
            }
            // ThrowNew and Catch require Throwable ancestry.
            if matches!(
                context,
                ClassNameContext::ThrowNew | ClassNameContext::Catch
            ) {
                return Some(self.is_throwable_descendant(class_name, 0));
            }
            return Some(true);
        }
        // Truly unknown.
        None
    }
}

#[cfg(test)]
#[path = "class_completion_tests.rs"]
mod tests;
