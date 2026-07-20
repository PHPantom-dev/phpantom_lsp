//! Hover support (`textDocument/hover`).
//!
//! This module resolves the symbol under the cursor and returns a
//! human-readable description including type information, method
//! signatures, and docblock descriptions.
//!
//! The implementation reuses the same symbol-map lookup that powers
//! go-to-definition, and the same type-resolution pipeline that
//! powers completion.

mod formatting;

use std::sync::Arc;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::resolver::ResolutionCtx;
use crate::docblock::extract_template_params_full;
use crate::php_type::PhpType;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind, SymbolSpan, VarDefKind};
use crate::types::*;
use crate::util::{find_class_at_offset, short_name, strip_fqn_prefix};

use formatting::*;

// ─── Origin Indicators ─────────────────────────────────────────────────────

/// Describes the origin of a member relative to the class it appears on.
///
/// Used to render a subtle indicator line above the code block in hover
/// popups so that the user can see at a glance whether a member overrides
/// a parent, implements an interface contract, or was synthesized.
enum MemberOrigin {
    /// The member overrides a parent class method/property/constant.
    Override(String),
    /// The member implements an interface method/constant.
    Implements(String),
    /// The member is virtual (synthesized from `@method`, `@property`,
    /// `@mixin`, or a framework provider).
    Virtual,
    /// The member is a macro registration.
    Macro,
}

fn format_database_column_details(source: &DatabaseColumnSource) -> Vec<String> {
    let nullable = if source.nullable { "yes" } else { "no" };
    let mut lines = vec![
        "database:".to_string(),
        format!("  type: `{}`", source.database_type),
        format!("  nullable: `{}`", nullable),
    ];
    if let Some(default) = &source.default {
        lines.push(format!("  default: `{}`", default));
    }
    if let Some(mode) = &source.generated_mode {
        lines.push(format!("  generated: `{}`", mode));
    } else if source.generated_expression.is_some() {
        lines.push("  generated: `virtual`".to_string());
    }
    if let Some(expression) = &source.generated_expression {
        lines.push(format!("  expression: `{}`", expression));
    }
    lines
}

fn format_attribute_default_details(source: &AttributeDefaultSource) -> Vec<String> {
    vec![
        "application:".to_string(),
        format!("  default: `{}`", source.value),
    ]
}

fn format_property_source(source: &PropertySource) -> Vec<String> {
    match source {
        PropertySource::DatabaseColumn {
            column,
            attribute_default,
        } => {
            let mut lines = vec!["source: database column".to_string()];
            lines.extend(format_database_column_details(column));
            if let Some(default) = attribute_default {
                lines.extend(format_attribute_default_details(default));
            }
            lines
        }
        PropertySource::Cast {
            cast,
            column,
            attribute_default,
        } => {
            let mut lines = vec![format!("source: cast `{}`", cast)];
            if let Some(column) = column {
                lines.extend(format_database_column_details(column));
            }
            if let Some(default) = attribute_default {
                lines.extend(format_attribute_default_details(default));
            }
            lines
        }
        PropertySource::Accessor { method, column } => {
            let mut lines = vec![format!("source: accessor `{}`", method)];
            if let Some(column) = column {
                lines.extend(format_database_column_details(column));
            }
            lines
        }
        PropertySource::AttributeDefault { default, column } => {
            let mut lines = vec!["source: attribute default".to_string()];
            if let Some(column) = column {
                lines.extend(format_database_column_details(column));
            }
            lines.extend(format_attribute_default_details(default));
            lines
        }
        PropertySource::ComputedProperty { method } => {
            vec![format!("source: computed property `{}`", method)]
        }
        PropertySource::Relationship { method, kind } => {
            vec![format!("source: relationship `{}` ({})", method, kind)]
        }
        PropertySource::RelationshipCount { relationship } => {
            vec![format!("source: relationship count `{}`", relationship)]
        }
    }
}

/// Check whether the **raw** (unmerged) class declares a member with the
/// given name and kind.
///
/// The `owner` passed to hover methods is fully resolved (inheritance +
/// virtual providers merged in).  To distinguish "this class overrides
/// the parent's method" from "this class merely inherits it", we load
/// the raw class from the class_loader and check its own member lists.
fn raw_class_has_member(
    owner: &ClassInfo,
    member_name: &str,
    member_kind: &MemberKindForOrigin,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    // Build the FQN the same way the class loader expects.
    let fqn = owner.fqn();

    // Load the raw class.  If the loader returns None (e.g. the class
    // is only known through the current file's AST and not yet indexed),
    // fall back to assuming the member is declared — this avoids hiding
    // indicators when the project is only partially indexed.
    let raw = match class_loader(&fqn) {
        Some(c) => c,
        None => return true,
    };

    match member_kind {
        MemberKindForOrigin::Method => raw
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(member_name)),
        MemberKindForOrigin::Property => raw.properties.iter().any(|p| p.name == member_name),
        MemberKindForOrigin::Constant => raw.constants.iter().any(|c| c.name == member_name),
    }
}

/// Build the origin indicator lines for a member.
///
/// Checks whether the member is actually declared on the owner class
/// (not just inherited), then inspects the parent class and implemented
/// interfaces (via `class_loader`) to determine whether the member
/// overrides a parent or implements an interface contract.  Also checks
/// `is_virtual` for synthesized members.
///
/// Returns a (possibly empty) string of Markdown lines to prepend to the
/// hover content.
fn build_origin_lines(
    member_name: &str,
    owner: &ClassInfo,
    is_virtual: bool,
    is_macro: bool,
    member_kind: MemberKindForOrigin,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let mut origins: Vec<MemberOrigin> = Vec::new();

    if is_macro {
        origins.push(MemberOrigin::Macro);
    } else if is_virtual {
        origins.push(MemberOrigin::Virtual);
    }

    // Only check for override / implements when the member is actually
    // declared on the owner class itself (not merely inherited from a
    // parent).  Without this gate, an inherited method would incorrectly
    // show "overrides ParentClass".
    let declared_on_owner = raw_class_has_member(owner, member_name, &member_kind, class_loader);

    if declared_on_owner {
        // Check parent class for override.
        if let Some(ref parent_name) = owner.parent_class
            && let Some(parent) = class_loader(parent_name)
        {
            let has_member = match member_kind {
                MemberKindForOrigin::Method => parent
                    .methods
                    .iter()
                    .any(|m| m.name.eq_ignore_ascii_case(member_name)),
                MemberKindForOrigin::Property => {
                    parent.properties.iter().any(|p| p.name == member_name)
                }
                MemberKindForOrigin::Constant => {
                    parent.constants.iter().any(|c| c.name == member_name)
                }
            };
            if has_member {
                origins.push(MemberOrigin::Override(short_name(parent_name).to_string()));
            }
        }

        // Check interfaces for implements.
        for iface_name in &owner.interfaces {
            if let Some(iface) = class_loader(iface_name) {
                let has_member = match member_kind {
                    MemberKindForOrigin::Method => iface
                        .methods
                        .iter()
                        .any(|m| m.name.eq_ignore_ascii_case(member_name)),
                    MemberKindForOrigin::Property => {
                        iface.properties.iter().any(|p| p.name == member_name)
                    }
                    MemberKindForOrigin::Constant => {
                        iface.constants.iter().any(|c| c.name == member_name)
                    }
                };
                if has_member {
                    origins.push(MemberOrigin::Implements(short_name(iface_name).to_string()));
                }
            }
        }
    }

    if origins.is_empty() {
        return String::new();
    }

    let parts: Vec<String> = origins
        .iter()
        .map(|o| match o {
            MemberOrigin::Override(name) => format!("↑ overrides **{}**", name),
            MemberOrigin::Implements(name) => format!("◆ implements **{}**", name),
            MemberOrigin::Virtual => "👻 virtual".to_string(),
            MemberOrigin::Macro => "🔌 macro".to_string(),
        })
        .collect();

    // Join with " · " when multiple apply (e.g. override + implements).
    format!("{}\n\n", parts.join(" · "))
}

/// The kind of member being checked for origin indicators.
///
/// This is separate from `MemberKind` in the definition module because
/// origin checking only needs the three broad categories.
pub(crate) enum MemberKindForOrigin {
    Method,
    Property,
    Constant,
}

/// Find the class that originally declares a member.
///
/// When a member is inherited (not declared on `owner` itself), this
/// walks up the parent chain and checks traits and mixins to find the
/// class that actually declares the member.  Returns a fully-resolved
/// `ClassInfo` for the declaring class, or falls back to `owner` when
/// the declaring class cannot be determined.
///
/// This is used by hover and completion-resolve so that the code block
/// shows `class Model { public static function find(...) }` rather than
/// `class User { ... }` when `find()` is inherited from `Model`.
pub(crate) fn find_declaring_class(
    owner: &ClassInfo,
    member_name: &str,
    member_kind: &MemberKindForOrigin,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Arc<ClassInfo> {
    // If the member is declared directly on the owner, no need to search.
    if raw_class_has_member(owner, member_name, member_kind, class_loader) {
        return Arc::new(owner.clone());
    }

    // Check traits used by the owner.
    for trait_name in &owner.used_traits {
        if let Some(trait_class) = class_loader(trait_name) {
            let has = match member_kind {
                MemberKindForOrigin::Method => trait_class
                    .methods
                    .iter()
                    .any(|m| m.name.eq_ignore_ascii_case(member_name)),
                MemberKindForOrigin::Property => {
                    trait_class.properties.iter().any(|p| p.name == member_name)
                }
                MemberKindForOrigin::Constant => {
                    trait_class.constants.iter().any(|c| c.name == member_name)
                }
            };
            if has {
                return trait_class;
            }
        }
    }

    // Walk the parent chain.
    let mut ancestor_name = owner.parent_class;
    let mut depth = 0u32;
    while let Some(ref name) = ancestor_name {
        depth += 1;
        if depth > 20 {
            break;
        }
        if let Some(ancestor) = class_loader(name) {
            // Check traits on the ancestor first.
            for trait_name in &ancestor.used_traits {
                if let Some(trait_class) = class_loader(trait_name) {
                    let has = match member_kind {
                        MemberKindForOrigin::Method => trait_class
                            .methods
                            .iter()
                            .any(|m| m.name.eq_ignore_ascii_case(member_name)),
                        MemberKindForOrigin::Property => {
                            trait_class.properties.iter().any(|p| p.name == member_name)
                        }
                        MemberKindForOrigin::Constant => {
                            trait_class.constants.iter().any(|c| c.name == member_name)
                        }
                    };
                    if has {
                        return trait_class;
                    }
                }
            }

            // Check the ancestor class itself.
            let has = match member_kind {
                MemberKindForOrigin::Method => ancestor
                    .methods
                    .iter()
                    .any(|m| m.name.eq_ignore_ascii_case(member_name)),
                MemberKindForOrigin::Property => {
                    ancestor.properties.iter().any(|p| p.name == member_name)
                }
                MemberKindForOrigin::Constant => {
                    ancestor.constants.iter().any(|c| c.name == member_name)
                }
            };
            if has {
                return ancestor;
            }
            ancestor_name = ancestor.parent_class;
        } else {
            break;
        }
    }

    // Check @mixin classes.
    for mixin_name in &owner.mixins {
        if let Some(mixin_class) = class_loader(mixin_name) {
            let has = match member_kind {
                MemberKindForOrigin::Method => mixin_class
                    .methods
                    .iter()
                    .any(|m| m.name.eq_ignore_ascii_case(member_name)),
                MemberKindForOrigin::Property => {
                    mixin_class.properties.iter().any(|p| p.name == member_name)
                }
                MemberKindForOrigin::Constant => {
                    mixin_class.constants.iter().any(|c| c.name == member_name)
                }
            };
            if has {
                return mixin_class;
            }
        }
    }

    // Fallback: couldn't find the declaring class, use the owner.
    Arc::new(owner.clone())
}

// Re-export `pub(crate)` items so external callers keep using `crate::hover::`.
pub(crate) use formatting::{
    extract_description_from_info, extract_docblock_description, extract_var_description_from_info,
    hover_for_function, shorten_php_type,
};

/// Result of searching for a member on a [`ClassInfo`] for hover purposes.
///
/// Returned by [`Backend::find_member_for_hover`] so the caller can
/// dispatch to the correct `hover_for_*` method without repeating the
/// lookup logic.
enum HoverMemberHit {
    Method(Box<MethodInfo>),
    Property(PropertyInfo),
    Constant(ConstantInfo),
}

impl Backend {
    /// Resolve `@see` references to file locations where possible.
    ///
    /// For each raw `@see` string, attempts to resolve symbol references
    /// (class names, `Class::member()`, `Class::$prop`) to a `file://`
    /// URI with a line fragment so that the hover popup renders them as
    /// clickable links.  URLs and unresolvable symbols get `None`.
    pub(crate) fn resolve_see_refs(
        &self,
        see_refs: &[String],
        uri: &str,
        content: &str,
    ) -> Vec<ResolvedSeeRef> {
        see_refs
            .iter()
            .map(|raw| {
                // Extract the first token (the symbol or URL).
                let target = raw
                    .split_once(|c: char| c.is_whitespace())
                    .map(|(t, _)| t.trim())
                    .unwrap_or(raw.as_str());

                // URLs don't need resolution.
                if target.starts_with("http://") || target.starts_with("https://") {
                    return ResolvedSeeRef {
                        raw: raw.clone(),
                        location_uri: None,
                    };
                }

                // Try to resolve as a class or class::member reference.
                let location_uri = self.resolve_see_target(target, uri, content);

                ResolvedSeeRef {
                    raw: raw.clone(),
                    location_uri,
                }
            })
            .collect()
    }

    /// Resolve a single `@see` target to a `file://` URI with line fragment.
    ///
    /// Handles:
    /// - `ClassName` → class keyword offset
    /// - `ClassName::method()` → method name offset
    /// - `ClassName::$property` → property name offset
    /// - `ClassName::CONSTANT` → constant name offset
    fn resolve_see_target(&self, target: &str, uri: &str, content: &str) -> Option<String> {
        // Check for Class::member syntax.
        if let Some(sep) = target.find("::") {
            let class_name = &target[..sep];
            let mut member_part = target[sep + 2..].to_string();
            // Strip trailing "()" from method references.
            if member_part.ends_with("()") {
                member_part.truncate(member_part.len() - 2);
            }
            // Strip leading "$" from property references.
            let member_name = member_part.strip_prefix('$').unwrap_or(&member_part);

            let cls = self.find_or_load_class(class_name)?;
            let (class_uri, class_content) =
                self.find_class_file_content(&cls.name, uri, content)?;

            // Find the member's name_offset.
            let offset = cls
                .get_method_ci(member_name)
                .map(|m| m.name_offset)
                .or_else(|| {
                    cls.properties
                        .iter()
                        .find(|p| p.name == member_name)
                        .map(|p| p.name_offset)
                })
                .or_else(|| {
                    cls.constants
                        .iter()
                        .find(|c| c.name == member_name)
                        .map(|c| c.name_offset)
                })
                .filter(|&off| off > 0)?;

            let pos = crate::util::offset_to_position(&class_content, offset as usize);
            let parsed_uri = Url::parse(&class_uri).ok()?;
            Some(format!("{}#L{}", parsed_uri, pos.line + 1))
        } else {
            // Plain class name.
            let cls = self.find_or_load_class(target)?;
            let (class_uri, class_content) =
                self.find_class_file_content(&cls.name, uri, content)?;

            if cls.keyword_offset == 0 {
                return None;
            }
            let pos = crate::util::offset_to_position(&class_content, cls.keyword_offset as usize);
            let parsed_uri = Url::parse(&class_uri).ok()?;
            Some(format!("{}#L{}", parsed_uri, pos.line + 1))
        }
    }

    /// Search `class` for a member matching `member_name`.
    ///
    /// When `is_method_call` is true, only methods are considered.
    /// Otherwise properties and constants are tried first, with a
    /// final fallback to methods (handles method references without
    /// call parentheses).
    fn find_member_for_hover(
        class: &ClassInfo,
        member_name: &str,
        is_method_call: bool,
    ) -> Option<HoverMemberHit> {
        if is_method_call {
            class
                .get_method_ci(member_name)
                .map(|m| HoverMemberHit::Method(Box::new(m.clone())))
        } else {
            if let Some(prop) = class.properties.iter().find(|p| p.name == member_name) {
                return Some(HoverMemberHit::Property(prop.clone()));
            }
            if let Some(constant) = class.constants.iter().find(|c| c.name == member_name) {
                return Some(HoverMemberHit::Constant(constant.clone()));
            }
            class
                .get_method_ci(member_name)
                .map(|m| HoverMemberHit::Method(Box::new(m.clone())))
        }
    }

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
                    let short_path = path
                        .rsplit("/resources/views/")
                        .next()
                        .map(|p| format!("resources/views/{}", p))
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
        };

        Some(make_hover(format!("**{}** `{}`\n\n{}", label, key, detail)))
    }

    /// Look up a global constant by name, returning its value if found.
    ///
    /// Searches in order:
    /// 1. `global_defines` — constants already parsed from user files.
    /// 2. `autoload_constant_index` — lazily parses the defining file.
    /// 3. `autoload_file_paths` — last-resort lazy parse of known
    ///    autoload files for constants the byte-level scanner missed.
    /// 4. `stub_constant_index` — built-in PHP constants from stubs.
    ///    Lazily parses the stub file via `update_ast` (which populates
    ///    `global_defines`), then re-checks.
    ///
    /// Returns `Some(Some(val))` when the constant exists with a known
    /// value, `Some(None)` when it exists but the value is unknown, and
    /// `None` when the constant was not found at all.
    pub(crate) fn lookup_global_constant(&self, name: &str) -> Option<Option<String>> {
        // Phase 1: already-parsed constants.
        let lookup = self
            .global_defines
            .read()
            .get(name)
            .map(|info| info.value.clone());
        if lookup.is_some() {
            return lookup;
        }

        // Phase 2: autoload constant index — lazily parse the file.
        let path = self.autoload_constant_index.read().get(name).cloned();
        if let Some(path) = path
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            let file_uri = crate::util::path_to_uri(&path);
            self.update_ast(&file_uri, &content);
            let lookup = self
                .global_defines
                .read()
                .get(name)
                .map(|info| info.value.clone());
            if lookup.is_some() {
                return lookup;
            }
        }

        // Phase 3: lazily parse known autoload files for constants
        // the byte-level scanner missed (e.g. inside
        // `if (!defined(...))` guards).
        {
            let paths = self.autoload_file_paths.read().clone();
            for path in &paths {
                let uri = crate::util::path_to_uri(path);
                if self.parsed_uris.read().contains(&uri) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(path) {
                    self.update_ast(&uri, &content);
                    let lookup = self
                        .global_defines
                        .read()
                        .get(name)
                        .map(|info| info.value.clone());
                    if lookup.is_some() {
                        return lookup;
                    }
                }
            }
        }

        // Phase 4: built-in PHP constants from embedded stubs.
        // Parse the stub via update_ast (which populates global_defines),
        // then re-check.  This is the same lazy-parse pattern as Phases
        // 2 and 3 — no special raw-source scanning needed.
        let stub_const_idx = self.stub_constant_index.read();
        if let Some(&stub_source) = stub_const_idx.get(name) {
            let stub_uri = format!("phpantom-stub://const/{}", name);
            self.update_ast(&stub_uri, stub_source);
            let lookup = self
                .global_defines
                .read()
                .get(name)
                .map(|info| info.value.clone());
            if lookup.is_some() {
                return lookup;
            }
            // Stub was parsed but constant not found in global_defines —
            // it exists in the index, so report it with unknown value.
            return Some(None);
        }

        None
    }

    /// Produce hover information for a variable.
    fn hover_variable(
        &self,
        name: &str,
        uri: &str,
        content: &str,
        cursor_offset: u32,
        current_class: Option<&ClassInfo>,
        ctx: &FileContext,
    ) -> Option<Hover> {
        let var_name = format!("${}", name);

        // When the cursor is on the `$` of an assignment like
        // `$x = new Foo()`, the cursor offset equals the assignment
        // statement's start offset.  The variable resolution pipeline
        // skips statements whose start is at or after the cursor
        // (`stmt.start >= cursor_offset`), so the assignment is
        // excluded.  Nudge the offset by one byte so the statement's
        // start is strictly less than the cursor, allowing the
        // assignment to be included.  We only do this for assignments
        // (not parameters, foreach, etc.) where `effective_from`
        // differs from the definition offset.
        let mut cursor_offset = cursor_offset;
        if self
            .lookup_var_def_effective_from(uri, name, cursor_offset)
            .is_some()
        {
            let offset = cursor_offset as usize;
            if let Some(ch) = content.get(offset..).and_then(|s| s.chars().next()) {
                cursor_offset += ch.len_utf8() as u32;
            }
        }

        // $this resolves to the enclosing class, but not inside static methods.
        if name == "this" {
            let in_static = self
                .symbol_maps
                .read()
                .get(uri)
                .is_some_and(|map| map.is_in_static_method(cursor_offset));
            if !in_static && let Some(cc) = current_class {
                let ns_line = namespace_line(cc.file_namespace.as_deref());
                return Some(make_hover(format!(
                    "```php\n<?php\n{}$this = {}\n```",
                    ns_line, cc.name
                )));
            }
            return Some(make_hover("```php\n<?php\n$this\n```".to_string()));
        }

        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let constant_loader = self.constant_loader();
        let loaders = crate::completion::resolver::Loaders {
            function_loader: Some(&function_loader as &dyn Fn(&str) -> Option<FunctionInfo>),
            constant_loader: Some(&constant_loader),
        };

        // Try the type-string path first.  This preserves generic
        // parameters (e.g. `Generator<int, Pencil>`) and scalar types
        // (e.g. `int`) that the ClassInfo-based path would lose.
        if let Some(resolved_type) =
            crate::completion::variable::resolution::resolve_variable_php_type(
                &var_name,
                content,
                cursor_offset,
                current_class,
                &ctx.classes,
                &class_loader,
                loaders,
            )
        {
            // When the type is a template parameter, show its variance
            // and bound (e.g. "**template-covariant** `TNode` of `AstNode`")
            // above the code block so the user sees the constraint.
            let template_line =
                self.find_template_info_for_type(&resolved_type, uri, cursor_offset);

            let hover_body = build_variable_hover_body(
                &var_name,
                &resolved_type,
                &class_loader,
                template_line.as_deref(),
            );
            return Some(make_hover(hover_body));
        }

        // `resolve_variable_php_type` already calls `resolve_variable_types`
        // internally, so if it returned `None` the variable is truly
        // unresolved.  Show a plain variable hover.
        Some(make_hover(format!("```php\n<?php\n{}\n```", var_name)))
    }

    /// Produce hover information for a class reference.
    fn hover_class_reference(
        &self,
        name: &str,
        uri: &str,
        content: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cursor_offset: u32,
    ) -> Option<Hover> {
        let class_info = class_loader(name);

        if let Some(cls) = class_info {
            Some(self.hover_for_class_info(&cls, uri, content))
        } else {
            // Check whether this is a template parameter in scope.
            if let Some(tpl) = self.find_template_def_for_hover(uri, name, cursor_offset) {
                return Some(tpl);
            }
            None
        }
    }

    /// Build a template-info line for a type string that might be a
    /// template parameter.  Returns `None` when the type is not a
    /// template param in scope.
    ///
    /// For example, `"TNode"` at a cursor inside a class with
    /// `@template-covariant TNode of AstNode` returns
    /// `Some("**template-covariant** \`TNode\` of \`AstNode\`")`.
    fn find_template_info_for_type(
        &self,
        ty: &PhpType,
        uri: &str,
        cursor_offset: u32,
    ) -> Option<String> {
        // Only bare named types can be template params.
        let name = match ty {
            PhpType::Named(n) if is_bare_identifier(n) => n.as_str(),
            _ => return None,
        };

        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        let def = map.find_template_def(name, cursor_offset)?;

        let bound_display = if let Some(ref bound) = def.bound {
            format!(" of `{}`", bound.shorten())
        } else {
            String::new()
        };

        Some(format!(
            "**{}** `{}`{}",
            def.variance.tag_name(),
            def.name,
            bound_display
        ))
    }

    /// Check whether `name` is a `@template` parameter in scope at
    /// `cursor_offset` and, if so, produce a hover showing the template
    /// name and its upper bound.
    fn find_template_def_for_hover(
        &self,
        uri: &str,
        name: &str,
        cursor_offset: u32,
    ) -> Option<Hover> {
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        let def = map.find_template_def(name, cursor_offset)?;

        let bound_display = if let Some(ref bound) = def.bound {
            format!(" of `{}`", bound)
        } else {
            String::new()
        };

        Some(make_hover(format!(
            "**{}** `{}`{}",
            def.variance.tag_name(),
            def.name,
            bound_display
        )))
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

    /// Build hover content for a method.
    pub(crate) fn hover_for_method(
        &self,
        method: &MethodInfo,
        owner: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        uri: &str,
        content: &str,
    ) -> Hover {
        // When the method has no declared return type and no @return
        // docblock, try to infer the return type from the method body.
        // This mirrors what completion/hover resolution does, so the
        // hover display matches the resolved type the user sees.
        let inferred_return_type: Option<crate::php_type::PhpType> =
            if method.return_type.is_none() && method.name_offset != 0 && !method.is_virtual {
                crate::completion::call_resolution::try_infer_body_return_type(&owner.fqn(), method)
                    .filter(|t| !t.is_mixed() && !t.is_void())
            } else {
                None
            };

        // Use the inferred type as the effective return type when
        // the method has no declared one.
        let effective_return = method
            .return_type
            .as_ref()
            .or(inferred_return_type.as_ref());

        let visibility = format_visibility(method.visibility);
        let static_kw = if method.is_static { "static " } else { "" };
        let native_params = format_native_params(&method.parameters);

        // Use native return type in the code block, effective type as docblock annotation.
        let native_ret = method
            .native_return_type
            .as_ref()
            .map(|r| format!(": {}", r))
            .unwrap_or_default();

        let member_line = format!(
            "{}{}function {}({}){};",
            visibility, static_kw, method.name, native_params, native_ret
        );

        let mut lines = Vec::new();

        // When the return type or a parameter type is a template
        // parameter on the method or owning class, show the template's
        // variance and bound so the user understands the constraint.
        // Method-level templates take priority over class-level ones.
        let mut seen_templates: Vec<PhpType> = Vec::new();
        if let Some(ret) = effective_return
            && let Some(tpl_line) = find_template_info_in_method_or_class(ret, method, owner)
        {
            seen_templates.push(ret.clone());
            lines.push(tpl_line);
        }
        for param in &method.parameters {
            if let Some(ref hint) = param.type_hint
                && !seen_templates.iter().any(|s| s == hint)
                && let Some(tpl_line) = find_template_info_in_method_or_class(hint, method, owner)
            {
                seen_templates.push(hint.clone());
                lines.push(tpl_line);
            }
        }

        // Origin indicator (override / implements / virtual).
        let origin = build_origin_lines(
            &method.name,
            owner,
            method.is_virtual,
            method.is_macro,
            MemberKindForOrigin::Method,
            class_loader,
        );
        if !origin.is_empty() {
            // `build_origin_lines` already includes a trailing "\n\n".
            lines.push(origin.trim_end().to_string());
        }

        if let Some(ref desc) = method.description {
            lines.push(desc.clone());
        }

        if let Some(ref msg) = method.deprecation_message {
            lines.push(format_deprecation_line(msg));
        }

        for url in &method.links {
            lines.push(format!("[{}]({})", url, url));
        }

        let resolved_see = self.resolve_see_refs(&method.see_refs, uri, content);
        format_see_refs(&resolved_see, &method.links, &mut lines);

        // Build the readable param/return section as markdown.
        let show_inferred = method.is_inferred_return || inferred_return_type.is_some();
        if let Some(section) = build_param_return_section(
            &method.parameters,
            effective_return,
            method.native_return_type.as_ref(),
            method.return_description.as_deref(),
            show_inferred,
        ) {
            lines.push(section);
        }

        let code = build_class_member_block(
            &owner.name,
            owner.file_namespace.as_deref(),
            owner_kind_keyword(owner),
            &owner_name_suffix(owner),
            &member_line,
        );
        lines.push(code);

        if let Some(prov) = self.provenance_line_for_class(&owner.fqn()) {
            lines.push(prov);
        }

        make_hover(lines.join("\n\n"))
    }

    /// Build hover content for a property.
    pub(crate) fn hover_for_property(
        &self,
        property: &PropertyInfo,
        owner: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Hover {
        let visibility = format_visibility(property.visibility);
        let static_kw = if property.is_static { "static " } else { "" };

        // Use native type hint in the code block, effective type as docblock annotation.
        let native_type = property
            .native_type_hint
            .as_ref()
            .map(|t| format!("{} ", t))
            .unwrap_or_default();

        let member_line = format!(
            "{}{}{}${};",
            visibility, static_kw, native_type, property.name
        );

        let var_section = build_var_section(
            property.type_hint.as_ref(),
            property.native_type_hint.as_ref(),
        );

        let mut lines = Vec::new();

        // When the property type is a template parameter on the owning
        // class, show the template's variance and bound so the user
        // understands the constraint (e.g. "**template-covariant**
        // `TNode` of `AstNode`").
        if let Some(ref type_hint) = property.type_hint
            && let Some(tpl_line) = find_template_info_in_class(type_hint, owner)
        {
            lines.push(tpl_line);
        }

        // Origin indicator (override / implements / virtual).
        let origin = build_origin_lines(
            &property.name,
            owner,
            property.is_virtual,
            false,
            MemberKindForOrigin::Property,
            class_loader,
        );
        if !origin.is_empty() {
            lines.push(origin.trim_end().to_string());
        }

        if let Some(ref desc) = property.description {
            lines.push(desc.clone());
        }

        if let Some(section) = var_section {
            lines.push(section);
        }

        if let Some(ref source) = property.source {
            lines.push(format_property_source(source).join("\n"));
        }

        if let Some(ref msg) = property.deprecation_message {
            lines.push(format_deprecation_line(msg));
        }

        let code = build_class_member_block(
            &owner.name,
            owner.file_namespace.as_deref(),
            owner_kind_keyword(owner),
            &owner_name_suffix(owner),
            &member_line,
        );
        lines.push(code);

        if let Some(prov) = self.provenance_line_for_class(&owner.fqn()) {
            lines.push(prov);
        }

        make_hover(lines.join("\n\n"))
    }

    /// Build hover content for a class constant.
    pub(crate) fn hover_for_constant(
        &self,
        constant: &ConstantInfo,
        owner: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Hover {
        let member_line = if constant.is_enum_case {
            if let Some(ref val) = constant.enum_value {
                format!("case {} = {};", constant.name, val)
            } else {
                format!("case {};", constant.name)
            }
        } else {
            let visibility = format_visibility(constant.visibility);
            let type_hint = constant
                .type_hint
                .as_ref()
                .map(|t| format!(": {}", t))
                .unwrap_or_default();
            let value_suffix = constant
                .value
                .as_ref()
                .map(|v| format!(" = {}", v))
                .unwrap_or_default();
            format!(
                "{}const {}{}{};",
                visibility, constant.name, type_hint, value_suffix
            )
        };

        let mut lines = Vec::new();

        // Origin indicator (implements / virtual).
        let origin = build_origin_lines(
            &constant.name,
            owner,
            constant.is_virtual,
            false,
            MemberKindForOrigin::Constant,
            class_loader,
        );
        if !origin.is_empty() {
            lines.push(origin.trim_end().to_string());
        }

        if let Some(ref desc) = constant.description {
            lines.push(desc.clone());
        }

        if let Some(ref msg) = constant.deprecation_message {
            lines.push(format_deprecation_line(msg));
        }

        // Constants don't have a native vs effective type split, so no doc annotation.
        let code = build_class_member_block(
            &owner.name,
            owner.file_namespace.as_deref(),
            owner_kind_keyword(owner),
            &owner_name_suffix(owner),
            &member_line,
        );
        lines.push(code);

        if let Some(prov) = self.provenance_line_for_class(&owner.fqn()) {
            lines.push(prov);
        }

        make_hover(lines.join("\n\n"))
    }

    /// Build hover content for a class/interface/trait/enum.
    pub(crate) fn hover_for_class_info(&self, cls: &ClassInfo, uri: &str, content: &str) -> Hover {
        let kind_str = match cls.kind {
            ClassLikeKind::Class => {
                if cls.is_abstract {
                    "abstract class"
                } else if cls.is_final {
                    "final class"
                } else {
                    "class"
                }
            }
            ClassLikeKind::Interface => "interface",
            ClassLikeKind::Trait => "trait",
            ClassLikeKind::Enum => "enum",
        };

        let mut extends_implements = String::new();

        // For interfaces, `parent_class` is the first element of
        // `interfaces` (both come from the same `extends` clause),
        // so skip it to avoid duplicating the name.
        if cls.kind != ClassLikeKind::Interface
            && let Some(ref parent) = cls.parent_class
        {
            extends_implements.push_str(&format!(" extends {}", short_name(parent)));
        }

        if !cls.interfaces.is_empty() {
            let keyword = if cls.kind == ClassLikeKind::Interface {
                "extends"
            } else {
                "implements"
            };
            let short_ifaces: Vec<&str> = cls.interfaces.iter().map(|i| short_name(i)).collect();
            extends_implements.push_str(&format!(" {} {}", keyword, short_ifaces.join(", ")));
        }

        let signature = format!("{} {}{}", kind_str, cls.name, extends_implements);
        let ns_line = namespace_line(cls.file_namespace.as_deref());

        let mut lines = Vec::new();

        if let Some(desc) = extract_docblock_description(cls.class_docblock.as_deref()) {
            lines.push(desc);
        }

        if let Some(ref msg) = cls.deprecation_message {
            lines.push(format_deprecation_line(msg));
        }

        for url in &cls.links {
            lines.push(format!("[{}]({})", url, url));
        }

        let resolved_see = self.resolve_see_refs(&cls.see_refs, uri, content);
        format_see_refs(&resolved_see, &cls.links, &mut lines);

        // Show template parameters with variance and bounds.
        if let Some(ref docblock) = cls.class_docblock {
            let tpl_entries: Vec<String> = extract_template_params_full(docblock)
                .into_iter()
                .map(|(name, bound, variance, default)| {
                    let bound_display = bound
                        .map(|b| format!(" of `{}`", b.shorten()))
                        .unwrap_or_default();
                    let default_display =
                        default.map(|d| format!(" = `{}`", d)).unwrap_or_default();
                    format!(
                        "**{}** `{}`{}{}",
                        variance.tag_name(),
                        name,
                        bound_display,
                        default_display
                    )
                })
                .collect();
            if !tpl_entries.is_empty() {
                lines.push(tpl_entries.join("  \n"));
            }
        }

        // For enums, show cases inside the code block.
        // For traits, show public method signatures inside the code block.
        let body_lines = if cls.kind == ClassLikeKind::Enum {
            build_enum_case_body(cls)
        } else if cls.kind == ClassLikeKind::Trait {
            build_trait_summary_body(cls)
        } else {
            String::new()
        };

        if body_lines.is_empty() {
            lines.push(format!("```php\n<?php\n{}{}\n```", ns_line, signature));
        } else {
            lines.push(format!(
                "```php\n<?php\n{}{} {{\n{}}}\n```",
                ns_line, signature, body_lines
            ));
        }

        if let Some(prov) = self.provenance_line_for_class(&cls.fqn()) {
            lines.push(prov);
        }

        make_hover(lines.join("\n\n"))
    }
}

/// Resolve the namespace for a type string by loading the base type
/// through the class loader, falling back to parsing FQN strings.
///
/// Extracts the first class-like name from the type string (before any
/// `<` generic params), resolves it via the class loader, and returns
/// the resolved class's `file_namespace`.  When the class loader cannot
/// find the type (e.g. a cross-file FQN like `\App\Models\User` that
/// is not loaded), falls back to extracting the namespace directly from
/// the FQN string.
/// Build the hover body for a variable, rendering union types as
/// separate code blocks separated by a horizontal rule (`---`).
///
/// For a single type (or scalar/generic) this produces one code block
/// showing e.g. `$user = User`.
///
/// For a union like `Lamp|Faucet` it produces two code blocks
/// (`$ambiguous = Lamp` and `$ambiguous = Faucet`) joined by a
/// markdown horizontal rule so the editor renders a visible divider.
fn build_variable_hover_body(
    var_name: &str,
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    template_line: Option<&str>,
) -> String {
    let members = ty.union_members();

    // Count how many members are non-trivial class types (not scalars,
    // not `null`, not `void`, etc.).  Only render separate blocks when
    // there are 2+ class-like types; a simple `Foo|null` should stay
    // in one block.
    let class_like_count = members.iter().filter(|m| !m.is_scalar()).count();

    // When there is only one component, or only one class-like type
    // (the rest being scalars / null), render a single code block.
    if members.len() <= 1 || class_like_count < 2 {
        let short_type = ty.shorten().to_string();
        let ns = resolve_type_namespace_structured(ty, class_loader);
        let ns_line = namespace_line(ns.as_deref());
        let code_block = format!(
            "```php\n<?php\n{}{} = {}\n```",
            ns_line, var_name, short_type
        );
        return if let Some(tpl) = template_line {
            format!("{}\n\n{}", tpl, code_block)
        } else {
            code_block
        };
    }

    // Multiple union branches — render each as its own code block
    // separated by a markdown horizontal rule.
    let mut blocks: Vec<String> = Vec::with_capacity(members.len());
    for member in &members {
        let short = member.shorten().to_string();
        let ns = resolve_type_namespace_structured(member, class_loader);
        let ns_line = namespace_line(ns.as_deref());
        blocks.push(format!(
            "```php\n<?php\n{}{} = {}\n```",
            ns_line, var_name, short
        ));
    }

    let body = blocks.join("\n\n---\n\n");
    if let Some(tpl) = template_line {
        format!("{}\n\n{}", tpl, body)
    } else {
        body
    }
}

/// Extract the namespace for a structured `PhpType` by looking up its
/// base class name via the class loader, or by parsing the namespace
/// from the FQN string itself.
fn resolve_type_namespace_structured(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let base = ty.base_name()?;

    if let Some(cls) = class_loader(base) {
        return cls
            .file_namespace
            .as_ref()
            .filter(|ns| !ns.is_empty() && !ns.starts_with("___"))
            .map(|ns| ns.to_string());
    }

    // Fallback: parse the namespace from the FQN string itself.
    // E.g. `App\Models\User` → `App\Models`.
    // Strip leading `\` — input may be a raw docblock type like
    // `\App\Models\User` that hasn't been through resolve_type_string.
    let canonical = strip_fqn_prefix(base);
    if let Some(pos) = canonical.rfind('\\') {
        let ns = &canonical[..pos];
        if !ns.is_empty() {
            return Some(ns.to_string());
        }
    }

    None
}

/// Check whether `type_str` is a `@template` parameter declared on
/// the method's own docblock or the owning class's docblock.  Method-level
/// templates take priority.  Returns a formatted info line like
/// `"**template** \`T\` of \`Model\`"`, or `None` when the type is
/// not a template param in either scope.
fn find_template_info_in_method_or_class(
    ty: &PhpType,
    method: &MethodInfo,
    owner: &ClassInfo,
) -> Option<String> {
    if let Some(line) = find_template_info_in_method(ty, method) {
        return Some(line);
    }
    find_template_info_in_class(ty, owner)
}

/// Check whether `type_str` is a `@template` parameter declared on
/// the method's own docblock.  Returns a formatted info line like
/// `"**template** \`T\` of \`Model\`"`, or `None` when the type is
/// not a method-level template param.
fn find_template_info_in_method(ty: &PhpType, method: &MethodInfo) -> Option<String> {
    let name = match ty {
        PhpType::Named(n) => n.as_str(),
        _ => return None,
    };

    // Method-level template_params stores just the names.
    if !method.template_params.iter().any(|p| p == name) {
        return None;
    }

    let bound_display = method
        .template_param_bounds
        .get(&crate::atom::atom(name))
        .map(|b| format!(" of `{}`", b.shorten()))
        .unwrap_or_default();

    // Method-level templates don't carry variance info (always invariant).
    Some(format!("**template** `{}`{}", name, bound_display))
}

/// Check whether `type_str` is a `@template` parameter declared on
/// `owner`'s class docblock.  Returns a formatted info line like
/// `"**template-covariant** \`TNode\` of \`AstNode\`"`, or `None`
/// when the type is not a template param on the class.
fn find_template_info_in_class(ty: &PhpType, owner: &ClassInfo) -> Option<String> {
    let name = match ty {
        PhpType::Named(n) => n.as_str(),
        _ => return None,
    };

    let docblock = owner.class_docblock.as_deref()?;
    let tpl = extract_template_params_full(docblock)
        .into_iter()
        .find(|(tpl_name, _, _, _)| tpl_name == name)?;

    let (tpl_name, bound, variance, default) = tpl;
    let bound_display = bound
        .map(|b| format!(" of `{}`", b.shorten()))
        .unwrap_or_default();
    let default_display = default.map(|d| format!(" = `{}`", d)).unwrap_or_default();

    Some(format!(
        "**{}** `{}`{}{}",
        variance.tag_name(),
        tpl_name,
        bound_display,
        default_display
    ))
}

/// Returns `true` when `s` is a simple, unqualified identifier (no
/// namespace separator).  The caller guarantees that `s` came from a
/// [`PhpType::Named`] match, so we only need to check for `\`.
fn is_bare_identifier(s: &str) -> bool {
    !s.is_empty() && !s.contains('\\')
}

/// Maximum number of enum cases or trait methods to show before
/// truncating with a "and N more…" comment.
const MAX_BODY_ITEMS: usize = 30;

/// Build the body lines for an enum hover showing its cases.
///
/// Only enum cases are shown (not regular class constants).
/// Each case is rendered as `    case Name = 'value';` or `    case Name;`.
/// If there are more than [`MAX_BODY_ITEMS`] cases, the list is truncated
/// with a `// and N more…` comment.
fn build_enum_case_body(cls: &ClassInfo) -> String {
    let cases: Vec<&ConstantInfo> = cls.constants.iter().filter(|c| c.is_enum_case).collect();

    if cases.is_empty() {
        return String::new();
    }

    let mut body = String::new();
    let shown = cases.len().min(MAX_BODY_ITEMS);

    for case in &cases[..shown] {
        if let Some(ref val) = case.enum_value {
            body.push_str(&format!("    case {} = {};\n", case.name, val));
        } else {
            body.push_str(&format!("    case {};\n", case.name));
        }
    }

    if cases.len() > MAX_BODY_ITEMS {
        body.push_str(&format!(
            "    // and {} more…\n",
            cases.len() - MAX_BODY_ITEMS
        ));
    }

    body
}

/// Build the body lines for a trait hover showing public member signatures.
///
/// Shows public methods (one-line signatures without bodies), public
/// properties, and public constants. Uses native types only and short
/// (unqualified) class names for a scannable summary.
///
/// If there are more than [`MAX_BODY_ITEMS`] members, the list is
/// truncated with a `// and N more…` comment.
fn build_trait_summary_body(cls: &ClassInfo) -> String {
    let mut member_lines: Vec<String> = Vec::new();

    // Public constants.
    for constant in &cls.constants {
        if constant.visibility != Visibility::Public {
            continue;
        }
        let type_hint = constant
            .type_hint
            .as_ref()
            .map(|t| format!(": {}", t))
            .unwrap_or_default();
        let value_suffix = constant
            .value
            .as_ref()
            .map(|v| format!(" = {}", v))
            .unwrap_or_default();
        member_lines.push(format!(
            "    const {}{}{};",
            constant.name, type_hint, value_suffix
        ));
    }

    // Public properties.
    for prop in &cls.properties {
        if prop.visibility != Visibility::Public {
            continue;
        }
        let static_kw = if prop.is_static { "static " } else { "" };
        let native_type = prop
            .native_type_hint
            .as_ref()
            .map(|t| format!("{} ", t))
            .unwrap_or_default();
        member_lines.push(format!(
            "    public {}{}${};",
            static_kw, native_type, prop.name
        ));
    }

    // Public methods.
    for method in &cls.methods {
        if method.visibility != Visibility::Public {
            continue;
        }
        let static_kw = if method.is_static { "static " } else { "" };
        let native_params = format_native_params(&method.parameters);
        let native_ret = method
            .native_return_type
            .as_ref()
            .map(|r| format!(": {}", r))
            .unwrap_or_default();
        member_lines.push(format!(
            "    public {}function {}({}){};",
            static_kw, method.name, native_params, native_ret
        ));
    }

    if member_lines.is_empty() {
        return String::new();
    }

    let shown = member_lines.len().min(MAX_BODY_ITEMS);
    let mut body: String = member_lines[..shown].join("\n");
    body.push('\n');

    if member_lines.len() > MAX_BODY_ITEMS {
        body.push_str(&format!(
            "    // and {} more…\n",
            member_lines.len() - MAX_BODY_ITEMS
        ));
    }

    body
}

/// Extract the value of a constant from PHP source text.
///
/// Scans for patterns like:
/// - `define('NAME', value)` or `define("NAME", value)`
/// - `const NAME = value;`
///
/// Returns `Some(value_string)` when found, `None` when the constant
/// definition could not be located or the value could not be extracted.
///
/// **Note:** Production code should use `update_ast` to parse constants
/// through the AST pipeline (which populates `global_defines`).  This
/// function exists only for unit tests.
#[cfg(test)]
pub(crate) fn extract_constant_value_from_source(name: &str, source: &str) -> Option<String> {
    // Try `define('NAME', value)` pattern.
    for quote in &["'", "\""] {
        let needle = format!("define({quote}{name}{quote}");
        if let Some(pos) = source.find(&needle) {
            // Extract only the second argument.  Stop at the first
            // unquoted comma (third argument) or closing paren,
            // whichever comes first.
            let after = &source[pos + needle.len()..];
            if let Some(comma) = after.find(',') {
                let value_start = &after[comma + 1..];
                let trimmed = value_start.trim_start();
                // Find where the second argument ends: either an
                // unquoted comma (start of optional third arg) or
                // the closing paren.
                let end =
                    find_unquoted_comma(trimmed).or_else(|| find_balanced_close_paren(trimmed));
                if let Some(end) = end {
                    let val = trimmed[..end].trim();
                    if !val.is_empty() {
                        // Empty string literals are placeholders for
                        // runtime-defined values — show the type instead.
                        if val == "''" || val == "\"\"" {
                            return Some("string".to_string());
                        }
                        return Some(val.to_string());
                    }
                }
            }
        }
    }

    // Try `const NAME = value;` pattern.
    let const_needle = format!("const {name}");
    for (i, _) in source.match_indices(&const_needle) {
        let after = &source[i + const_needle.len()..];
        let trimmed = after.trim_start();
        if let Some(rest) = trimmed.strip_prefix('=') {
            let value_part = rest.trim_start();
            if let Some(semi) = value_part.find(';') {
                let val = value_part[..semi].trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }

    None
}

/// Find the position of the first unquoted comma in `s`.
///
/// Skips over single- and double-quoted string literals so that
/// commas inside string values are not mistaken for argument
/// separators.
#[cfg(test)]
fn find_unquoted_comma(s: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let mut prev = b'\0';

    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'\'' if !in_double && prev != b'\\' => in_single = !in_single,
            b'"' if !in_single && prev != b'\\' => in_double = !in_double,
            b',' if !in_single && !in_double => return Some(i),
            _ => {}
        }
        prev = b;
    }
    None
}

/// Find the position of the closing `)` that matches an implicit
/// opening paren, handling one level of nesting and string literals.
#[cfg(test)]
fn find_balanced_close_paren(s: &str) -> Option<usize> {
    let mut depth = 0u32;
    let mut in_single = false;
    let mut in_double = false;
    let mut prev = b'\0';

    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'\'' if !in_double && prev != b'\\' => in_single = !in_single,
            b'"' if !in_single && prev != b'\\' => in_double = !in_double,
            b'(' if !in_single && !in_double => depth += 1,
            b')' if !in_single && !in_double => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        prev = b;
    }
    None
}

#[cfg(test)]
mod tests;
