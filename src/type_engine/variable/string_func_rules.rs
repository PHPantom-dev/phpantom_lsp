/// Constant folding for the string standard library.
///
/// A signature like `strtolower(string $string): string` describes the
/// widest result the function can produce, not the result *this* call
/// produces.  When every argument the transform reads is a literal the
/// answer is a literal too, and callers that check a value against a
/// literal union (`'class'|'interface'|'trait'`) depend on getting it:
/// `strtolower($kind)` over `'Class'|'Interface'|'Trait'` is
/// `'class'|'interface'|'trait'`, not `string`.
///
/// The rules live beside [`super::array_func_rules`] and read their
/// arguments through the same [`ArrayFuncArgs`] seam, so the AST walker
/// and the text-driven call resolver both reach them.
///
/// Folding only happens when it is exact.  Every transform here is
/// byte-oriented in PHP, and the case-mapping ones have been
/// locale-independent ASCII since PHP 8.2, so an argument that is not
/// pure ASCII declines rather than guessing at a Unicode mapping the
/// runtime would not perform.
use crate::php_type::{PhpType, TypeKind};

use super::array_func_rules::ArrayFuncArgs;

/// How many alternatives a folded union may carry.
///
/// The transforms are applied per alternative, so a wide union costs
/// one pass each and produces a union just as wide.  A subject with
/// more alternatives than this is a data table rather than a set of
/// cases a caller distinguishes, and the union's pairwise absorption is
/// quadratic in its member count.
const MAX_ALTERNATIVES: usize = 32;

/// The literal value a call to a string builtin produces, or `None`
/// when the call is not one of the folded functions or an argument it
/// reads is not a literal.
pub(in crate::type_engine) fn string_func_literal_type(
    func_name: &str,
    args: &dyn ArrayFuncArgs,
) -> Option<PhpType> {
    // A fully-qualified call (`\strtolower($s)`) carries the leading
    // separator in its identifier text; the table below is keyed on the
    // bare name.
    let name = func_name.trim_start_matches('\\').to_ascii_lowercase();

    let folded: Vec<String> = match name.as_str() {
        // `mb_*` agrees with the ASCII mapping on ASCII input, which is
        // the only input that gets this far.
        "strtolower" | "mb_strtolower" => map_subject(args, |s| s.to_ascii_lowercase())?,
        "strtoupper" | "mb_strtoupper" => map_subject(args, |s| s.to_ascii_uppercase())?,
        "ucfirst" => map_subject(args, |s| change_first(s, u8::to_ascii_uppercase))?,
        "lcfirst" => map_subject(args, |s| change_first(s, u8::to_ascii_lowercase))?,
        "ucwords" if !args.has_arg(1) => map_subject(args, ucwords)?,
        "strrev" => map_subject(args, |s| s.chars().rev().collect())?,
        // The two-argument spellings take a character list that replaces
        // the default set, so only the default is folded here.
        "trim" if !args.has_arg(1) => map_subject(args, |s| trim_default(s, true, true))?,
        "ltrim" if !args.has_arg(1) => map_subject(args, |s| trim_default(s, true, false))?,
        "rtrim" | "chop" if !args.has_arg(1) => {
            map_subject(args, |s| trim_default(s, false, true))?
        }
        "str_repeat" => {
            let times = literal_int(args, 1)?;
            // A negative count is a fatal error and a huge one is a
            // memory bomb; neither is worth a literal answer.
            if !(0..=1024).contains(&times) {
                return None;
            }
            map_subject(args, |s| s.repeat(times as usize))?
        }
        // `str_replace` also accepts arrays for the search and the
        // replacement, which pair up element-wise; only the plain
        // string-for-string spelling is folded.
        "str_replace" => {
            let search = single_literal(args, 0)?;
            let replace = single_literal(args, 1)?;
            if search.is_empty() {
                return None;
            }
            let subjects = literal_alternatives(args, 2)?;
            subjects
                .iter()
                .map(|s| s.replace(&search, &replace))
                .collect()
        }
        _ => return None,
    };

    Some(union_of_literals(folded))
}

/// Apply `transform` to every alternative of the call's first argument.
fn map_subject(
    args: &dyn ArrayFuncArgs,
    transform: impl Fn(&str) -> String,
) -> Option<Vec<String>> {
    let subjects = literal_alternatives(args, 0)?;
    Some(subjects.iter().map(|s| transform(s)).collect())
}

/// The literal string values the argument at `index` can hold.
///
/// A union answers with one entry per member; anything that is not a
/// string literal throughout (a bare `string`, a class, a literal int)
/// declines, because a partial answer would drop the alternatives it
/// could not fold.
fn literal_alternatives(args: &dyn ArrayFuncArgs, index: usize) -> Option<Vec<String>> {
    if names_a_constant(args, index) {
        return None;
    }
    let ty = args.arg_raw_type(index)?;
    let mut out = Vec::new();
    collect_literals(&ty, &mut out)?;
    if out.is_empty() || out.len() > MAX_ALTERNATIVES {
        return None;
    }
    Some(out)
}

/// Whether the argument at `index` is written as a bare constant name.
///
/// The stubs give `PHP_VERSION`, `PHP_OS`, `DIRECTORY_SEPARATOR` and
/// friends a literal value, but that value describes the machine the
/// stubs were generated on, not the one the code will run on. Folding a
/// builtin over one would turn an environment-dependent answer into a
/// literal the rest of the engine then treats as certain:
/// `str_replace(DIRECTORY_SEPARATOR, '/', $path)` must not decide what
/// the separator is. The declared type is the honest answer there, so
/// this declines and the call keeps it.
fn names_a_constant(args: &dyn ArrayFuncArgs, index: usize) -> bool {
    // The seam reports integer literals through the same accessor; a
    // constant name never starts with a digit.
    args.arg_atom_text(index)
        .is_some_and(|text| !text.starts_with(|c: char| c.is_ascii_digit()))
}

/// The single literal string the argument at `index` holds, or `None`
/// when it is a union or not a literal.
fn single_literal(args: &dyn ArrayFuncArgs, index: usize) -> Option<String> {
    let mut alternatives = literal_alternatives(args, index)?;
    match alternatives.len() {
        1 => alternatives.pop(),
        _ => None,
    }
}

/// The literal integer the argument at `index` holds.
fn literal_int(args: &dyn ArrayFuncArgs, index: usize) -> Option<i64> {
    match args.arg_raw_type(index)?.kind() {
        TypeKind::Literal(value) => value.parse_i64(),
        _ => None,
    }
}

/// Append every string literal `ty` can be, declining as a whole if any
/// alternative is something else.
fn collect_literals(ty: &PhpType, out: &mut Vec<String>) -> Option<()> {
    match ty.kind() {
        TypeKind::Literal(value) => {
            let content = value.string_content()?;
            // PHP's string functions are byte-oriented and the case
            // transforms map ASCII only; a multibyte subject would need
            // the runtime's own answer, not ours.
            if !content.is_ascii() {
                return None;
            }
            if !out.iter().any(|seen| seen == content.as_ref()) {
                out.push(content.into_owned());
            }
            Some(())
        }
        TypeKind::Union(members) => {
            for member in members {
                collect_literals(member, out)?;
            }
            Some(())
        }
        _ => None,
    }
}

/// The type describing one folded result, or the union of several.
fn union_of_literals(mut values: Vec<String>) -> PhpType {
    values.dedup();
    let mut members: Vec<PhpType> = Vec::with_capacity(values.len());
    for value in values {
        let ty = PhpType::literal_string_value(value);
        if !members.contains(&ty) {
            members.push(ty);
        }
    }
    match members.len() {
        1 => members.remove(0),
        _ => PhpType::union(members),
    }
}

/// `ucfirst` / `lcfirst`: recase the leading byte, leave the rest alone.
fn change_first(s: &str, recase: fn(&u8) -> u8) -> String {
    let mut bytes = s.as_bytes().to_vec();
    if let Some(first) = bytes.first_mut() {
        *first = recase(first);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

/// The characters `ucwords` treats as word separators by default.
const UCWORDS_DELIMITERS: &[u8] = b" \t\r\n\x0c\x0b";

/// `ucwords` with its default delimiter set.
fn ucwords(s: &str) -> String {
    let mut bytes = s.as_bytes().to_vec();
    let mut at_word_start = true;
    for byte in &mut bytes {
        if at_word_start {
            *byte = byte.to_ascii_uppercase();
        }
        at_word_start = UCWORDS_DELIMITERS.contains(byte);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

/// The characters the `trim` family strips by default.
const TRIM_CHARACTERS: &[char] = &[' ', '\t', '\n', '\r', '\0', '\x0b'];

/// `trim` / `ltrim` / `rtrim` with their default character list.
fn trim_default(s: &str, left: bool, right: bool) -> String {
    let trimmed = match left {
        true => s.trim_start_matches(TRIM_CHARACTERS),
        false => s,
    };
    match right {
        true => trimmed.trim_end_matches(TRIM_CHARACTERS),
        false => trimmed,
    }
    .to_string()
}

/// Whether `func_name` is one of the folded string builtins.
///
/// Callers use this to skip building an argument adapter for the calls
/// that could never fold.
pub(in crate::type_engine) fn is_foldable_string_func(func_name: &str) -> bool {
    matches!(
        func_name
            .trim_start_matches('\\')
            .to_ascii_lowercase()
            .as_str(),
        "strtolower"
            | "mb_strtolower"
            | "strtoupper"
            | "mb_strtoupper"
            | "ucfirst"
            | "lcfirst"
            | "ucwords"
            | "strrev"
            | "trim"
            | "ltrim"
            | "rtrim"
            | "chop"
            | "str_repeat"
            | "str_replace"
    )
}

#[cfg(test)]
#[path = "string_func_rules_tests.rs"]
mod tests;
