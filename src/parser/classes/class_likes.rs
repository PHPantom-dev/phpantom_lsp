//! One builder per class-like kind: the [`ClassInfo`] of a `class`,
//! `interface`, `trait` or `enum` declaration, from its header, its
//! members and the tags of its docblock.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::attribute::AttributeList;
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom, atom_bytes};
use crate::docblock;
use crate::parser::{DocblockCtx, attributes, extract_hint_type, merge_deprecation_info};
use crate::php_type::PhpType;
use crate::types::*;

use super::add_implicit_stringable;

/// Docblock-derived metadata common to all class-like declarations.
///
/// Produced by [`extract_class_docblock`] and consumed by each builder
/// below to avoid repeating the same extraction calls for classes,
/// interfaces, traits, and enums.
#[derive(Default)]
struct ClassDocblockInfo {
    /// Deprecation message from `@deprecated`, or `None` if not deprecated.
    deprecation_message: Option<String>,
    /// `@template` parameters declared on the class-like.
    template_params: Vec<Atom>,
    /// Upper bounds for template parameters (`@template T of Bound`).
    template_param_bounds: AtomMap<PhpType>,
    /// Default values for template parameters (`@template T of bool = false`).
    template_param_defaults: AtomMap<PhpType>,
    /// Generic arguments from `@extends` / `@phpstan-extends`.
    extends_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Generic arguments from `@implements` / `@phpstan-implements`.
    implements_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Generic arguments from `@use` / `@phpstan-use`.
    use_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Type aliases from `@phpstan-type` / `@psalm-type`.
    type_aliases: AtomMap<TypeAliasDef>,
    /// Mixin class names from `@mixin` tags.
    mixins: Vec<Atom>,
    /// Generic type arguments from `@mixin` tags.
    ///
    /// Each entry is `(MixinClassName, [TypeArg1, TypeArg2, …])`.
    /// Only populated for mixins that have generic arguments.
    mixin_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Required base class from a `@phpstan-require-extends` tag (traits only).
    require_extends: Option<Atom>,
    /// Required interfaces from `@phpstan-require-implements` tags (traits only).
    require_implements: Vec<Atom>,
    /// URLs from `@link` and `@see` tags in the class-level docblock.
    links: Vec<String>,
    /// `@see` references from the class-level docblock.
    see_refs: Vec<crate::types::SeeRef>,
    /// Raw class-level docblock text, kept for the consumers that render
    /// or re-scan the original text.
    raw_docblock: Option<String>,
    /// Byte offset of `raw_docblock` in the file.
    docblock_start: u32,
    /// `@method` / `@property` tags parsed out of `raw_docblock`.
    doc_members: Option<Arc<DocblockMembers>>,
}

/// Extract all docblock-derived metadata from a class-like AST node.
///
/// Returns [`ClassDocblockInfo::default()`] when no docblock context is
/// available or when the node has no preceding doc comment.
fn extract_class_docblock<'a>(
    node: &impl HasSpan,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> ClassDocblockInfo {
    let Some(ctx) = doc_ctx else {
        return ClassDocblockInfo::default();
    };
    let Some((doc_text, docblock_start)) =
        crate::symbol_map::docblock::get_docblock_text_with_offset(ctx.trivias, ctx.content, node)
    else {
        return ClassDocblockInfo::default();
    };
    let Some(info) = docblock::parse_docblock_for_tags(doc_text) else {
        return ClassDocblockInfo::default();
    };

    let params_full = docblock::extract_template_params_full_from_info(&info);
    let template_params: Vec<Atom> = params_full.iter().map(|(n, _, _, _)| atom(n)).collect();
    let template_param_bounds: AtomMap<PhpType> = params_full
        .iter()
        .filter_map(|(name, bound, _, _)| bound.as_ref().map(|b| (atom(name), b.clone())))
        .collect();
    let template_param_defaults: AtomMap<PhpType> = params_full
        .into_iter()
        .filter_map(|(name, _, _, default)| default.map(|d| (atom(&name), d)))
        .collect();

    let mixin_data = docblock::extract_mixin_tags_from_info(&info);
    let mixins: Vec<Atom> = mixin_data.iter().map(|(name, _)| atom(name)).collect();
    let mixin_generics: Vec<(Atom, Vec<PhpType>)> = mixin_data
        .into_iter()
        .filter(|(_, args)| !args.is_empty())
        .map(|(name, args)| (atom(&name), args))
        .collect();

    ClassDocblockInfo {
        deprecation_message: docblock::extract_deprecation_message_from_info(&info),
        template_params,
        template_param_bounds,
        template_param_defaults,
        extends_generics: docblock::extract_generics_tag_from_info(&info, "@extends")
            .into_iter()
            .map(|(n, args)| (atom(&n), args))
            .collect(),
        implements_generics: docblock::extract_generics_tag_from_info(&info, "@implements")
            .into_iter()
            .map(|(n, args)| (atom(&n), args))
            .collect(),
        use_generics: docblock::extract_generics_tag_from_info(&info, "@use")
            .into_iter()
            .map(|(n, args)| (atom(&n), args))
            .collect(),
        type_aliases: docblock::extract_type_aliases_from_info(&info)
            .into_iter()
            .map(|(k, v)| (atom(&k), v))
            .collect(),
        mixins,
        mixin_generics,
        require_extends: docblock::extract_require_extends_from_info(&info).map(|n| atom(&n)),
        require_implements: docblock::extract_require_implements_from_info(&info)
            .into_iter()
            .map(|n| atom(&n))
            .collect(),
        links: docblock::extract_link_urls_from_info(&info),
        see_refs: docblock::extract_see_refs_from_info(&info),
        raw_docblock: Some(doc_text.to_string()),
        docblock_start,
        doc_members: DocblockMembers::from_info(&info),
    }
}

/// Where a declaration starts: at its first attribute, else at its
/// keyword.
fn decl_start_offset(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    keyword_offset: u32,
) -> u32 {
    attribute_lists
        .first()
        .map_or(keyword_offset, |a| a.span().start.offset)
}

/// The [`ClassInfo`] of a `class` declaration.
pub(super) fn extract_class<'a>(
    class: &'a Class<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> ClassInfo {
    let parent_class = class
        .extends
        .as_ref()
        .and_then(|ext| ext.types.first().map(|ident| atom_bytes(ident.value())));

    let mut interfaces: Vec<Atom> = class
        .implements
        .as_ref()
        .map(|imp| {
            imp.types
                .iter()
                .map(|ident| atom_bytes(ident.value()))
                .collect()
        })
        .unwrap_or_default();

    let doc_info = extract_class_docblock(class, doc_ctx);

    let ExtractedMembers {
        methods,
        properties,
        constants,
        used_traits,
        trait_precedences,
        trait_aliases,
        inline_use_generics,
    } = Backend::extract_class_like_members(
        class.members.iter(),
        doc_ctx,
        &doc_info.template_params,
        &doc_info.template_param_bounds,
    );
    add_implicit_stringable(&mut interfaces, &methods);

    let mut use_generics: Vec<(Atom, Vec<PhpType>)> = doc_info.use_generics;
    use_generics.extend(inline_use_generics);

    let keyword_offset = class.class.span.start.offset;
    let content = doc_ctx.map(|c| c.content).unwrap_or("");
    let laravel_metadata = crate::virtual_members::laravel::extract_laravel_metadata(
        class,
        &methods,
        &used_traits,
        &use_generics,
        content,
        doc_ctx,
    );
    let attr_targets = attributes::extract_attribute_targets(&class.attribute_lists, content);

    let class_depr = merge_deprecation_info(
        doc_info.deprecation_message,
        &class.attribute_lists,
        doc_ctx,
    );
    ClassInfo {
        kind: ClassLikeKind::Class,
        name: atom_bytes(class.name.value),
        methods: crate::types::into_arcs(methods).into(),
        properties: crate::types::into_arcs(properties).into(),
        constants: crate::types::into_arcs(constants).into(),
        start_offset: class.left_brace.start.offset,
        end_offset: class.right_brace.end.offset,
        keyword_offset,
        decl_start_offset: decl_start_offset(&class.attribute_lists, keyword_offset),
        parent_class,
        interfaces,
        used_traits,
        mixins: doc_info.mixins,
        mixin_generics: doc_info.mixin_generics,
        require_extends: doc_info.require_extends,
        require_implements: doc_info.require_implements,
        is_final: class.modifiers.contains_final(),
        is_abstract: class.modifiers.contains_abstract(),
        is_readonly: class.modifiers.contains_readonly(),
        deprecation_message: class_depr.message,
        deprecated_replacement: class_depr.replacement,
        links: doc_info.links,
        see_refs: doc_info.see_refs,
        template_params: doc_info.template_params,
        template_param_bounds: doc_info.template_param_bounds,
        template_param_defaults: doc_info.template_param_defaults,
        extends_generics: doc_info.extends_generics,
        implements_generics: doc_info.implements_generics,
        use_generics,
        type_aliases: doc_info.type_aliases,
        trait_precedences,
        trait_aliases,
        class_docblock: doc_info.raw_docblock,
        class_docblock_start: doc_info.docblock_start,
        doc_members: doc_info.doc_members,
        attribute_targets: attr_targets,
        laravel: Some(Box::new(laravel_metadata)),
        ..Default::default()
    }
}

/// The [`ClassInfo`] of an `interface` declaration.
pub(super) fn extract_interface<'a>(
    iface: &'a Interface<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> ClassInfo {
    // Interfaces can extend multiple parent interfaces.  Store the first
    // one in `parent_class` for backward compatibility with
    // single-inheritance resolution, and all of them in `interfaces` so
    // that transitive interface inheritance checks work correctly.
    let mut all_parents: Vec<Atom> = iface
        .extends
        .as_ref()
        .map(|ext| {
            ext.types
                .iter()
                .map(|ident| atom_bytes(ident.value()))
                .collect()
        })
        .unwrap_or_default();

    let parent_class = all_parents.first().copied();

    let doc_info = extract_class_docblock(iface, doc_ctx);

    let ExtractedMembers {
        methods,
        properties,
        constants,
        used_traits,
        trait_precedences,
        trait_aliases,
        inline_use_generics,
    } = Backend::extract_class_like_members(
        iface.members.iter(),
        doc_ctx,
        &doc_info.template_params,
        &doc_info.template_param_bounds,
    );
    add_implicit_stringable(&mut all_parents, &methods);

    let mut use_generics = doc_info.use_generics;
    use_generics.extend(inline_use_generics);

    let keyword_offset = iface.interface.span.start.offset;
    let iface_depr = merge_deprecation_info(
        doc_info.deprecation_message,
        &iface.attribute_lists,
        doc_ctx,
    );
    ClassInfo {
        kind: ClassLikeKind::Interface,
        name: atom_bytes(iface.name.value),
        methods: crate::types::into_arcs(methods).into(),
        properties: crate::types::into_arcs(properties).into(),
        constants: crate::types::into_arcs(constants).into(),
        start_offset: iface.left_brace.start.offset,
        end_offset: iface.right_brace.end.offset,
        keyword_offset,
        decl_start_offset: decl_start_offset(&iface.attribute_lists, keyword_offset),
        parent_class,
        interfaces: all_parents,
        used_traits,
        mixins: doc_info.mixins,
        mixin_generics: doc_info.mixin_generics,
        require_extends: doc_info.require_extends,
        require_implements: doc_info.require_implements,
        deprecation_message: iface_depr.message,
        deprecated_replacement: iface_depr.replacement,
        links: doc_info.links,
        see_refs: doc_info.see_refs,
        template_params: doc_info.template_params,
        template_param_bounds: doc_info.template_param_bounds,
        template_param_defaults: doc_info.template_param_defaults,
        extends_generics: doc_info.extends_generics,
        implements_generics: doc_info.implements_generics,
        use_generics,
        type_aliases: doc_info.type_aliases,
        trait_precedences,
        trait_aliases,
        class_docblock: doc_info.raw_docblock,
        class_docblock_start: doc_info.docblock_start,
        doc_members: doc_info.doc_members,
        ..Default::default()
    }
}

/// The [`ClassInfo`] of a `trait` declaration.
pub(super) fn extract_trait<'a>(
    trait_def: &'a Trait<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> ClassInfo {
    let doc_info = extract_class_docblock(trait_def, doc_ctx);

    let ExtractedMembers {
        methods,
        properties,
        constants,
        used_traits,
        trait_precedences,
        trait_aliases,
        inline_use_generics,
    } = Backend::extract_class_like_members(
        trait_def.members.iter(),
        doc_ctx,
        &doc_info.template_params,
        &doc_info.template_param_bounds,
    );

    let keyword_offset = trait_def.r#trait.span.start.offset;
    let trait_depr = merge_deprecation_info(
        doc_info.deprecation_message,
        &trait_def.attribute_lists,
        doc_ctx,
    );
    let laravel = crate::virtual_members::laravel::extract_laravel_trait_metadata(
        trait_def,
        &methods,
        doc_ctx.map_or("", |ctx| ctx.content),
    );
    ClassInfo {
        kind: ClassLikeKind::Trait,
        name: atom_bytes(trait_def.name.value),
        methods: crate::types::into_arcs(methods).into(),
        properties: crate::types::into_arcs(properties).into(),
        constants: crate::types::into_arcs(constants).into(),
        start_offset: trait_def.left_brace.start.offset,
        end_offset: trait_def.right_brace.end.offset,
        keyword_offset,
        decl_start_offset: decl_start_offset(&trait_def.attribute_lists, keyword_offset),
        used_traits,
        mixins: doc_info.mixins,
        mixin_generics: doc_info.mixin_generics,
        require_extends: doc_info.require_extends,
        require_implements: doc_info.require_implements,
        deprecation_message: trait_depr.message,
        deprecated_replacement: trait_depr.replacement,
        links: doc_info.links,
        see_refs: doc_info.see_refs,
        template_params: doc_info.template_params,
        template_param_bounds: doc_info.template_param_bounds,
        template_param_defaults: doc_info.template_param_defaults,
        use_generics: inline_use_generics,
        type_aliases: doc_info.type_aliases,
        trait_precedences,
        trait_aliases,
        class_docblock: doc_info.raw_docblock,
        class_docblock_start: doc_info.docblock_start,
        doc_members: doc_info.doc_members,
        laravel,
        ..Default::default()
    }
}

/// The [`ClassInfo`] of an `enum` declaration.
pub(super) fn extract_enum<'a>(
    enum_def: &'a Enum<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> ClassInfo {
    let ExtractedMembers {
        methods,
        mut properties,
        constants,
        mut used_traits,
        trait_precedences,
        trait_aliases,
        inline_use_generics,
    } = Backend::extract_class_like_members(
        enum_def.members.iter(),
        doc_ctx,
        &[],
        &AtomMap::default(),
    );
    push_enum_case_properties(enum_def, &constants, &mut properties);

    // Enums implicitly implement UnitEnum or BackedEnum.  We add the
    // interface with a leading backslash so that `resolve_to_fqn` treats it
    // as fully-qualified and does not prepend the current namespace.
    // `resolve_to_fqn` then strips the `\` to produce the canonical form
    // (`BackedEnum` / `UnitEnum`).  The class_loader / merge_traits_into
    // path will pick up the interface from the SPL stubs and merge its
    // methods (cases, from, tryFrom, …) automatically.
    let implicit_interface = if enum_def.backing_type_hint.is_some() {
        "\\BackedEnum"
    } else {
        "\\UnitEnum"
    };
    used_traits.push(atom(implicit_interface));

    let doc_info = extract_class_docblock(enum_def, doc_ctx);

    let mut use_generics = doc_info.use_generics;
    use_generics.extend(inline_use_generics);

    let mut interfaces: Vec<Atom> = enum_def
        .implements
        .as_ref()
        .map(|imp| {
            imp.types
                .iter()
                .map(|ident| atom_bytes(ident.value()))
                .collect()
        })
        .unwrap_or_default();

    // Also add the implicit interface to the interfaces list so that
    // hierarchy checks (`is_subtype_of`) recognise backed enums as subtypes
    // of `BackedEnum` and all enums as subtypes of `UnitEnum`.
    if !interfaces
        .iter()
        .any(|i| i.trim_start_matches('\\') == implicit_interface.trim_start_matches('\\'))
    {
        interfaces.push(atom(implicit_interface));
    }

    let keyword_offset = enum_def.r#enum.span.start.offset;
    let enum_depr = merge_deprecation_info(
        doc_info.deprecation_message,
        &enum_def.attribute_lists,
        doc_ctx,
    );
    ClassInfo {
        kind: ClassLikeKind::Enum,
        name: atom_bytes(enum_def.name.value),
        methods: crate::types::into_arcs(methods).into(),
        properties: crate::types::into_arcs(properties).into(),
        constants: crate::types::into_arcs(constants).into(),
        start_offset: enum_def.left_brace.start.offset,
        end_offset: enum_def.right_brace.end.offset,
        keyword_offset,
        decl_start_offset: decl_start_offset(&enum_def.attribute_lists, keyword_offset),
        interfaces,
        used_traits,
        mixins: doc_info.mixins,
        mixin_generics: doc_info.mixin_generics,
        require_extends: doc_info.require_extends,
        require_implements: doc_info.require_implements,
        // Enums are implicitly final and cannot be extended.
        is_final: true,
        deprecation_message: enum_depr.message,
        deprecated_replacement: enum_depr.replacement,
        links: doc_info.links,
        see_refs: doc_info.see_refs,
        implements_generics: doc_info.implements_generics,
        use_generics,
        type_aliases: doc_info.type_aliases,
        trait_precedences,
        trait_aliases,
        class_docblock: doc_info.raw_docblock,
        class_docblock_start: doc_info.docblock_start,
        doc_members: doc_info.doc_members,
        backed_type: enum_def.backing_type_hint.as_ref().and_then(|h| {
            let ty = extract_hint_type(&h.hint);
            if ty.is_string_type() {
                Some(BackedEnumType::String)
            } else if ty.is_int() {
                Some(BackedEnumType::Int)
            } else {
                None
            }
        }),
        ..Default::default()
    }
}

/// Every enum case exposes a readonly `name` property, and backed enums
/// additionally expose a `value` property.  These are real instance
/// properties in PHP (declared on the UnitEnum/BackedEnum interfaces via
/// stubs, but interface properties are not merged into implementors), so
/// synthesize them here.  A value typed as the enum could be any of its
/// cases, so `name` is the union of the case names and `value` the union of
/// the backing values (the bare backing type when one cannot be folded).
fn push_enum_case_properties(
    enum_def: &Enum<'_>,
    constants: &[ConstantInfo],
    properties: &mut Vec<PropertyInfo>,
) {
    let case_names: Vec<PhpType> = constants
        .iter()
        .filter(|c| c.is_enum_case)
        .map(|c| PhpType::literal_string_raw(format!("'{}'", c.name)))
        .collect();
    let name_type = union_of_distinct(case_names).unwrap_or_else(|| PhpType::named(atom("string")));
    properties.push(PropertyInfo::virtual_property_typed(
        "name",
        Some(&name_type),
    ));
    if let Some(hint) = enum_def.backing_type_hint.as_ref() {
        let backing_type = extract_hint_type(&hint.hint);
        let case_values = constants
            .iter()
            .filter(|c| c.is_enum_case)
            .map(|c| {
                c.enum_value.as_deref().and_then(
                    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value,
                )
            })
            .collect::<Option<Vec<PhpType>>>()
            .and_then(union_of_distinct);
        properties.push(PropertyInfo::virtual_property_typed(
            "value",
            Some(&case_values.unwrap_or(backing_type)),
        ));
    }
}

/// The union of `types` with duplicates dropped; `None` when empty.
fn union_of_distinct(types: Vec<PhpType>) -> Option<PhpType> {
    let mut unique: Vec<PhpType> = Vec::with_capacity(types.len());
    for t in types {
        if !unique.contains(&t) {
            unique.push(t);
        }
    }
    match unique.len() {
        0 => None,
        1 => unique.pop(),
        _ => Some(PhpType::union(unique)),
    }
}
