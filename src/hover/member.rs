//! Member hover: methods, properties, and class constants.
//!
//! Builds the Markdown popup for a resolved class member, including the
//! origin indicator line (override / implements / virtual / macro), the
//! declaring-class lookup, and the Laravel property-source breakdown.

use std::sync::Arc;

use tower_lsp::lsp_types::{Hover, HoverContents};

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;
use crate::symbol_map::SubjectText;
use crate::types::*;
use crate::util::short_name;

use super::HoverSite;
use super::formatting::*;
use super::templates::{find_template_info_in_class, find_template_info_in_method_or_class};

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

pub(super) fn format_property_source(source: &PropertySource) -> Vec<String> {
    match source {
        PropertySource::DeclaredDefault { .. } => Vec::new(),
        PropertySource::DatabaseColumn {
            column,
            attribute_default,
            mutator,
        } => {
            let mut lines = vec![if mutator.is_some() {
                "source: database column and mutator".to_string()
            } else {
                "source: database column".to_string()
            }];
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
            mutator,
        } => {
            let mut lines = vec![if mutator.is_some() {
                format!("source: cast `{}` and mutator", cast)
            } else {
                format!("source: cast `{}`", cast)
            }];
            if let Some(column) = column {
                lines.extend(format_database_column_details(column));
            }
            if let Some(default) = attribute_default {
                lines.extend(format_attribute_default_details(default));
            }
            lines
        }
        PropertySource::Accessor {
            column, mutator, ..
        } => {
            let mut lines = vec![if mutator.is_some() {
                "source: accessor and mutator".to_string()
            } else {
                "source: accessor".to_string()
            }];
            if let Some(column) = column {
                lines.extend(format_database_column_details(column));
            }
            lines
        }
        PropertySource::AttributeDefault {
            default,
            column,
            mutator,
        } => {
            let mut lines = vec![if mutator.is_some() {
                "source: attribute default and mutator".to_string()
            } else {
                "source: attribute default".to_string()
            }];
            if let Some(column) = column {
                lines.extend(format_database_column_details(column));
            }
            lines.extend(format_attribute_default_details(default));
            lines
        }
        PropertySource::ComputedProperty { mutator, .. } => vec![if mutator.is_some() {
            "source: computed property and mutator".to_string()
        } else {
            "source: computed property".to_string()
        }],
        PropertySource::Relationship {
            method,
            kind,
            pivot_accessor,
            pivot_using,
            pivot_columns,
        } => {
            let mut lines = vec![format!("source: relationship `{}` ({})", method, kind)];
            if let Some(accessor) = pivot_accessor {
                lines.push(format!("pivot accessor: `${}`", accessor));
            }
            if let Some(using) = pivot_using {
                lines.push(format!("pivot: `{}`", using));
            }
            if !pivot_columns.is_empty() {
                lines.push(format!("pivot columns: {}", pivot_columns.join(", ")));
            }
            lines
        }
        PropertySource::RelationshipCount { relationship } => {
            vec![format!("source: relationship count `{}`", relationship)]
        }
        PropertySource::Pivot => {
            vec!["source: pivot (many-to-many relationship)".to_string()]
        }
        // The `👻 virtual` marker already says the member comes from a
        // docblock tag; a "source:" line would be noise.
        PropertySource::DocblockTag => Vec::new(),
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
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    let fqn = owner.fqn();

    // Load the raw class.  If the loader returns None (e.g. the class
    // is only known through the current file's AST and not yet indexed),
    // fall back to assuming the member is declared — this avoids hiding
    // indicators when the project is only partially indexed.
    match class_loader(Fqn::new(&fqn)) {
        Some(raw) => declares_member(&raw, member_name, member_kind),
        None => true,
    }
}

/// Whether `class` itself declares a member with the given name and kind.
fn declares_member(
    class: &ClassInfo,
    member_name: &str,
    member_kind: &MemberKindForOrigin,
) -> bool {
    match member_kind {
        MemberKindForOrigin::Method => class
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(member_name)),
        MemberKindForOrigin::Property => class.properties.iter().any(|p| p.name == member_name),
        MemberKindForOrigin::Constant => class.constants.iter().any(|c| c.name == member_name),
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
    class_loader: &ResolvedClassLoader<'_>,
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
        if let Some(ref parent_name) = owner.parent_class
            && let Some(parent) = class_loader(Fqn::new(parent_name))
            && declares_member(&parent, member_name, &member_kind)
        {
            origins.push(MemberOrigin::Override(short_name(parent_name).to_string()));
        }

        for iface_name in &owner.interfaces {
            if let Some(iface) = class_loader(Fqn::new(iface_name))
                && declares_member(&iface, member_name, &member_kind)
            {
                origins.push(MemberOrigin::Implements(short_name(iface_name).to_string()));
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
/// searches the traits, the parent chain and the interfaces in PHP's
/// precedence order, then the mixins, to find the class that actually
/// declares the member.  Returns a fully-resolved
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
    class_loader: &ResolvedClassLoader<'_>,
) -> Arc<ClassInfo> {
    if raw_class_has_member(owner, member_name, member_kind, class_loader) {
        return Arc::new(owner.clone());
    }

    let declares = |class: &ClassInfo| declares_member(class, member_name, member_kind);
    if let Some((_, declaring)) =
        crate::inheritance::find_declaring_ancestor(owner, class_loader, &declares)
    {
        return declaring;
    }
    owner
        .mixins
        .iter()
        .filter_map(|name| class_loader(Fqn::new(name)))
        .find(|mixin| declares(mixin))
        .unwrap_or_else(|| Arc::new(owner.clone()))
}

/// Result of searching for a member on a [`ClassInfo`] for hover purposes.
///
/// Returned by [`Backend::find_member_for_hover`] so the caller can
/// dispatch to the correct `hover_for_*` method without repeating the
/// lookup logic.
pub(super) enum HoverMemberHit {
    Method(Box<MethodInfo>),
    Property(Box<PropertyInfo>),
    Constant(Box<ConstantInfo>),
}

impl Backend {
    /// Hover for `$subject->member` / `Subject::member`: the member on
    /// every class the subject can be, once per declaring class.
    pub(super) fn hover_member_access(
        &self,
        site: &HoverSite,
        subject_text: &SubjectText,
        member_name: &str,
        is_static: bool,
        is_method_call: bool,
    ) -> Option<Hover> {
        let HoverSite {
            uri,
            content,
            offset: cursor_offset,
            loaders,
            ..
        } = *site;
        let resolved_class_loader = loaders.resolved_class_loader();
        let rctx = loaders.resolution_ctx_at(content, cursor_offset);

        let access_kind = AccessKind::of_member_access(is_static);

        let source = self.symbol_map_source(uri, content)?;
        let candidates =
            ResolvedType::into_arced_classes(crate::type_engine::resolver::resolve_target_classes(
                subject_text.as_str(source),
                access_kind,
                &rctx,
            ));

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
                &resolved_class_loader,
                &self.resolved_class_cache,
            );
            let find_result = Self::find_member_for_hover(&merged, member_name, is_method_call);

            let (mut member_result, mut owner) = if find_result.is_some() {
                (find_result, merged.clone())
            } else {
                // Fall back to the candidate directly — it may
                // contain model-specific members (e.g. Eloquent
                // scope methods injected onto Builder<Model>)
                // that the FQN-keyed cache does not have.
                let result = Self::find_member_for_hover(target_class, member_name, is_method_call);
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
                                    is_method_call,
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
                                    is_method_call,
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
                            &resolved_class_loader,
                        )
                    {
                        method.return_type = Some(date_return_type);
                        method.is_inferred_return = true;
                    }
                    let declaring = find_declaring_class(
                        &owner,
                        member_name,
                        &MemberKindForOrigin::Method,
                        &resolved_class_loader,
                    );
                    Some((
                        declaring.fqn().to_string(),
                        self.hover_for_method(
                            &method,
                            &declaring,
                            &resolved_class_loader,
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
                        &resolved_class_loader,
                    );
                    Some((
                        declaring.fqn().to_string(),
                        self.hover_for_property(prop, &declaring, &resolved_class_loader),
                    ))
                }
                Some(HoverMemberHit::Constant(ref constant)) => {
                    let declaring = find_declaring_class(
                        &owner,
                        &constant.name,
                        &MemberKindForOrigin::Constant,
                        &resolved_class_loader,
                    );
                    Some((
                        declaring.fqn().to_string(),
                        self.hover_for_constant(constant, &declaring, &resolved_class_loader),
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

    /// Close out a member's hover: append the rendered declaration and,
    /// when the class's provenance is known (a vendor package, a stub),
    /// a line naming it, then join everything into one `Hover`.
    ///
    /// Shared by [`hover_for_method`](Self::hover_for_method),
    /// [`hover_for_property`](Self::hover_for_property), and
    /// [`hover_for_constant`](Self::hover_for_constant), which differ only
    /// in what they push onto `lines` before reaching this tail.
    fn finish_member_hover(
        &self,
        owner: &ClassInfo,
        member_line: &str,
        mut lines: Vec<String>,
    ) -> Hover {
        let code = build_class_member_block(
            &owner.name,
            owner.file_namespace.as_deref(),
            owner_kind_keyword(owner),
            &owner_name_suffix(owner),
            member_line,
        );
        lines.push(code);

        if let Some(prov) = self.provenance_line_for_class(&owner.fqn()) {
            lines.push(prov);
        }

        make_hover(lines.join("\n\n"))
    }

    /// Search `class` for a member matching `member_name`.
    ///
    /// When `is_method_call` is true, only methods are considered.
    /// Otherwise properties and constants are tried first, with a
    /// final fallback to methods (handles method references without
    /// call parentheses).
    pub(super) fn find_member_for_hover(
        class: &ClassInfo,
        member_name: &str,
        is_method_call: bool,
    ) -> Option<HoverMemberHit> {
        if is_method_call {
            class
                .get_method_ci(member_name)
                .map(|m| HoverMemberHit::Method(Box::new(m.clone())))
        } else {
            if let Some(prop) = class.get_property(member_name) {
                return Some(HoverMemberHit::Property(Box::new(prop.clone())));
            }
            if let Some(constant) = class.get_constant(member_name) {
                return Some(HoverMemberHit::Constant(Box::new(constant.clone())));
            }
            class
                .get_method_ci(member_name)
                .map(|m| HoverMemberHit::Method(Box::new(m.clone())))
        }
    }

    /// Build hover content for a method.
    pub(crate) fn hover_for_method(
        &self,
        method: &MethodInfo,
        owner: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
        uri: &str,
        content: &str,
    ) -> Hover {
        // When the method has no declared return type, or its only declared
        // type is `mixed` (native or docblock — carries no information),
        // try to infer the return type from the method body. This mirrors
        // what completion/hover resolution does, so the hover display
        // matches the resolved type the user sees.
        let inferred_return_type: Option<crate::php_type::PhpType> =
            if method.return_type.as_ref().is_none_or(|t| t.is_mixed())
                && method.name_offset != 0
                && !method.is_virtual
            {
                crate::type_engine::call_resolution::try_infer_body_return_type(
                    self,
                    &owner.fqn(),
                    method,
                    // Hovering the declaration itself asks what the method
                    // returns in general, not at one call site.
                    &[],
                )
                .filter(|t| !t.is_mixed() && !t.is_void())
            } else {
                None
            };

        // Inference only ever produces something when the declared type was
        // absent or `mixed`, so preferring it here cannot shadow a real
        // declared type — it only replaces an uninformative `mixed`.
        let effective_return = inferred_return_type
            .as_ref()
            .or(method.return_type.as_ref());

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

        if let Some(msg) = method.deprecation_message() {
            lines.push(format_deprecation_line(msg));
        }

        for url in method.links() {
            lines.push(format!("[{}]({})", url, url));
        }

        let resolved_see = self.resolve_see_refs(method.see_refs(), uri, content);
        format_see_refs(&resolved_see, method.links(), &mut lines);

        let show_inferred = method.is_inferred_return || inferred_return_type.is_some();
        if let Some(section) = build_param_return_section(
            &method.parameters,
            effective_return,
            method.native_return_type.as_ref(),
            method.return_description(),
            show_inferred,
        ) {
            lines.push(section);
        }

        self.finish_member_hover(owner, &member_line, lines)
    }

    /// Build hover content for a property.
    pub(crate) fn hover_for_property(
        &self,
        property: &PropertyInfo,
        owner: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
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
            let source_lines = format_property_source(source);
            if !source_lines.is_empty() {
                lines.push(source_lines.join("\n"));
            }
        }

        if let Some(ref msg) = property.deprecation_message {
            lines.push(format_deprecation_line(msg));
        }

        self.finish_member_hover(owner, &member_line, lines)
    }

    /// Build hover content for a class constant.
    pub(crate) fn hover_for_constant(
        &self,
        constant: &ConstantInfo,
        owner: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Hover {
        let member_line = if constant.is_enum_case {
            if let Some(ref val) = constant.enum_value {
                format!("case {} = {};", constant.name, val)
            } else {
                format!("case {};", constant.name)
            }
        } else {
            format!(
                "{}{}",
                format_visibility(constant.visibility),
                constant_declaration(constant)
            )
        };

        let mut lines = Vec::new();

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
        self.finish_member_hover(owner, &member_line, lines)
    }
}
