//! Qualifying the class names in a class constant's initializer.
//!
//! An initializer is kept as source text (`ConstantInfo::value`) and read
//! by whichever file reads the constant, where the imports and namespace of
//! the block that wrote it are not in scope.  The classes it names are
//! therefore spelled out fully qualified while the declaring file is parsed,
//! the way every other name a class carries is.

use crate::text_scan;

/// Freeze file magic constants before an initializer is read from another
/// file or inherited from a parent or trait. Strings and comments retain
/// their source spelling; only the parser's magic-constant tokens change.
pub(crate) fn expand_file_constants(expression: &str, uri: &str) -> Option<String> {
    if !mentions_file_constant(expression) {
        return None;
    }
    let path = crate::util::uri_to_path(uri)?;
    let path = dunce::canonicalize(&path).unwrap_or(path);
    let directory = path.parent().unwrap_or(&path);
    use mago_span::HasSpan;
    use mago_syntax::{cst::MagicConstant, walker::Walker};
    struct Constants;
    impl<'ast, 'arena> Walker<'ast, 'arena, Vec<(usize, usize, bool)>> for Constants {
        fn walk_in_magic_constant(
            &self,
            magic: &'ast MagicConstant<'arena>,
            spans: &mut Vec<(usize, usize, bool)>,
        ) {
            if matches!(magic, MagicConstant::Directory(_) | MagicConstant::File(_)) {
                let span = magic.span();
                spans.push((
                    span.start.offset as usize,
                    span.end.offset as usize,
                    matches!(magic, MagicConstant::Directory(_)),
                ));
            }
        }
    }
    let source = format!("<?php {expression};");
    crate::parser::with_parsed_program(&source, "file_constant_initializer", |program, _| {
        let mut spans = Vec::new();
        Constants.walk_program(program, &mut spans);
        if spans.is_empty() {
            return None;
        }
        let mut output = expression.to_owned();
        for (start, end, is_directory) in spans.into_iter().rev() {
            let value = if is_directory { directory } else { &path };
            let literal =
                crate::php_type::LiteralValue::string_value(value.to_string_lossy()).as_raw();
            output.replace_range(start - 6..end - 6, &literal);
        }
        Some(output)
    })
}

/// Whether `expression` spells `__DIR__` or `__FILE__` in any case, so the
/// parse in [`expand_file_constants`] runs only where it can change anything.
fn mentions_file_constant(expression: &str) -> bool {
    contains_ignore_ascii_case(expression, "__DIR__")
        || contains_ignore_ascii_case(expression, "__FILE__")
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Freeze `__DIR__` and `__FILE__` in a class's constant and parameter
/// initializers to the declaring file, on every path that indexes
/// a class from source.  PHP substitutes them at compile time, so the value
/// read through inheritance or from a call site in another file is the
/// declaring file's.
pub(crate) fn expand_class_file_constants(class: &mut crate::types::ClassInfo, uri: &str) {
    use std::sync::Arc;
    if class.constants.iter().any(|constant| {
        constant
            .value
            .as_deref()
            .is_some_and(mentions_file_constant)
    }) {
        for constant in class.constants.make_mut() {
            if let Some(value) = constant
                .value
                .as_deref()
                .and_then(|value| expand_file_constants(value, uri))
            {
                Arc::make_mut(constant).value = Some(value);
            }
        }
    }
    if class
        .methods
        .iter()
        .any(|method| mentions_in_parameters(&method.parameters))
    {
        for method in class.methods.make_mut() {
            if mentions_in_parameters(&method.parameters) {
                expand_parameter_file_constants(&mut Arc::make_mut(method).parameters, uri);
            }
        }
    }
}

fn mentions_in_parameters(parameters: &[crate::types::ParameterInfo]) -> bool {
    parameters.iter().any(|parameter| {
        parameter
            .default_value
            .as_deref()
            .is_some_and(mentions_file_constant)
    })
}

/// Freeze `__DIR__` and `__FILE__` in parameter defaults to the declaring
/// file, so a caller elsewhere that omits the argument reads the callee's.
pub(crate) fn expand_parameter_file_constants(
    parameters: &mut crate::types::SharedVec<crate::types::ParameterInfo>,
    uri: &str,
) {
    if !mentions_in_parameters(parameters) {
        return;
    }
    for parameter in parameters.make_mut() {
        if let Some(value) = parameter
            .default_value
            .as_deref()
            .and_then(|value| expand_file_constants(value, uri))
        {
            parameter.default_value = Some(value);
        }
    }
}

/// `expression` with the class of every `Class::member` access in it
/// (`Defaults::MODE`, `Defaults::class`, `Suit::Hearts->value`) spelled
/// fully qualified, with a leading `\`, or `None` when no class in it needs
/// that.
///
/// `resolve` qualifies a relative name against the imports and namespace of
/// the block `expression` was written in, and `namespace` is that block's
/// namespace, which a `namespace\Foo` name is relative to.  A name already
/// written fully qualified is kept, and so are `self`, `static` and
/// `parent`, which name a class only the reader of the constant knows.
/// Names inside strings and comments are not class names, and neither is a
/// bare name (a global constant) or the member after `::` or `->`.
pub(crate) fn qualify_class_names(
    expression: &str,
    namespace: Option<&str>,
    resolve: &dyn Fn(&str) -> String,
) -> Option<String> {
    // Nothing is read off a class without `::`, which keeps the scan off
    // the plain literals most initializers are.
    if !expression.contains("::") {
        return None;
    }
    let bytes = expression.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    // Whether the last token was `::` or `->`, after which a name is the
    // member being read rather than a class.
    let mut after_access = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' || b == b'"' {
            i = text_scan::skip_string_forward(bytes, i);
            after_access = false;
        } else if let Some(end) = text_scan::skip_php_comment(bytes, i) {
            i = end;
        } else if bytes[i..].starts_with(b"<<<") {
            i = skip_heredoc(bytes, i);
            after_access = false;
        } else if bytes[i..].starts_with(b"::") || bytes[i..].starts_with(b"->") {
            i += 2;
            after_access = true;
        } else if b == b'$' || b.is_ascii_digit() {
            // The letters of a variable or of a number (`0xFF`, `1e3`) are
            // not a name.
            i += 1;
            while bytes.get(i).is_some_and(|&b| is_name_byte(b)) {
                i += 1;
            }
            after_access = false;
        } else if is_name_start(b) || (b == b'\\' && starts_name(bytes, i + 1)) {
            let start = i;
            i += 1;
            while bytes
                .get(i)
                .is_some_and(|&b| is_name_byte(b) || (b == b'\\' && starts_name(bytes, i + 1)))
            {
                i += 1;
            }
            if std::mem::take(&mut after_access) || !followed_by_access(bytes, i) {
                continue;
            }
            if let Some(fqn) = qualify(&expression[start..i], namespace, resolve) {
                out.push_str(&expression[copied..start]);
                out.push('\\');
                out.push_str(&fqn);
                copied = i;
            }
        } else {
            if !b.is_ascii_whitespace() {
                after_access = false;
            }
            i += 1;
        }
    }
    if copied == 0 {
        return None;
    }
    out.push_str(&expression[copied..]);
    Some(out)
}

/// `expression` with every constant name in it that has one meaning
/// spelled fully qualified, with a leading `\`, or `None` when no name in it
/// needs that.
///
/// Those are the names a `use const` imports, qualified names (`Sub\LIMIT`,
/// whose first segment the class imports resolve) and `namespace\LIMIT`,
/// written in a block whose namespace is `namespace`, with `use_map` and
/// `constant_imports` its class and `use const` imports.  An unqualified
/// name no import claims is kept: inside a namespace, PHP reads it as that
/// namespace's constant when there is one and the global one otherwise, so
/// whoever reads the expression looks it up that way (see
/// [`qualify_unqualified_constant_names`]).
pub(crate) fn qualify_constant_names(
    expression: &str,
    namespace: &Option<String>,
    use_map: &std::collections::HashMap<String, String>,
    constant_imports: &std::collections::HashMap<String, String>,
) -> Option<String> {
    // Every name this can change is qualified or one of the imports, which
    // keeps the parse off nearly every initializer.
    if !names_outside_literals(expression).any(|name| {
        (!name.starts_with('\\') && name.contains('\\')) || constant_imports.contains_key(name)
    }) {
        return None;
    }
    rewrite_constant_names(expression, |name, qualified| {
        if qualified {
            Some(crate::names::resolve_to_fqn(name, use_map, namespace))
        } else {
            constant_imports.get(name).cloned()
        }
    })
}

/// `expression` with each unqualified constant name `resolve` answers for
/// replaced by the fully qualified name it returns, or `None` when it
/// answers for none.
///
/// For text written inside a namespace and read elsewhere, after
/// [`qualify_constant_names`] has spelled out the names that have one
/// meaning: `resolve` picks between the namespace's constant and the global
/// one, which only an index of the declared constants can say.
pub(crate) fn qualify_unqualified_constant_names(
    expression: &str,
    resolve: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if !mentions_name(expression) {
        return None;
    }
    rewrite_constant_names(expression, |name, qualified| {
        (!qualified).then(|| resolve(name)).flatten()
    })
}

/// Whether `expression` reads a constant by an unqualified name, which
/// [`qualify_unqualified_constant_names`] would have to look up.
pub(crate) fn names_unqualified_constant(expression: &str) -> bool {
    mentions_name(expression)
        && constant_names(expression)
            .iter()
            .any(|&(_, _, qualified)| !qualified)
}

/// Whether `expression` spells an unqualified name outside its string
/// literals and comments, other than `true`, `false` and `null`: a cheap
/// test that it may name such a constant, before parsing it to find out.
fn mentions_name(expression: &str) -> bool {
    names_outside_literals(expression).any(|name| {
        !name.contains('\\')
            && !["true", "false", "null"]
                .iter()
                .any(|literal| name.eq_ignore_ascii_case(literal))
    })
}

/// Each name `expression` spells outside its string literals, comments and
/// heredocs, qualified ones whole and with any leading `\`.  The letters of
/// a variable or of a number are not one.
fn names_outside_literals(expression: &str) -> impl Iterator<Item = &str> {
    let bytes = expression.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\'' || b == b'"' {
                i = text_scan::skip_string_forward(bytes, i);
            } else if let Some(end) = text_scan::skip_php_comment(bytes, i) {
                i = end;
            } else if bytes[i..].starts_with(b"<<<") {
                i = skip_heredoc(bytes, i);
            } else if b == b'$' || b.is_ascii_digit() {
                i += 1;
                while bytes.get(i).is_some_and(|&b| is_name_byte(b)) {
                    i += 1;
                }
            } else if is_name_start(b) || (b == b'\\' && starts_name(bytes, i + 1)) {
                let start = i;
                i += 1;
                while bytes
                    .get(i)
                    .is_some_and(|&b| is_name_byte(b) || (b == b'\\' && starts_name(bytes, i + 1)))
                {
                    i += 1;
                }
                return Some(&expression[start..i]);
            } else {
                i += 1;
            }
        }
        None
    })
}

/// `expression` with each constant name it reads (not a class, function or
/// member name) replaced by `\` and what `qualify` returns for it, or `None`
/// when `qualify` returns nothing for any.
///
/// `qualify` is handed the name as written and whether it is qualified
/// (`Sub\LIMIT`, `namespace\LIMIT`).  A name written fully qualified is
/// never handed over.
fn rewrite_constant_names(
    expression: &str,
    qualify: impl Fn(&str, bool) -> Option<String>,
) -> Option<String> {
    // `qualify` may look constants up, which can parse other files, so it
    // runs once the parse is done with.
    let mut output: Option<String> = None;
    for (start, end, qualified) in constant_names(expression).into_iter().rev() {
        let Some(fqn) = expression
            .get(start..end)
            .and_then(|name| qualify(name, qualified))
        else {
            continue;
        };
        output
            .get_or_insert_with(|| expression.to_owned())
            .replace_range(start..end, &format!("\\{}", fqn.trim_start_matches('\\')));
    }
    output
}

/// The byte range in `expression` of each constant name it reads that is
/// not written fully qualified, in source order, with whether the name is
/// qualified.
fn constant_names(expression: &str) -> Vec<(usize, usize, bool)> {
    use mago_span::HasSpan;
    use mago_syntax::{
        cst::{ConstantAccess, Identifier},
        walker::Walker,
    };
    struct Names;
    impl<'ast, 'arena> Walker<'ast, 'arena, Vec<(usize, usize, bool)>> for Names {
        fn walk_in_constant_access(
            &self,
            access: &'ast ConstantAccess<'arena>,
            names: &mut Vec<(usize, usize, bool)>,
        ) {
            let qualified = match access.name {
                Identifier::Local(_) => false,
                Identifier::Qualified(_) => true,
                Identifier::FullyQualified(_) => return,
            };
            let span = access.name.span();
            names.push((
                span.start.offset as usize,
                span.end.offset as usize,
                qualified,
            ));
        }
    }
    const PREFIX: usize = "<?php ".len();
    let source = format!("<?php {expression};");
    crate::parser::with_parsed_program(&source, "constant_initializer", |program, _| {
        let mut names = Vec::new();
        Names.walk_program(program, &mut names);
        for (start, end, _) in &mut names {
            *start -= PREFIX;
            *end -= PREFIX;
        }
        names
    })
}

/// The fully qualified name, without a leading `\`, of the class written as
/// `name` in a block whose namespace is `namespace`, or `None` when `name`
/// is kept as written.
fn qualify(
    name: &str,
    namespace: Option<&str>,
    resolve: &dyn Fn(&str) -> String,
) -> Option<String> {
    if name.starts_with('\\') || crate::class_lookup::is_class_keyword(name) {
        return None;
    }
    // `namespace\Foo` is `Foo` in the current namespace, never an import.
    const NAMESPACE_PREFIX: &str = "namespace\\";
    if name
        .get(..NAMESPACE_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(NAMESPACE_PREFIX))
    {
        let rest = &name[NAMESPACE_PREFIX.len()..];
        return Some(match namespace {
            Some(ns) => format!("{ns}\\{rest}"),
            None => rest.to_string(),
        });
    }
    Some(resolve(name))
}

/// Whether the token after the name ending at `end` is `::`, so that the
/// name is the class a member is read off.
fn followed_by_access(bytes: &[u8], end: usize) -> bool {
    let mut i = end;
    loop {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        match text_scan::skip_php_comment(bytes, i) {
            Some(after) => i = after,
            None => return bytes[i..].starts_with(b"::"),
        }
    }
}

/// The offset just past the heredoc or nowdoc whose `<<<` is at `start`:
/// past its closing label, or the end of `bytes` when it is never closed.
pub(crate) fn skip_heredoc(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 3;
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    if matches!(bytes.get(i), Some(b'\'' | b'"')) {
        i += 1;
    }
    let label_start = i;
    while bytes.get(i).is_some_and(|&b| is_name_byte(b)) {
        i += 1;
    }
    let label = &bytes[label_start..i];
    if label.is_empty() {
        return i;
    }
    // The body starts on the next line, and ends at the first line that,
    // after any indentation, starts with the label and does not continue
    // it.
    while let Some(newline) = text_scan::find_byte(bytes, i, b'\n') {
        i = newline + 1;
        while matches!(bytes.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        let end = i + label.len();
        if bytes[i..].starts_with(label) && !bytes.get(end).is_some_and(|&b| is_name_byte(b)) {
            return end;
        }
    }
    bytes.len()
}

/// Whether a name starts at `at`.
fn starts_name(bytes: &[u8], at: usize) -> bool {
    bytes.get(at).is_some_and(|&b| is_name_start(b))
}

/// Whether `b` can start a PHP name.  Every byte of a multi-byte UTF-8
/// character is one, as PHP allows any of them in a name.
fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

/// Whether `b` can continue a PHP name.
fn is_name_byte(b: u8) -> bool {
    is_name_start(b) || b.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        expand_file_constants, names_unqualified_constant, qualify_class_names,
        qualify_constant_names, qualify_unqualified_constant_names,
    };

    #[test]
    fn file_constants_expand_only_actual_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dunce::canonicalize(dir.path())
            .unwrap()
            .join("unsaved-constant-definition.php");
        let uri = crate::util::path_to_uri(&path);
        let quoted = crate::php_type::LiteralValue::string_value(path.to_string_lossy()).as_raw();
        assert_eq!(expand_file_constants("__file__", &uri), Some(quoted));
        assert!(expand_file_constants("__DIR__ . __FILE__", &uri).is_some());
        for expression in [
            "42",
            "__LINE__",
            "'__DIR__'",
            "\"__FILE__\"",
            "1 /* __DIR__ */",
            "<<<'TEXT'\n__FILE__\nTEXT",
        ] {
            assert_eq!(
                expand_file_constants(expression, &uri),
                None,
                "{expression}"
            );
        }
        assert_eq!(expand_file_constants("__DIR__", "untitled:constants"), None);
    }

    /// Resolves the way the declaring file of these tests would: `Defaults`
    /// is imported from `Vendor`, and anything else is in `Lib`.
    fn qualify(expression: &str) -> Option<String> {
        qualify_class_names(expression, Some("Lib"), &|name| match name {
            "Defaults" => "Vendor\\Defaults".to_string(),
            _ => format!("Lib\\{name}"),
        })
    }

    #[test]
    fn qualifies_the_class_of_a_member_access() {
        assert_eq!(
            qualify("Defaults::MODE").as_deref(),
            Some("\\Vendor\\Defaults::MODE")
        );
        assert_eq!(
            qualify("Labels::DEFAULT").as_deref(),
            Some("\\Lib\\Labels::DEFAULT")
        );
        assert_eq!(
            qualify("Defaults::class").as_deref(),
            Some("\\Vendor\\Defaults::class")
        );
        assert_eq!(
            qualify("Sub\\Labels::DEFAULT").as_deref(),
            Some("\\Lib\\Sub\\Labels::DEFAULT")
        );
    }

    #[test]
    fn qualifies_every_access_in_a_compound_expression() {
        assert_eq!(
            qualify("[Defaults::MODE => Labels::A, 'b' => Labels :: B]").as_deref(),
            Some("[\\Vendor\\Defaults::MODE => \\Lib\\Labels::A, 'b' => \\Lib\\Labels :: B]")
        );
        assert_eq!(
            qualify("Defaults::A | Labels::B").as_deref(),
            Some("\\Vendor\\Defaults::A | \\Lib\\Labels::B")
        );
        assert_eq!(
            qualify("Suit::Hearts->value").as_deref(),
            Some("\\Lib\\Suit::Hearts->value")
        );
    }

    #[test]
    fn keeps_names_that_are_not_a_class_to_qualify() {
        // Already fully qualified, or bound by whoever reads the constant.
        assert_eq!(qualify("\\Vendor\\Defaults::MODE"), None);
        assert_eq!(qualify("[self::A, static::B, parent::C, SELF::D]"), None);
        // A member, after `::` or `->`, however it is spaced.
        assert_eq!(qualify("self:: A"), None);
        assert_eq!(qualify("self::A->value"), None);
        // A global constant, a number, and plain literals.
        assert_eq!(qualify("JSON_THROW_ON_ERROR | self::FLAGS"), None);
        assert_eq!(qualify("0x1F | self::E1"), None);
        assert_eq!(qualify("'plain'"), None);
    }

    #[test]
    fn skips_strings_comments_and_heredocs() {
        assert_eq!(qualify("'see Defaults::MODE'"), None);
        assert_eq!(qualify("\"see \\\" Defaults::MODE\""), None);
        assert_eq!(qualify("[\n    // see Defaults::MODE\n    1,\n]"), None);
        assert_eq!(qualify("[/* Defaults::MODE */ 1]"), None);
        assert_eq!(
            qualify("[<<<EOT\n    Defaults::MODE\n    EOT, Labels::A]").as_deref(),
            Some("[<<<EOT\n    Defaults::MODE\n    EOT, \\Lib\\Labels::A]")
        );
        assert_eq!(
            qualify("[<<<'EOT'\nDefaults::MODE\nEOT\n]"),
            None,
            "a nowdoc's body is not code"
        );
    }

    #[test]
    fn a_comment_between_the_class_and_its_member_keeps_it_a_class() {
        assert_eq!(
            qualify("Defaults /* the defaults */ ::MODE").as_deref(),
            Some("\\Vendor\\Defaults /* the defaults */ ::MODE")
        );
    }

    #[test]
    fn a_namespace_relative_name_is_in_the_current_namespace() {
        assert_eq!(
            qualify("namespace\\Defaults::MODE").as_deref(),
            Some("\\Lib\\Defaults::MODE")
        );
        assert_eq!(
            qualify_class_names("namespace\\Defaults::MODE", None, &|name| name.to_string())
                .as_deref(),
            Some("\\Defaults::MODE")
        );
    }

    /// Qualifies constants the way a file in `namespace Lib;` with
    /// `use Vendor\Sub;` and `use const Vendor\DEFAULT_LIMIT;` would.
    fn qualify_constants(expression: &str) -> Option<String> {
        let use_map = HashMap::from([("Sub".to_string(), "Vendor\\Sub".to_string())]);
        let constant_imports = HashMap::from([(
            "DEFAULT_LIMIT".to_string(),
            "Vendor\\DEFAULT_LIMIT".to_string(),
        )]);
        qualify_constant_names(
            expression,
            &Some("Lib".to_string()),
            &use_map,
            &constant_imports,
        )
    }

    #[test]
    fn qualifies_constants_with_one_meaning() {
        assert_eq!(
            qualify_constants("DEFAULT_LIMIT").as_deref(),
            Some("\\Vendor\\DEFAULT_LIMIT")
        );
        assert_eq!(
            qualify_constants("Sub\\DEPTH").as_deref(),
            Some("\\Vendor\\Sub\\DEPTH")
        );
        assert_eq!(
            qualify_constants("Other\\DEPTH").as_deref(),
            Some("\\Lib\\Other\\DEPTH")
        );
        assert_eq!(
            qualify_constants("namespace\\LIMIT").as_deref(),
            Some("\\Lib\\LIMIT")
        );
        assert_eq!(
            qualify_constants("[DEFAULT_LIMIT => LIMIT, 'k' => Sub\\DEPTH * 2]").as_deref(),
            Some("[\\Vendor\\DEFAULT_LIMIT => LIMIT, 'k' => \\Vendor\\Sub\\DEPTH * 2]")
        );
    }

    #[test]
    fn keeps_names_that_are_not_a_constant_to_qualify() {
        // An unqualified name no import claims is read in either namespace.
        assert_eq!(qualify_constants("LIMIT | PHP_INT_SIZE"), None);
        // Already fully qualified.
        assert_eq!(qualify_constants("\\Vendor\\DEFAULT_LIMIT"), None);
        // A class, a member and a function, however they are named.
        assert_eq!(qualify_constants("DEFAULT_LIMIT::A"), None);
        assert_eq!(qualify_constants("self::DEFAULT_LIMIT"), None);
        assert_eq!(qualify_constants("new DEFAULT_LIMIT()"), None);
        assert_eq!(qualify_constants("DEFAULT_LIMIT()"), None);
        // Text that only spells the name.
        assert_eq!(qualify_constants("'DEFAULT_LIMIT \\\\ Sub\\\\DEPTH'"), None);
        assert_eq!(qualify_constants("1 /* DEFAULT_LIMIT */"), None);
    }

    #[test]
    fn qualifies_unqualified_constants_the_resolver_answers_for() {
        let resolve = |name: &str| (name == "LIMIT").then(|| "Lib\\LIMIT".to_string());
        assert_eq!(
            qualify_unqualified_constant_names("LIMIT * 2 + OTHER", resolve).as_deref(),
            Some("\\Lib\\LIMIT * 2 + OTHER")
        );
        assert_eq!(
            qualify_unqualified_constant_names("Sub\\LIMIT", resolve),
            None,
            "a qualified name has one meaning already"
        );
        assert!(!names_unqualified_constant("'text' . 42"));
        assert!(!names_unqualified_constant("[true, null, FALSE]"));
        assert!(!names_unqualified_constant("\\Lib\\LIMIT"));
        assert!(!names_unqualified_constant("self::LIMIT"));
        assert!(names_unqualified_constant("[LIMIT]"));
    }

    #[test]
    fn a_global_name_is_still_spelled_fully_qualified() {
        // The reading file's own imports must not get a say in it.
        assert_eq!(
            qualify_class_names("Defaults::MODE", None, &|name| name.to_string()).as_deref(),
            Some("\\Defaults::MODE")
        );
    }
}
