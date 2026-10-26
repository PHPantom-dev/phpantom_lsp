//! Declared properties, and the types an untyped property takes from the
//! `$this->prop = …` assignments in the class's methods.

use mago_span::HasSpan;
use mago_syntax::cst::attribute::AttributeList;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::class_like::property::{Property, PropertyItem};
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::*;

use crate::atom::{atom, bytes_to_str};
use crate::docblock;
use crate::parser::{
    DeprecationInfo, DocblockCtx, extract_hint_type, extract_language_level_type,
    extract_visibility, merge_deprecation_info,
};
use crate::php_type::PhpType;
use crate::types::*;

use super::resolve_written_class;

/// The [`PropertyInfo`]s a property declaration declares, with its
/// `#[LanguageLevelTypeAware]` type, its docblock's `@var`, `@deprecated`,
/// `@see` and description, and its `#[Deprecated]` attribute applied.
pub(super) fn extract_property(
    member: &ClassLikeMember<'_>,
    property: &Property<'_>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Vec<PropertyInfo> {
    let mut prop_infos = extract_property_info(property, doc_ctx.map(|c| c.content));

    // The attribute lists of the property variant, for the
    // `#[Deprecated]` check below.
    let prop_attr_lists: &Sequence<'_, AttributeList<'_>> = match property {
        Property::Plain(p) => &p.attribute_lists,
        Property::Hooked(h) => &h.attribute_lists,
    };

    // Apply #[LanguageLevelTypeAware] override to property types.  When
    // present, the attribute's version-appropriate type string replaces
    // the native type hint.
    if let Some(ctx) = doc_ctx
        && let Some(ver) = ctx.php_version
        && let Some(override_type) = extract_language_level_type(prop_attr_lists, ctx, ver)
    {
        for prop in &mut prop_infos {
            prop.type_hint = Some(override_type.clone());
            prop.native_type_hint = Some(override_type.clone());
        }
    }

    // Apply PHPDoc `@var` override, `@deprecated`, `@see`, and description
    // for each property.
    if let Some(ctx) = doc_ctx
        && let Some(doc_text) =
            docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, member)
    {
        let info = docblock::parse_docblock_for_tags(doc_text);
        let docblock_msg = info
            .as_ref()
            .and_then(docblock::extract_deprecation_message_from_info);
        let see_refs = info
            .as_ref()
            .map(docblock::extract_see_refs_from_info)
            .unwrap_or_default();
        if !see_refs.is_empty() {
            for prop in &mut prop_infos {
                prop.see_refs = see_refs.clone();
            }
        }
        // Use merge_deprecation_info for version-aware suppression and
        // replacement extraction.
        apply_deprecation(
            &mut prop_infos,
            merge_deprecation_info(docblock_msg, prop_attr_lists, doc_ctx),
        );
        if let Some(parsed_doc) = info.as_ref().and_then(docblock::extract_var_type_from_info) {
            for prop in &mut prop_infos {
                let effective = docblock::resolve_effective_type_typed(
                    prop.type_hint.as_ref(),
                    Some(&parsed_doc),
                );
                prop.type_hint = effective;
            }
        }
        let description = info
            .as_ref()
            .and_then(crate::hover::extract_description_from_info)
            .or_else(|| {
                info.as_ref()
                    .and_then(crate::hover::extract_var_description_from_info)
            });
        if description.is_some() {
            for prop in &mut prop_infos {
                prop.description = description.clone();
            }
        }
    }

    // If no deprecation was found from the docblock, check the
    // #[Deprecated] attribute.  This covers properties that have an
    // attribute but no docblock at all.
    if prop_infos.iter().all(|p| p.deprecation_message.is_none())
        && let Some(ctx) = doc_ctx
    {
        apply_deprecation(
            &mut prop_infos,
            merge_deprecation_info(None, prop_attr_lists, Some(ctx)),
        );
    }

    prop_infos
}

/// Mark every property of one declaration deprecated by `depr_info`.
fn apply_deprecation(prop_infos: &mut [PropertyInfo], depr_info: DeprecationInfo) {
    if let Some(ref msg) = depr_info.message {
        for prop in prop_infos.iter_mut() {
            prop.deprecation_message = Some(msg.clone());
        }
    }
    if let Some(ref repl) = depr_info.replacement {
        for prop in prop_infos.iter_mut() {
            prop.deprecated_replacement = Some(repl.clone());
        }
    }
}

/// The [`PropertyInfo`] of each variable a property declaration declares,
/// typed by its native hint.
fn extract_property_info(property: &Property, content: Option<&str>) -> Vec<PropertyInfo> {
    let is_static = property.modifiers().iter().any(|m| m.is_static());
    let is_readonly = property.modifiers().iter().any(|m| m.is_readonly());
    let visibility = extract_visibility(property.modifiers().iter());

    let native_hint = property.hint().map(|h| extract_hint_type(h));

    let mut props = Vec::with_capacity(property.variables().len());
    let mut push_item = |item: &PropertyItem| {
        let default_value = match item {
            PropertyItem::Concrete(c) => content.and_then(|src| {
                let start = c.value.span().start.offset as usize;
                let end = c.value.span().end.offset as usize;
                src.get(start..end).map(Box::from)
            }),
            PropertyItem::Abstract(_) => None,
        };

        let var = item.variable();
        let raw_name = bytes_to_str(var.name).to_string();
        let name = if let Some(stripped) = raw_name.strip_prefix('$') {
            atom(stripped)
        } else {
            atom(&raw_name)
        };

        props.push(PropertyInfo {
            name,
            name_offset: var.span.start.offset,
            type_hint: native_hint.clone(),
            native_type_hint: native_hint.clone(),
            description: None,
            is_static,
            is_readonly,
            visibility,
            deprecation_message: None,
            deprecated_replacement: None,
            see_refs: Vec::new(),
            is_virtual: false,
            source: default_value.map(|value| PropertySource::DeclaredDefault { value }),
        });
    };

    match property {
        Property::Plain(plain) => {
            for item in &plain.items {
                push_item(item);
            }
        }
        Property::Hooked(hooked) => push_item(&hooked.item),
    }

    props
}

/// Infer types for untyped properties from `$this->prop = ...`
/// assignments in method bodies (constructors and setters alike), the way
/// PHPStan and Psalm do.  Two RHS shapes are recognized: `new ClassName()`
/// (resolved to an FQN eagerly so downstream code does not need
/// short-name resolution logic) and a parameter of the enclosing method
/// whose type is known.  Only applies when neither a native type hint nor
/// a docblock type is present on the property; assignments to the same
/// property in different methods union their types.
pub(super) fn infer_assigned_property_types(
    properties: &mut [PropertyInfo],
    methods: &[MethodInfo],
    method_bodies: &[(usize, &MethodBody<'_>)],
    doc_ctx: Option<&DocblockCtx<'_>>,
) {
    let untyped_property = |properties: &[PropertyInfo], name: &str| {
        properties
            .iter()
            .any(|p| p.name == name && p.type_hint.is_none() && p.native_type_hint.is_none())
    };
    if !properties
        .iter()
        .any(|p| p.type_hint.is_none() && p.native_type_hint.is_none())
    {
        return;
    }
    let mut inferred: Vec<(String, Vec<PhpType>)> = Vec::new();
    for &(method_idx, body) in method_bodies {
        let MethodBody::Concrete(concrete) = body else {
            continue;
        };
        for (stmt_idx, stmt) in concrete.statements.iter().enumerate() {
            let Statement::Expression(expr_stmt) = stmt else {
                continue;
            };
            let Expression::Assignment(assign) = expr_stmt.expression else {
                continue;
            };
            if !matches!(
                assign.operator,
                mago_syntax::cst::assignment::AssignmentOperator::Assign(_)
            ) {
                continue;
            }
            let Expression::Access(Access::Property(pa)) = assign.lhs else {
                continue;
            };
            let Expression::Variable(Variable::Direct(dv)) = pa.object else {
                continue;
            };
            if dv.name != b"$this" {
                continue;
            }
            let ClassLikeMemberSelector::Identifier(ident) = &pa.property else {
                continue;
            };
            let prop_name = bytes_to_str(ident.value);
            if !untyped_property(properties, prop_name) {
                continue;
            }

            let assigned_type = match assign.rhs {
                Expression::Instantiation(inst) => {
                    if let Expression::Identifier(class_ident) = inst.class {
                        let fqn = resolve_written_class(bytes_to_str(class_ident.value()), doc_ctx);
                        Some(PhpType::named(atom(&fqn)))
                    } else {
                        None
                    }
                }
                Expression::Variable(Variable::Direct(rhs_var)) => {
                    let var_name = bytes_to_str(rhs_var.name);
                    // A parameter reassigned earlier in the body no longer
                    // holds its declared type — skip it.
                    let reassigned = concrete.statements.iter().take(stmt_idx).any(|prior| {
                        if let Statement::Expression(prior_stmt) = prior
                            && let Expression::Assignment(prior_assign) = prior_stmt.expression
                            && let Expression::Variable(Variable::Direct(lhs)) = prior_assign.lhs
                        {
                            bytes_to_str(lhs.name) == var_name
                        } else {
                            false
                        }
                    });
                    if reassigned {
                        None
                    } else {
                        methods[method_idx]
                            .parameters
                            .iter()
                            .find(|p| p.name == var_name)
                            .and_then(|p| p.type_hint.clone())
                    }
                }
                // The `[]` a collection starts out as is kept as the empty
                // shape until every assignment is in: see below.
                Expression::Array(array) => Some(if array.elements.is_empty() {
                    PhpType::array_shape(Vec::new())
                } else {
                    PhpType::array()
                }),
                Expression::LegacyArray(array) => Some(if array.elements.is_empty() {
                    PhpType::array_shape(Vec::new())
                } else {
                    PhpType::array()
                }),
                _ => None,
            };

            if let Some(ty) = assigned_type {
                match inferred.iter_mut().find(|(name, _)| name == prop_name) {
                    Some((_, types)) => {
                        if !types.contains(&ty) {
                            types.push(ty);
                        }
                    }
                    None => inferred.push((prop_name.to_string(), vec![ty])),
                }
            }
        }
    }
    for (prop_name, mut types) in inferred {
        // An empty array is a value of every array type, so a typed array
        // assigned elsewhere already covers the `[]` the property starts
        // out as.  On its own it stands for any array, since the property
        // is filled in later.
        let empty = PhpType::array_shape(Vec::new());
        if types.contains(&empty) {
            let covered = types.iter().any(|t| *t != empty && t.is_array_like());
            types.retain(|t| *t != empty);
            if !covered {
                types.push(PhpType::array());
            }
        }
        if let Some(prop) = properties
            .iter_mut()
            .find(|p| p.name == prop_name && p.type_hint.is_none() && p.native_type_hint.is_none())
        {
            prop.type_hint = Some(if types.len() == 1 {
                types.pop().expect("checked len == 1")
            } else {
                PhpType::union(types)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Backend;
    use crate::atom::atom;
    use crate::types::ClassInfo;

    fn property_type(classes: &[(ClassInfo, Option<String>)], class: &str, prop: &str) -> String {
        classes
            .iter()
            .find(|(c, _)| c.name == atom(class))
            .unwrap_or_else(|| panic!("class {class} not found"))
            .0
            .properties
            .iter()
            .find(|p| p.name == atom(prop))
            .unwrap_or_else(|| panic!("property {prop} not found on {class}"))
            .type_hint
            .as_ref()
            .map(|t| t.to_string())
            .unwrap_or_else(|| "<none>".to_string())
    }

    /// An untyped property assigned a typed parameter in the constructor
    /// or a setter picks up the parameter's type.
    #[test]
    fn untyped_property_infers_type_from_assigned_parameter() {
        let src = r#"<?php
namespace App;
use Psy\Context;
class CtorAssigned {
    private $ctx;
    public function __construct(Context $ctx) {
        $this->ctx = $ctx;
    }
}
class SetterAssigned {
    protected $ctx;
    /** @param list<Context> $items */
    public function setItems(array $items) {
        $this->items = $items;
    }
    public function setContext(Context $ctx) {
        $this->ctx = $ctx;
    }
    private $items;
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        assert_eq!(property_type(&classes, "CtorAssigned", "ctx"), "Context");
        assert_eq!(property_type(&classes, "SetterAssigned", "ctx"), "Context");
        // Docblock @param types win over the native hint, and a property
        // declared after the setter is still matched.
        assert_eq!(
            property_type(&classes, "SetterAssigned", "items"),
            "list<Context>"
        );
    }

    /// Assignments of different types in different methods union; a
    /// parameter reassigned before the property write contributes nothing.
    #[test]
    fn untyped_property_inference_unions_and_skips_reassigned_params() {
        let src = r#"<?php
class Holder {
    private $subject;
    private $tainted;
    public function setUser(User $u) {
        $this->subject = $u;
    }
    public function setPost(Post $p) {
        $this->subject = $p;
    }
    public function setTainted(User $u) {
        $u = 5;
        $this->tainted = $u;
    }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        assert_eq!(property_type(&classes, "Holder", "subject"), "User|Post");
        assert_eq!(property_type(&classes, "Holder", "tainted"), "<none>");
    }

    /// Declared types are never overridden, and only plain `=` infers.
    #[test]
    fn untyped_property_inference_respects_declared_types_and_operators() {
        let src = r#"<?php
class Holder {
    private User $typed;
    /** @var Widget */
    private $doc;
    private $joined;
    public function setAll(Post $p, Post $q, string $s) {
        $this->typed = $p;
        $this->doc = $q;
        $this->joined .= $s;
    }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        assert_eq!(property_type(&classes, "Holder", "typed"), "User");
        assert_eq!(property_type(&classes, "Holder", "doc"), "Widget");
        assert_eq!(property_type(&classes, "Holder", "joined"), "<none>");
    }

    /// An array literal makes an untyped property an `array`, and the `[]`
    /// it starts out as gives way to a typed array assigned elsewhere.
    #[test]
    fn untyped_property_infers_array_from_array_literal() {
        let src = r#"<?php
class Holder {
    private $plain;
    private $filled;
    private $legacy;
    public function __construct() {
        $this->plain = [];
        $this->filled = [];
        $this->legacy = array(1, 2);
    }
    /** @param list<User> $users */
    public function setFilled(array $users) {
        $this->filled = $users;
    }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        assert_eq!(property_type(&classes, "Holder", "plain"), "array");
        assert_eq!(property_type(&classes, "Holder", "filled"), "list<User>");
        assert_eq!(property_type(&classes, "Holder", "legacy"), "array");
    }

    /// `$this->prop = new ClassName()` infers in any method, not just the
    /// constructor, and resolves the name through the use-map.
    #[test]
    fn untyped_property_infers_type_from_instantiation_in_setter() {
        let src = r#"<?php
namespace App;
use Lib\Logger;
class Service {
    private $logger;
    public function init() {
        $this->logger = new Logger();
    }
}
"#;
        let classes = Backend::parse_php_versioned_with_namespaces(src, None);
        assert_eq!(property_type(&classes, "Service", "logger"), "Lib\\Logger");
    }
}
