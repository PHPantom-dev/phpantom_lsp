//! Destructuring offset diagnostic.
//!
//! `[$a, $b] = $x` reads offsets `0` and `1` of `$x`, and `['id' => $id] =
//! $x` reads offset `'id'`.  When the array's type rules an offset out (a
//! string-keyed array read positionally, a sealed shape without the key),
//! the target is assigned `null` and PHP warns about an undefined key.
//! This flags those elements.
//!
//! Only an array type that settles the question is judged.  Anything that
//! may hold the key, or that is not an array at all (`mixed`, an object
//! implementing `ArrayAccess`, an unresolved type), is left alone.

use mago_span::HasSpan;
use mago_syntax::cst::array::ArrayElement;
use mago_syntax::cst::assignment::{Assignment, AssignmentOperator};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::walker::Walker;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::literal_bytes_to_str;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::variable::forward_walk::{
    destructured_element_type, destructuring_rhs_type,
};

use super::helpers::make_diagnostic;

/// An offset a destructuring element reads, as PHP stores it: a decimal
/// integer string such as `'1'` is the integer key `1`.
#[derive(Clone)]
enum OffsetKey {
    Int(i64),
    Str(String),
}

impl OffsetKey {
    fn from_string(s: &str) -> Self {
        match s.parse::<i64>() {
            Ok(n) if n.to_string() == s => OffsetKey::Int(n),
            _ => OffsetKey::Str(s.to_string()),
        }
    }

    /// The key as [`destructured_element_type`] and shape lookups spell it.
    fn as_lookup(&self) -> String {
        match self {
            OffsetKey::Int(n) => n.to_string(),
            OffsetKey::Str(s) => s.clone(),
        }
    }

    fn display(&self) -> String {
        match self {
            OffsetKey::Int(n) => n.to_string(),
            OffsetKey::Str(s) => format!("'{s}'"),
        }
    }
}

struct OffsetIssue {
    start: usize,
    end: usize,
    key: String,
    array_type: String,
}

impl Backend {
    pub fn collect_destructuring_offset_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let file_ctx = self.file_context(uri);
        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&file_ctx);

        let issues: Vec<OffsetIssue> =
            with_parsed_program(content, "destructuring_offset_diagnostics", |program, _| {
                let mut assignments: Vec<&Assignment<'_>> = Vec::new();
                let walker = DestructuringCollector;
                for stmt in program.statements.iter() {
                    walker.walk_statement(stmt, &mut assignments);
                }

                let mut issues = Vec::new();
                for assignment in assignments {
                    let offset = assignment.span().start.offset;
                    if !super::incremental::reports_site(offset) {
                        continue;
                    }
                    let var_ctx = loaders.var_ctx_at(content, offset);

                    if let Some(rhs_type) = destructuring_rhs_type(assignment, &var_ctx) {
                        check_pattern(assignment.lhs, &rhs_type, &mut issues);
                    }
                }
                issues
            });

        for issue in &issues {
            let Some(range) = self.offset_range_to_lsp_range(uri, content, issue.start, issue.end)
            else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::WARNING,
                "unknown_offset",
                format!(
                    "Offset {} does not exist on {}",
                    issue.key, issue.array_type
                ),
            ));
        }
    }
}

struct DestructuringCollector;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<&'ast Assignment<'arena>>> for DestructuringCollector {
    fn walk_in_assignment(
        &self,
        assignment: &'ast Assignment<'arena>,
        data: &mut Vec<&'ast Assignment<'arena>>,
    ) {
        if matches!(assignment.operator, AssignmentOperator::Assign(_))
            && matches!(assignment.lhs, Expression::Array(_) | Expression::List(_))
        {
            data.push(assignment);
        }
    }
}

/// Report every element of the destructuring pattern `lhs` whose offset
/// `rhs_type` cannot hold, descending into nested patterns whose offset
/// it may.
fn check_pattern(lhs: &Expression<'_>, rhs_type: &PhpType, issues: &mut Vec<OffsetIssue>) {
    let elements = match lhs {
        Expression::Array(arr) => arr.elements.iter(),
        Expression::List(list) => list.elements.iter(),
        _ => return,
    };

    let mut positional_index: i64 = 0;
    for elem in elements {
        let (key, value) = match elem {
            ArrayElement::KeyValue(kv) => match literal_key(kv.key) {
                Some(key) => (key, kv.value),
                None => continue,
            },
            ArrayElement::Value(val) => {
                let key = OffsetKey::Int(positional_index);
                positional_index += 1;
                (key, val.value)
            }
            ArrayElement::Missing(_) => {
                positional_index += 1;
                continue;
            }
            _ => continue,
        };

        if lacks_offset(rhs_type, &key) == Some(true) {
            let span = elem.span();
            issues.push(OffsetIssue {
                start: span.start.offset as usize,
                end: span.end.offset as usize,
                key: key.display(),
                array_type: rhs_type.to_string(),
            });
            continue;
        }

        if matches!(value, Expression::Array(_) | Expression::List(_))
            && let Some(inner) = destructured_element_type(rhs_type, Some(&key.as_lookup()))
        {
            check_pattern(value, &inner, issues);
        }
    }
}

/// The offset a literal destructuring key names, `None` for any other key
/// expression (a constant, a variable).
fn literal_key(expr: &Expression<'_>) -> Option<OffsetKey> {
    match expr {
        Expression::Literal(Literal::String(lit)) => {
            let value = literal_bytes_to_str(lit.value?)?;
            Some(OffsetKey::from_string(value))
        }
        Expression::Literal(Literal::Integer(lit)) => {
            Some(OffsetKey::Int(i64::try_from(lit.value?).ok()?))
        }
        _ => None,
    }
}

/// Whether `ty` is an array that cannot hold `key`.
///
/// `Some(true)` when it cannot, `Some(false)` when it may, and `None`
/// when `ty` is not an array whose keys are known.  A union cannot hold
/// the key only when none of its members can; `null` takes no part,
/// because destructuring it assigns `null` without a warning.
fn lacks_offset(ty: &PhpType, key: &OffsetKey) -> Option<bool> {
    if let Some(parts) = ty.shape_parts() {
        if parts
            .entries
            .iter()
            .any(|entry| entry.key.as_deref().is_some_and(|k| k.contains("::")))
        {
            return None;
        }
        let mut positional = 0i64;
        for entry in parts.entries {
            let entry_key = match entry.key.as_deref() {
                Some(k) => OffsetKey::from_string(k.trim_matches(['\'', '"'])),
                None => {
                    positional += 1;
                    OffsetKey::Int(positional - 1)
                }
            };
            if same_key(&entry_key, key) {
                return Some(false);
            }
        }
        return match parts.tail {
            Some((tail_key, _)) => key_type_lacks(tail_key, key),
            None => Some(true),
        };
    }

    match ty.kind() {
        TypeKind::Nullable(inner) => lacks_offset(inner, key),
        TypeKind::Union(members) => {
            let mut any_array = false;
            for member in members.iter().filter(|m| !m.is_null()) {
                if !lacks_offset(member, key)? {
                    return Some(false);
                }
                any_array = true;
            }
            any_array.then_some(true)
        }
        TypeKind::Array(_) => Some(false),
        TypeKind::Generic(g) => match g.name.to_ascii_lowercase().as_str() {
            "list" | "non-empty-list" => key_type_lacks(&PhpType::int(), key),
            "array" | "non-empty-array" if g.args.len() >= 2 => key_type_lacks(&g.args[0], key),
            "array" | "non-empty-array" => Some(false),
            _ => None,
        },
        TypeKind::Named(name)
            if name.eq_ignore_ascii_case("array")
                || name.eq_ignore_ascii_case("non-empty-array") =>
        {
            Some(false)
        }
        _ => None,
    }
}

fn same_key(a: &OffsetKey, b: &OffsetKey) -> bool {
    match (a, b) {
        (OffsetKey::Int(x), OffsetKey::Int(y)) => x == y,
        (OffsetKey::Str(x), OffsetKey::Str(y)) => x == y,
        _ => false,
    }
}

/// Whether an array keyed by `key_type` cannot hold `key`.
///
/// Only the two plain key families are judged: integer keys never hold a
/// string offset, and string keys never hold an integer one.  A string
/// key type that admits numeric strings is not judged, because PHP stores
/// such a key as an integer.
fn key_type_lacks(key_type: &PhpType, key: &OffsetKey) -> Option<bool> {
    if key_type.is_int_subtype() {
        return Some(matches!(key, OffsetKey::Str(_)));
    }
    if key_type.is_string_subtype() {
        if admits_numeric_string(key_type) {
            return Some(false);
        }
        return Some(matches!(key, OffsetKey::Int(_)));
    }
    Some(false)
}

fn admits_numeric_string(key_type: &PhpType) -> bool {
    match key_type.kind() {
        TypeKind::Named(name) => name.eq_ignore_ascii_case("numeric-string"),
        TypeKind::Literal(lit) => lit.is_numeric_string(),
        TypeKind::Nullable(inner) => admits_numeric_string(inner),
        TypeKind::Union(members) => members.iter().any(admits_numeric_string),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_lacks(ty: &str, key: OffsetKey) -> Option<bool> {
        lacks_offset(&PhpType::parse(ty), &key)
    }

    #[test]
    fn literal_numeric_string_is_an_int_key() {
        assert!(matches!(OffsetKey::from_string("1"), OffsetKey::Int(1)));
        assert!(matches!(OffsetKey::from_string("01"), OffsetKey::Str(_)));
        assert!(matches!(OffsetKey::from_string("a"), OffsetKey::Str(_)));
    }

    #[test]
    fn judges_key_families() {
        assert_eq!(
            key_lacks("array<string, int>", OffsetKey::Int(0)),
            Some(true)
        );
        assert_eq!(
            key_lacks("array<string, int>", OffsetKey::Str("a".into())),
            Some(false)
        );
        assert_eq!(
            key_lacks("list<int>", OffsetKey::Str("a".into())),
            Some(true)
        );
        assert_eq!(key_lacks("list<int>", OffsetKey::Int(3)), Some(false));
        assert_eq!(
            key_lacks("array<numeric-string, int>", OffsetKey::Int(0)),
            Some(false)
        );
        assert_eq!(key_lacks("int[]", OffsetKey::Int(0)), Some(false));
        assert_eq!(key_lacks("mixed", OffsetKey::Int(0)), None);
    }

    #[test]
    fn judges_shapes() {
        assert_eq!(
            key_lacks("array{a: int}", OffsetKey::Str("b".into())),
            Some(true)
        );
        assert_eq!(
            key_lacks("array{a?: int}", OffsetKey::Str("a".into())),
            Some(false)
        );
        assert_eq!(
            key_lacks("array{int, string}", OffsetKey::Int(1)),
            Some(false)
        );
        assert_eq!(
            key_lacks("array{int, string}", OffsetKey::Int(2)),
            Some(true)
        );
        assert_eq!(key_lacks("array{0: int}", OffsetKey::Int(0)), Some(false));
        assert_eq!(
            key_lacks("array{a: int}|null", OffsetKey::Int(0)),
            Some(true)
        );
        assert_eq!(
            key_lacks("array{a: int}|array{b: int}", OffsetKey::Str("b".into())),
            Some(false)
        );
        assert_eq!(key_lacks("array{a: int}|mixed", OffsetKey::Int(0)), None);
    }
}
