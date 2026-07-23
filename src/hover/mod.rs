//! Hover support (`textDocument/hover`).
//!
//! This module resolves the symbol under the cursor and returns a
//! human-readable description including type information, method
//! signatures, and docblock descriptions.
//!
//! The implementation reuses the same symbol-map lookup that powers
//! go-to-definition, and the same type-resolution pipeline that
//! powers completion.
//!
//! [`Backend::handle_hover`] dispatches by symbol kind into the sibling
//! submodules: [`member`] (methods / properties / constants),
//! [`variable`], [`class`], [`see_refs`], [`templates`], and
//! [`constants`].  The [`formatting`] submodule holds the shared
//! Markdown builders.

mod class;
mod constants;
mod formatting;
mod member;
mod see_refs;
mod templates;
mod variable;

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::resolver::ResolutionCtx;
use crate::php_type::PhpType;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind, SymbolSpan, VarDefKind};
use crate::types::*;
use crate::util::find_class_at_offset;

use formatting::*;
use member::HoverMemberHit;

// Re-export `pub(crate)` items so external callers keep using `crate::hover::`.
pub(crate) use formatting::{
    extract_description_from_info, extract_docblock_description, extract_var_description_from_info,
    hover_for_function, shorten_php_type,
};
pub(crate) use member::{MemberKindForOrigin, find_declaring_class};

impl Backend {
    /// Return a Markdown provenance line for a class FQN, or `None` for
    /// project-local classes.
    pub(crate) fn provenance_line_for_class(&self, fqn: &str) -> Option<String> {
        let class_uri = self.fqn_uri_index.read().get(fqn).cloned()?;
        let (origin, pkg_name) = self.package_info_for_uri(&class_uri);
        format_provenance_line(origin, pkg_name.as_deref())
    }

    /// Return a Markdown provenance line for a function by name.
    ///
    /// By the time hover asks for provenance the function has already
    /// been resolved, so `global_functions` holds its defining URI
    /// (including `phpantom-stub-fn://` for built-ins). Falls back to
    /// the autoload index for functions discovered by the byte-level
    /// scan but not yet parsed.
    pub(crate) fn provenance_line_for_function(&self, func_name: &str) -> Option<String> {
        let uri = self
            .global_functions
            .read()
            .get(func_name)
            .map(|(uri, _)| uri.clone());
        if let Some(uri) = uri {
            let (origin, pkg_name) = self.package_info_for_uri(&uri);
            return format_provenance_line(origin, pkg_name.as_deref());
        }
        let path = self
            .autoload_function_index
            .read()
            .get(func_name)
            .cloned()?;
        let (origin, pkg_name) = self.package_info_for_path(&path);
        format_provenance_line(origin, pkg_name.as_deref())
    }

    /// Handle a `textDocument/hover` request.
    ///
    /// Returns `Some(Hover)` when the symbol under the cursor can be
    /// resolved to a meaningful description, or `None` when resolution
    /// fails or the cursor is not on a navigable symbol.
    pub fn handle_hover(&self, uri: &str, content: &str, position: Position) -> Option<Hover> {
        let _body_infer_guard = self.activate_body_return_inferrer();
        let _auth_user_guard = self.activate_auth_user_resolver();
        let offset = crate::util::position_to_offset(content, position);

        // Try the exact cursor offset first.
        if let Some(symbol) = self.lookup_symbol_map(uri, offset)
            && let Some(Some(mut hover)) =
                crate::util::catch_panic_unwind_safe("hover", uri, Some(position), || {
                    self.hover_from_symbol(&symbol, uri, content, offset)
                })
        {
            hover.range = Some(symbol_span_to_range(content, &symbol));
            return Some(hover);
        }

        // Retry one byte earlier for end-of-token edge cases.
        if offset > 0
            && let Some(symbol) = self.lookup_symbol_map(uri, offset - 1)
            && let Some(Some(mut hover)) =
                crate::util::catch_panic_unwind_safe("hover", uri, Some(position), || {
                    self.hover_from_symbol(&symbol, uri, content, offset - 1)
                })
        {
            hover.range = Some(symbol_span_to_range(content, &symbol));
            return Some(hover);
        }

        // ── model-property<Model> string hover ─────────────────
        // When the cursor is inside a string argument whose parameter
        // is typed as model-property<Model>, show the property's type
        // and source info (same as hovering over $model->property).
        if let Some(hover) = self.try_model_property_hover(uri, content, position) {
            return Some(hover);
        }

        None
    }

    /// Dispatch a symbol-map hit to the appropriate hover path.
    fn hover_from_symbol(
        &self,
        symbol: &SymbolSpan,
        uri: &str,
        content: &str,
        cursor_offset: u32,
    ) -> Option<Hover> {
        let kind = &symbol.kind;
        let ctx = self.file_context_at(uri, cursor_offset);
        let current_class = find_class_at_offset(&ctx.classes, cursor_offset);
        let class_loader = self.class_loader(&ctx);
        let function_loader = self.function_loader(&ctx);

        match kind {
            SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
                // Suppress hover when the cursor is on a variable at its
                // definition site where the type is already visible in
                // the signature (properties, static/global declarations).
                // For parameters, assignments, foreach bindings, and catch
                // bindings, hover is useful to show the resolved type and
                // any docblock descriptions.
                if let Some(def_kind) = self.lookup_var_def_kind_at(uri, name, cursor_offset)
                    && !matches!(
                        def_kind,
                        VarDefKind::Assignment
                            | VarDefKind::CompoundAssignment
                            | VarDefKind::Parameter
                            | VarDefKind::Foreach
                            | VarDefKind::Catch
                            | VarDefKind::ArrayDestructuring
                            | VarDefKind::ListDestructuring
                    )
                {
                    return None;
                }
                self.hover_variable(name, uri, content, cursor_offset, current_class, &ctx)
            }

            SymbolKind::MemberAccess {
                subject_text,
                member_name,
                is_static,
                is_method_call,
                ..
            } => {
                let rctx = ResolutionCtx {
                    current_class,
                    all_classes: &ctx.classes,
                    content,
                    cursor_offset,
                    class_loader: &class_loader,
                    laravel_macro_this_resolver: None,
                    resolved_class_cache: Some(&self.resolved_class_cache),
                    function_loader: Some(&function_loader),
                    scope_var_resolver: None,
                    is_in_static_method: false,
                    preserve_static: false,
                };

                let access_kind = if *is_static {
                    AccessKind::DoubleColon
                } else {
                    AccessKind::Arrow
                };

                let candidates = ResolvedType::into_arced_classes(
                    crate::completion::resolver::resolve_target_classes(
                        subject_text,
                        access_kind,
                        &rctx,
                    ),
                );

                // Collect hover results from all union candidates,
                // deduplicating by declaring class so that a member
                // inherited from the same interface/parent is shown
                // only once.
                let mut hover_markdowns: Vec<String> = Vec::new();
                let mut seen_declaring_classes: Vec<String> = Vec::new();

                for target_class in &candidates {
                    // Always use a fully-resolved class so that inherited
                    // docblock types (return types, parameter types,
                    // descriptions) are visible on hover.  The candidate
                    // from `resolve_target_classes` may carry model-specific
                    // scope methods that are not in the FQN-keyed cache, so
                    // fall back to the candidate when the member is not
                    // found on the fully-resolved version.
                    let merged = crate::virtual_members::resolve_class_fully_cached(
                        target_class,
                        &class_loader,
                        &self.resolved_class_cache,
                    );
                    let find_result =
                        Self::find_member_for_hover(&merged, member_name, *is_method_call);

                    let (mut member_result, mut owner) = if find_result.is_some() {
                        (find_result, merged.clone())
                    } else {
                        // Fall back to the candidate directly — it may
                        // contain model-specific members (e.g. Eloquent
                        // scope methods injected onto Builder<Model>)
                        // that the FQN-keyed cache does not have.
                        let result =
                            Self::find_member_for_hover(target_class, member_name, *is_method_call);
                        (result, target_class.clone())
                    };

                    // ── Enrich with call-site generic substitution ──
                    // The merged (cached) class has raw template param
                    // names (e.g. TModel) because the cache is FQN-keyed
                    // and shared across call sites.  The candidate from
                    // resolve_target_classes carries concrete substitutions
                    // (e.g. TModel→BlogAuthor).  When the merged member's
                    // return type still references a template param, swap
                    // it with the candidate's substituted return type and
                    // use the candidate as the owner.
                    if !merged.template_params.is_empty() {
                        let tpl_strings: Vec<String> = merged
                            .template_params
                            .iter()
                            .map(|a| a.to_string())
                            .collect();
                        match &member_result {
                            Some(HoverMemberHit::Method(method)) => {
                                if let Some(ref ret) = method.return_type
                                    && ret.references_any_template_param(&tpl_strings)
                                    && let Some(HoverMemberHit::Method(subst_method)) =
                                        Self::find_member_for_hover(
                                            target_class,
                                            member_name,
                                            *is_method_call,
                                        )
                                {
                                    member_result = Some(HoverMemberHit::Method(subst_method));
                                    owner = target_class.clone();
                                }
                            }
                            Some(HoverMemberHit::Property(prop)) => {
                                if let Some(ref hint) = prop.type_hint
                                    && hint.references_any_template_param(&tpl_strings)
                                    && let Some(HoverMemberHit::Property(subst_prop)) =
                                        Self::find_member_for_hover(
                                            target_class,
                                            member_name,
                                            *is_method_call,
                                        )
                                {
                                    member_result = Some(HoverMemberHit::Property(subst_prop));
                                    owner = target_class.clone();
                                }
                            }
                            _ => {}
                        }
                    }

                    let hover = match member_result {
                        Some(HoverMemberHit::Method(ref method)) => {
                            let mut method = method.clone();
                            if let Some((_date_class, date_return_type)) =
                                Self::configured_laravel_date_return(
                                    &owner,
                                    member_name,
                                    &class_loader,
                                )
                            {
                                method.return_type = Some(date_return_type);
                                method.is_inferred_return = true;
                            }
                            let declaring = find_declaring_class(
                                &owner,
                                member_name,
                                &MemberKindForOrigin::Method,
                                &class_loader,
                            );
                            Some((
                                declaring.name.to_string(),
                                self.hover_for_method(
                                    &method,
                                    &declaring,
                                    &class_loader,
                                    uri,
                                    content,
                                ),
                            ))
                        }
                        Some(HoverMemberHit::Property(ref prop)) => {
                            let declaring = find_declaring_class(
                                &owner,
                                &prop.name,
                                &MemberKindForOrigin::Property,
                                &class_loader,
                            );
                            Some((
                                declaring.name.to_string(),
                                self.hover_for_property(prop, &declaring, &class_loader),
                            ))
                        }
                        Some(HoverMemberHit::Constant(ref constant)) => {
                            let declaring = find_declaring_class(
                                &owner,
                                &constant.name,
                                &MemberKindForOrigin::Constant,
                                &class_loader,
                            );
                            Some((
                                declaring.name.to_string(),
                                self.hover_for_constant(constant, &declaring, &class_loader),
                            ))
                        }
                        None => None,
                    };

                    if let Some((declaring_name, h)) = hover {
                        // Deduplicate: if we already have a hover from this
                        // declaring class, skip it (e.g. both Lamp and Faucet
                        // implement Switchable::turnOff — show once).
                        if seen_declaring_classes.contains(&declaring_name) {
                            continue;
                        }
                        seen_declaring_classes.push(declaring_name);
                        if let HoverContents::Markup(mc) = h.contents {
                            hover_markdowns.push(mc.value);
                        }
                    }
                }

                if hover_markdowns.is_empty() {
                    None
                } else if hover_markdowns.len() == 1 {
                    Some(make_hover(hover_markdowns.into_iter().next().unwrap()))
                } else {
                    Some(make_hover(hover_markdowns.join("\n\n---\n\n")))
                }
            }

            SymbolKind::ClassReference { name, .. } => {
                // Check whether this class reference is in a `new ClassName` context.
                // If so, show the __construct method hover instead of the class hover.
                let before = content.get(..symbol.start as usize)?;
                let trimmed = before.trim_end();
                let is_new_context = trimmed.ends_with("new")
                    && trimmed
                        .as_bytes()
                        .get(trimmed.len().wrapping_sub(4))
                        .is_none_or(|&b| !b.is_ascii_alphanumeric() && b != b'_');

                if is_new_context && let Some(cls) = class_loader(name) {
                    let merged = crate::virtual_members::resolve_class_fully_cached(
                        &cls,
                        &class_loader,
                        &self.resolved_class_cache,
                    );
                    if let Some(constructor) = merged.get_method_ci("__construct") {
                        return Some(self.hover_for_method(
                            constructor,
                            &merged,
                            &class_loader,
                            uri,
                            content,
                        ));
                    }
                }

                self.hover_class_reference(name, uri, content, &class_loader, cursor_offset)
            }

            SymbolKind::ClassDeclaration { .. }
            | SymbolKind::MemberDeclaration { .. }
            | SymbolKind::NamespaceDeclaration { .. } => {
                // The user is already at the definition site — showing
                // hover here would just repeat what they can already see.
                None
            }

            SymbolKind::FunctionCall { name, .. } => {
                self.hover_function_call(name, uri, content, &ctx, &function_loader)
            }

            SymbolKind::SelfStaticParent(ssp_kind) => {
                let is_this = *ssp_kind == SelfStaticParentKind::This;

                let resolved = match ssp_kind {
                    SelfStaticParentKind::Self_ | SelfStaticParentKind::Static => {
                        current_class.cloned()
                    }
                    SelfStaticParentKind::This => self
                        .resolve_closure_this_override(uri, content, cursor_offset)
                        .or_else(|| current_class.cloned()),
                    SelfStaticParentKind::Parent => current_class
                        .and_then(|cc| cc.parent_class.as_ref())
                        .and_then(|parent_name| {
                            class_loader(parent_name).map(Arc::unwrap_or_clone)
                        }),
                };
                if let Some(cls) = resolved {
                    let mut lines = Vec::new();

                    if let Some(desc) = extract_docblock_description(cls.class_docblock.as_deref())
                    {
                        lines.push(desc);
                    }

                    if let Some(ref msg) = cls.deprecation_message {
                        lines.push(format_deprecation_line(msg));
                    }

                    let ns_line = namespace_line(cls.file_namespace.as_deref());
                    if is_this {
                        lines.push(format!(
                            "```php\n<?php\n{}$this = {}\n```",
                            ns_line, cls.name
                        ));
                    } else {
                        let keyword_str = match ssp_kind {
                            SelfStaticParentKind::Self_ => "self",
                            SelfStaticParentKind::Static => "static",
                            SelfStaticParentKind::Parent => "parent",
                            SelfStaticParentKind::This => unreachable!(),
                        };
                        lines.push(format!(
                            "```php\n<?php\n{}{} = {}\n```",
                            ns_line, keyword_str, cls.name
                        ));
                    }

                    Some(make_hover(lines.join("\n\n")))
                } else {
                    None
                }
            }

            SymbolKind::ConstantReference { name } => {
                let lookup = self.lookup_global_constant(name);

                // `lookup` is `Some(Some(val))` when the constant
                // exists with a known value, `Some(None)` when it
                // exists but the value is unknown, and `None` when
                // the constant was not found at all.
                match lookup {
                    Some(Some(val)) => Some(make_hover(format!(
                        "```php\n<?php\nconst {} = {};\n```",
                        name, val
                    ))),
                    Some(None) => Some(make_hover(format!("```php\n<?php\nconst {};\n```", name))),
                    None => None,
                }
            }

            SymbolKind::LaravelStringKey { kind, key } => self.hover_laravel_string_key(kind, key),

            SymbolKind::CommandOwnParam { name, is_option } => {
                hover_command_own_param(content, cursor_offset as usize, name, *is_option)
            }

            SymbolKind::LaravelMacroString { .. }
            | SymbolKind::Keyword
            | SymbolKind::CastType
            | SymbolKind::Comment => None,
        }
    }

    /// Build hover content for a Laravel string key (route name, config
    /// key, view name, or translation key).
    fn hover_laravel_string_key(
        &self,
        kind: &crate::symbol_map::LaravelStringKind,
        key: &str,
    ) -> Option<Hover> {
        use crate::symbol_map::LaravelStringKind;

        let (label, detail) = match kind {
            LaravelStringKind::Route => {
                // Try to resolve the route to show where it's defined.
                let locations =
                    crate::virtual_members::laravel::resolve_laravel_string_key(self, kind, key);
                let detail = if let Some(loc) = locations.first() {
                    let path = loc.uri.path();
                    let short_path = path
                        .rsplit("/routes/")
                        .next()
                        .map(|p| format!("routes/{}", p))
                        .unwrap_or_else(|| path.to_string());
                    format!("Defined in `{}`", short_path)
                } else {
                    "Route name".to_string()
                };
                ("Route", detail)
            }
            LaravelStringKind::Config => {
                // Try to resolve the config key to show its value.
                let locations =
                    crate::virtual_members::laravel::resolve_laravel_string_key(self, kind, key);
                let detail = if let Some(loc) = locations.first() {
                    let path = loc.uri.path();
                    let short_path = path
                        .rsplit("/config/")
                        .next()
                        .map(|p| format!("config/{}", p))
                        .unwrap_or_else(|| path.to_string());
                    format!("Defined in `{}`", short_path)
                } else {
                    "Config key".to_string()
                };
                ("Config", detail)
            }
            LaravelStringKind::View => {
                // Show the resolved file path.
                let locations =
                    crate::virtual_members::laravel::resolve_laravel_string_key(self, kind, key);
                let detail = if let Some(loc) = locations.first() {
                    let path = loc.uri.path();
                    // Show the path relative to the workspace root so
                    // custom view directories (from `config/view.php`)
                    // display cleanly rather than as absolute paths.
                    let short_path = self
                        .workspace_root
                        .read()
                        .as_deref()
                        .and_then(|root| path.strip_prefix(&format!("{}/", root.to_string_lossy())))
                        .map(|rel| rel.to_string())
                        .unwrap_or_else(|| path.to_string());
                    format!("`{}`", short_path)
                } else {
                    "View template".to_string()
                };
                ("View", detail)
            }
            LaravelStringKind::Trans => {
                let locations =
                    crate::virtual_members::laravel::resolve_laravel_string_key(self, kind, key);
                let detail = if let Some(loc) = locations.first() {
                    let path = loc.uri.path();
                    let short_path = path
                        .rsplit("/lang/")
                        .next()
                        .map(|p| format!("lang/{}", p))
                        .unwrap_or_else(|| path.to_string());
                    format!("Defined in `{}`", short_path)
                } else {
                    "Translation key".to_string()
                };
                ("Trans", detail)
            }
            LaravelStringKind::Command => {
                let index = self.laravel_commands.read();
                let detail = if let Some(entry) = index.get(key) {
                    let mut parts = Vec::new();
                    if let Some(fqn) = &entry.fqn {
                        parts.push(format!("Defined by `{}`", fqn));
                    }
                    let sig = &entry.signature;
                    if !sig.arguments.is_empty() {
                        let args: Vec<String> =
                            sig.arguments.iter().map(|a| a.name.clone()).collect();
                        parts.push(format!("Arguments: `{}`", args.join("`, `")));
                    }
                    if !sig.options.is_empty() {
                        let opts: Vec<String> = sig
                            .options
                            .iter()
                            .map(|o| format!("--{}", o.name))
                            .collect();
                        parts.push(format!("Options: `{}`", opts.join("`, `")));
                    }
                    if parts.is_empty() {
                        "Artisan command".to_string()
                    } else {
                        parts.join("\n\n")
                    }
                } else {
                    "Artisan command".to_string()
                };
                ("Command", detail)
            }
        };

        Some(make_hover(format!("**{}** `{}`\n\n{}", label, key, detail)))
    }

    /// Produce hover information for a function call.
    fn hover_function_call(
        &self,
        name: &str,
        uri: &str,
        content: &str,
        _ctx: &FileContext,
        function_loader: &dyn Fn(&str) -> Option<FunctionInfo>,
    ) -> Option<Hover> {
        if let Some(mut func) = function_loader(name) {
            let is_configured_date_helper = matches!(
                name.trim_start_matches('\\').rsplit('\\').next(),
                Some("now" | "today")
            );
            // Only when the configured date class actually resolves do we
            // override the declared return type; the `(inferred)` annotation
            // must reflect that override, not merely the helper's name (a
            // user-defined `now()` in a non-Laravel project keeps its own
            // return type unannotated).
            let mut inferred_date_return = false;
            if is_configured_date_helper
                && let Some(date_class) = self
                    .find_or_load_class(crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)
            {
                let date_type = crate::php_type::PhpType::Named(date_class.fqn().to_string());
                func.return_type = Some(date_type);
                inferred_date_return = true;
            }
            let resolved_see = self.resolve_see_refs(&func.see_refs, uri, content);
            let provenance = self.provenance_line_for_function(name);
            Some(hover_for_function(
                &func,
                Some(&resolved_see),
                provenance,
                inferred_date_return,
            ))
        } else {
            None
        }
    }

    /// Try hover for a string literal inside a `model-property<Model>`
    /// parameter context.  Shows the same property info as hovering
    /// over `$model->property`.
    fn try_model_property_hover(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Hover> {
        use crate::completion::eloquent_string::detect_string_call_context;

        let sc = detect_string_call_context(content, position)?;

        // For hover we need the FULL string content, not just the
        // partial typed before the cursor.  Scan forward from the
        // content start to find the closing quote.
        let full_string = {
            let bytes = content.as_bytes();
            let start = sc.string_content_start;
            let quote = sc.quote_char as u8;
            let mut end = start;
            while end < bytes.len() && bytes[end] != quote {
                end += 1;
            }
            &content[start..end]
        };
        if full_string.is_empty() {
            return None;
        }

        let ctx = self.file_context(uri);
        let call_expr = match &sc.subject {
            Some(subj) if sc.is_static => format!("{}::{}", subj, sc.method_name),
            Some(subj) => format!("{}->{}", subj, sc.method_name),
            None => sc.method_name.clone(),
        };

        let resolved = self.resolve_callable_target(&call_expr, content, position, &ctx)?;
        let param = resolved.parameters.get(sc.arg_index)?;
        let param_type = param.type_hint.as_ref()?;

        let model_name = extract_model_name_from_model_property_type(param_type)?;

        let class_loader = self.class_loader(&ctx);
        let model_class = class_loader(&model_name)?;
        let resolved_model = crate::virtual_members::resolve_class_fully_cached(
            &model_class,
            &class_loader,
            &self.resolved_class_cache,
        );

        let property = resolved_model
            .properties
            .iter()
            .find(|p| &*p.name == full_string)?;

        Some(self.hover_for_property(property, &resolved_model, &class_loader))
    }
}

/// Hover for `$this->argument('user')` / `$this->option('queue')`: resolve
/// the parameter against the enclosing command's parsed `$signature` and show
/// its description and shape.
fn hover_command_own_param(
    content: &str,
    offset: usize,
    name: &str,
    is_option: bool,
) -> Option<Hover> {
    let signature = crate::virtual_members::laravel::command_signature_at_offset(content, offset)?;
    let param = if is_option {
        signature.option(name)
    } else {
        signature.argument(name)
    }?;

    let kind = if is_option { "Option" } else { "Argument" };
    let display = if is_option {
        format!("--{}", param.name)
    } else {
        param.name.clone()
    };

    let mut lines = vec![format!("**{} `{}`**", kind, display)];
    if let Some(desc) = &param.description {
        lines.push(desc.clone());
    }
    let mut traits: Vec<String> = Vec::new();
    if param.is_array {
        traits.push("array".to_string());
    }
    if !is_option && param.optional {
        traits.push("optional".to_string());
    }
    if is_option && param.takes_value {
        traits.push("takes a value".to_string());
    }
    if let Some(shortcut) = &param.shortcut {
        traits.push(format!("shortcut -{}", shortcut));
    }
    if let Some(default) = &param.default {
        traits.push(format!("default `{}`", default));
    }
    if !traits.is_empty() {
        lines.push(format!("_{}_", traits.join(", ")));
    }

    Some(make_hover(lines.join("\n\n")))
}

/// Extract a model name from a `model-property<Model>` type, including
/// when nested inside array/list generic arguments.
pub(crate) fn extract_model_name_from_model_property_type(ty: &PhpType) -> Option<String> {
    if let PhpType::Generic(name, args) = ty
        && name.eq_ignore_ascii_case("model-property")
        && args.len() == 1
    {
        return args[0].base_name().map(|s| s.to_string());
    }
    if let PhpType::Generic(name, args) = ty
        && (crate::php_type::is_array_like_name(name) || name.eq_ignore_ascii_case("list"))
    {
        for arg in args {
            if let PhpType::Generic(n, inner) = arg
                && n.eq_ignore_ascii_case("model-property")
                && inner.len() == 1
            {
                return inner[0].base_name().map(|s| s.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
