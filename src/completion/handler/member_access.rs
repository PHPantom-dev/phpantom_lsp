//! Strategy: member access completion after `->`, `?->`, or `::`, plus
//! the related "suggest an overridable parent member" strategy that
//! fires when declaring a new method/constant/property in a class body.

use std::sync::Arc;

use tower_lsp::lsp_types::{
    CompletionContext, CompletionItem, CompletionList, CompletionResponse, Position,
};

use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::completion::resolver::{ResolutionCtx, resolve_target_classes};
use crate::symbol_map::SymbolKind;
use crate::text_position::position_to_offset;
use crate::types::{ClassInfo, CompletionTarget, FileContext, ResolvedType};

impl Backend {
    // ─── Strategy: member access completion ──────────────────────────────

    /// Try to offer member completions after `->`, `?->`, or `::`.
    ///
    /// Resolves the subject to one or more `ClassInfo` values, merges
    /// inherited members, and builds completion items filtered by access
    /// kind and visibility.
    ///
    /// Returns `None` when there is no access operator before the cursor
    /// or when resolution produces no results.
    pub(super) fn try_member_access_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        completion_context: Option<&CompletionContext>,
    ) -> Option<CompletionResponse> {
        // ── Primary path: AST-based detection via symbol map ────────
        // The symbol map's `MemberAccess` correctly handles `(new Foo)->`,
        // call-result chains, array access chains, and null-safe chains.
        // Fall back to text scanning when the symbol map has no hit.
        let target = self
            .extract_completion_target_from_symbol_map(uri, content, position)
            .or_else(|| crate::completion::target::extract_completion_target(content, position))?;

        let cursor_offset = position_to_offset(content, position);
        let current_class = find_class_at_offset(&ctx.classes, cursor_offset);
        let prefix = if completion_context.is_some() {
            Self::member_completion_prefix(content, position)
        } else {
            String::new()
        };

        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let laravel_macro_this_resolver = |target: &str| {
            let facade = class_loader(target)?;
            let target_fqn = facade.fqn().to_string();
            let concrete = self
                .facade_macro_concrete(&target_fqn)
                .unwrap_or(target_fqn);
            self.find_or_load_class(&concrete)
        };

        // `static::` in a final class is equivalent to `self::` but
        // suggests the class can be subclassed — which it can't.
        // Suppress suggestions to nudge the developer toward `self::`.
        let suppress = target.subject == "static" && current_class.is_some_and(|cc| cc.is_final);

        // ── Resolve subject to concrete types ───────────────────────
        // We resolve the subject BEFORE checking the cache so that the
        // cache key can include the actual resolved types. This prevents
        // stale results if a variable (e.g. `$model`) changes type
        // within the same file.
        let rctx = ResolutionCtx {
            current_class,
            all_classes: &ctx.classes,
            content,
            cursor_offset,
            class_loader: &class_loader,
            laravel_macro_this_resolver: Some(&laravel_macro_this_resolver),
            resolved_class_cache: Some(&self.resolved_class_cache),
            function_loader: Some(&function_loader),
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: false,
        };
        let mut resolved = if suppress {
            vec![]
        } else {
            resolve_target_classes(&target.subject, target.access_kind, &rctx)
        };

        // ── Incomplete-expression retry ─────────────────────────────
        if resolved.is_empty() && !suppress && target.subject.starts_with('$') {
            let patched = Self::patch_incomplete_member_access(content, position);
            if patched != content {
                let patched_classes: Vec<Arc<crate::types::ClassInfo>> =
                    self.parse_php(&patched).into_iter().map(Arc::new).collect();
                let patched_offset = position_to_offset(&patched, position);
                let patched_current = find_class_at_offset(&patched_classes, patched_offset);
                let patched_rctx = ResolutionCtx {
                    current_class: patched_current,
                    all_classes: &patched_classes,
                    content: &patched,
                    cursor_offset: patched_offset,
                    class_loader: &class_loader,
                    laravel_macro_this_resolver: Some(&laravel_macro_this_resolver),
                    resolved_class_cache: Some(&self.resolved_class_cache),
                    function_loader: Some(&function_loader),
                    scope_var_resolver: None,
                    is_in_static_method: false,
                    preserve_static: false,
                };
                resolved =
                    resolve_target_classes(&target.subject, target.access_kind, &patched_rctx);
            }
        }

        if resolved.is_empty() {
            return None;
        }

        let resolved_types: Vec<String> = resolved
            .iter()
            .map(|rt| rt.type_string.to_string())
            .collect();
        let cache_key =
            Self::member_completion_cache_key(uri, &target, current_class, &resolved_types);

        // Wrap resolution + inheritance merging in catch_unwind so
        // that a stack overflow (e.g. from deep trait/inheritance
        // resolution when the subject is a call expression like
        // `collect($x)->`) doesn't crash the LSP server process.
        let started = std::time::Instant::now();
        let cached_items = self.member_completion_cache.lock().get(&cache_key).cloned();
        let cache_hit = cached_items.is_some();
        let member_items = cached_items.or_else(|| {
            crate::util::catch_panic_unwind_safe(
                "member-access completion",
                uri,
                Some(position),
                || {
                    let candidates = ResolvedType::into_arced_classes(resolved);
                    if candidates.is_empty() {
                        return vec![];
                    }

                    // `parent::`, `self::`, and `static::` are syntactically
                    // `::` but semantically different from external static
                    // access.
                    let effective_access =
                        if matches!(target.subject.as_str(), "parent" | "self" | "static") {
                            crate::AccessKind::ParentDoubleColon
                        } else {
                            target.access_kind
                        };

                    crate::completion::builder::build_union_completion_items(
                        &candidates,
                        effective_access,
                        current_class,
                        &class_loader,
                        &self.resolved_class_cache,
                        uri,
                    )
                },
            )
            .inspect(|items| {
                if !items.is_empty() {
                    let mut cache = self.member_completion_cache.lock();
                    // Simple size limit: evict everything if the cache gets too big.
                    // This is crude but effective at preventing memory leaks
                    // in long-running sessions.
                    if cache.len() > 200 {
                        cache.clear();
                    }
                    cache.insert(cache_key.clone(), items.clone());
                }
            })
        });

        match member_items {
            Some(all_items) if !all_items.is_empty() => {
                let is_filtered = !prefix.is_empty();
                let unfiltered_count = all_items.len();
                let items = if is_filtered {
                    Self::filter_member_completion_items(all_items, &prefix)
                } else {
                    all_items
                };

                // ── Suppress snippet parentheses when `(` already follows ──
                let items = if super::paren_follows_cursor(content, position) {
                    super::strip_snippet_parens(items)
                } else {
                    items
                };
                let returned_count = items.len();

                let elapsed = started.elapsed();
                if elapsed >= std::time::Duration::from_millis(20) || !cache_hit {
                    tracing::debug!(
                        target: "performance",
                        "PHPantom: member completion subject={} access={:?} prefix={} cache={} resolved=[{}] took {:?}, returned {}/{} items",
                        target.subject,
                        target.access_kind,
                        prefix,
                        if cache_hit { "hit" } else { "miss" },
                        resolved_types.join(","),
                        elapsed,
                        returned_count,
                        unfiltered_count
                    );
                }

                if is_filtered {
                    Some(CompletionResponse::List(CompletionList {
                        is_incomplete: false,
                        items,
                    }))
                } else {
                    Some(CompletionResponse::Array(items))
                }
            }
            _ => None,
        }
    }

    fn member_completion_cache_key(
        uri: &str,
        target: &CompletionTarget,
        current_class: Option<&ClassInfo>,
        resolved_types: &[String],
    ) -> String {
        format!(
            "{}\n{:?}\n{}\n{}\n{}",
            uri,
            target.access_kind,
            target.subject,
            current_class
                .map(|c| c.fqn().to_string())
                .unwrap_or_default(),
            resolved_types.join(",")
        )
    }

    fn member_completion_prefix(content: &str, position: Position) -> String {
        let cursor_offset = position_to_offset(content, position) as usize;
        let bytes = content.as_bytes();
        let mut start = cursor_offset.min(bytes.len());
        while start > 0 {
            let b = bytes[start - 1];
            if b.is_ascii_alphanumeric() || b == b'_' {
                start -= 1;
            } else {
                break;
            }
        }

        let has_member_operator = (start >= 2
            && ((bytes[start - 2] == b'-' && bytes[start - 1] == b'>')
                || (bytes[start - 2] == b':' && bytes[start - 1] == b':')))
            || (start >= 3
                && bytes[start - 3] == b'?'
                && bytes[start - 2] == b'-'
                && bytes[start - 1] == b'>');
        if !has_member_operator {
            return String::new();
        }

        content[start..cursor_offset.min(content.len())].to_string()
    }

    fn filter_member_completion_items(
        items: Vec<CompletionItem>,
        prefix: &str,
    ) -> Vec<CompletionItem> {
        if prefix.is_empty() {
            return items;
        }

        let prefix_lower = prefix.to_ascii_lowercase();
        let started = std::time::Instant::now();
        let filtered: Vec<CompletionItem> = items
            .into_iter()
            .filter(|item| item.label.to_ascii_lowercase().starts_with(&prefix_lower))
            .collect();
        let elapsed = started.elapsed();
        if elapsed >= std::time::Duration::from_micros(500) {
            tracing::debug!(
                target: "performance",
                "PHPantom: filter_member_completion_items (prefix: {}) took {:?}",
                prefix,
                elapsed
            );
        }
        filtered
    }

    /// Suggest parent/interface members to override when typing a member
    /// name in a class body (methods after `function`, constants after
    /// `const`, properties after `$`).
    pub(super) fn try_method_override_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<Vec<CompletionItem>> {
        use crate::completion::context::override_completion::{
            NameOverrideCompletionOpts, build_constant_override_completions,
            build_override_completions, build_property_override_completions,
            collect_overridable_constants, collect_overridable_methods,
            collect_overridable_properties, enclosing_class_at_position,
            extract_method_name_partial, indent_for_position, is_after_const_keyword,
            is_after_function_keyword, is_property_declaration_name_position, line_start_position,
        };

        let class = enclosing_class_at_position(&ctx.classes, content, position)?;
        if !matches!(
            class.kind,
            crate::types::ClassLikeKind::Class | crate::types::ClassLikeKind::Trait
        ) {
            return None;
        }
        if class.parent_class.is_none() && class.interfaces.is_empty() {
            return None;
        }

        let class_loader = self.class_loader(ctx);
        let (partial, range) = extract_method_name_partial(content, position)?;

        if is_after_function_keyword(content, position) {
            let methods = collect_overridable_methods(class, &partial, &class_loader);
            if methods.is_empty() {
                return None;
            }
            let indent = indent_for_position(content, position, class);
            let items = build_override_completions(
                &methods,
                &crate::completion::context::override_completion::OverrideCompletionOpts {
                    use_map: &ctx.use_map,
                    file_namespace: &ctx.namespace,
                    indent: &indent,
                    replace_range: range,
                    php_version: self.php_version(),
                    line_start: line_start_position(content, position),
                },
            );
            return if items.is_empty() { None } else { Some(items) };
        }

        if is_after_const_keyword(content, position) {
            let constants = collect_overridable_constants(class, &partial, &class_loader);
            if constants.is_empty() {
                return None;
            }
            let indent = indent_for_position(content, position, class);
            let items = build_constant_override_completions(
                &constants,
                &NameOverrideCompletionOpts {
                    use_map: &ctx.use_map,
                    file_namespace: &ctx.namespace,
                    indent: &indent,
                    replace_range: range,
                    php_version: self.php_version(),
                    line_start: line_start_position(content, position),
                },
            );
            return if items.is_empty() { None } else { Some(items) };
        }

        if is_property_declaration_name_position(content, position) {
            let props = collect_overridable_properties(class, &partial, &class_loader);
            if props.is_empty() {
                return None;
            }
            let indent = indent_for_position(content, position, class);
            let items = build_property_override_completions(
                &props,
                &NameOverrideCompletionOpts {
                    use_map: &ctx.use_map,
                    file_namespace: &ctx.namespace,
                    indent: &indent,
                    replace_range: range,
                    php_version: self.php_version(),
                    line_start: line_start_position(content, position),
                },
            );
            return if items.is_empty() { None } else { Some(items) };
        }

        None
    }

    /// Extract a [`CompletionTarget`] from the symbol map's precomputed
    /// `MemberAccess` data.
    ///
    /// Returns `None` when the symbol map has no `MemberAccess` at or
    /// just before the cursor (e.g. the AST is broken at the cursor
    /// position).  The caller should fall back to text-based extraction.
    fn extract_completion_target_from_symbol_map(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionTarget> {
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        let cursor_offset = position_to_offset(content, position);

        // The cursor may be at the end of a partially-typed member name
        // (e.g. `$obj->get|`), so the MemberAccess span may end before
        // the cursor.  Walk backward through identifier characters from
        // the cursor to find where the member name starts, then look up
        // the span that starts at or contains the access operator.
        let bytes = content.as_bytes();
        let mut search_offset = cursor_offset as usize;
        while search_offset > 0 && {
            let b = bytes[search_offset - 1];
            b.is_ascii_alphanumeric() || b == b'_'
        } {
            search_offset -= 1;
        }

        // Check for `->` or `?->` before the member name start
        let has_arrow = search_offset >= 2
            && bytes[search_offset - 2] == b'-'
            && bytes[search_offset - 1] == b'>';
        let has_nullsafe_arrow = search_offset >= 3
            && bytes[search_offset - 3] == b'?'
            && bytes[search_offset - 2] == b'-'
            && bytes[search_offset - 1] == b'>';
        let has_double_colon = search_offset >= 2
            && bytes[search_offset - 2] == b':'
            && bytes[search_offset - 1] == b':';

        if !has_arrow && !has_nullsafe_arrow && !has_double_colon {
            return None;
        }

        // Look up the operator position in the symbol map.  For `->` the
        // span covers the subject + operator + member, so the operator
        // byte is within the span.  We look up a byte inside the
        // operator to find the MemberAccess span.
        let operator_offset = if has_nullsafe_arrow {
            (search_offset - 3) as u32
        } else {
            (search_offset - 2) as u32
        };

        if let Some(span) = map.lookup(operator_offset)
            && let SymbolKind::MemberAccess {
                subject_text,
                is_static,
                ..
            } = &span.kind
        {
            let access_kind = if *is_static {
                crate::AccessKind::DoubleColon
            } else {
                crate::AccessKind::Arrow
            };
            return Some(CompletionTarget {
                access_kind,
                subject: subject_text.clone(),
            });
        }

        None
    }
}
