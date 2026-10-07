/// Array literal inference and array function helpers.
///
/// These are utility helpers that support the forward-walking variable
/// resolver in [`super::forward_walk`] and the foreach/destructuring
/// resolution module.
use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::array_func_rules::{ArrayFuncArgs, array_func_element_type, array_func_raw_type};

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::docblock;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

/// Infer the raw PHPStan-style type for an array literal (`[…]` or
/// `array(…)`) from its keys and value expressions.
///
/// The literal is built the way PHP builds it, one element at a time: a
/// constant key sets that entry (a repeated key overwrites the earlier one
/// in place), a value takes the next free integer key, and a spread copies
/// its source's entries across, keeping string keys and renumbering integer
/// keys onto the end. While every element's keys are known the result is a
/// shape. A spread of an array of unknown length leaves the entries written
/// beside it known, and makes the shape an unsealed one; a runtime key turns
/// it into `array<K, V>`/`list<T>`.
pub(in crate::type_engine) fn infer_array_literal_raw_type<'b>(
    elements: impl Iterator<Item = &'b ArrayElement<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let mut builder = LiteralBuilder::default();
    for elem in elements {
        match elem {
            ArrayElement::KeyValue(kv) => {
                let value_type = infer_element_type(kv.value, ctx).unwrap_or_else(PhpType::mixed);
                match extract_array_key_text(kv.key) {
                    Some(key_text) => {
                        builder.set_key(key_text, constant_key_type(kv.key), value_type);
                    }
                    // A key that is not a literal has no name to record, and
                    // naming the entry after the key's *type* would invent a
                    // shape field nobody wrote. The whole literal falls back
                    // to `array<K, V>` instead.
                    None => {
                        builder.loosen();
                        builder.is_list = false;
                        push_unique(&mut builder.key_types, dynamic_key_type(kv.key, ctx));
                        push_unique(&mut builder.types, value_type);
                    }
                }
            }
            ArrayElement::Value(v) => {
                // A positional shape must keep one entry per element to
                // preserve arity, so an unresolvable element becomes
                // `mixed`. The `list<T>` fallback ignores it instead.
                let resolved = infer_element_type(v.value, ctx);
                builder.append(resolved);
            }
            ArrayElement::Variadic(v) => {
                let raw = super::foreach_resolution::resolve_expression_type(v.value, ctx);
                builder.spread(raw.as_ref());
            }
            ArrayElement::Missing(_) => {}
        }
    }

    // A literal whose every entry is known is recorded as that shape, so
    // that integer-literal indexing (`$pair[1]`) and list destructuring
    // select the element at that position and out-of-bounds indices are
    // known to be absent. `[]` is `array{}` for the same reason: a bare
    // `array` would not say it is empty, and a later write's result could
    // not absorb it when branches rejoin.
    //
    // Entries written beside a spread of unknown length are still known one
    // by one, so they are kept as an unsealed shape with the spread's keys
    // and values as its tail. With nothing written beside it, the tail is
    // the whole array and the plain `array<K, V>`/`list<T>` says it.
    if let Some(exact) = builder.exact.take() {
        let positional = exact.iter().all(|(_, entry)| entry.key.is_none());
        let within_limit = !positional || exact.len() <= MAX_POSITIONAL_SHAPE_LEN;
        match builder.tail.take() {
            None if within_limit => {
                return Some(PhpType::array_shape(
                    exact.into_iter().map(|(_, entry)| entry).collect(),
                ));
            }
            Some(tail) if within_limit && !exact.is_empty() => {
                let entries = exact.into_iter().map(|(_, entry)| entry).collect();
                let shape = if builder.is_list && positional {
                    PhpType::list_shape(entries)
                } else {
                    PhpType::array_shape(entries)
                };
                let value = join_alternatives(tail.values, MAX_ELEMENT_ALTERNATIVES)
                    .unwrap_or_else(PhpType::mixed);
                return Some(PhpType::unsealed_shape(
                    shape,
                    join_key_types(tail.keys),
                    value,
                ));
            }
            _ => {}
        }
    }
    builder.loose_type()
}

/// Maximum number of positional entries to record as a tuple-style shape.
/// Beyond this the array is almost certainly a homogeneous collection
/// rather than a fixed-arity tuple, so it is widened to `list<T>` to avoid
/// unbounded shape growth.
const MAX_POSITIONAL_SHAPE_LEN: usize = 32;

/// The shape an array literal builds when every key and value in it is
/// written out, `value_type` typing each value.
///
/// This is the shape [`infer_array_literal_raw_type`] records for the same
/// literal, for a caller with no scope to resolve the elements against: a
/// comparison against `[null]` pins its subject to `array{null}`. `None`
/// when an element is a spread or has a key that is not a literal, when
/// `value_type` cannot type a value, and for a literal too long to be worth
/// a shape.
pub(in crate::type_engine) fn written_array_literal_type<'b>(
    elements: &[ArrayElement<'b>],
    value_type: impl Fn(&'b Expression<'b>) -> Option<PhpType>,
) -> Option<PhpType> {
    if elements.len() > MAX_POSITIONAL_SHAPE_LEN {
        return None;
    }
    let mut builder = LiteralBuilder::default();
    for elem in elements {
        match elem {
            ArrayElement::KeyValue(kv) => {
                let key_text = extract_array_key_text(kv.key)?;
                builder.set_key(key_text, constant_key_type(kv.key), value_type(kv.value)?);
            }
            ArrayElement::Value(v) => builder.append(Some(value_type(v.value)?)),
            ArrayElement::Variadic(_) => return None,
            ArrayElement::Missing(_) => {}
        }
    }
    let entries = builder.exact?;
    Some(PhpType::array_shape(
        entries.into_iter().map(|(_, entry)| entry).collect(),
    ))
}

/// Maximum number of distinct alternatives to keep in the element union
/// before falling back to the base scalar types. A literal array that names
/// more distinct values than this is a data table rather than a set of
/// alternatives worth reasoning about, and the union's pairwise absorption
/// is quadratic in its member count.
const MAX_ELEMENT_ALTERNATIVES: usize = 32;

/// The entries of an array literal beyond the ones known one by one, once
/// a spread of unknown length has put some there.
#[derive(Default)]
struct LiteralTail {
    keys: Vec<PhpType>,
    values: Vec<PhpType>,
}

/// The array an array literal builds, as far as its elements so far go.
struct LiteralBuilder {
    /// The literal's entries alongside the runtime key each lands on, while
    /// every one of them is known. `None` once an element's keys are not.
    exact: Option<Vec<(String, crate::php_type::ShapeEntry)>>,
    /// The entries a spread of unknown length added beside `exact`. Once
    /// there are some, which integer key comes next is no longer known, so
    /// a positional element joins them rather than `exact`.
    tail: Option<LiteralTail>,
    /// The integer key the next positional element takes.
    next_index: i64,
    /// The key and value types every element contributes, for when the
    /// literal is not a shape.
    key_types: Vec<PhpType>,
    types: Vec<PhpType>,
    /// Whether a spread copied values whose type is not known.
    unknown_values: bool,
    /// Whether every key is one PHP numbered in order, making the literal a
    /// list.
    is_list: bool,
}

impl Default for LiteralBuilder {
    fn default() -> Self {
        LiteralBuilder {
            exact: Some(Vec::new()),
            tail: None,
            next_index: 0,
            key_types: Vec::new(),
            types: Vec::new(),
            unknown_values: false,
            is_list: true,
        }
    }
}

impl LiteralBuilder {
    /// Write `value_type` under the constant key `key_text`, whose type as a
    /// key is `key_type`.
    fn set_key(&mut self, key_text: String, key_type: PhpType, value_type: PhpType) {
        self.is_list = false;
        let index = crate::php_type::canonical_int_key(&key_text);
        if let Some(index) = index {
            self.next_index = self.next_index.max(index.saturating_add(1));
        }
        push_unique(&mut self.key_types, key_type);
        push_unique(&mut self.types, value_type.clone());
        let Some(exact) = self.exact.as_mut() else {
            return;
        };
        let runtime_key = index.map_or(key_text.clone(), |index| index.to_string());
        let entry = crate::php_type::ShapeEntry {
            key: Some(key_text),
            value_type,
            optional: false,
        };
        match exact.iter_mut().find(|(key, _)| *key == runtime_key) {
            // PHP keeps an overwritten key where it was.
            Some((_, existing)) => existing.value_type = entry.value_type,
            None => exact.push((runtime_key, entry)),
        }
    }

    /// Write a value under the next free integer key.
    fn append(&mut self, value_type: Option<PhpType>) {
        push_unique(&mut self.key_types, PhpType::int());
        if let Some(tail) = self.tail.as_mut() {
            push_unique(&mut tail.keys, PhpType::int());
            push_unique(
                &mut tail.values,
                value_type.clone().unwrap_or_else(PhpType::mixed),
            );
        } else if let Some(exact) = self.exact.as_mut() {
            // Positional while it lands on the index a reader counting the
            // positional entries before it would expect. Once an explicit
            // integer key has moved the index along, it is spelled out.
            let positional_count = exact
                .iter()
                .filter(|(_, entry)| entry.key.is_none())
                .count();
            let positional = usize::try_from(self.next_index) == Ok(positional_count);
            exact.push((
                self.next_index.to_string(),
                crate::php_type::ShapeEntry {
                    key: (!positional).then(|| self.next_index.to_string()),
                    value_type: value_type.clone().unwrap_or_else(PhpType::mixed),
                    optional: false,
                },
            ));
        }
        self.next_index = self.next_index.saturating_add(1);
        if let Some(value_type) = value_type {
            push_unique(&mut self.types, value_type);
        }
    }

    /// Copy the entries of a spread `...$source` across, `source` being what
    /// the spread resolved to.
    fn spread(&mut self, source: Option<&PhpType>) {
        // The listed entries of an unsealed shape are copied like a sealed
        // shape's. A list's come first, ahead of the rest of the list; any
        // other shape's are copied after its tail, since the tail cannot
        // overwrite the value they are listed with.
        if let Some(unsealed) = source.and_then(PhpType::as_unsealed_shape) {
            if unsealed.shape.is_list_shape() {
                self.spread(Some(&unsealed.shape));
                self.spread(Some(&PhpType::list(unsealed.value.clone())));
            } else {
                let tail = PhpType::generic_array(unsealed.key.clone(), unsealed.value.clone());
                self.spread(Some(&tail));
                self.spread(Some(&unsealed.shape));
            }
            return;
        }
        if let Some(entries) = source.and_then(spread_entries) {
            for (key, value_type) in entries {
                match key {
                    Some(key) => {
                        let key_type = PhpType::string();
                        self.set_key(key, key_type, value_type);
                    }
                    None => self.append(Some(value_type)),
                }
            }
            return;
        }
        let key_type = source.and_then(PhpType::iterable_key_type);
        // Integer keys are renumbered onto the end, so only the string keys
        // keep what they are.
        let copied_key = match &key_type {
            Some(key) if key.is_int_subtype() => PhpType::int(),
            Some(key) if key.is_string_subtype() => key.clone(),
            _ => array_key_type(),
        };
        if !copied_key.is_int_subtype() {
            self.is_list = false;
        }
        push_unique(&mut self.key_types, copied_key.clone());
        // A spread copies values the source already knows, so its element
        // type carries over as written, the same as a value element beside
        // it.
        let elem = source.and_then(PhpType::iterable_element_type);
        match &elem {
            Some(elem) => push_unique(&mut self.types, elem.clone()),
            None => self.unknown_values = true,
        }
        let Some(exact) = self.exact.as_mut() else {
            return;
        };
        let elem = elem.unwrap_or_else(PhpType::mixed);
        // A string key the source may hold overwrites the entry written
        // under it, which is then one or the other.
        if !copied_key.is_int_subtype() {
            for (runtime_key, entry) in exact.iter_mut() {
                if crate::php_type::canonical_int_key(runtime_key).is_none()
                    && string_key_may_match(&copied_key, runtime_key)
                {
                    entry.value_type = PhpType::join_runtime_value_types(vec![
                        entry.value_type.clone(),
                        elem.clone(),
                    ]);
                }
            }
        }
        let tail = self.tail.get_or_insert_default();
        push_unique(&mut tail.keys, copied_key);
        push_unique(&mut tail.values, elem);
    }

    /// Stop tracking the literal as a shape.
    fn loosen(&mut self) {
        self.exact = None;
        self.tail = None;
    }

    /// The literal as `array<K, V>`, or `list<T>` when its keys are.
    fn loose_type(self) -> Option<PhpType> {
        if self.types.is_empty() {
            return None;
        }
        // Preserved literals need absorbing against their siblings, so that a
        // list written as `[$stringVar, 'yes', 'no']` is `list<string>` rather
        // than `list<string|'yes'|'no'>`.
        let value_type = if self.unknown_values {
            PhpType::mixed()
        } else {
            join_alternatives(self.types, MAX_ELEMENT_ALTERNATIVES)?
        };
        if self.is_list {
            Some(PhpType::list(value_type))
        } else {
            Some(PhpType::generic_array(
                join_key_types(self.key_types),
                value_type,
            ))
        }
    }
}

/// The entries a spread of `source` copies, in order, when `source` is a
/// shape whose every entry is known to be there: `Some(key)` for a string
/// key, which the spread keeps, and `None` for an integer key, which it
/// renumbers.
pub(in crate::type_engine) fn spread_entries(
    source: &PhpType,
) -> Option<Vec<(Option<String>, PhpType)>> {
    let crate::php_type::TypeKind::ArrayShape(entries) = source.kind() else {
        return None;
    };
    let keys = crate::php_type::runtime_shape_keys(entries)?;
    entries
        .iter()
        .zip(keys)
        .map(|(entry, key)| {
            // A class-constant key is stored as its spelling, which says
            // nothing about the key it evaluates to.
            if entry.optional || key.contains("::") {
                return None;
            }
            let key = crate::php_type::canonical_int_key(&key)
                .is_none()
                .then_some(key);
            Some((key, entry.value_type.clone()))
        })
        .collect()
}

/// Whether a key of type `key_type` may be the string `key`: any string
/// key may be, unless every alternative is a literal naming another one.
fn string_key_may_match(key_type: &PhpType, key: &str) -> bool {
    key_type
        .union_members()
        .into_iter()
        .any(|member| match member.as_literal() {
            Some(literal) => literal
                .string_content()
                .is_none_or(|content| content == key),
            None => !member.is_int_subtype(),
        })
}

/// Collapse a set of alternatives into one type, or `None` when empty.
fn join_alternatives(mut types: Vec<PhpType>, max_alternatives: usize) -> Option<PhpType> {
    if types.is_empty() {
        return None;
    }
    if types.iter().any(|t| t.as_literal().is_some()) {
        if types.len() > max_alternatives {
            types = types.iter().map(PhpType::widen_scalar_literals).collect();
        }
        return Some(PhpType::join_runtime_value_types(types));
    }
    if types.len() == 1 {
        return types.into_iter().next();
    }
    Some(PhpType::union(types))
}

/// Append `ty` unless an equal member is already recorded.
fn push_unique(types: &mut Vec<PhpType>, ty: PhpType) {
    if !types.contains(&ty) {
        types.push(ty);
    }
}

/// The `array-key` pseudo-type, used where a key is neither known to be
/// `int` nor `string`.
fn array_key_type() -> PhpType {
    PhpType::named(atom("array-key"))
}

/// Collapse the key types an array literal's entries contribute.
///
/// `array-key` covers every legal key, so one unplaceable key makes the
/// whole union `array-key` rather than leaving the redundant
/// `array-key|string` a plain union would produce.
fn join_key_types(key_types: Vec<PhpType>) -> PhpType {
    if key_types.iter().any(PhpType::is_array_key) {
        return array_key_type();
    }
    // Unlike a value union, the alternatives here are worth keeping as
    // written: a `Foo::class` key is a `class-string<Foo>`, and widening it
    // to `string` costs a `array<class-string, …>` parameter its match.
    if key_types.is_empty() {
        array_key_type()
    } else {
        PhpType::union(key_types)
    }
}

/// The type an array takes on for a key PHP evaluates at runtime.
///
/// PHP coerces every array key to `int` or `string`, so anything that does
/// not resolve to one of those (`mixed`, a union spanning both, a value the
/// walker could not place) is reported as `array-key`.
fn dynamic_key_type<'b>(key: &'b Expression<'b>, ctx: &VarResolutionCtx<'_>) -> PhpType {
    match infer_element_type(key, ctx) {
        Some(resolved) if resolved.is_int_subtype() || resolved.is_string_subtype() => resolved,
        _ => array_key_type(),
    }
}

/// The key type an array literal takes on from a constant key expression.
fn constant_key_type<'b>(key: &'b Expression<'b>) -> PhpType {
    match key {
        Expression::Literal(Literal::Integer(_) | Literal::True(_) | Literal::False(_)) => {
            PhpType::int()
        }
        _ => PhpType::string(),
    }
}

/// The name a constant array key contributes to an array shape, or `None`
/// when the key is only known at runtime.
///
/// PHP coerces a non-string, non-int key before using it, so the booleans
/// and `null` land on the `1`, `0` and `''` keys they index at runtime
/// rather than on a key named after the type they were written as.
fn extract_array_key_text<'b>(key: &'b Expression<'b>) -> Option<String> {
    match key {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to unquoting `raw`,
            // which is also where a value that is not UTF-8 (`"\x8b"`) lands.
            Some(
                s.value
                    .and_then(literal_bytes_to_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        crate::text_scan::unquote_php_string(bytes_to_str(s.raw))
                            .unwrap_or(bytes_to_str(s.raw))
                            .to_string()
                    }),
            )
        }
        Expression::Literal(Literal::Integer(i)) => Some(bytes_to_str(i.raw).to_string()),
        Expression::Literal(Literal::True(_)) => Some("1".to_string()),
        Expression::Literal(Literal::False(_)) => Some("0".to_string()),
        Expression::Literal(Literal::Null(_)) => Some(String::new()),
        _ => None,
    }
}

/// Infer the type of a single array element value expression.
///
/// A scalar literal keeps its exact value here. The array's contents are
/// fully known at the point the literal is written, so `[1, 1.5, '123']`
/// records `1|1.5|'123'` and a read off it can still be proven `numeric`.
/// Precision is given up where the array is *mutated* inside a loop
/// instead: a push or keyed write there widens before it reaches
/// [`merge_push_type`] and friends, because a value arriving on every pass
/// says the array is being built up rather than written out.
///
/// [`merge_push_type`]: super::array_shape_writes::merge_push_type
fn infer_element_type<'b>(
    value: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    match value {
        // ── Nested array literals ──
        Expression::Array(arr) => infer_array_literal_raw_type(arr.elements.iter(), ctx)
            .or_else(|| Some(PhpType::array())),
        Expression::LegacyArray(arr) => infer_array_literal_raw_type(arr.elements.iter(), ctx)
            .or_else(|| Some(PhpType::array())),
        // ── Object instantiation ──
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(ident) => {
                let name = bytes_to_str(ident.value()).to_string();
                let fqn = crate::util::resolve_source_class_name(
                    &name,
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.all_classes,
                    ctx.class_loader,
                );
                Some(PhpType::named(atom(&fqn)))
            }
            Expression::Self_(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
            Expression::Static(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
            _ => None,
        },
        Expression::Call(_) => {
            // Resolve call return type via the unified pipeline.
            super::foreach_resolution::resolve_expression_type(value, ctx)
        }
        Expression::Variable(Variable::Direct(dv)) => {
            let var_text = bytes_to_str(dv.name).to_string();
            let offset = value.span().start.offset as usize;
            // A `[$x]` written inside a ternary branch or `match` arm whose
            // condition proved something about `$x` builds an array of what
            // was proven, not of the type `$x` had before the test.  The
            // scope entry below still describes the statement as a whole.
            if let Some(narrowed) = ctx.arm_narrowed(&var_text) {
                return Some(crate::types::ResolvedType::types_joined(narrowed));
            }
            // When a scope variable resolver is available (i.e. we are
            // inside the forward walker), read the variable's type
            // directly from the in-progress ScopeState instead of
            // calling the full resolution pipeline which would trigger
            // a recursive method-body walk.
            //
            // The scope comes before the docblock because it is the only
            // one of the two that knows where the literal is written: a
            // `[$x]` inside `if (!is_array($x))` builds an array of the
            // narrowed `$x`, while the `@param array<T>|T $x` the
            // docblock states describes `$x` at the top of the function
            // and would put the ruled-out array arm back in the element.
            let scope_type = ctx.scope_var_resolver.and_then(|resolver| {
                let prefixed = if var_text.starts_with('$') {
                    var_text.clone()
                } else {
                    format!("${}", var_text)
                };
                let from_scope = resolver(&prefixed);
                (!from_scope.is_empty())
                    .then(|| crate::types::ResolvedType::types_joined(&from_scope))
            });
            if let Some(t) = scope_type {
                return Some(t);
            }
            // A `@var`/`@param` annotation read straight out of the source
            // (e.g. `@var list<User> $items`), for a variable nothing above
            // could type.
            let annotated = || {
                docblock::find_iterable_raw_type_in_source(ctx.content, offset, &var_text)
                    .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
            };
            // Inside the forward walker the scope above was the only
            // narrowing-aware answer on offer: running the full pipeline
            // here would walk the enclosing body all over again.  The
            // annotation is what is left.
            if ctx.scope_var_resolver.is_some() {
                return annotated();
            }
            // Outside the walker the full pipeline (parameter type hints,
            // `@param`/`@var` docblocks, assignments, foreach bindings)
            // answers this, and it goes first because it reads the same
            // annotations *and* the narrowing that holds where the literal
            // is written.  A `[$n]` under `if (is_int($n))` builds
            // `array{int}`, not the `int|float` the annotation states for
            // the assignment above it.
            let current_class = ctx
                .all_classes
                .iter()
                .find(|c| c.name == ctx.current_class.name)
                .map(|c| c.as_ref());
            crate::type_engine::variable::resolution::resolve_variable_php_type(
                &var_text,
                ctx.content,
                offset as u32,
                current_class,
                ctx.all_classes,
                ctx.class_loader,
                ctx.backend,
                ctx.loaders,
            )
            .or_else(annotated)
        }
        // ── Parenthesized ──
        Expression::Parenthesized(p) => infer_element_type(p.expression, ctx),
        // ── Property access, method calls on objects, etc. ──
        // Delegate to the unified pipeline which resolves property
        // type hints and method return types through the class
        // hierarchy.
        _ => super::foreach_resolution::resolve_expression_type(value, ctx),
    }
}

/// [`ArrayFuncArgs`] over a parsed argument list.
struct AstArrayFuncArgs<'a, 'ast, 'ctx> {
    args: &'a ArgumentList<'ast>,
    ctx: &'a VarResolutionCtx<'ctx>,
}

impl ArrayFuncArgs for AstArrayFuncArgs<'_, '_, '_> {
    fn arg_raw_type(&self, index: usize) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        super::resolution::resolve_arg_raw_type(expr, self.ctx)
    }

    fn bool_literal(&self, index: usize) -> Option<bool> {
        match super::resolution::nth_arg_expr(self.args, index)? {
            Expression::Literal(Literal::True(_)) => Some(true),
            Expression::Literal(Literal::False(_)) => Some(false),
            _ => None,
        }
    }

    fn has_arg(&self, index: usize) -> bool {
        super::resolution::nth_arg_expr(self.args, index).is_some()
    }

    fn is_spread(&self, index: usize) -> bool {
        matches!(
            self.args.arguments.iter().nth(index),
            Some(Argument::Positional(pos)) if pos.ellipsis.is_some()
        )
    }

    fn callback_declared_return_type(&self, index: usize) -> Option<PhpType> {
        // A written `: ReturnType` carries the file's own spelling of the
        // class (`Support\Pen` behind a `use App\Support;`), and it is
        // compared against types that arrived fully qualified, so the
        // spelling is canonicalised on the way out.
        let qualify = |ty: PhpType| crate::util::resolve_php_type_names(&ty, self.ctx.class_loader);
        match super::resolution::nth_arg_expr(self.args, index)? {
            Expression::Closure(closure) => closure
                .return_type_hint
                .as_ref()
                .map(|rth| qualify(extract_hint_type(&rth.hint))),
            Expression::ArrowFunction(arrow) => arrow
                .return_type_hint
                .as_ref()
                .map(|rth| qualify(extract_hint_type(&rth.hint))),
            // `array_map('intval', $xs)` names its callback instead of
            // spelling it out; the named function's own return type is what
            // the call produces.
            Expression::Literal(Literal::String(s)) => {
                let name =
                    super::array_func_rules::callable_string_function_name(bytes_to_str(s.raw))?;
                (self.ctx.loaders.function_loader?)(name, 0)?.return_type
            }
            // `array_map(Row::fromCache(...), $rows)` hands over the method
            // itself, so its declared return is what each element becomes.
            Expression::PartialApplication(pa) => {
                let span = pa.span();
                let text = self
                    .ctx
                    .content
                    .get(span.start.offset as usize..span.end.offset as usize)?;
                crate::completion::source::helpers::resolve_first_class_callable_return_type(
                    text,
                    &self.ctx.as_resolution_ctx(),
                )
            }
            _ => None,
        }
    }

    fn callback_inferred_return_type(&self, index: usize, param_type: &PhpType) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        infer_callback_return_type(expr, param_type, self.ctx)
    }

    fn arg_atom_text(&self, index: usize) -> Option<String> {
        match super::resolution::nth_arg_expr(self.args, index)? {
            Expression::ConstantAccess(ca) => {
                Some(crate::util::strip_fqn_prefix(bytes_to_str(ca.name.value())).to_string())
            }
            Expression::Literal(Literal::Integer(i)) => Some(bytes_to_str(i.raw).to_string()),
            _ => None,
        }
    }

    fn callback_param_narrowing(
        &self,
        index: usize,
        param_index: usize,
        subject: &PhpType,
    ) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        super::callback_narrowing::narrow_callback_param(
            expr,
            param_index,
            subject,
            Some(&self.ctx.class_loader),
        )
    }

    fn narrows(&self, inferred: &PhpType, declared: &PhpType) -> bool {
        crate::class_lookup::is_subtype_of_typed(inferred, declared, self.ctx.class_loader)
    }

    fn guard_split(&self, guard: &str, subject: &PhpType) -> Option<(Option<PhpType>, bool)> {
        crate::type_engine::types::narrowing::split_type_by_guard_name(
            guard,
            subject,
            Some(&self.ctx.class_loader),
        )
    }
}

/// For known array-producing functions, resolve the **raw output type**
/// (e.g. `list<User>`) from the input arguments.
///
/// Used by foreach and destructuring resolution so that iterating over
/// `array_filter(...)` etc. preserves element types.  Element-extracting
/// functions are handled by [`resolve_array_func_element_type`], which the
/// caller consults first.
pub(in crate::type_engine) fn resolve_array_func_raw_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    array_func_raw_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// For known array functions, resolve the **element type**
/// (e.g. `User`) of the output.
///
/// Used by `resolve_rhs_expression` so that `$item = array_pop($users)`
/// resolves `$item` to `User`.
pub(in crate::type_engine) fn resolve_array_func_element_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    array_func_element_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// Constant-fold a string builtin whose arguments are all literals.
///
/// See [`super::string_func_rules`] for which functions fold and why the
/// literal answer matters.
pub(in crate::type_engine) fn resolve_string_func_literal_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    if !super::string_func_rules::is_foldable_string_func(func_name) {
        return None;
    }
    super::string_func_rules::string_func_literal_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// Extract per-argument source text from a parsed `ArgumentList`.
///
/// Returns one `String` per argument by walking the AST nodes and
/// extracting their spans. This avoids serialising the argument list
/// to a flat string and then re-splitting with `split_text_args`.
pub(in crate::type_engine) fn extract_arg_texts_from_ast(
    argument_list: &mago_syntax::cst::ArgumentList<'_>,
    content: &str,
) -> Vec<String> {
    argument_list
        .arguments
        .iter()
        .map(|arg| {
            let value_span = match arg {
                mago_syntax::cst::argument::Argument::Positional(pos) => pos.value.span(),
                mago_syntax::cst::argument::Argument::Named(named) => named.value.span(),
            };
            let start = value_span.start.offset as usize;
            let end = value_span.end.offset as usize;
            let value = if end <= content.len() {
                &content[start..end]
            } else {
                ""
            };
            // Preserve the `name:` prefix for named arguments so that
            // downstream argument binding (`bind_text_args_to_params`) can
            // route them to the parameter they target rather than their
            // source-order slot. Without it, `f(b: 1, a: 2)` would bind `a`
            // to the value `1` and misresolve conditional return types and
            // template parameters that key on `a`.
            match arg {
                mago_syntax::cst::argument::Argument::Named(named) => {
                    let name = crate::atom::bytes_to_str(named.name.value);
                    format!("{name}: {value}")
                }
                mago_syntax::cst::argument::Argument::Positional(_) => value.to_string(),
            }
        })
        .collect()
}

fn first_param_name(params: &FunctionLikeParameterList<'_>) -> Option<String> {
    params
        .parameters
        .first()
        .map(|param| bytes_to_str(param.variable.name).to_string())
}

/// Infer the return type of a callback (arrow function or closure) by
/// resolving its body expression with the first parameter seeded to
/// `param_type`.
///
/// For arrow functions: resolves `arrow.expression` directly.
/// For closures: finds the first `return` statement and resolves its
/// expression.
fn infer_callback_return_type(
    callback_expr: &Expression<'_>,
    param_type: &PhpType,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let (param_name, body_expr) = match callback_expr {
        // A callback that takes no parameters ignores the element it is
        // handed, but its body still decides the result.
        Expression::ArrowFunction(arrow) => {
            let name = first_param_name(&arrow.parameter_list);
            (name, arrow.expression)
        }
        Expression::Closure(closure) => {
            let name = first_param_name(&closure.parameter_list);
            // Find the first return statement's expression.
            let ret_expr = closure.body.statements.iter().find_map(|stmt| {
                if let Statement::Return(ret) = stmt {
                    ret.value.as_ref()
                } else {
                    None
                }
            })?;
            (name, *ret_expr)
        }
        _ => return None,
    };

    // Build a scope resolver that maps the callback parameter to the
    // input element type.  Include ClassInfo when available so that
    // property access resolution can find the class members.
    //
    // A union element type seeds one entry per alternative rather than one
    // entry holding the whole union: two instantiations of the same class
    // (`Builder<A>|Builder<B>`) resolve to one class, and a single entry
    // could only carry the union as its type string, leaving a `@return T`
    // on that class with no instantiation to substitute from.
    let seed_member = |ty: &PhpType| -> ResolvedType {
        match ty.base_name().and_then(|name| (ctx.class_loader)(name)) {
            Some(cls) => ResolvedType::from_both(ty.clone(), (*cls).clone()),
            None => ResolvedType::from_type_string(ty.clone()),
        }
    };
    let resolved_param: Vec<ResolvedType> = match param_type.kind() {
        crate::php_type::TypeKind::Union(members) => members.iter().map(seed_member).collect(),
        _ => vec![seed_member(param_type)],
    };

    // A full closure body may reassign the parameter before returning it
    // (`$result['a'] = (string) $result['a']; return $result;`), so its
    // statements are walked with the shared forward walker — seeded with
    // the same call-site type — before the return expression is resolved.
    // Reading the parameter straight from `resolved_param` (as an arrow
    // function's single-expression body still does below) would answer
    // with the type the callback receives rather than the one it hands
    // back.
    let walked_locals = if let Expression::Closure(closure) = callback_expr {
        Some(walk_closure_body_scope(
            closure,
            param_name.as_deref(),
            &resolved_param,
            ctx,
        ))
    } else {
        None
    };

    let scope_resolver = move |var: &str| -> Vec<ResolvedType> {
        if let Some(locals) = &walked_locals
            && let Some(types) = locals.get(&atom(var))
            && !types.is_empty()
        {
            return types.clone();
        }
        if param_name.as_deref() == Some(var) {
            resolved_param.clone()
        } else {
            vec![]
        }
    };

    // Create a synthetic context with the scope resolver.
    let body_offset = body_expr.span().start.offset;
    let infer_ctx = VarResolutionCtx {
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        scope_var_resolver: Some(&scope_resolver),
        ..VarResolutionCtx::new(
            "",
            ctx.current_class,
            ctx.all_classes,
            ctx.content,
            body_offset,
            ctx.class_loader,
        )
    };

    super::foreach_resolution::resolve_expression_type(body_expr, &infer_ctx)
}

/// Walk a closure's own body with the shared forward walker, seeded with
/// what the call site hands its parameter, and return the scope its
/// statements leave behind.
///
/// This is a transient lookup seeded from the call site rather than the
/// closure's own declared scope, so, like
/// [`super::forward_walk::resolve_in_method_body`], it must not write into
/// an active diagnostic scope cache — reading from one is safe, since the
/// offsets walked belong to this same file.
fn walk_closure_body_scope(
    closure: &Closure<'_>,
    param_name: Option<&str>,
    resolved_param: &[ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) -> super::forward_walk::Locals {
    let fw_ctx =
        super::forward_walk::ForwardWalkCtx::from_var_ctx(ctx).with_cursor_offset(u32::MAX);
    let mut scope = super::forward_walk::ScopeState::new();
    if let Some(name) = param_name {
        scope.seed(name, resolved_param.to_vec());
    }

    let _suspend = super::forward_walk::suspend_snapshot_recording();
    let _barrier = super::forward_walk::suspend_return_edges();
    super::forward_walk::walk_body_forward(closure.body.statements.iter(), &mut scope, &fw_ctx);

    scope.locals
}
