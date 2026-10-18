//! The members of a class-like body: the walk over them, and the class
//! constants, enum cases and trait uses.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::constant::ClassLikeConstant;
use mago_syntax::cst::class_like::enum_case::{EnumCase, EnumCaseItem};
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::class_like::trait_use::{
    TraitUse, TraitUseAdaptation, TraitUseMethodReference, TraitUseSpecification,
};

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom, atom_bytes};
use crate::docblock;
use crate::parser::{DocblockCtx, extract_hint_type, extract_visibility, merge_deprecation_info};
use crate::php_type::PhpType;
use crate::types::*;

use super::methods::extract_method;
use super::properties::{extract_property, infer_assigned_property_types};

impl Backend {
    /// Extract methods, properties, constants, and used trait names from
    /// class-like members.
    ///
    /// This is shared between `Statement::Class`, `Statement::Interface`,
    /// `Statement::Trait`, and `Statement::Enum` since all use the same
    /// `ClassLikeMember` representation.
    ///
    /// When `doc_ctx` is provided, PHPDoc `@return` and `@var` tags are used
    /// to refine (or supply) type information for methods and properties.
    pub(crate) fn extract_class_like_members<'a>(
        members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
        doc_ctx: Option<&DocblockCtx<'a>>,
        class_template_params: &[Atom],
        class_template_param_bounds: &AtomMap<PhpType>,
    ) -> ExtractedMembers {
        let mut extracted = ExtractedMembers::default();
        let mut method_bodies: Vec<(usize, &MethodBody<'_>)> = Vec::new();

        for member in members {
            match member {
                ClassLikeMember::Method(method) => {
                    let Some(info) = extract_method(
                        method,
                        doc_ctx,
                        class_template_params,
                        class_template_param_bounds,
                        &mut extracted.properties,
                    ) else {
                        continue;
                    };
                    extracted.methods.push(info);
                    method_bodies.push((extracted.methods.len() - 1, &method.body));
                }
                ClassLikeMember::Property(property) => {
                    extracted
                        .properties
                        .extend(extract_property(member, property, doc_ctx));
                }
                ClassLikeMember::Constant(constant) => {
                    extract_constants(member, constant, doc_ctx, &mut extracted.constants);
                }
                ClassLikeMember::EnumCase(enum_case) => {
                    extracted
                        .constants
                        .push(extract_enum_case(member, enum_case, doc_ctx));
                }
                ClassLikeMember::TraitUse(trait_use) => {
                    extract_trait_use(trait_use, doc_ctx, &mut extracted);
                }
            }
        }

        infer_assigned_property_types(
            &mut extracted.properties,
            &extracted.methods,
            &method_bodies,
            doc_ctx,
        );
        extracted
    }
}

/// Push a [`ConstantInfo`] for each constant a `const` declaration
/// declares.
fn extract_constants(
    member: &ClassLikeMember<'_>,
    constant: &ClassLikeConstant<'_>,
    doc_ctx: Option<&DocblockCtx<'_>>,
    constants: &mut Vec<ConstantInfo>,
) {
    let type_hint = constant.hint.as_ref().map(|h| extract_hint_type(h));
    let visibility = extract_visibility(constant.modifiers.iter());
    let const_docblock_text = doc_ctx
        .and_then(|ctx| docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, member));
    let const_docblock_info = const_docblock_text.and_then(docblock::parse_docblock_for_tags);
    let depr_info = {
        let docblock_msg = const_docblock_info
            .as_ref()
            .and_then(docblock::extract_deprecation_message_from_info);
        merge_deprecation_info(docblock_msg, &constant.attribute_lists, doc_ctx)
    };
    let deprecation_message = depr_info.message;
    let constant_deprecated_replacement = depr_info.replacement;
    let const_see_refs = const_docblock_info
        .as_ref()
        .map(docblock::extract_see_references_from_info)
        .unwrap_or_default();
    let const_description = const_docblock_info
        .as_ref()
        .and_then(crate::hover::extract_description_from_info);
    for item in constant.items.iter() {
        let value = doc_ctx.and_then(|ctx| {
            let start = item.value.span().start.offset as usize;
            let end = item.value.span().end.offset as usize;
            ctx.content.get(start..end).map(|s| s.to_string())
        });
        constants.push(ConstantInfo {
            name: atom_bytes(item.name.value),
            name_offset: item.name.span.start.offset,
            type_hint: type_hint.clone(),
            visibility,
            deprecation_message: deprecation_message.clone(),
            deprecated_replacement: constant_deprecated_replacement.clone(),
            see_refs: const_see_refs.clone(),
            description: const_description.clone(),
            is_enum_case: false,
            enum_value: None,
            value,
            is_virtual: false,
        });
    }
}

/// The [`ConstantInfo`] of an enum `case`.
fn extract_enum_case(
    member: &ClassLikeMember<'_>,
    enum_case: &EnumCase<'_>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> ConstantInfo {
    let case_docblock_text = doc_ctx
        .and_then(|ctx| docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, member));
    let case_docblock_info = case_docblock_text.and_then(docblock::parse_docblock_for_tags);
    let depr_info = {
        let docblock_msg = case_docblock_info
            .as_ref()
            .and_then(docblock::extract_deprecation_message_from_info);
        merge_deprecation_info(docblock_msg, &enum_case.attribute_lists, doc_ctx)
    };
    let enum_value = if let EnumCaseItem::Backed(backed) = &enum_case.item {
        let start = backed.value.span().start.offset as usize;
        let end = backed.value.span().end.offset as usize;
        doc_ctx
            .and_then(|ctx| ctx.content.get(start..end))
            .map(|s| s.to_string())
    } else {
        None
    };
    ConstantInfo {
        name: atom_bytes(enum_case.item.name().value),
        name_offset: enum_case.item.name().span.start.offset,
        type_hint: None,
        visibility: Visibility::Public,
        deprecation_message: depr_info.message,
        deprecated_replacement: depr_info.replacement,
        see_refs: Vec::new(),
        description: None,
        is_enum_case: true,
        enum_value,
        value: None,
        is_virtual: false,
    }
}

/// Record a trait `use`: the traits it names, the `@use` generics of its
/// docblock, and the `insteadof` and `as` rules of its adaptation block.
fn extract_trait_use(
    trait_use: &TraitUse<'_>,
    doc_ctx: Option<&DocblockCtx<'_>>,
    extracted: &mut ExtractedMembers,
) {
    for trait_name_ident in trait_use.trait_names.iter() {
        extracted
            .used_traits
            .push(atom_bytes(trait_name_ident.value()));
    }

    // Extract `@use` generics from the docblock on the trait `use`
    // statement itself.  In Laravel, the Eloquent Builder declares:
    //
    //   /** @use BuildsQueries<TModel> */
    //   use BuildsQueries;
    //
    // This binds the trait's template parameter to the class's own
    // template parameter.
    if let Some(ctx) = doc_ctx
        && let Some(doc_text) =
            docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, trait_use)
    {
        let tags = docblock::extract_generics_tag(doc_text, "@use");
        extracted
            .inline_use_generics
            .extend(tags.into_iter().map(|(n, args)| (atom(&n), args)));
    }

    // Parse trait adaptation block (`{ ... }`) if present.  This handles
    // `insteadof` (precedence) and `as` (alias) declarations for resolving
    // trait method conflicts.
    let TraitUseSpecification::Concrete(spec) = &trait_use.specification else {
        return;
    };
    for adaptation in spec.adaptations.iter() {
        match adaptation {
            TraitUseAdaptation::Precedence(prec) => {
                let insteadof: Vec<Atom> = prec
                    .trait_names
                    .iter()
                    .map(|id| atom_bytes(id.value()))
                    .collect();
                extracted.trait_precedences.push(TraitPrecedence {
                    trait_name: atom_bytes(prec.method_reference.trait_name.value()),
                    method_name: atom_bytes(prec.method_reference.method_name.value),
                    insteadof,
                });
            }
            TraitUseAdaptation::Alias(alias_adapt) => {
                let (trait_name, method_name) = match &alias_adapt.method_reference {
                    TraitUseMethodReference::Identifier(ident) => (None, atom_bytes(ident.value)),
                    TraitUseMethodReference::Absolute(abs) => (
                        Some(atom_bytes(abs.trait_name.value())),
                        atom_bytes(abs.method_name.value),
                    ),
                };
                let visibility = alias_adapt.modifier.as_ref().map(|m| {
                    if m.is_private() {
                        Visibility::Private
                    } else if m.is_protected() {
                        Visibility::Protected
                    } else {
                        Visibility::Public
                    }
                });
                extracted.trait_aliases.push(TraitAlias {
                    trait_name,
                    method_name,
                    alias: alias_adapt.alias.as_ref().map(|a| atom_bytes(a.value)),
                    visibility,
                });
            }
        }
    }
}
