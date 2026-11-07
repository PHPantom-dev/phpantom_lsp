//! The members and arguments Yii DI definitions name, and the classes they
//! bind (see [`crate::yii::definitions`]).
//!
//! A definition (`yiisoft/definitions`) is an array that names the class it
//! builds with a `class` entry, or is keyed by one in the array around it
//! (`MailerInterface::class => [...]`), and calls the methods and sets the
//! properties its `'name()'` and `'$name'` keys name.  It is recognised by
//! that shape wherever it is written: a `di` config file, the `widgets`
//! group, a service provider's `getDefinitions()`, or an argument of
//! another definition.
//!
//! A `'name()'` or `'$name'` key is a [`SymbolKind::MemberAccess`] on the
//! class, so it navigates, renames, and is checked like any other access.
//! The arguments a method key passes by name are
//! [`StringKeyKind::YiiDefinitionArgument`] keys, and the classes the
//! definitions bind and build are kept beside the spans as
//! [`YiiDefinitionSite`]s.

use mago_span::HasSpan;
use mago_syntax::cst::sequence::TokenSeparatedSequence;

use super::*;
use crate::symbol_map::{StringKeyKind, YiiDefinitionSite};
use crate::virtual_members::laravel::helpers::resolved_string_literal;
use crate::yii::definitions::{definition_argument_key, definition_key};

/// The class a definition builds, as the file names it.
struct DefinedClass {
    /// The class as written, for the member accesses to resolve.
    subject: SubjectText,
    /// The class, fully qualified, when the file says which one it is.
    fqn: Option<String>,
}

/// Read the array `elements` for the definitions it holds: itself, when it
/// has a `class` entry, and each entry keyed by a class whose value is one.
///
/// A definition nested in this one's values is an array of its own, which
/// the walk reads when it gets there.
pub(in crate::symbol_map::extraction) fn emit_yii_definition_spans(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.yii {
        return;
    }
    if let Some(class) = class_entry(elements, ctx) {
        emit_definition_keys(elements, &class, ctx);
    }
    for element in elements.iter() {
        let ArrayElement::KeyValue(entry) = element else {
            continue;
        };
        let Some(id) = defined_class(entry.key, ctx) else {
            continue;
        };
        let binds = match entry.value {
            Expression::Array(array) => definition_array(&array.elements, &id, ctx),
            Expression::LegacyArray(array) => definition_array(&array.elements, &id, ctx),
            value => is_definition_value(value),
        };
        if binds {
            record_definition_site(entry.key, &id, ctx);
        }
    }
}

/// Read the array a class-string key is given, when it is a definition of
/// that class, and say whether it is.
///
/// One with a `class` entry of its own builds that class instead (and its
/// keys are read with it); one with neither that nor a `'name()'` or
/// `'$name'` key is something else keyed by classes, such as the event
/// listeners of the `events` group.
fn definition_array(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    id: &DefinedClass,
    ctx: &mut ExtractionCtx<'_>,
) -> bool {
    if class_entry_value(elements).is_some() {
        return true;
    }
    if !elements
        .iter()
        .any(|element| definition_key_name(element, ctx.content).is_some())
    {
        return false;
    }
    emit_definition_keys(elements, id, ctx);
    true
}

/// Whether `value`, given to a class-string key, is a definition other than
/// an array: the class to build, a reference to another entry, or a
/// closure that builds the object.
fn is_definition_value(value: &Expression<'_>) -> bool {
    match value {
        Expression::Closure(_) | Expression::ArrowFunction(_) => true,
        value if crate::parser::class_fetch_target(value).is_some() => true,
        Expression::Call(Call::StaticMethod(call)) => {
            let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                return false;
            };
            let class = match call.class {
                Expression::Identifier(identifier) => bytes_to_str(identifier.value()),
                _ => return false,
            };
            let class = class.rsplit('\\').next().unwrap_or(class);
            bytes_to_str(method.value).eq_ignore_ascii_case("to")
                && matches!(class, "Reference" | "DynamicReference")
        }
        _ => false,
    }
}

/// The value of the `class` entry of a definition array.
fn class_entry_value<'e, 'a>(
    elements: &'e TokenSeparatedSequence<'a, ArrayElement<'a>>,
) -> Option<&'e Expression<'a>> {
    elements.iter().find_map(|element| {
        let ArrayElement::KeyValue(entry) = element else {
            return None;
        };
        let Expression::Literal(Literal::String(key)) = entry.key else {
            return None;
        };
        (key.value.and_then(crate::atom::literal_bytes_to_str) == Some("class"))
            .then_some(entry.value)
    })
}

/// The class a definition array's `class` entry builds, recorded as a site
/// of the class.
fn class_entry(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) -> Option<DefinedClass> {
    let value = class_entry_value(elements)?;
    let class = defined_class(value, ctx)?;
    record_definition_site(value, &class, ctx);
    Some(class)
}

/// The class a `Foo::class` (or `self::class`) expression names.
fn defined_class(expr: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<DefinedClass> {
    let target = crate::parser::class_fetch_target(expr)?;
    let fqn = match target {
        Expression::Identifier(identifier) => {
            let written = bytes_to_str(identifier.value());
            Some(
                ctx.resolved_name_at(identifier.span().start.offset)
                    .unwrap_or(written)
                    .trim_start_matches('\\')
                    .to_string(),
            )
        }
        Expression::Self_(_) | Expression::Static(_) => ctx
            .class_name_offset
            .and_then(|offset| ctx.resolved_name_at(offset))
            .map(|fqn| fqn.trim_start_matches('\\').to_string()),
        _ => return None,
    };
    let text = expr_to_subject_text(target);
    if text.is_empty() {
        return None;
    }
    let span = target.span();
    Some(DefinedClass {
        subject: SubjectText::new(text, span.start.offset, span.end.offset, ctx.content),
        fqn,
    })
}

/// Record where a definition names `class`, by the class name's own range.
fn record_definition_site(
    expr: &Expression<'_>,
    class: &DefinedClass,
    ctx: &mut ExtractionCtx<'_>,
) {
    let (Some(fqn), Some(Expression::Identifier(identifier))) =
        (&class.fqn, crate::parser::class_fetch_target(expr))
    else {
        return;
    };
    let span = identifier.span();
    ctx.yii_definitions.push(YiiDefinitionSite {
        start: span.start.offset,
        end: span.end.offset,
        class: crate::atom::atom(fqn),
    });
}

/// What a definition key names: a method (`'setLogger()'`) or a property
/// (`'$pageSize'`), with the byte range of the name in the key.
enum DefinitionKey<'a> {
    Method(&'a str, u32, u32),
    Property(&'a str, u32, u32),
}

/// The member a definition array's `element` names by its key.
fn definition_key_name<'a>(element: &ArrayElement<'a>, content: &str) -> Option<DefinitionKey<'a>> {
    let ArrayElement::KeyValue(entry) = element else {
        return None;
    };
    let (key, start, end) = resolved_string_literal(entry.key, content)?;
    let (start, end) = (start as u32, end as u32);
    match definition_key(key)? {
        (name, true) => Some(DefinitionKey::Method(
            name,
            start,
            start + name.len() as u32,
        )),
        (name, false) => Some(DefinitionKey::Property(name, start + 1, end)),
    }
}

/// Emit the members the keys of a definition of `class` name, and the
/// arguments its method keys pass by name.
fn emit_definition_keys(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    class: &DefinedClass,
    ctx: &mut ExtractionCtx<'_>,
) {
    for element in elements.iter() {
        let Some(key) = definition_key_name(element, ctx.content) else {
            continue;
        };
        let (name, start, end, is_method) = match key {
            DefinitionKey::Method(name, start, end) => (name, start, end, true),
            DefinitionKey::Property(name, start, end) => (name, start, end, false),
        };
        // The constructor is not a member the class need declare: one
        // without passes its arguments to nothing.  Its arguments are still
        // named after its parameters.
        if !(is_method && name.eq_ignore_ascii_case("__construct")) {
            ctx.spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::MemberAccess {
                    subject_text: class.subject.clone(),
                    member_name: crate::atom::atom(name),
                    is_static: true,
                    is_method_call: is_method,
                    docblock_ref: DocblockMemberRef::YiiDefinition,
                    is_array_callable: false,
                    is_nullsafe: false,
                },
            });
        }
        if is_method
            && let Some(fqn) = &class.fqn
            && let ArrayElement::KeyValue(entry) = element
        {
            emit_argument_keys(entry.value, fqn, name, ctx);
        }
    }
}

/// Emit the arguments the array a method key is given passes by name.
fn emit_argument_keys(
    value: &Expression<'_>,
    class: &str,
    method: &str,
    ctx: &mut ExtractionCtx<'_>,
) {
    let elements = match value {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return,
    };
    for element in elements.iter() {
        let ArrayElement::KeyValue(entry) = element else {
            continue;
        };
        let Some((name, start, end)) = resolved_string_literal(entry.key, ctx.content) else {
            continue;
        };
        if !is_identifier(name) {
            continue;
        }
        ctx.spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::YiiDefinitionArgument,
                key: definition_argument_key(class, method, name),
                is_write: false,
                is_optional: false,
            },
        });
    }
}
