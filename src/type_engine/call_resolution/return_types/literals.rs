//! Typing argument and expression text that needs no resolution: literals,
//! operators over them, and the `self`/`static`/`parent` keywords a hint or
//! constant initializer spells.

use std::sync::Arc;

use crate::Backend;
use crate::atom::atom;
use crate::class_lookup::is_self_or_static;
use crate::php_type::PhpType;
use crate::text_scan::{find_matching_forward, is_whole_string_literal};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::*;

use super::operands::{
    contains_top_level_concat, join_operand_types, split_top_level_coalesce, split_top_level_elvis,
};

/// `text` with a leading `self::`/`static::`/`parent::` replaced by the class
/// it names, so a term read out of a constant's initialiser resolves against
/// the class that declared it rather than the one being read from.
pub(super) fn qualify_class_keyword<'t>(
    text: &'t str,
    class: &ClassInfo,
) -> std::borrow::Cow<'t, str> {
    let (keyword, rest) = match text.split_once("::") {
        Some(parts) => parts,
        None => return std::borrow::Cow::Borrowed(text),
    };
    let qualifier = if is_self_or_static(keyword) {
        class.fqn()
    } else if let Some(parent) = class
        .parent_class
        .filter(|_| keyword.eq_ignore_ascii_case("parent"))
    {
        parent
    } else {
        return std::borrow::Cow::Borrowed(text);
    };
    // Fully qualified, so reading it as a class named in code does not
    // qualify it against the reader's namespace a second time.
    std::borrow::Cow::Owned(format!("\\{qualifier}::{rest}"))
}

/// Resolve `self`/`static`/`parent` in a return-type hint to concrete
/// class names, so downstream consumers see real FQNs rather than
/// keywords, then apply the Eloquent-collection patch every hint gets.
///
/// `parent` always resolves to `owner`'s parent (or is left as-is when it
/// has none). `self`/`static` is what genuinely differs between the
/// instance and static call paths, so `replace_self` decides that part:
/// the instance path prefers the receiver's own generic type
/// (`Builder<User>`) so a fluent chain keeps its bound template argument,
/// while a static call replaces only the bare name.
pub(super) fn resolve_hint_keywords(
    hint: PhpType,
    owner: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    replace_self: impl FnOnce(PhpType) -> PhpType,
) -> PhpType {
    let resolved = if hint.is_parent_ref() {
        owner
            .parent_class
            .as_ref()
            .map(|p| PhpType::named(atom(p.as_ref())))
            .unwrap_or(hint)
    } else if hint.contains_self_ref() {
        replace_self(hint)
    } else {
        hint
    };
    crate::virtual_members::laravel::replace_eloquent_collections_in_type(&resolved, class_loader)
        .unwrap_or(resolved)
}

/// The type an operator expression produces, for the operators whose
/// answer can be read from the source text without a full parse.
///
/// Concatenation (`$a . $b`) always yields `string`, whatever its operands
/// are. The elvis operator (`$body ?: ''`) yields the union of both sides —
/// resolved recursively through [`Backend::resolve_arg_text_to_type`], the
/// same way assigning the expression to a variable first would resolve
/// through the AST-based `resolve_conditional_chain`.
///
/// A full three-part ternary (`$a ? $b : $c`) is left unanswered here, and
/// so is arithmetic (`+`, `-`, `*`, …) unless every operand resolves to an
/// integer and no operator can turn the result into a float: arithmetic's
/// result otherwise depends on whether its operands are int or float (and
/// `+` alone can mean array union).
pub(in crate::type_engine::call_resolution) fn resolve_operator_type(
    text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if contains_top_level_concat(text) {
        return Some(PhpType::named(atom("string")));
    }
    // A bitwise or integer expression over constants is the value PHP
    // computes for it (`JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR` is one mask,
    // not two flags).
    // Only an expression that actually has an operator folds: a single term
    // would ask this very resolver about the same text again.
    if crate::type_engine::types::const_fold::has_top_level_int_operator(text) {
        let resolve = |term: &str| Backend::resolve_arg_text_to_type(term, ctx);
        if let Some(value) =
            crate::type_engine::types::const_fold::fold_int_expression(text, &resolve)
        {
            return Some(PhpType::literal_int(value.to_string()));
        }
        // An expression over integers whose values are unknown (`$n - 1`)
        // is still an `int`, as long as no operator can turn it into a float.
        if crate::type_engine::types::const_fold::is_int_expression(text, &resolve) {
            return Some(PhpType::int());
        }
    }
    // `??` binds looser than `?:`, so it is split first: the left operand of
    // `$a ?? $b ?: $c` is `$a` and the right is the whole ternary.  The
    // coalesce only yields its left operand when that operand is not null,
    // so the `null` arm cannot survive into the result.
    if let Some((left, right)) = split_top_level_coalesce(text) {
        // A left operand that is only ever null contributes nothing, which
        // `non_null_type` reports as `None` — the same answer an unresolvable
        // operand gives, and the right operand carries the result either way.
        let left_ty = Backend::resolve_arg_text_to_type(left, ctx).and_then(|ty| {
            if ty.is_null() {
                None
            } else {
                Some(ty.non_null_type().unwrap_or(ty))
            }
        });
        let right_ty = Backend::resolve_arg_text_to_type(right, ctx);
        return join_operand_types(left_ty, right_ty);
    }

    if let Some((left, right)) = split_top_level_elvis(text) {
        let left_ty = Backend::resolve_arg_text_to_type(left, ctx);
        let right_ty = Backend::resolve_arg_text_to_type(right, ctx);
        return join_operand_types(left_ty, right_ty);
    }
    None
}

/// The literal type of a string or integer literal argument, or `None` for
/// anything else.
///
/// [`resolve_literal_type`] widens both to their base type, which is what
/// most argument checks want. A template bound straight from the argument
/// (`@param T $a` with `id('hello')`) keeps the value instead, the way the
/// forward walker types `$a = 'hello'`. A double-quoted string that
/// interpolates is only known to be a string.
pub(crate) fn literal_arg_type(text: &str) -> Option<PhpType> {
    if is_whole_string_literal(text) && (text.starts_with('\'') || !text.contains('$')) {
        return Some(PhpType::literal_string_raw(text));
    }
    let numeric = text.strip_prefix('-').unwrap_or(text);
    let is_octal = numeric.len() > 1 && numeric.starts_with('0');
    if is_octal || numeric.is_empty() || !numeric.bytes().all(|b| b.is_ascii_digit() || b == b'_') {
        return None;
    }
    let digits: String = text.chars().filter(|&c| c != '_').collect();
    let value = digits.parse::<i64>().ok()?;
    Some(PhpType::literal_int(value.to_string()))
}

/// Resolve a literal expression to its PHP type.
///
/// Returns `Some(PhpType)` for string literals (`"…"`, `'…'`), integer
/// literals (`42`, `-1`), float literals (`3.14`), boolean literals
/// (`true`, `false`), `null`, and array literals (`[…]`).
pub(in crate::type_engine::call_resolution) fn resolve_literal_type(text: &str) -> Option<PhpType> {
    // Closure / arrow function literals: fn(...), function(...), and the
    // `static`-prefixed forms of both.
    if crate::completion::source::helpers::is_closure_like_text(text) {
        return Some(PhpType::named(atom("Closure")));
    }

    // String literals: "…" or '…'
    if is_whole_string_literal(text) {
        return Some(PhpType::named(atom("string")));
    }

    // null
    if text.eq_ignore_ascii_case("null") {
        return Some(PhpType::null());
    }

    // Boolean literals — preserve true/false as distinct types so that
    // template argument inference keeps the precise type (e.g. `C<false>`
    // instead of widening to `C<bool>`).
    if text.eq_ignore_ascii_case("true") {
        return Some(PhpType::true_());
    }
    if text.eq_ignore_ascii_case("false") {
        return Some(PhpType::false_());
    }

    // Array literals: [...] or array(...), when the bracket that opens
    // the text is the one that closes it (`[1] === [2]` is no array).
    let closes_at_end = |open: usize, open_byte: u8, close_byte: u8| {
        find_matching_forward(text, open, open_byte, close_byte) == Some(text.len() - 1)
    };
    if (text.starts_with('[') && closes_at_end(0, b'[', b']'))
        || (text.starts_with("array(") && closes_at_end(5, b'(', b')'))
    {
        return Some(PhpType::named(atom("array")));
    }

    // Numeric literals — try int first, then float.
    // Strip an optional leading minus for negative literals.
    let numeric = text.strip_prefix('-').unwrap_or(text);
    if !numeric.is_empty()
        && numeric.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        && numeric.bytes().any(|b| b.is_ascii_digit())
    {
        return Some(PhpType::named(atom("int")));
    }
    if !numeric.is_empty()
        && numeric
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'_')
        && numeric.bytes().filter(|&b| b == b'.').count() == 1
        && numeric.bytes().any(|b| b.is_ascii_digit())
    {
        return Some(PhpType::named(atom("float")));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{literal_arg_type, resolve_literal_type};

    #[test]
    fn a_comparison_of_two_strings_is_not_a_string_literal() {
        assert_eq!(resolve_literal_type("'a' === 'b'"), None);
        assert_eq!(literal_arg_type("'a' . 'b'"), None);
        assert_eq!(
            resolve_literal_type("'a'")
                .map(|ty| ty.to_string())
                .as_deref(),
            Some("string")
        );
    }

    #[test]
    fn an_array_literal_is_one_bracket_pair() {
        assert_eq!(resolve_literal_type("[1] === [2]"), None);
        assert_eq!(
            resolve_literal_type("[1, [2]]")
                .map(|ty| ty.to_string())
                .as_deref(),
            Some("array")
        );
        assert_eq!(
            resolve_literal_type("array(1, ')')")
                .map(|ty| ty.to_string())
                .as_deref(),
            Some("array")
        );
    }
}
