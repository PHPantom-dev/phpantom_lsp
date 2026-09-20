//! Symbol spans for PHPUnit's code-coverage attributes.
//!
//! `#[CoversClass(Calculator::class)]` needs nothing from us: `Calculator`
//! is an ordinary class reference the expression extractor already sees.
//! What it cannot see is a coverage target PHPUnit spells as a *string* —
//! the method name in `#[CoversMethod(Calculator::class, 'add')]`, the
//! function name in `#[CoversFunction('helper')]`, and the class name when
//! it is written `#[CoversClass('App\Calculator')]` instead of with
//! `::class`.  This module turns those literals into navigable spans.
//!
//! The annotation form of the same metadata (`@covers`, `@uses`,
//! `@coversDefaultClass`) is handled in [`super::super::docblock`].

use mago_span::{HasSpan, Position, Span};
use mago_syntax::cst::argument::{PartialArgument, PartialArgumentList};

use super::*;

/// Namespace the coverage attributes live in.
const PHPUNIT_ATTR_NS: &str = "PHPUnit\\Framework\\Attributes\\";
/// What importing from that namespace looks like, for the short-name guard.
const PHPUNIT_ATTR_IMPORT: &str = "use PHPUnit\\Framework\\Attributes\\";

/// The kind of code unit an attribute names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    /// `#[CoversClass(Foo::class)]`, `#[CoversTrait(T::class)]`, and the
    /// `ClassesThat…` selectors — one class-like name.
    ClassLike,
    /// `#[CoversMethod(Foo::class, 'bar')]` — a class-like name followed by
    /// one of its members.
    Method,
    /// `#[CoversFunction('bar')]` — a global function.
    Function,
}

fn target_kind(short_name: &str) -> Option<TargetKind> {
    match short_name {
        "CoversClass"
        | "UsesClass"
        | "CoversTrait"
        | "UsesTrait"
        | "CoversClassesThatImplementInterface"
        | "UsesClassesThatImplementInterface"
        | "CoversClassesThatExtendClass"
        | "UsesClassesThatExtendClass" => Some(TargetKind::ClassLike),
        "CoversMethod" | "UsesMethod" => Some(TargetKind::Method),
        "CoversFunction" | "UsesFunction" => Some(TargetKind::Function),
        _ => None,
    }
}

/// Classify an attribute by name.
///
/// Attributes written out in full are matched against the PHPUnit namespace.
/// A short name additionally requires the file to import from that namespace,
/// so a project's own `CoversMethod` attribute is left alone.  The result of
/// that check is cached in `import_cache`, as the Laravel container
/// attributes do, to avoid rescanning the file per attribute.
fn coverage_attribute(
    class_name: &str,
    import_cache: &mut Option<bool>,
    content: &str,
) -> Option<TargetKind> {
    match class_name.rsplit_once('\\') {
        Some((namespace, short)) => {
            if !namespace.eq_ignore_ascii_case(PHPUNIT_ATTR_NS.trim_end_matches('\\')) {
                return None;
            }
            target_kind(short)
        }
        None => {
            let kind = target_kind(class_name)?;
            let imported =
                *import_cache.get_or_insert_with(|| content.contains(PHPUNIT_ATTR_IMPORT));
            imported.then_some(kind)
        }
    }
}

/// Emit the spans for coverage targets of `arg_list` that are written as
/// string literals, if `class_name` is one of PHPUnit's coverage attributes.
pub(super) fn try_emit_coverage_attribute_spans(
    class_name: &str,
    arg_list: &PartialArgumentList<'_>,
    import_cache: &mut Option<bool>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(kind) = coverage_attribute(class_name, import_cache, content) else {
        return;
    };

    let mut arguments = arg_list.arguments.iter();
    let Some(first) = arguments.next() else {
        return;
    };

    if kind == TargetKind::Function {
        // PHPUnit wants the fully-qualified function name, written without a
        // leading separator.
        if let Some((name, span)) = string_argument(first, content)
            && is_qualified_identifier(name)
        {
            spans.push(SymbolSpan {
                start: span.start.offset,
                end: span.end.offset,
                kind: SymbolKind::FunctionCall {
                    name: crate::atom::atom(name.trim_start_matches('\\')),
                    is_definition: false,
                    is_docblock_reference: false,
                },
            });
        }
        return;
    }

    // The class-like name is either `Foo::class` or a string holding its FQN.
    // Only the string form needs a class reference of its own: `Foo::class`
    // is an ordinary expression the extractor has already recorded.
    if let Some((name, span)) = string_argument(first, content) {
        if !is_qualified_identifier(name) {
            return;
        }
        spans.push(class_ref_span(
            span.start.offset,
            span.end.offset,
            // A coverage target is always fully qualified, whether or not
            // the string was written with a leading separator.
            &format!("\\{}", name.trim_start_matches('\\')),
        ));
    }

    // Mark the target as coverage metadata, for both spellings: the string
    // reference pushed just above, and the `Foo::class` one the expression
    // extractor recorded before this ran (`extract_from_attribute_lists`
    // walks the argument list first).
    retag_covers_target_in_range(spans, first.span());

    if kind == TargetKind::ClassLike {
        return;
    }

    let Some(member) = arguments.next() else {
        return;
    };
    let Some((member_name, member_span)) = string_argument(member, content) else {
        return;
    };
    if !is_identifier(member_name) {
        return;
    }
    let Some((subject_text, subject_span)) = class_like_argument(first, content) else {
        return;
    };

    spans.push(SymbolSpan {
        start: member_span.start.offset,
        end: member_span.end.offset,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::new(
                subject_text,
                subject_span.start.offset,
                subject_span.end.offset,
                content,
            ),
            member_name: crate::atom::atom(member_name),
            is_static: true,
            // Coverage metadata names a code unit rather than calling it, so
            // the relaxed lookup a docblock reference gets is the right one:
            // `#[CoversMethod]` is also how a property hook is targeted.
            is_method_call: false,
            docblock_ref: DocblockMemberRef::Coverage,
            is_array_callable: false,
            is_nullsafe: false,
        },
    });
}

/// Mark every class reference inside `range` as PHPUnit coverage metadata.
///
/// Scans the whole vec rather than just its tail: a coverage attribute's
/// argument spans are not necessarily the most recently pushed (a docblock on
/// the same declaration contributes spans of its own).  Coverage attributes
/// appear only in test files and only a handful per declaration, so the scan
/// is not on any hot path.
fn retag_covers_target_in_range(spans: &mut [SymbolSpan], range: Span) {
    for span in spans {
        if span.start >= range.start.offset
            && span.end <= range.end.offset
            && let SymbolKind::ClassReference { context, .. } = &mut span.kind
        {
            *context = ClassRefContext::CoversTarget;
        }
    }
}

/// The subject text and span of an argument that names a class, written
/// either `Foo::class` or as a string holding the name.
fn class_like_argument(arg: &PartialArgument<'_>, content: &str) -> Option<(String, Span)> {
    class_constant_argument(arg)
        .or_else(|| string_argument(arg, content).map(|(name, span)| (name.to_owned(), span)))
}

/// The subject text and span of a `Foo::class` argument.
fn class_constant_argument<'a>(arg: &'a PartialArgument<'a>) -> Option<(String, Span)> {
    let Some(Expression::Access(Access::ClassConstant(access))) = arg.value() else {
        return None;
    };
    let names_class_constant = matches!(
        &access.constant,
        ClassLikeConstantSelector::Identifier(ident)
            if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
    );
    if !names_class_constant {
        return None;
    }
    let subject = expr_to_subject_text(access.class);
    (!subject.is_empty()).then(|| (subject, access.class.span()))
}

/// The content of a plain string-literal argument and the span it occupies
/// inside its quotes.
fn string_argument<'a>(arg: &PartialArgument<'_>, content: &'a str) -> Option<(&'a str, Span)> {
    let Some(Expression::Literal(literal::Literal::String(literal))) = arg.value() else {
        return None;
    };
    let start = literal.span.start.offset + 1;
    let end = literal.span.end.offset - 1;
    if start >= end || end as usize > content.len() {
        return None;
    }
    let inner = Span::new(
        literal.span.file_id,
        Position::new(start),
        Position::new(end),
    );
    Some((&content[start as usize..end as usize], inner))
}

/// Whether `name` is a plain PHP identifier, so that prose such as
/// `#[CoversMethod(Foo::class, 'not a name')]` is not turned into a symbol.
fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// [`is_identifier`] for a name that may carry a namespace.
fn is_qualified_identifier(name: &str) -> bool {
    let name = name.trim_start_matches('\\');
    !name.is_empty() && name.split('\\').all(is_identifier)
}
