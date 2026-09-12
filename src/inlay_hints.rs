//! Inlay hints (`textDocument/inlayHint`).
//!
//! Displays inline annotations in the editor for:
//! - **Parameter name hints** at call sites (e.g. `/*needle:*/ $x`).
//! - **By-reference indicators** for arguments passed by reference (`&`).
//! - **Closure parameter type hints** for untyped closure/arrow function
//!   parameters when the type can be inferred from the callable context.
//! - **Closure return type hints** for closures/arrow functions without an
//!   explicit return type when the callable context specifies one.
//!
//! The handler walks precomputed [`CallSite`] entries from the
//! [`SymbolMap`] within the requested viewport range, resolves each
//! callable to obtain parameter metadata, and emits [`InlayHint`]
//! entries for arguments that would benefit from a label.

use std::sync::atomic::Ordering;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::reference_index::ReferenceIndexKey;
use crate::symbol_map::{CallSite, SymbolKind, SymbolMap, UntypedClosureSite};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::{ClassInfo, ClassLikeKind, FileContext, MAX_INHERITANCE_DEPTH, Visibility};

impl Backend {
    /// Entry point for the `textDocument/inlayHint` request.
    ///
    /// Called by the native [`LanguageServer::inlay_hint`] trait method
    /// (available since `tower-lsp` 0.19).
    pub async fn inlay_hint_request(
        &self,
        params: InlayHintParams,
    ) -> jsonrpc::Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.to_string();
        let range = params.range;
        // Building the hints resolves every callable called in the viewport,
        // and the editor re-requests them on each scroll and each refresh a
        // keystroke triggers, so the work runs off the request task.
        let backend = self.clone_for_blocking();
        let result = crate::server::run_blocking_cancel_safe("inlay_hint", move || {
            backend.with_file_content("textDocument/inlayHint", &uri, None, |content, _| {
                backend.handle_inlay_hints(&uri, content, range)
            })
        })
        .await
        .flatten();

        // Declarations whose reference count is missing or stale were
        // queued while the hints were built; they are counted off the
        // request path and delivered by a refresh.
        self.schedule_member_ref_counts();

        Ok(result.flatten())
    }

    /// Handle a `textDocument/inlayHint` request.
    ///
    /// Returns inlay hints for call-site parameter names and by-reference
    /// indicators within the given range.
    pub fn handle_inlay_hints(
        &self,
        uri: &str,
        content: &str,
        range: Range,
    ) -> Option<Vec<InlayHint>> {
        let symbol_map = self.symbol_maps.read().get(uri).cloned()?;
        let ctx = self.file_context(uri);

        // A template's request range arrives in Blade coordinates; the
        // symbol map's offsets are in the virtual PHP.
        let virtual_range = self.translate_blade_range_to_php(uri, range);

        let range_start = position_to_offset(content, virtual_range.start);
        let range_end = position_to_offset(content, virtual_range.end);

        let mut hints = Vec::new();

        for call_site in &symbol_map.call_sites {
            // Skip call sites entirely outside the requested range.
            if call_site.args_end < range_start || call_site.args_start > range_end {
                continue;
            }

            if call_site.arg_count == 0 {
                continue;
            }

            self.emit_parameter_hints(
                call_site,
                content,
                (range_start, range_end),
                &ctx,
                &mut hints,
            );
        }

        // ── Closure / arrow function hints ──────────────────────────
        if !symbol_map.untyped_closure_sites.is_empty() {
            self.emit_closure_hints(
                content,
                &symbol_map.untyped_closure_sites,
                &symbol_map.call_sites,
                (range_start, range_end),
                &ctx,
                &mut hints,
            );
        }

        self.emit_declaration_count_hints(
            uri,
            content,
            &symbol_map,
            (range_start, range_end),
            &mut hints,
        );

        // Translate hints back to Blade if needed.  A hint anchored in the
        // injected prologue has no template text to attach to.
        if self.is_blade_file(uri) {
            hints.retain_mut(
                |hint| match self.try_translate_php_to_blade(uri, hint.position) {
                    Some(position) => {
                        hint.position = position;
                        true
                    }
                    None => false,
                },
            );
        }

        Some(hints)
    }

    fn emit_declaration_count_hints(
        &self,
        uri: &str,
        content: &str,
        symbol_map: &SymbolMap,
        range: (u32, u32),
        hints: &mut Vec<InlayHint>,
    ) {
        if !self.workspace_indexed.load(Ordering::Acquire) {
            return;
        }

        // A standalone function belongs to no class, so the class walk
        // below never reaches one.  These go first, or a file holding
        // nothing but functions would leave at the `else` below with no
        // hints at all.
        self.emit_function_count_hints(uri, content, symbol_map, range, hints);

        let Some(classes) = self.symbols.uri_classes_index.read().get(uri).cloned() else {
            return;
        };
        let ctx = self.file_context(uri);
        let class_loader = self.class_loader(&ctx);

        for class in &classes {
            let class_fqn = class.fqn();

            if class.keyword_offset != 0 && offset_in_range(class.keyword_offset, range) {
                let ref_count = self.ref_count(&ReferenceIndexKey::class(&class_fqn));
                let mut label = reference_label(ref_count);

                if class.kind == ClassLikeKind::Interface || class.is_abstract {
                    let impls = self.find_implementors(
                        &class.name,
                        &class_fqn,
                        &class_loader,
                        false,
                        false,
                        true,
                    );
                    label.push_str(" | ");
                    label.push_str(&implementation_label(impls.len()));
                }

                push_count_hint(
                    hints,
                    line_end_position(content, class.keyword_offset as usize),
                    label,
                );
            }

            for method in &class.methods {
                if method.name_offset == 0
                    || method.is_virtual
                    || method.name.starts_with("__")
                    || method.visibility == Visibility::Private
                    || !offset_in_range(method.name_offset, range)
                    || self.method_has_prototype(class, &method.name)
                {
                    continue;
                }

                let Some(ref_count) = self.member_ref_count_cached(
                    uri,
                    method.name_offset,
                    class_fqn,
                    method.name,
                    method.is_static,
                ) else {
                    continue;
                };
                push_count_hint(
                    hints,
                    line_end_position(content, method.name_offset as usize),
                    reference_label(ref_count as usize),
                );
            }

            for prop in &class.properties {
                if prop.name_offset == 0
                    || prop.is_virtual
                    || prop.visibility == Visibility::Private
                    || !offset_in_range(prop.name_offset, range)
                    || self.traits_have_property(&class.used_traits, &prop.name, 0)
                    || self.ancestor_has_property(class, &prop.name)
                {
                    continue;
                }

                let member = match prop.name.strip_prefix('$') {
                    Some(stripped) => crate::atom::atom(stripped),
                    None => prop.name,
                };
                let Some(ref_count) = self.member_ref_count_cached(
                    uri,
                    prop.name_offset,
                    class_fqn,
                    member,
                    prop.is_static,
                ) else {
                    continue;
                };
                push_count_hint(
                    hints,
                    line_end_position(content, prop.name_offset as usize),
                    reference_label(ref_count as usize),
                );
            }

            for constant in &class.constants {
                if constant.name_offset == 0
                    || constant.visibility == Visibility::Private
                    || !offset_in_range(constant.name_offset, range)
                    || self.traits_have_constant(&class.used_traits, &constant.name, 0)
                    || self.ancestor_has_constant(class, &constant.name)
                {
                    continue;
                }

                let Some(ref_count) = self.member_ref_count_cached(
                    uri,
                    constant.name_offset,
                    class_fqn,
                    constant.name,
                    true,
                ) else {
                    continue;
                };
                push_count_hint(
                    hints,
                    line_end_position(content, constant.name_offset as usize),
                    reference_label(ref_count as usize),
                );
            }
        }
    }

    /// Emit a reference count beside every standalone function the file
    /// declares.
    ///
    /// Methods are counted through `member_ref_count_cached`, which
    /// exists because a method name has to be attributed to a class and
    /// followed up an inheritance chain.  A function has neither, so its
    /// count is a single lookup in the reference index, the way a class
    /// declaration's is.
    fn emit_function_count_hints(
        &self,
        uri: &str,
        content: &str,
        symbol_map: &SymbolMap,
        range: (u32, u32),
        hints: &mut Vec<InlayHint>,
    ) {
        for span in &symbol_map.spans {
            let SymbolKind::FunctionCall {
                name,
                is_definition: true,
                ..
            } = &span.kind
            else {
                continue;
            };

            if !offset_in_range(span.start, range) {
                continue;
            }

            let key = self.function_reference_key(uri, span.start, name);
            push_count_hint(
                hints,
                line_end_position(content, span.start as usize),
                reference_label(self.ref_count(&key)),
            );
        }
    }

    fn method_has_prototype(&self, class: &ClassInfo, method_name: &str) -> bool {
        let mut current = class.clone();
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let parent_name = match current.parent_class {
                Some(name) => name,
                None => break,
            };
            let parent = match self.find_or_load_class(&parent_name) {
                Some(p) => ClassInfo::clone(&p),
                None => break,
            };
            if parent
                .methods
                .iter()
                .any(|m| m.name == method_name && !m.is_virtual)
                || self.traits_have_method(&parent.used_traits, method_name, 0)
            {
                return true;
            }
            current = parent;
        }

        self.traits_have_method(&class.used_traits, method_name, 0)
            || self.interfaces_have_method(class, method_name)
    }

    fn traits_have_method(
        &self,
        trait_names: &[crate::atom::Atom],
        method_name: &str,
        depth: usize,
    ) -> bool {
        if depth > MAX_INHERITANCE_DEPTH as usize {
            return false;
        }

        for trait_name in trait_names {
            let Some(trait_info) = self.find_or_load_class(trait_name) else {
                continue;
            };
            if trait_info
                .methods
                .iter()
                .any(|m| m.name == method_name && !m.is_virtual)
                || self.traits_have_method(&trait_info.used_traits, method_name, depth + 1)
            {
                return true;
            }
        }

        false
    }

    fn interfaces_have_method(&self, class: &ClassInfo, method_name: &str) -> bool {
        let mut current = Some(class.clone());
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let Some(cls) = current else {
                break;
            };
            for iface_name in &cls.interfaces {
                if self.interface_has_method(iface_name, method_name, 0) {
                    return true;
                }
            }
            current = cls.parent_class.as_deref().and_then(|parent| {
                self.find_or_load_class(parent)
                    .map(|p| ClassInfo::clone(&p))
            });
        }
        false
    }

    fn interface_has_method(&self, iface_name: &str, method_name: &str, depth: usize) -> bool {
        if depth > MAX_INHERITANCE_DEPTH as usize {
            return false;
        }
        let Some(iface) = self.find_or_load_class(iface_name) else {
            return false;
        };
        iface
            .methods
            .iter()
            .any(|m| m.name == method_name && !m.is_virtual)
            || iface
                .interfaces
                .iter()
                .any(|parent| self.interface_has_method(parent, method_name, depth + 1))
    }

    fn ancestor_has_property(&self, class: &ClassInfo, prop_name: &str) -> bool {
        let mut current = class.clone();
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let parent_name = match current.parent_class {
                Some(name) => name,
                None => return false,
            };
            let parent = match self.find_or_load_class(&parent_name) {
                Some(p) => ClassInfo::clone(&p),
                None => return false,
            };
            if parent.properties.iter().any(|p| p.name == prop_name)
                || self.traits_have_property(&parent.used_traits, prop_name, 0)
            {
                return true;
            }
            current = parent;
        }
        false
    }

    fn traits_have_property(
        &self,
        trait_names: &[crate::atom::Atom],
        prop_name: &str,
        depth: usize,
    ) -> bool {
        if depth > MAX_INHERITANCE_DEPTH as usize {
            return false;
        }

        for trait_name in trait_names {
            let Some(trait_info) = self.find_or_load_class(trait_name) else {
                continue;
            };
            if trait_info.properties.iter().any(|p| p.name == prop_name)
                || self.traits_have_property(&trait_info.used_traits, prop_name, depth + 1)
            {
                return true;
            }
        }

        false
    }

    fn ancestor_has_constant(&self, class: &ClassInfo, constant_name: &str) -> bool {
        let mut current = class.clone();
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let parent_name = match current.parent_class {
                Some(name) => name,
                None => return false,
            };
            let parent = match self.find_or_load_class(&parent_name) {
                Some(p) => ClassInfo::clone(&p),
                None => return false,
            };
            if parent.constants.iter().any(|c| c.name == constant_name)
                || self.traits_have_constant(&parent.used_traits, constant_name, 0)
            {
                return true;
            }
            current = parent;
        }
        false
    }

    fn traits_have_constant(
        &self,
        trait_names: &[crate::atom::Atom],
        constant_name: &str,
        depth: usize,
    ) -> bool {
        if depth > MAX_INHERITANCE_DEPTH as usize {
            return false;
        }

        for trait_name in trait_names {
            let Some(trait_info) = self.find_or_load_class(trait_name) else {
                continue;
            };
            if trait_info.constants.iter().any(|c| c.name == constant_name)
                || self.traits_have_constant(&trait_info.used_traits, constant_name, depth + 1)
            {
                return true;
            }
        }

        false
    }

    fn ref_count(&self, key: &ReferenceIndexKey) -> usize {
        self.reference_index
            .read()
            .get(key)
            .map(|entries| entries.values().map(|&count| count as usize).sum())
            .unwrap_or(0)
    }

    /// Emit parameter-name and by-reference hints for a single call site.
    ///
    /// `range` is the requested viewport as byte offsets, already
    /// translated to virtual PHP coordinates by the caller so it can be
    /// compared against the symbol map's argument offsets.
    fn emit_parameter_hints(
        &self,
        call_site: &CallSite,
        content: &str,
        range: (u32, u32),
        ctx: &FileContext,
        hints: &mut Vec<InlayHint>,
    ) {
        // The call site's start offset gives the resolver its cursor context.
        let resolved = match self.resolve_callable_target_at_offset(
            &call_site.call_expression,
            content,
            call_site.args_start,
            ctx,
        ) {
            Some(r) => r,
            None => return,
        };

        let params = &resolved.parameters;
        if params.is_empty() {
            return;
        }

        let (range_start, range_end) = range;

        // Build a set of parameter names consumed by named arguments so
        // positional arguments can be mapped to the remaining parameters.
        let named_consumed: std::collections::HashSet<&str> = call_site
            .named_arg_names
            .iter()
            .map(|n| n.as_str())
            .collect();

        // Parameters not consumed by named args, in declaration order.
        // Each positional argument is assigned to the next entry in this
        // list.  For variadic parameters the last entry is reused.
        let remaining_params: Vec<usize> = params
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let name = p.name.strip_prefix('$').unwrap_or(&p.name);
                !named_consumed.contains(name)
            })
            .map(|(i, _)| i)
            .collect();

        let mut positional_counter: usize = 0;

        for (arg_idx, &arg_offset) in call_site.arg_offsets.iter().enumerate() {
            // Skip named arguments — the parameter name is already visible.
            if call_site.named_arg_indices.contains(&(arg_idx as u32)) {
                continue;
            }

            // Skip spread arguments — a single `...$args` may expand into
            // multiple parameters, so any single parameter name would be
            // misleading.  Still advance the positional counter because
            // the spread occupies at least one parameter slot.
            if call_site.spread_arg_indices.contains(&(arg_idx as u32)) {
                positional_counter += 1;
                continue;
            }

            // Determine which parameter this positional argument corresponds
            // to. Named arguments consume specific parameters out of order,
            // so positional arguments fill the remaining slots sequentially.
            let param_idx = if positional_counter < remaining_params.len() {
                remaining_params[positional_counter]
            } else if params.last().is_some_and(|p| p.is_variadic) {
                params.len() - 1
            } else {
                // More positional arguments than remaining parameters and
                // the last param is not variadic. Skip (likely a bug in
                // user code; we don't hint).
                positional_counter += 1;
                continue;
            };

            positional_counter += 1;

            // Skip rendering arguments outside the viewport range, but only
            // after the positional counter above has been advanced — the
            // counter must track every argument regardless of visibility so
            // that arguments rendered later still map to the right parameter.
            if arg_offset < range_start || arg_offset > range_end {
                continue;
            }

            let param = &params[param_idx];

            let mut label_parts: Vec<String> = Vec::new();

            if param.is_reference {
                label_parts.push("&".to_string());
            }

            let param_display_name = param.name.strip_prefix('$').unwrap_or(&param.name);

            // Skip the hint when the argument is a simple variable whose
            // name matches the parameter name (the hint would be redundant).
            // For example: `foo($needle)` when the param is `$needle`.
            if !param.is_reference && should_suppress_hint(param_display_name, content, arg_offset)
            {
                continue;
            }

            // For single-argument calls where the function name already
            // makes the parameter obvious, skip the hint.
            if !param.is_reference
                && call_site.arg_count == 1
                && is_obvious_single_param(&call_site.call_expression, param_display_name)
            {
                continue;
            }

            label_parts.push(format!("{}:", param_display_name));

            let label_text = label_parts.join("");
            if label_text.is_empty() {
                continue;
            }

            let hint_position = offset_to_position(content, arg_offset as usize);

            hints.push(InlayHint {
                position: hint_position,
                label: InlayHintLabel::String(label_text),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: param
                    .type_hint
                    .as_ref()
                    .map(|t| InlayHintTooltip::String(format!("{} {}", t, param.name))),
                padding_left: None,
                padding_right: Some(true),
                data: None,
            });
        }
    }

    /// Emit parameter-type and return-type inlay hints for closures and
    /// arrow functions whose types can be inferred from the callable context.
    fn emit_closure_hints(
        &self,
        content: &str,
        sites: &[UntypedClosureSite],
        call_sites: &[CallSite],
        range: (u32, u32),
        ctx: &FileContext,
        hints: &mut Vec<InlayHint>,
    ) {
        let (range_start, range_end) = range;
        for site in sites {
            // Quick range check: use close_paren_offset if available,
            // otherwise the first untyped param offset.
            let representative_offset = site
                .close_paren_offset
                .or_else(|| site.untyped_params.first().map(|&(_, off)| off));
            if let Some(off) = representative_offset {
                if off < range_start || off > range_end {
                    continue;
                }
            } else {
                continue;
            }

            // Find the matching CallSite so we can extract the full
            // argument text for template substitution.  We match by
            // call expression string and verify that any of the
            // closure site's offsets fall within the call site's
            // argument range.  We check ALL untyped-param offsets
            // and the close-paren offset since the representative
            // offset alone may not be inside the parent call's range
            // for all AST shapes.
            let call_args_text: Option<&str> = {
                let closure_offsets: Vec<u32> = site
                    .untyped_params
                    .iter()
                    .map(|&(_, off)| off)
                    .chain(site.close_paren_offset)
                    .collect();
                call_sites
                    .iter()
                    .find(|cs| {
                        cs.call_expression == site.parent_call_expression
                            && closure_offsets
                                .iter()
                                .any(|&off| off >= cs.args_start && off <= cs.args_end)
                    })
                    .and_then(|cs| content.get(cs.args_start as usize..cs.args_end as usize))
            };

            // Resolve the callable to get the parameter's type signature.
            // Pass the call-site argument text so that function/method-level
            // @template parameters are inferred from the sibling arguments
            // and substituted into parameter type hints (e.g. turning
            // `callable(T): T` into `callable(int): int`).
            let resolved = match self.resolve_callable_target_with_args_at_offset(
                &site.parent_call_expression,
                content,
                representative_offset.unwrap_or(0),
                ctx,
                call_args_text,
            ) {
                Some(r) => r,
                None => continue,
            };

            let param_info = match resolved.parameters.get(site.arg_index_in_parent) {
                Some(p) => p,
                None => continue,
            };
            let callable_type = match param_info.type_hint.as_ref() {
                Some(t) => t,
                None => continue,
            };

            // ── Parameter type hints ────────────────────────────────
            if let Some(callable_params) = callable_type.callable_param_types() {
                for &(param_idx, param_offset) in &site.untyped_params {
                    if let Some(cp) = callable_params.get(param_idx) {
                        let shortened = cp.type_hint.shorten();
                        let type_str = shortened.to_string();
                        if type_str.is_empty() || shortened.is_mixed() {
                            continue;
                        }

                        let hint_position = offset_to_position(content, param_offset as usize);

                        hints.push(InlayHint {
                            position: hint_position,
                            label: InlayHintLabel::String(format!("{} ", type_str)),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: None,
                            padding_left: None,
                            padding_right: Some(false),
                            data: None,
                        });
                    }
                }
            }

            // ── Return type hint ────────────────────────────────────
            if let Some(close_paren) = site.close_paren_offset
                && let Some(ret_type) = callable_type.callable_return_type()
            {
                let shortened = ret_type.shorten();
                let type_str = shortened.to_string();
                if !type_str.is_empty() && !shortened.is_mixed() {
                    let hint_position = offset_to_position(content, close_paren as usize);

                    hints.push(InlayHint {
                        position: hint_position,
                        label: InlayHintLabel::String(format!(": {}", type_str)),
                        kind: Some(InlayHintKind::TYPE),
                        text_edits: None,
                        tooltip: None,
                        padding_left: None,
                        padding_right: None,
                        data: None,
                    });
                }
            }
        }
    }
}

fn push_count_hint(hints: &mut Vec<InlayHint>, position: Position, label: String) {
    hints.push(InlayHint {
        position,
        label: InlayHintLabel::String(format!(" {label}")),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: None,
        data: None,
    });
}

fn reference_label(count: usize) -> String {
    if count == 1 {
        "1 reference".to_string()
    } else {
        format!("{count} references")
    }
}

fn implementation_label(count: usize) -> String {
    if count == 1 {
        "1 implementation".to_string()
    } else {
        format!("{count} implementations")
    }
}

fn offset_in_range(offset: u32, range: (u32, u32)) -> bool {
    offset >= range.0 && offset <= range.1
}

fn line_end_position(content: &str, byte_offset: usize) -> Position {
    let line_end = content[byte_offset..]
        .find('\n')
        .map(|i| byte_offset + i)
        .unwrap_or(content.len());

    // Delegate to the canonical converter so the `character` column is
    // counted in UTF-16 code units (per the LSP spec), consistent with
    // every other position the server emits.
    offset_to_position(content, line_end)
}

/// Check whether the argument at `arg_offset` is a simple variable whose
/// name (without `$`) matches the parameter name, making a hint redundant.
///
/// Also suppresses hints when the argument is a property access or method
/// call whose trailing identifier matches the parameter name:
/// `foo($this->needle)` for param `$needle`.
fn should_suppress_hint(param_name: &str, content: &str, arg_offset: u32) -> bool {
    let rest = &content[arg_offset as usize..];

    // Case 1: Simple variable `$paramName`.
    if let Some(var_rest) = rest.strip_prefix('$') {
        let var_name: String = var_rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if eq_ignore_case_snake(&var_name, param_name) {
            return true;
        }
    }

    // Case 2: The argument text ends with `->paramName` or `?->paramName`.
    // Find the end of this argument (next comma or closing paren at depth 0).
    let arg_text = extract_argument_text(rest);
    if let Some(trailing) = extract_trailing_identifier(arg_text)
        && eq_ignore_case_snake(trailing, param_name)
    {
        return true;
    }

    // Case 3: Boolean/null literals matching the parameter name pattern.
    // `foo(true)` for param `$enabled`, `foo(null)` for param `$default`.
    let trimmed = arg_text.trim();
    if matches!(
        trimmed,
        "true" | "false" | "null" | "TRUE" | "FALSE" | "NULL"
    ) {
        return false;
    }

    // Case 4: String literal whose content matches param name.
    // `foo('needle')` for param `$needle`.
    if (trimmed.starts_with('\'') || trimmed.starts_with('"')) && trimmed.len() >= 2 {
        let quote = trimmed.as_bytes()[0];
        if trimmed.as_bytes().last() == Some(&quote) {
            let inner = &trimmed[1..trimmed.len() - 1];
            if eq_ignore_case_snake(inner, param_name) {
                return true;
            }
        }
    }

    false
}

/// Extract the argument text up to the next top-level comma or closing
/// paren, respecting nesting of `()`, `[]`, and `{}`.
fn extract_argument_text(s: &str) -> &str {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev_was_escape = false;

    for (i, ch) in s.char_indices() {
        if prev_was_escape {
            prev_was_escape = false;
            continue;
        }
        if ch == '\\' && (in_single_quote || in_double_quote) {
            prev_was_escape = true;
            continue;
        }
        if in_single_quote {
            if ch == '\'' {
                in_single_quote = false;
            }
            continue;
        }
        if in_double_quote {
            if ch == '"' {
                in_double_quote = false;
            }
            continue;
        }
        match ch {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '(' => depth_paren += 1,
            ')' => {
                if depth_paren == 0 {
                    return &s[..i];
                }
                depth_paren -= 1;
            }
            '[' => depth_bracket += 1,
            ']' => depth_bracket = (depth_bracket - 1).max(0),
            '{' => depth_brace += 1,
            '}' => depth_brace = (depth_brace - 1).max(0),
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return &s[..i];
            }
            _ => {}
        }
    }
    s
}

/// Extract the trailing identifier from a member-access expression.
/// For `$this->foo->bar`, returns `"bar"`.
/// For `SomeClass::method`, returns `"method"`.
fn extract_trailing_identifier(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    // Look for `->identifier` or `::identifier` at the end.
    let pos = trimmed.rfind("->").or_else(|| trimmed.rfind("::"))?;
    let after = &trimmed[pos + 2..];
    // The trailing part should be a simple identifier.
    if after.chars().all(|c| c.is_alphanumeric() || c == '_') && !after.is_empty() {
        Some(after)
    } else {
        None
    }
}

/// Compare two identifiers ignoring case and treating snake_case
/// as equivalent to camelCase.
///
/// For example, `eq_ignore_case_snake("myParam", "my_param")` returns true.
fn eq_ignore_case_snake(a: &str, b: &str) -> bool {
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    // Normalize both to lowercase without underscores and compare.
    let norm_a: String = a
        .chars()
        .filter(|c| *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect();
    let norm_b: String = b
        .chars()
        .filter(|c| *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect();
    norm_a == norm_b
}

/// Check whether a single-parameter call has an obvious relationship
/// between the function/method name and the parameter, making the hint
/// redundant noise.
///
/// For example, `strlen($text)` — the function name already implies
/// the parameter is a string.
fn is_obvious_single_param(call_expression: &str, _param_name: &str) -> bool {
    // Extract the function/method name from the call expression.
    let func_name = if let Some(pos) = call_expression.rfind("->") {
        &call_expression[pos + 2..]
    } else if let Some(pos) = call_expression.rfind("::") {
        &call_expression[pos + 2..]
    } else if let Some(name) = call_expression.strip_prefix("new ") {
        // Constructor calls: `new Foo($bar)` — always show.
        let _ = name;
        return false;
    } else {
        call_expression
    };

    // Common single-param functions where the hint is noise.
    matches!(
        func_name.to_ascii_lowercase().as_str(),
        "count"
            | "strlen"
            | "isset"
            | "empty"
            | "unset"
            | "print"
            | "echo"
            | "var_dump"
            | "print_r"
            | "var_export"
            | "intval"
            | "floatval"
            | "strval"
            | "boolval"
            | "trim"
            | "ltrim"
            | "rtrim"
            | "strtolower"
            | "strtoupper"
            | "ucfirst"
            | "lcfirst"
            | "abs"
            | "ceil"
            | "floor"
            | "round"
            | "is_null"
            | "is_array"
            | "is_string"
            | "is_int"
            | "is_integer"
            | "is_float"
            | "is_double"
            | "is_bool"
            | "is_numeric"
            | "is_object"
            | "is_callable"
            | "json_encode"
            | "json_decode"
            | "serialize"
            | "unserialize"
            | "base64_encode"
            | "base64_decode"
            | "urlencode"
            | "urldecode"
            | "rawurlencode"
            | "rawurldecode"
            | "htmlspecialchars"
            | "htmlentities"
            | "md5"
            | "sha1"
            | "crc32"
            | "chr"
            | "ord"
            | "array_values"
            | "array_keys"
            | "array_unique"
            | "array_flip"
            | "array_reverse"
            | "array_pop"
            | "array_shift"
            | "sort"
            | "rsort"
            | "asort"
            | "arsort"
            | "ksort"
            | "krsort"
            | "shuffle"
            | "reset"
            | "end"
            | "current"
            | "next"
            | "prev"
            | "type"
            | "gettype"
            | "class_exists"
            | "interface_exists"
            | "trait_exists"
            | "function_exists"
            | "defined"
            | "compact"
            | "sizeof"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open a file and return its hints once the member reference counts
    /// the first request queued have been computed.
    fn declaration_hints(backend: &Backend, uri: &str, content: &str) -> Vec<InlayHint> {
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));
        backend.update_ast(uri, content);
        backend.workspace_indexed.store(true, Ordering::Release);

        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: content.lines().count() as u32,
                character: 0,
            },
        };
        backend.handle_inlay_hints(uri, content, range);
        backend.compute_pending_member_ref_counts();
        backend
            .handle_inlay_hints(uri, content, range)
            .unwrap_or_default()
    }

    /// The label of the hint on `line`, if any.
    fn hint_on_line(hints: &[InlayHint], line: u32) -> Option<String> {
        hints
            .iter()
            .find(|hint| hint.position.line == line)
            .map(|hint| match &hint.label {
                InlayHintLabel::String(label) => label.clone(),
                InlayHintLabel::LabelParts(parts) => {
                    parts.iter().map(|part| part.value.as_str()).collect()
                }
            })
    }

    #[test]
    fn function_count_includes_unqualified_calls_from_a_namespaced_file() {
        let backend = Backend::new_test();
        backend.update_ast(
            "file:///app/Service.php",
            "<?php\nnamespace App;\nfunction run(): void {\n    helper();\n    helper();\n}\n",
        );

        let hints = declaration_hints(
            &backend,
            "file:///helpers.php",
            "<?php\nfunction helper(): void {}\n",
        );

        assert_eq!(hint_on_line(&hints, 1).as_deref(), Some(" 2 references"));
    }

    #[test]
    fn function_count_ignores_the_case_a_call_is_spelled_with() {
        let backend = Backend::new_test();
        backend.update_ast(
            "file:///app/Service.php",
            "<?php\nnamespace App;\nfunction run(): void {\n    HELPER();\n}\n",
        );

        let hints = declaration_hints(
            &backend,
            "file:///helpers.php",
            "<?php\nfunction helper(): void {}\n",
        );

        assert_eq!(hint_on_line(&hints, 1).as_deref(), Some(" 1 reference"));
    }

    #[test]
    fn declaration_count_hints_skip_magic_methods() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
class User {
    public function __construct() {}
    public function save(): void {}
}
"#;

        let hints = declaration_hints(&backend, uri, content);

        assert!(hints.iter().any(|hint| hint.position.line == 1));
        assert!(hints.iter().any(|hint| hint.position.line == 3));
        assert!(!hints.iter().any(|hint| hint.position.line == 2));
    }

    #[test]
    fn member_count_ignores_a_member_of_the_same_name_on_another_class() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
class User {
    public int $id = 0;
    public function save(): void {}
}
class Order {
    public int $id = 0;
    public function save(): void {}
}
function persist(Order $order): void {
    echo $order->id;
    $order->save();
}
"#;

        let hints = declaration_hints(&backend, uri, content);

        assert_eq!(hint_on_line(&hints, 2).as_deref(), Some(" 0 references"));
        assert_eq!(hint_on_line(&hints, 3).as_deref(), Some(" 0 references"));
        assert_eq!(hint_on_line(&hints, 6).as_deref(), Some(" 1 reference"));
        assert_eq!(hint_on_line(&hints, 7).as_deref(), Some(" 1 reference"));
    }

    #[test]
    fn member_count_includes_references_through_a_subclass() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
class Model {
    public function save(): void {}
}
class Order extends Model {
}
function persist(Order $order, Model $model): void {
    $order->save();
    $model->save();
}
"#;

        let hints = declaration_hints(&backend, uri, content);

        assert_eq!(hint_on_line(&hints, 2).as_deref(), Some(" 2 references"));
    }

    #[test]
    fn class_count_ignores_a_class_of_the_same_name_in_another_namespace() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
class Widget {}
namespace App;
class Widget {}
function build(): void {
    $first = new \App\Widget();
    $second = new \App\Widget();
}
"#;

        let hints = declaration_hints(&backend, uri, content);

        assert_eq!(hint_on_line(&hints, 1).as_deref(), Some(" 0 references"));
        assert_eq!(hint_on_line(&hints, 3).as_deref(), Some(" 2 references"));
    }

    #[test]
    fn declaration_count_hint_column_uses_utf16_units() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        // The declaration line ends with a non-BMP character (2 UTF-16
        // code units, 1 Unicode scalar), so a chars-based column would be
        // one short of the LSP-mandated UTF-16 column.
        let content = "<?php\nclass User {} // \u{1F600}\n";

        backend.update_ast(uri, content);
        backend.workspace_indexed.store(true, Ordering::Release);

        let hints = backend
            .handle_inlay_hints(
                uri,
                content,
                Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 0,
                    },
                },
            )
            .unwrap_or_default();

        let class_hint = hints
            .iter()
            .find(|hint| hint.position.line == 1)
            .expect("expected a reference-count hint on the class declaration line");
        // "class User {} // " is 17 UTF-16 units; the emoji adds 2 → 19.
        assert_eq!(class_hint.position.character, 19);
    }

    #[test]
    fn test_should_suppress_simple_variable_match() {
        let content = "$needle, $haystack";
        assert!(should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_not_suppress_different_variable() {
        let content = "$foo, $bar";
        assert!(!should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_suppress_property_access_match() {
        let content = "$this->needle, $other";
        assert!(should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_suppress_string_literal_match() {
        let content = "'needle', $other";
        assert!(should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_not_suppress_boolean_literal() {
        let content = "true, $other";
        assert!(!should_suppress_hint("enabled", content, 0));
    }

    #[test]
    fn test_extract_argument_text_basic() {
        assert_eq!(extract_argument_text("$x, $y)"), "$x");
        assert_eq!(extract_argument_text("$x)"), "$x");
        assert_eq!(extract_argument_text("foo($a, $b), $c)"), "foo($a, $b)");
    }

    #[test]
    fn test_extract_trailing_identifier() {
        assert_eq!(extract_trailing_identifier("$this->foo"), Some("foo"));
        assert_eq!(extract_trailing_identifier("$obj->bar->baz"), Some("baz"));
        assert_eq!(
            extract_trailing_identifier("SomeClass::method"),
            Some("method")
        );
        assert_eq!(extract_trailing_identifier("$simple"), None);
    }

    #[test]
    fn test_eq_ignore_case_snake() {
        assert!(eq_ignore_case_snake("myParam", "myParam"));
        assert!(eq_ignore_case_snake("myParam", "myparam"));
        assert!(eq_ignore_case_snake("my_param", "myParam"));
        assert!(eq_ignore_case_snake("myParam", "my_param"));
        assert!(!eq_ignore_case_snake("foo", "bar"));
    }

    #[test]
    fn test_is_obvious_single_param() {
        assert!(is_obvious_single_param("strlen", "string"));
        assert!(is_obvious_single_param("count", "array"));
        assert!(is_obvious_single_param("json_encode", "value"));
        assert!(!is_obvious_single_param("customFunc", "value"));
        assert!(!is_obvious_single_param("new Foo", "bar"));
    }

    #[test]
    fn a_standalone_function_gets_a_reference_count() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
function helper(): void {}

helper();
helper();
"#;

        let hints = declaration_hints(&backend, uri, content);

        assert_eq!(
            hint_on_line(&hints, 1).as_deref(),
            Some(" 2 references"),
            "a function declared outside a class is counted like a class is"
        );
    }

    #[test]
    fn a_file_of_only_functions_still_gets_hints() {
        // The declaration walk starts from the file's classes, and a
        // function belongs to none, so a file holding nothing else used
        // to leave that walk before emitting anything.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = r#"<?php
function unused(): void {}
"#;

        let hints = declaration_hints(&backend, uri, content);

        assert_eq!(
            hint_on_line(&hints, 1).as_deref(),
            Some(" 0 references"),
            "a file with no classes at all still reports its functions"
        );
    }
}
