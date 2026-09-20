//! Diagnostics for a docblock type that contradicts its native type hint.
//!
//! A `@param` or `@return` annotation is there to *refine* the native
//! declaration, not to widen it.  When the annotation admits `null` and the
//! native hint rules it out, the two describe different sets of values:
//!
//! ```php
//! /** @param ?string $name */
//! function greet(string $name): void {}
//! ```
//!
//! The engine cannot honour the annotation without contradicting the
//! signature, and a caller that passes `null` is stopped by the runtime
//! whatever the docblock claims, so the declaration is flagged.
//!
//! The opposite pairing is not a contradiction.  A nullable native hint
//! carries its `null` over to the documented type on its own, exactly as
//! PHPStan's `TypehintHelper::decideType` does, so leaving `|null` out of the
//! docblock of a `?string` parameter is idiomatic rather than wrong:
//!
//! ```php
//! /** @param non-empty-string $name */
//! function greet(?string $name): void {}   // reads as `non-empty-string|null`
//! ```

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::declare::DeclareBody;
use mago_syntax::cst::function_like::parameter::{
    FunctionLikeParameterDefaultValue, FunctionLikeParameterList,
};
use mago_syntax::cst::function_like::r#return::FunctionLikeReturnTypeHint;
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::*;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::docblock::{
    DocblockInfo, extract_param_raw_type_from_info, extract_return_type_from_info,
    get_docblock_info_for_node,
};
use crate::parser::{extract_hint_type, with_parsed_program};
use crate::php_type::{PhpType, TypeKind};

use super::helpers::make_diagnostic;

/// Diagnostic code used for docblock/native type-hint contradictions.
pub(crate) const DOCBLOCK_NATIVE_MISMATCH_CODE: &str = "docblock_native_mismatch";

/// One flagged annotation: the source range to underline and the message.
struct Finding {
    start: usize,
    end: usize,
    message: String,
}

/// The program trivia and source text needed to look up a node's docblock.
struct Ctx<'a> {
    trivia: &'a [Trivia<'a>],
    content: &'a str,
}

impl Backend {
    /// Flag every `@param`/`@return` annotation that denies `null` where the
    /// native type hint it annotates admits it.
    pub fn collect_docblock_native_mismatch_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let findings = with_parsed_program(content, "docblock_native_mismatch", |program, _| {
            let ctx = Ctx {
                trivia: program.trivia.as_slice(),
                content,
            };
            let mut findings: Vec<Finding> = Vec::new();
            for stmt in program.statements.iter() {
                walk_statement(stmt, &ctx, &mut findings);
            }
            findings
        });

        for finding in findings {
            if let Some(range) =
                self.offset_range_to_lsp_range(uri, content, finding.start, finding.end)
            {
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::WARNING,
                    DOCBLOCK_NATIVE_MISMATCH_CODE,
                    finding.message,
                ));
            }
        }
    }
}

/// Descend through the statement forms that can hold a declaration.
fn walk_statement(stmt: &Statement<'_>, ctx: &Ctx<'_>, out: &mut Vec<Finding>) {
    match stmt {
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                walk_statement(inner, ctx, out);
            }
        }
        Statement::Block(block) => {
            for inner in block.statements.iter() {
                walk_statement(inner, ctx, out);
            }
        }
        Statement::Declare(declare) => match &declare.body {
            DeclareBody::Statement(inner) => walk_statement(inner, ctx, out),
            DeclareBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    walk_statement(inner, ctx, out);
                }
            }
        },
        Statement::Class(class) => walk_members(&class.members, ctx, out),
        Statement::Interface(interface) => walk_members(&interface.members, ctx, out),
        Statement::Trait(trait_def) => walk_members(&trait_def.members, ctx, out),
        Statement::Enum(enum_def) => walk_members(&enum_def.members, ctx, out),
        Statement::Function(func) => check_declaration(
            func,
            &func.parameter_list,
            func.return_type_hint.as_ref(),
            ctx,
            out,
        ),
        _ => {}
    }
}

fn walk_members<'arena>(
    members: &Sequence<'arena, ClassLikeMember<'arena>>,
    ctx: &Ctx<'_>,
    out: &mut Vec<Finding>,
) {
    for member in members.iter() {
        if let ClassLikeMember::Method(method) = member {
            check_declaration(
                method,
                &method.parameter_list,
                method.return_type_hint.as_ref(),
                ctx,
                out,
            );
        }
    }
}

/// Compare every annotated parameter and the return type of one declaration
/// against the docblock immediately above it.
fn check_declaration(
    node: &impl HasSpan,
    parameters: &FunctionLikeParameterList<'_>,
    return_type_hint: Option<&FunctionLikeReturnTypeHint<'_>>,
    ctx: &Ctx<'_>,
    out: &mut Vec<Finding>,
) {
    let Some(info) = get_docblock_info_for_node(ctx.trivia, ctx.content, node) else {
        return;
    };

    for param in parameters.parameters.iter() {
        let Some(hint) = param.hint.as_ref() else {
            continue;
        };
        let name = bytes_to_str(param.variable.name);
        let mut native = extract_hint_type(hint);
        // `string $name = null` is PHP's pre-8.4 implicit-nullable form: the
        // signature accepts null without spelling it, so a `?string` docblock
        // agrees with it.
        if param
            .default_value
            .as_ref()
            .is_some_and(|default| default_is_null(default, ctx.content))
        {
            native = native.or_null();
        }
        let Some(documented) = extract_param_raw_type_from_info(&info, name) else {
            continue;
        };
        if !admits_null_the_native_hint_denies(&documented, &native) {
            continue;
        }
        out.push(Finding {
            start: hint.span().start.offset as usize,
            end: param.variable.span.end.offset as usize,
            message: format!(
                "Documented type '{}' for {} accepts null, but the native type hint '{}' does not",
                documented, name, native
            ),
        });
    }

    check_return(&info, return_type_hint, out);
}

/// Whether a parameter's default value is the literal `null`.
fn default_is_null(default: &FunctionLikeParameterDefaultValue<'_>, content: &str) -> bool {
    let span = default.value.span();
    content
        .get(span.start.offset as usize..span.end.offset as usize)
        .is_some_and(|text| text.trim().eq_ignore_ascii_case("null"))
}

fn check_return(
    info: &DocblockInfo,
    return_type_hint: Option<&FunctionLikeReturnTypeHint<'_>>,
    out: &mut Vec<Finding>,
) {
    let Some(hint) = return_type_hint else {
        return;
    };
    let native = extract_hint_type(&hint.hint);
    let Some(documented) = extract_return_type_from_info(info) else {
        return;
    };
    if !admits_null_the_native_hint_denies(&documented, &native) {
        return;
    }
    let span = hint.hint.span();
    out.push(Finding {
        start: span.start.offset as usize,
        end: span.end.offset as usize,
        message: format!(
            "Documented return type '{}' accepts null, but the native type hint '{}' does not",
            documented, native
        ),
    });
}

/// Whether `documented` admits a `null` that `native` rules out.
fn admits_null_the_native_hint_denies(documented: &PhpType, native: &PhpType) -> bool {
    // A nullable native hint hands its `null` to the documented type, so the
    // annotation can never be the wider of the two.  This mirrors the
    // effective-type merge in `resolve_effective_type_typed`.
    if native.accepts_null() {
        return false;
    }

    spells_null(documented)
}

/// Whether the type expression itself puts `null` in the set of values it
/// describes.
///
/// Only the spellings that name `null` outright count.  `mixed` admits null
/// too, but a docblock that widens a native hint all the way to `mixed` is a
/// different mistake than one that contradicts its nullability, and a bare
/// class-like name settles nothing: `@param T $x` may be a `@template`
/// parameter and `@param UserId $x` an imported `@psalm-type` alias, either of
/// which can resolve to a nullable type.
fn spells_null(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Nullable(_) => true,
        TypeKind::Named(name) => name.eq_ignore_ascii_case("null"),
        TypeKind::Union(members) => members.iter().any(spells_null),
        _ => false,
    }
}
