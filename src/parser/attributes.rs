//! Extraction of the native PHP `#[Attribute]` target bitmask, and of the
//! Doctrine ORM mapping a class's attributes declare.
//!
//! This is unrelated to Laravel's model attributes (`#[Connection]`,
//! `#[Table]`, etc., handled in
//! [`crate::virtual_members::laravel::model_extraction`]) — it is PHP's own
//! `Attribute::TARGET_*` mechanism for restricting where a user-defined
//! attribute class may be applied.

use mago_span::HasSpan;
use mago_syntax::cst::argument::PartialArgument;
use mago_syntax::cst::attribute::{Attribute, AttributeList};
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::property::Property;
use mago_syntax::cst::sequence::Sequence;

use crate::atom::{Atom, atom, bytes_to_str, last_segment};
use crate::types::{DoctrineMapping, attribute_target};

use super::DocblockCtx;

/// Doctrine ORM's entity mapping attribute.
const DOCTRINE_ENTITY: &str = "Doctrine\\ORM\\Mapping\\Entity";

/// Doctrine ORM's mapping namespace, which holds the property attributes
/// below.
const DOCTRINE_MAPPING_NS: &str = "Doctrine\\ORM\\Mapping\\";

/// The property attributes that map a field, an identifier, or an
/// association: the names a repository's criteria and its magic
/// `findBy<Field>()` methods take.
const DOCTRINE_FIELD_ATTRIBUTES: &[&str] = &[
    "Column",
    "Id",
    "ManyToOne",
    "OneToOne",
    "OneToMany",
    "ManyToMany",
];

/// What a class's Doctrine attributes map: whether it is an entity, the
/// repository its `#[ORM\Entity]` names, and the properties its field,
/// identifier, and association attributes map.  `None` when there is none
/// of that, which is every class outside a Doctrine project.
///
/// The properties are the class's own, promoted constructor parameters
/// included; the ones it inherits or takes from traits are read from those
/// classes' own mappings.
pub(super) fn extract_doctrine_mapping<'a>(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    ctx: Option<&DocblockCtx<'_>>,
) -> Option<Box<DoctrineMapping>> {
    let entity = find_attribute(attribute_lists, ctx, DOCTRINE_ENTITY);
    let mut fields = Vec::new();
    let mut associations = Vec::new();
    let mut map = |lists: &Sequence<'_, AttributeList<'_>>, name: Atom| {
        let Some(attr) = field_attribute(lists, ctx) else {
            return;
        };
        if let Some(target) = association_target(attr, ctx) {
            associations.push((name, target));
        }
        fields.push(name);
    };
    for member in members {
        match member {
            ClassLikeMember::Property(property) => {
                let lists = match property {
                    Property::Plain(p) => &p.attribute_lists,
                    Property::Hooked(h) => &h.attribute_lists,
                };
                for variable in property.variables() {
                    map(lists, property_name(variable.name));
                }
            }
            ClassLikeMember::Method(method)
                if method.name.value.eq_ignore_ascii_case(b"__construct") =>
            {
                for param in method.parameter_list.parameters.iter() {
                    if param.is_promoted_property() {
                        map(&param.attribute_lists, property_name(param.variable.name));
                    }
                }
            }
            _ => {}
        }
    }
    if entity.is_none() && fields.is_empty() {
        return None;
    }
    Some(Box::new(DoctrineMapping {
        is_entity: entity.is_some(),
        repository: entity.and_then(|attr| repository_class(attr, ctx)),
        fields,
        associations,
        managed_entity: None,
    }))
}

fn property_name(variable: &[u8]) -> Atom {
    let name = bytes_to_str(variable);
    atom(name.strip_prefix('$').unwrap_or(name))
}

/// The attribute among `attribute_lists` that maps its property as a field,
/// an identifier, or an association, preferring an association's.  The
/// short name is checked first, so a property with unrelated attributes
/// resolves none of their names.
fn field_attribute<'l, 'a>(
    attribute_lists: &'l Sequence<'a, AttributeList<'a>>,
    ctx: Option<&DocblockCtx<'_>>,
) -> Option<&'l Attribute<'a>> {
    let mut found = None;
    for attr in attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
    {
        let short = bytes_to_str(last_segment(attr.name.value()));
        if DOCTRINE_FIELD_ATTRIBUTES.contains(&short)
            && attribute_fqn(attr, ctx)
                .strip_prefix(DOCTRINE_MAPPING_NS)
                .is_some_and(|rest| rest.eq_ignore_ascii_case(short))
        {
            if association_kind(attr).is_some() {
                return Some(attr);
            }
            found = Some(attr);
        }
    }
    found
}

/// The association attribute `attr` is, by its short name.
fn association_kind(attr: &Attribute<'_>) -> Option<&'static str> {
    let short = bytes_to_str(last_segment(attr.name.value()));
    ["ManyToOne", "OneToOne", "OneToMany", "ManyToMany"]
        .into_iter()
        .find(|kind| kind.eq_ignore_ascii_case(short))
}

/// For an association attribute, the entity its `targetEntity` names, in
/// the form [`repository_class`] gives a class name; `Some(None)` when it
/// names none.  `None` for an attribute that maps no association.
///
/// `targetEntity` is the first parameter of every association attribute
/// but `OneToMany`, whose first was `mappedBy` before ORM 3, so a
/// positional first argument counts there only as an `X::class`, which no
/// property name is.
fn association_target(attr: &Attribute<'_>, ctx: Option<&DocblockCtx<'_>>) -> Option<Option<Atom>> {
    let kind = association_kind(attr)?;
    let Some(arguments) = attr.argument_list.as_ref().map(|list| &list.arguments) else {
        return Some(None);
    };
    let value = arguments.iter().enumerate().find_map(|(i, arg)| match arg {
        PartialArgument::Positional(positional)
            if i == 0
                && (kind != "OneToMany"
                    || crate::parser::class_fetch_identifier(positional.value).is_some()) =>
        {
            Some(positional.value)
        }
        PartialArgument::Named(named) if named.name.value == b"targetEntity" => Some(named.value),
        _ => None,
    });
    Some(value.and_then(|value| class_name_argument(value, ctx, true)))
}

/// The attribute among `attribute_lists` whose name resolves to `fqn`.
fn find_attribute<'l, 'a>(
    attribute_lists: &'l Sequence<'a, AttributeList<'a>>,
    ctx: Option<&DocblockCtx<'_>>,
    fqn: &str,
) -> Option<&'l Attribute<'a>> {
    attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
        .find(|attr| attribute_fqn(attr, ctx).eq_ignore_ascii_case(fqn))
}

/// An attribute's name, read through the file's imports and namespace;
/// without a [`DocblockCtx`] the name as written is taken to be fully
/// qualified.
fn attribute_fqn(attr: &Attribute<'_>, ctx: Option<&DocblockCtx<'_>>) -> String {
    let raw = bytes_to_str(attr.name.value());
    match ctx {
        Some(ctx) => crate::names::resolve_to_fqn(raw, &ctx.use_map, &ctx.namespace),
        None => raw.strip_prefix('\\').unwrap_or(raw).to_string(),
    }
}

/// The repository class `#[ORM\Entity(repositoryClass: ...)]` names, in
/// the form name resolution qualifies along with the class's other
/// references: an `X::class` as written, and a string, which PHP never
/// reads through the file's imports, as a fully-qualified `\X`.
fn repository_class(attr: &Attribute<'_>, ctx: Option<&DocblockCtx<'_>>) -> Option<Atom> {
    // `repositoryClass` is the attribute constructor's first parameter.
    let arguments = &attr.argument_list.as_ref()?.arguments;
    let value = match arguments.first()? {
        PartialArgument::Positional(positional) => positional.value,
        PartialArgument::Named(_) => arguments.iter().find_map(|arg| match arg {
            PartialArgument::Named(named) if named.name.value == b"repositoryClass" => {
                Some(named.value)
            }
            _ => None,
        })?,
        _ => return None,
    };
    class_name_argument(value, ctx, false)
}

/// The class an attribute argument names: an `X::class` as written, and a
/// string, which PHP never reads through the file's imports, as a
/// fully-qualified `\X`.  With `in_namespace`, a string without a `\` names
/// a class of the file's namespace, as Doctrine reads an association's
/// target.
fn class_name_argument(
    value: &mago_syntax::cst::Expression<'_>,
    ctx: Option<&DocblockCtx<'_>>,
    in_namespace: bool,
) -> Option<Atom> {
    if let Some(name) = crate::parser::class_fetch_identifier(value) {
        return Some(atom(bytes_to_str(name.value())));
    }
    let ctx = ctx?;
    let span = value.span();
    let text = ctx
        .content
        .get(span.start.offset as usize..span.end.offset as usize)?;
    let name = crate::util::unescape_php_string_literal(text.trim())?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if in_namespace
        && !name.contains('\\')
        && let Some(namespace) = ctx.namespace.as_deref().filter(|ns| !ns.is_empty())
    {
        return Some(atom(&format!("\\{namespace}\\{name}")));
    }
    Some(atom(&format!("\\{}", name.trim_start_matches('\\'))))
}

/// Extract the PHP attribute target bitmask from a class's attribute lists.
///
/// Scans for `#[\Attribute]` or `#[\Attribute(flags)]` and returns the
/// target bitmask.  Returns `0` when the class is not an attribute class.
///
/// Recognises these patterns:
/// - `#[Attribute]` / `#[\Attribute]` → `TARGET_ALL` (default)
/// - `#[Attribute(Attribute::TARGET_CLASS)]` → `TARGET_CLASS`
/// - `#[Attribute(Attribute::TARGET_CLASS | Attribute::TARGET_METHOD)]` → bitwise OR
/// - `#[Attribute(TARGET_CLASS | TARGET_METHOD)]` → short-form constants
/// - Numeric literals (e.g. `#[Attribute(1)]`, `#[Attribute(63)]`)
pub(super) fn extract_attribute_targets(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    content: &str,
) -> u8 {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let short = last_segment(attr.name.value());
            if short != b"Attribute" {
                continue;
            }

            // `#[\Attribute]` without arguments → TARGET_ALL.
            let Some(arg_list) = attr.argument_list.as_ref() else {
                return attribute_target::TARGET_ALL;
            };

            // No arguments inside parentheses → TARGET_ALL.
            let Some(first_arg) = arg_list.arguments.first() else {
                return attribute_target::TARGET_ALL;
            };

            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let Some(text) = content.get(start..end) else {
                return attribute_target::TARGET_ALL;
            };

            return parse_attribute_target_flags(text);
        }
    }

    0
}

/// Parse a target-flag expression from the argument to `#[\Attribute(…)]`.
///
/// Handles `|`-separated lists of `Attribute::TARGET_*` or bare
/// `TARGET_*` constants, as well as plain integer literals.
fn parse_attribute_target_flags(text: &str) -> u8 {
    let text = text.trim();

    // Try plain integer literal first.
    if let Ok(n) = text.parse::<u8>() {
        return n;
    }

    let mut flags: u8 = 0;
    for part in text.split('|') {
        let part = part.trim();
        // Strip optional `Attribute::`, `\Attribute::`, or `self::` prefix.
        let constant = part
            .strip_prefix("Attribute::")
            .or_else(|| part.strip_prefix("\\Attribute::"))
            .or_else(|| part.strip_prefix("self::"))
            .unwrap_or(part);

        flags |= match constant {
            "TARGET_CLASS" => attribute_target::TARGET_CLASS,
            "TARGET_FUNCTION" => attribute_target::TARGET_FUNCTION,
            "TARGET_METHOD" => attribute_target::TARGET_METHOD,
            "TARGET_PROPERTY" => attribute_target::TARGET_PROPERTY,
            "TARGET_CLASS_CONSTANT" => attribute_target::TARGET_CLASS_CONSTANT,
            "TARGET_PARAMETER" => attribute_target::TARGET_PARAMETER,
            "TARGET_ALL" => attribute_target::TARGET_ALL,
            _ => {
                // Unrecognised constant — try parsing as an integer.
                constant.trim().parse::<u8>().unwrap_or_default()
            }
        };
    }

    // If we matched the `#[Attribute]` name but couldn't parse any
    // flags, default to TARGET_ALL.
    if flags == 0 {
        attribute_target::TARGET_ALL
    } else {
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_class_qualified() {
        assert_eq!(
            parse_attribute_target_flags("\\Attribute::TARGET_CLASS"),
            attribute_target::TARGET_CLASS,
        );
    }

    #[test]
    fn parse_target_class_unqualified() {
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_CLASS"),
            attribute_target::TARGET_CLASS,
        );
    }

    #[test]
    fn parse_target_method_self() {
        assert_eq!(
            parse_attribute_target_flags("self::TARGET_METHOD"),
            attribute_target::TARGET_METHOD,
        );
    }

    #[test]
    fn parse_target_bare_constant() {
        assert_eq!(
            parse_attribute_target_flags("TARGET_PROPERTY"),
            attribute_target::TARGET_PROPERTY,
        );
    }

    #[test]
    fn parse_target_numeric_literal() {
        assert_eq!(parse_attribute_target_flags("1"), 1);
        assert_eq!(parse_attribute_target_flags("63"), 63);
    }

    #[test]
    fn parse_target_bitwise_or() {
        let expected = attribute_target::TARGET_CLASS | attribute_target::TARGET_METHOD;
        assert_eq!(
            parse_attribute_target_flags("\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD"),
            expected,
        );
    }

    #[test]
    fn parse_target_all() {
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_ALL"),
            attribute_target::TARGET_ALL,
        );
    }

    #[test]
    fn parse_target_unrecognised_defaults_to_all() {
        // Completely unrecognisable text falls back to TARGET_ALL
        // because the class IS marked with #[Attribute(...)].
        assert_eq!(
            parse_attribute_target_flags("SOME_UNKNOWN_CONST"),
            attribute_target::TARGET_ALL,
        );
    }

    #[test]
    fn parse_target_mixed_qualified_and_bare() {
        let expected = attribute_target::TARGET_FUNCTION | attribute_target::TARGET_PARAMETER;
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_FUNCTION | TARGET_PARAMETER"),
            expected,
        );
    }

    #[test]
    fn parse_target_whitespace_handling() {
        assert_eq!(
            parse_attribute_target_flags("  Attribute::TARGET_CLASS  "),
            attribute_target::TARGET_CLASS,
        );
    }
}
