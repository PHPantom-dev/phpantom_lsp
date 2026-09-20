//! Shared resolution helpers for type narrowing: subject-key
//! extraction and resolving class-name lists into `ClassInfo` unions.

use std::sync::Arc;

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;

use mago_syntax::cst::*;

use crate::type_engine::resolver::VarResolutionCtx;

use super::*;

/// Resolve the `class_type` inside an `InstanceofExtraction` to its FQN.
///
/// When the extractor returns a short class name (e.g. `Foo`), the
/// `class_loader` may know the fully-qualified name (`App\Foo`).
/// Resolving early ensures that downstream comparisons (e.g.
/// `out.contains(&cls_type)`) and `ResolvedType` hints carry the FQN
/// rather than the short name.
pub(in crate::type_engine) fn resolve_extraction_to_fqn(
    extraction: &mut InstanceofExtraction,
    class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<ClassInfo>>,
) {
    if let TypeKind::Named(name) = extraction.class_type.kind() {
        let resolved = crate::util::resolve_name_via_loader(name, class_loader);
        if resolved != *name {
            extraction.class_type = PhpType::named(atom(&resolved));
        }
    }
}

/// Resolve the classes an `instanceof`-style check names.
///
/// The same as [`type_hint_to_classes_typed`] except for one case it has
/// no way to answer: a trait.  No value is ever an instance of a trait,
/// so a check that resolves to one is really a check against the classes
/// that use it — which is what `instanceof self` inside a trait method
/// means, `self` there being the host class rather than the trait.
/// Narrowing to the trait instead loses every member the hosts declare
/// themselves, since a trait need not declare what its methods use.
///
/// [`type_hint_to_classes_typed`]: super::super::resolution::type_hint_to_classes_typed
pub(in crate::type_engine) fn resolve_narrowing_target(
    ty: &PhpType,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<Arc<ClassInfo>> {
    let resolved = super::super::resolution::type_hint_to_classes_typed(
        ty,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    let mut out: Vec<Arc<ClassInfo>> = Vec::with_capacity(resolved.len());
    for cls in resolved {
        let hosts = crate::type_engine::trait_context::trait_host_classes(
            &cls,
            ctx.all_classes,
            ctx.class_loader,
            ctx.backend,
        );
        match hosts.is_empty() {
            // Either not a trait, or one whose hosts we cannot enumerate;
            // the trait itself is still the best answer available.
            true => ClassInfo::push_unique_arc(&mut out, cls),
            false => ClassInfo::extend_unique_arc(&mut out, hosts),
        }
    }
    out
}

/// Resolve a list of `PhpType` values into a deduplicated `Vec<ClassInfo>`.
///
/// This is a shared helper for the compound instanceof/assert narrowing
/// patterns that produce a union of classes from multiple branches.
pub(crate) fn resolve_class_names_to_union(
    classes: &[PhpType],
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ClassInfo> {
    let mut union = Vec::new();
    for ty in classes {
        for arc_cls in resolve_narrowing_target(ty, ctx) {
            ClassInfo::push_unique(&mut union, Arc::unwrap_or_clone(arc_cls));
        }
    }
    union
}

/// The subject key a property read keys under: the receiver's own key
/// followed by `->` and the property name.
fn property_subject_key(
    object: &Expression<'_>,
    property: &ClassLikeMemberSelector<'_>,
) -> Option<String> {
    let object_key = expr_to_subject_key(object)?;
    let ClassLikeMemberSelector::Identifier(ident) = property else {
        return None;
    };
    Some(format!("{}->{}", object_key, bytes_to_str(ident.value)))
}

/// Convert an AST expression to a subject key string for narrowing comparison.
///
/// Handles:
/// - `$var` → `"$var"`
/// - `$this->prop` → `"$this->prop"`
/// - `$this?->prop` → `"$this->prop"` (null-safe normalised)
/// - `$this->get()` → `"$this->get()"`
/// - `$this->option('from')` → `"$this->option('from')"`
/// - `mb_strpos($s, $m)` → `"mb_strpos($s,$m)"`
/// - `Carbon::parse($s)` → `"Carbon::parse($s)"`
///
/// Returns `None` for expressions that are not supported as narrowing subjects.
pub(in crate::type_engine) fn expr_to_subject_key(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => Some(bytes_to_str(dv.name).to_string()),
        // `->` and `?->` read the same storage, so both key under `->`.
        Expression::Access(
            Access::Property(PropertyAccess {
                object, property, ..
            })
            | Access::NullSafeProperty(NullSafePropertyAccess {
                object, property, ..
            }),
        ) => property_subject_key(object, property),
        // `self::$repo`, `static::$repo`, `Foo::$repo` — keyed under the
        // class as the source names it.  Two spellings of the same
        // storage (`self::$x` in `Foo` and `Foo::$x`) get different keys,
        // which costs a narrowing but never claims a wrong one.
        Expression::Access(Access::StaticProperty(sp)) => {
            let class = static_class_key(sp.class)?;
            let Variable::Direct(dv) = &sp.property else {
                return None;
            };
            Some(format!("{}::{}", class, bytes_to_str(dv.name)))
        }
        Expression::ArrayAccess(aa) => array_access_subject_key(aa),
        // A call keyed under its own written form: checking one call and
        // using another is the idiom the check is written for
        // (`if ($h->get() instanceof Foo) { $h->get()->m(); }`,
        // `if (mb_strpos($s, $m) !== false) { mb_substr($s, 0,
        // mb_strpos($s, $m)); }`), so the two occurrences share a key.
        // The arguments are part of the key, and each of them has to be a
        // subject in its own right — a call whose argument is itself a
        // statement (an assignment, an increment) is not the same call
        // twice.
        Expression::Call(
            Call::Method(MethodCall {
                object,
                method,
                argument_list,
                ..
            })
            | Call::NullSafeMethod(NullSafeMethodCall {
                object,
                method,
                argument_list,
                ..
            }),
        ) => method_call_key(object, method, argument_list),
        Expression::Call(Call::StaticMethod(sc)) => {
            let class = static_class_key(sc.class)?;
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return None;
            };
            let args = argument_list_key(&sc.argument_list)?;
            Some(format!(
                "{}::{}({})",
                class,
                bytes_to_str(ident.value),
                args
            ))
        }
        Expression::Call(Call::Function(fc)) => {
            let Expression::Identifier(ident) = fc.function else {
                return None;
            };
            let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
            if !is_deterministic_function(name) {
                return None;
            }
            let args = argument_list_key(&fc.argument_list)?;
            // PHP function names are case-insensitive, so `STRPOS(...)` and
            // `strpos(...)` are the same call.
            Some(format!("{}({})", name.to_ascii_lowercase(), args))
        }
        // See through parentheses so `($x instanceof Foo)` and grouped
        // subjects resolve to the same key as the bare form.
        Expression::Parenthesized(inner) => expr_to_subject_key(inner.expression),
        // Inline assignment as a subject: `($node = expr()) instanceof Foo`
        // narrows the assigned variable, so key on the assignment target.
        Expression::Assignment(assign) => expr_to_subject_key(assign.lhs),
        _ => None,
    }
}

/// Whether a scope key names a call rather than a variable or a
/// property/array path.
///
/// Every call key ends in the closing parenthesis of its argument list,
/// which no other key shape does.
pub(in crate::type_engine) fn is_call_key(key: &str) -> bool {
    key.ends_with(')')
}

/// Whether a call key carries arguments (`$this->option('from')`) rather
/// than being the argument-less form (`$this->get()`).
///
/// The two are seeded into the forward walker's scope by different
/// routes: an argument-less call key is re-resolved from the key text,
/// while an argument-carrying one is resolved from the expression it was
/// built from.
pub(in crate::type_engine) fn is_call_key_with_arguments(key: &str) -> bool {
    is_call_key(key) && !key.ends_with("()")
}

/// Whether a scope key names a path through a member rather than a bare
/// variable: `$a->b`, `$a["k"]`, `self::$b`.
///
/// These are the keys the walker seeds and strips as *narrowing* rather
/// than tracking as locals, so every gate that asks "is this a compound
/// subject?" has to agree on the answer.
pub(in crate::type_engine) fn is_member_path_key(key: &str) -> bool {
    key.contains("->") || key.contains('[') || key.contains("::$")
}

/// Split a subject key's trailing bracket segment off its base:
/// `$a["x"][$i]` → `("$a[\"x\"]", "$i")`.
///
/// Returns `None` unless the key *ends* in a bracket segment, so a key
/// that only reads through one (`$a["x"]->b`, `f($a["x"])`) is left to
/// the dispatcher arm that owns its trailing access. Brackets nest
/// (`$a[$b["k"]]`) and a literal key may itself contain one, so the scan
/// tracks depth and quoting rather than searching for `["`.
pub(in crate::type_engine) fn split_trailing_bracket(key: &str) -> Option<(&str, &str)> {
    if !key.ends_with(']') {
        return None;
    }
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut last_open = None;
    for (index, byte) in key.bytes().enumerate() {
        if quoted {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'[' => {
                if depth == 0 {
                    last_open = Some(index);
                }
                depth += 1;
            }
            b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if depth != 0 || quoted {
        return None;
    }
    let open = last_open?;
    Some((&key[..open], &key[open + 1..key.len() - 1]))
}

/// The literal key a bracket segment names, or `None` when the segment is
/// a variable index (`[$path]`) that addresses an element the shape
/// cannot name.
pub(in crate::type_engine) fn bracket_segment_literal(segment: &str) -> Option<&str> {
    segment
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
}

/// Whether `key` reads `var_name`, so that writing to the variable makes
/// whatever was tracked for the key stale.
///
/// A key rooted at the variable (`$row->id`, `$row["k"]`, `$cls::$id`) is
/// caught by a prefix test; a call key mentions its inputs anywhere inside
/// the argument list (`findPos($slug, $marker)`), so those are matched on
/// a token boundary — `$slug` must not match `$slugger`.
///
/// A variable index is read the same way (`$state["files"][$path]` stops
/// describing anything once `$path` moves), but only inside the brackets:
/// scanning the whole key would make writing `$repo` invalidate
/// `self::$repo`, which is different storage.
pub(in crate::type_engine) fn key_reads_variable(key: &str, var_name: &str) -> bool {
    if var_name.is_empty() {
        return false;
    }
    if let Some(rest) = key.strip_prefix(var_name)
        && (rest.starts_with("->") || rest.starts_with('[') || rest.starts_with("::"))
    {
        return true;
    }
    let bracketed_only = !is_call_key(key);
    if bracketed_only && !key.contains('[') {
        return false;
    }
    let mut depth = 0usize;
    let mut offset = 0;
    let mut rest = key;
    while let Some(pos) = rest.find(var_name) {
        if bracketed_only {
            depth = bracket_depth_at(key, offset, offset + pos, depth);
            offset += pos;
        }
        rest = &rest[pos + var_name.len()..];
        if (!bracketed_only || depth > 0) && !rest.starts_with(is_name_char) {
            return true;
        }
        if bracketed_only {
            offset += var_name.len();
        }
    }
    false
}

/// Advance a running bracket-nesting depth across `key[from..to]`.
fn bracket_depth_at(key: &str, from: usize, to: usize, mut depth: usize) -> usize {
    for byte in key.as_bytes()[from..to].iter() {
        match byte {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}

/// Whether `c` can continue a PHP identifier, so `$slug` is not found
/// inside `$slugger`.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()
}

/// Build the subject key for an array access: `$a["k"]` for a literal
/// index, `$a[$i]` or `$a[$count-2]` for a computed one.
///
/// A computed index addresses one element just as a literal one does, for
/// as long as every variable it reads holds the value it held at the
/// check. Writing to one of them drops the key — [`key_reads_variable`]
/// sees the index inside the brackets — which is the rule a call key's
/// arguments already follow. That is also why an index with no variable
/// in it is left out: it buys nothing the literal form does not already
/// cover. The index is left unquoted so it cannot collide with a literal
/// key that happens to spell a variable name.
///
/// Kept out of [`expr_to_subject_key`] so its frame stays small: a long
/// method chain recurses once per link and pays for every local the match
/// arms declare.
#[inline(never)]
fn array_access_subject_key(aa: &mago_syntax::cst::ArrayAccess<'_>) -> Option<String> {
    let base = expr_to_subject_key(aa.array)?;
    if let Some(key) = array_access_key_as_string(aa) {
        return Some(format!("{}[\"{}\"]", base, key));
    }
    let index = array_index_key(aa.index)?;
    index.contains('$').then(|| format!("{}[{}]", base, index))
}

/// Render an index expression that is not a literal key.
///
/// A bare variable (`$i`) is the common case; an offset computed from one
/// (`$count - 2`, `$i + 1`) is the same read written with arithmetic, so
/// it renders to a key too. Spaces are dropped so `$count - 2` and
/// `$count-2` are one subject, and parentheses are kept so `1-($i-2)` and
/// `(1-$i)-2` are not.
///
/// Only the arithmetic operators are rendered. An index that writes
/// (`$i++`), concatenates, or compares is not the same read twice, and
/// falls back to whatever [`expr_to_subject_key`] makes of it — for those
/// shapes, nothing.
pub(crate) fn array_index_key(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Binary(bin) => {
            let operator = arithmetic_operator_text(&bin.operator)?;
            let lhs = array_index_key(bin.lhs)?;
            let rhs = array_index_key(bin.rhs)?;
            Some(format!("{}{}{}", lhs, operator, rhs))
        }
        Expression::UnaryPrefix(unary) => {
            use mago_syntax::cst::unary::UnaryPrefixOperator;
            let sign = match unary.operator {
                UnaryPrefixOperator::Negation(_) => '-',
                UnaryPrefixOperator::Plus(_) => '+',
                _ => return None,
            };
            Some(format!("{}{}", sign, array_index_key(unary.operand)?))
        }
        Expression::Parenthesized(inner) => {
            let rendered = array_index_key(inner.expression)?;
            // Only a compound operand needs its grouping recorded; `($i)`
            // and `$i` are the same read and should share a key.
            Some(match inner.expression {
                Expression::Binary(_) => format!("({})", rendered),
                _ => rendered,
            })
        }
        Expression::Literal(Literal::Integer(i)) => Some(
            i.value
                .map(|v| v.to_string())
                .unwrap_or_else(|| bytes_to_str(i.raw).to_string()),
        ),
        other => expr_to_subject_key(other),
    }
}

/// The written form of an operator that computes an offset, or `None` for
/// one that does something else with its operands.
fn arithmetic_operator_text(operator: &BinaryOperator<'_>) -> Option<&'static str> {
    match operator {
        BinaryOperator::Addition(_) => Some("+"),
        BinaryOperator::Subtraction(_) => Some("-"),
        BinaryOperator::Multiplication(_) => Some("*"),
        BinaryOperator::Division(_) => Some("/"),
        BinaryOperator::Modulo(_) => Some("%"),
        BinaryOperator::Exponentiation(_) => Some("**"),
        _ => None,
    }
}

/// Build the subject key for a method call, matching the
/// `$obj->method(args)` text form the resolver's subject keys use.
fn method_call_key(
    object: &Expression<'_>,
    method: &ClassLikeMemberSelector<'_>,
    argument_list: &ArgumentList<'_>,
) -> Option<String> {
    let obj = expr_to_subject_key(object)?;
    let ClassLikeMemberSelector::Identifier(ident) = method else {
        return None;
    };
    let args = argument_list_key(argument_list)?;
    Some(format!("{}->{}({})", obj, bytes_to_str(ident.value), args))
}

/// The key for the class side of a static call: a written class name, one
/// of the class keywords, or an expression that is a subject in its own
/// right (`$class::make(…)`).
fn static_class_key(class: &Expression<'_>) -> Option<String> {
    match class {
        Expression::Identifier(ident) => {
            Some(crate::util::strip_fqn_prefix(bytes_to_str(ident.value())).to_string())
        }
        Expression::Self_(_) => Some("self".to_string()),
        Expression::Static(_) => Some("static".to_string()),
        Expression::Parent(_) => Some("parent".to_string()),
        other => expr_to_subject_key(other),
    }
}

/// Render an argument list into the comma-separated form a call key uses,
/// or `None` when any argument is not something two occurrences of the
/// call can be compared on.
fn argument_list_key(argument_list: &ArgumentList<'_>) -> Option<String> {
    let mut out = String::new();
    for (index, argument) in argument_list.arguments.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        match argument {
            Argument::Positional(positional) => {
                // `f(...$args)` spreads an array of unknown length, so the
                // written form does not say what the call receives.
                positional.ellipsis.is_none().then_some(())?;
                out.push_str(&argument_value_key(positional.value)?);
            }
            Argument::Named(named) => {
                out.push_str(bytes_to_str(named.name.value));
                out.push(':');
                out.push_str(&argument_value_key(named.value)?);
            }
        }
    }
    Some(out)
}

/// Render one argument value, or `None` when it is not deterministic.
///
/// Literals and constants render to their value; everything else has to
/// be a subject key in its own right, which is what makes a nested call
/// (`f(g($x))`) and a property argument (`f($this->name)`) usable and
/// rules out anything that writes (`f($i++)`, `f($x = 1)`).
fn argument_value_key(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Literal(Literal::String(s)) => {
            let raw_str = bytes_to_str(s.raw);
            let value = match s.value {
                Some(bytes) => literal_bytes_to_str(bytes)?,
                None => crate::text_scan::unquote_php_string(raw_str).unwrap_or(raw_str),
            };
            // Escape so that two different strings cannot render to the
            // same key: `f("a'b")` and `f("a", "b")` must stay distinct.
            let mut out = String::with_capacity(value.len() + 2);
            out.push('\'');
            for c in value.chars() {
                if c == '\\' || c == '\'' {
                    out.push('\\');
                }
                out.push(c);
            }
            out.push('\'');
            Some(out)
        }
        Expression::Literal(Literal::Integer(i)) => Some(
            i.value
                .map(|v| v.to_string())
                .unwrap_or_else(|| bytes_to_str(i.raw).to_string()),
        ),
        Expression::Literal(Literal::Float(f)) => Some(bytes_to_str(f.raw).to_string()),
        Expression::Literal(Literal::True(_)) => Some("true".to_string()),
        Expression::Literal(Literal::False(_)) => Some("false".to_string()),
        Expression::Literal(Literal::Null(_)) => Some("null".to_string()),
        Expression::ConstantAccess(ca) => {
            Some(crate::util::strip_fqn_prefix(bytes_to_str(ca.name.value())).to_string())
        }
        Expression::Access(Access::ClassConstant(cc)) => {
            let class = static_class_key(cc.class)?;
            match &cc.constant {
                ClassLikeConstantSelector::Identifier(ident) => {
                    Some(format!("{}::{}", class, bytes_to_str(ident.value)))
                }
                _ => None,
            }
        }
        Expression::Parenthesized(inner) => argument_value_key(inner.expression),
        other => expr_to_subject_key(other),
    }
}

/// Whether repeating a call to `name` in the same scope yields the same
/// value, so that a check on one occurrence describes the next.
///
/// Almost every function qualifies; the ones that do not either advance a
/// cursor they read from (`fgets`, `array_shift`, `next`), consult the
/// clock, or draw a random number.  Reusing a check across two of those
/// is exactly the case that would report a type the second call cannot be
/// relied on to have.
fn is_deterministic_function(name: &str) -> bool {
    // Matched case-insensitively because PHP function names are.
    !NON_DETERMINISTIC_FUNCTIONS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

/// Functions whose result depends on something other than their
/// arguments: the position of a stream or array cursor, the clock, or a
/// source of randomness.
const NON_DETERMINISTIC_FUNCTIONS: &[&str] = &[
    // Stream and file cursors.
    "fgetc",
    "fgetcsv",
    "fgets",
    "fgetss",
    "fread",
    "fscanf",
    "fpassthru",
    "feof",
    "ftell",
    "readline",
    "stream_get_contents",
    "stream_get_line",
    "readdir",
    "socket_read",
    "curl_exec",
    "curl_multi_getcontent",
    // Array cursors and array mutators that return an element.
    "array_pop",
    "array_shift",
    "array_splice",
    "current",
    "each",
    "end",
    "key",
    "next",
    "pos",
    "prev",
    "reset",
    // Iterator and generator state.
    "iterator_to_array",
    // Databases, sessions and output buffers hand back the next row or
    // the buffer as it stands right now.
    "mysqli_fetch_array",
    "mysqli_fetch_assoc",
    "mysqli_fetch_object",
    "mysqli_fetch_row",
    "pg_fetch_array",
    "pg_fetch_assoc",
    "pg_fetch_object",
    "pg_fetch_row",
    "ob_get_clean",
    "ob_get_contents",
    "ob_get_flush",
    "session_id",
    // The clock.
    "date",
    "getdate",
    "gettimeofday",
    "gmdate",
    "hrtime",
    "idate",
    "localtime",
    "microtime",
    "mktime",
    "gmmktime",
    "strtotime",
    "time",
    // Randomness.
    "lcg_value",
    "mt_rand",
    "rand",
    "random_bytes",
    "random_int",
    "shuffle",
    "str_shuffle",
    "uniqid",
    "array_rand",
];

/// Extract a literal key from an array access expression.
///
/// Returns the key string for `$a["test"]`, `$a['test']`, and `$a[0]`
/// (integer indices are stringified, matching PHP's integer/string key
/// coercion so `$a[0]` and `$a["0"]` narrow the same subject).  Returns
/// `None` for non-literal keys like `$a[$i]`.
pub(in crate::type_engine) fn array_access_key_as_string(
    aa: &mago_syntax::cst::ArrayAccess<'_>,
) -> Option<String> {
    array_index_literal_key(aa.index)
}

/// The same literal-key extraction [`array_access_key_as_string`] does,
/// taken directly from an index expression rather than the `ArrayAccess`
/// node that wraps it. Lets a caller that only has the index (e.g. one
/// key of a write's flattened `key_chain`) render it the same way a read
/// through the full `ArrayAccess` would.
pub(in crate::type_engine) fn array_index_literal_key(index: &Expression<'_>) -> Option<String> {
    use mago_syntax::cst::Literal;
    match index {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to stripping
            // quotes from `raw`.
            let key = match s.value {
                Some(bytes) => literal_bytes_to_str(bytes)?.to_string(),
                None => {
                    let raw_str = bytes_to_str(s.raw);
                    crate::text_scan::unquote_php_string(raw_str)
                        .unwrap_or(raw_str)
                        .to_string()
                }
            };
            Some(key)
        }
        Expression::Literal(Literal::Integer(i)) => {
            // PHP normalises integer-like keys, so `$a[0]` narrows the
            // same subject as `$a["0"]`.  Prefer the parsed value; fall
            // back to the raw token when it overflowed.
            i.value
                .map(|v| v.to_string())
                .or_else(|| Some(bytes_to_str(i.raw).to_string()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_rooted_at_the_variable_reads_it() {
        assert!(key_reads_variable("$row->id", "$row"));
        assert!(key_reads_variable("$row[\"id\"]", "$row"));
        assert!(key_reads_variable("$this->a->b", "$this->a"));
    }

    #[test]
    fn a_sibling_variable_with_a_shared_prefix_is_not_read() {
        assert!(!key_reads_variable("$rows->id", "$row"));
        assert!(!key_reads_variable("$row", "$row"));
        assert!(!key_reads_variable("findPos($slugger,$m)", "$slug"));
        assert!(!key_reads_variable("findPos($x,$marker2)", "$marker"));
    }

    #[test]
    fn a_call_key_reads_every_argument_it_names() {
        assert!(key_reads_variable("findPos($slug,$marker)", "$slug"));
        assert!(key_reads_variable("findPos($slug,$marker)", "$marker"));
        assert!(key_reads_variable("$opts->option('k')", "$opts"));
        assert!(key_reads_variable("f(g($x))", "$x"));
        assert!(!key_reads_variable("findPos($slug,$marker)", "$other"));
    }

    #[test]
    fn only_a_call_key_ends_in_a_parenthesis() {
        assert!(is_call_key("$h->get()"));
        assert!(is_call_key("mb_strpos($s,$m)"));
        assert!(!is_call_key("$this->handle"));
        assert!(!is_call_key("$row[\"id\"]"));

        assert!(!is_call_key_with_arguments("$h->get()"));
        assert!(is_call_key_with_arguments("mb_strpos($s,$m)"));
        assert!(!is_call_key_with_arguments("$this->handle"));
    }

    #[test]
    fn a_variable_index_is_read_by_the_key_that_brackets_it() {
        assert!(key_reads_variable("$state[\"files\"][$path]", "$path"));
        assert!(key_reads_variable("$state[$path][\"violations\"]", "$path"));
        assert!(key_reads_variable("$a[$b[\"k\"]]", "$b"));
        assert!(!key_reads_variable("$state[\"files\"][$pathname]", "$path"));
        assert!(!key_reads_variable("$state[\"files\"][$other]", "$path"));
        // Different storage that merely spells the same name outside any
        // bracket: writing the local must not drop the static property.
        assert!(!key_reads_variable("self::$repo", "$repo"));
        assert!(!key_reads_variable("$this->path", "$path"));
    }

    #[test]
    fn the_trailing_bracket_segment_splits_off_its_base() {
        assert_eq!(
            split_trailing_bracket("$a[\"x\"][\"y\"]"),
            Some(("$a[\"x\"]", "\"y\""))
        );
        assert_eq!(
            split_trailing_bracket("$a[\"x\"][$i]"),
            Some(("$a[\"x\"]", "$i"))
        );
        assert_eq!(
            split_trailing_bracket("$a[$b[\"k\"]]"),
            Some(("$a", "$b[\"k\"]"))
        );
        // Not a trailing bracket: the key's last access is something else.
        assert_eq!(split_trailing_bracket("$a[\"x\"]->b"), None);
        assert_eq!(split_trailing_bracket("f($a[\"x\"])"), None);
        assert_eq!(split_trailing_bracket("$a"), None);

        assert_eq!(bracket_segment_literal("\"y\""), Some("y"));
        assert_eq!(bracket_segment_literal("$i"), None);
    }

    #[test]
    fn a_state_advancing_function_is_not_deterministic() {
        assert!(!is_deterministic_function("fgets"));
        assert!(!is_deterministic_function("FGETS"));
        assert!(!is_deterministic_function("array_shift"));
        assert!(!is_deterministic_function("time"));
        assert!(is_deterministic_function("mb_strpos"));
        assert!(is_deterministic_function("myOwnHelper"));
    }
}
