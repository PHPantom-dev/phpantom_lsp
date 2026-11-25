//! Reading the constants and enums a type operator names.
//!
//! `key-of<ID_TABLE>`, `value-of<Suit>` and `ID_TABLE[K]` describe a set of
//! values as concrete as a written-out union, but the docblock parser only
//! sees the operand's name.  This module reads the constant or enum behind
//! the name and finishes the operator, for declared types and for the
//! template bounds a call binds through.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::Atom;
use crate::class_lookup::Fqn;
use crate::php_type::{LiteralValue, PhpType, ShapeEntry, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::ClassInfo;

use super::arg_text_types::array_literal_shape_type;

/// The literal an argument binds a template to when the template's own bound
/// is a type operator — `@template K of key-of<TABLE>`, `@template V of
/// value-of<TABLE>`, `@template E of TABLE[K]`.
///
/// `resolve_arg_text_to_type` widens a scalar literal to its base type, which
/// is what nearly every binding wants and exactly wrong here: the operator can
/// only be evaluated against the *specific* key or value the caller wrote, so
/// `'immutable'` has to stay `'immutable'` rather than becoming `string`. The
/// quotes are kept because that is the spelling `evaluate_index_access`
/// matches shape keys against.
///
/// Returns `None` when the bound is not a type operator or the argument is not
/// a scalar literal, leaving the ordinary binding modes to resolve it.
pub(crate) fn type_operator_bound_literal(
    bound: Option<&PhpType>,
    arg_text: &str,
) -> Option<PhpType> {
    let bound = bound?;
    if !matches!(
        bound.kind(),
        TypeKind::KeyOf(_) | TypeKind::ValueOf(_) | TypeKind::IndexAccess(..)
    ) {
        return None;
    }
    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(arg_text.trim())
        .filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
}

/// The array shape a constant read through a type operator describes.
///
/// `key-of<ID_TABLE>` and `ID_TABLE[K]` name an operand as concrete as an
/// inline `array{…}`, but the docblock parser only ever sees the name — it
/// cannot read the constant behind it. This does, from the constant's own
/// initializer text, for a global constant (`ID_TABLE`) and for the
/// `Class::CONST` spelling alike.
///
/// Returns `None` when the name is not a constant we can reach, when its
/// value is not an array literal, or when none of its keys are literal — in
/// each case the operator stays unevaluated and widens to its bound, which
/// is the honest reading of an operand nobody can read.
pub(crate) fn constant_operand_shape(name: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let value = match name.rsplit_once("::") {
        Some((class_part, const_name)) => {
            let class = load_operand_class(class_part, ctx)?;
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &class,
                ctx.file.class_loader,
                ctx.resolved_class_cache,
            );
            merged.get_constant(const_name)?.value.clone()?
        }
        None => resolve_docblock_constant_value(name, ctx)?,
    };
    array_literal_shape_type(&value, ctx)
}

/// The initializer text of the constant a name in a type stands for, or
/// `None` when no spelling of it is indexed.
///
/// The name may arrive in either spelling: some paths hand over what
/// `resolve_names` already qualified against the file's namespace, since
/// almost every bare name in a type position is a class, while the ones
/// that resolve names through the class loader alone leave a constant bare.
/// A constant is indexed under its fully-qualified name, so the enclosing
/// namespace is tried first for a bare name, then the name as given, then
/// the global constant of that short name the way PHP itself falls back,
/// and finally the file's `use const` imports.
///
/// Only the indexed lookup is used, never the one that ends by parsing
/// every autoload file: a name in a type is far more often a class or a
/// template parameter than a constant, and a miss must not charge the whole
/// autoload set.
fn resolve_docblock_constant_value(name: &str, ctx: &ResolutionCtx<'_>) -> Option<String> {
    let backend = ctx.file.backend?;
    // An absolute name names exactly one constant, with no namespace or
    // import in play.
    if let Some(absolute) = name.strip_prefix('\\') {
        return backend.lookup_indexed_global_constant(absolute);
    }
    let ns = docblock_namespace(ctx);
    let short = crate::util::short_name(name);
    if short == name
        && let Some(value) =
            ns.and_then(|ns| backend.lookup_indexed_global_constant(&format!("{ns}\\{name}")))
    {
        return Some(value);
    }
    if let Some(value) = backend.lookup_indexed_global_constant(name) {
        return Some(value);
    }
    if short != name
        && let Some(value) = backend.lookup_indexed_global_constant(short)
    {
        return Some(value);
    }
    // Last: an import, which is the one spelling neither the enclosing
    // namespace nor the global scope accounts for.  A `use const` alias may
    // arrive qualified with the enclosing namespace, the way a bare name in
    // a type is read as a class.
    let alias = match ns.and_then(|ns| name.strip_prefix(ns)?.strip_prefix('\\')) {
        Some(alias) if !alias.contains('\\') => alias,
        _ => name,
    };
    let imported = match ctx.file.uri.and_then(|uri| backend.file_state(uri)) {
        // The open file's import tables are already built.
        Some(state) => match state
            .symbol_imports_at(ctx.cursor_offset)
            .constants
            .get(alias)
        {
            Some(fqn) => fqn.clone(),
            None => {
                let (first, rest) = name.split_once('\\')?;
                let prefix =
                    crate::names::lookup_import(state.use_map_at(ctx.cursor_offset), first)?;
                format!("{prefix}\\{rest}")
            }
        },
        // Otherwise they are read from the source, and only once every
        // cheaper candidate has missed: a docblock naming an imported
        // constant is rare, and the walk runs over the already-parsed AST.
        None => {
            if !ctx.file.content.contains("const") {
                return None;
            }
            match backend
                .parse_symbol_imports(ctx.file.content)
                .constants
                .remove(alias)
            {
                Some(fqn) => fqn,
                None => {
                    let (first, rest) = name.split_once('\\')?;
                    let use_map = backend.parse_use_statements(ctx.file.content);
                    let prefix = crate::names::lookup_import(&use_map, first)?;
                    format!("{prefix}\\{rest}")
                }
            }
        }
    };
    backend.lookup_indexed_global_constant(&imported)
}

/// The substitution map that resolves every constant operand a set of types
/// reads through an unevaluated type operator.
///
/// Names in `skip` are left out: a surviving operator's operand is either a
/// constant or a template parameter, and looking a template parameter up as
/// a constant only wastes work.
pub(super) fn constant_operand_subs<'t>(
    types: impl Iterator<Item = &'t PhpType>,
    skip: &[Atom],
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut operands = Vec::new();
    for ty in types {
        if ty.contains_unevaluated_operator() {
            ty.unevaluated_operator_operands(&mut operands);
        }
    }

    let mut subs: HashMap<String, PhpType> = HashMap::new();
    for operand in operands {
        if subs.contains_key(&operand) || skip.iter().any(|p| p.as_str() == operand) {
            continue;
        }
        if let Some(shape) = constant_operand_shape(&operand, ctx) {
            subs.insert(operand, shape);
        }
    }
    subs
}

/// Evaluate the type operators a declared type reads through a constant.
///
/// `key-of<ID_TABLE>` on a parameter and `value-of<ID_TABLE>` on a return
/// describe a set of values as concrete as any written-out union, but the
/// docblock parser only saw a name it could not read and left the operator
/// standing. This reads the constant behind the name and finishes the
/// operator, so the type constrains its call sites like the union it is.
///
/// Every consumer of a declared parameter or return type goes through here,
/// not just the template path: a constant is no less readable from a
/// signature that declares no `@template`.
///
/// Returns `None` when the type has no unevaluated operator, when no operand
/// is a constant we can read, or when reading it changed nothing — in each
/// case the caller keeps the type it already has.
pub(crate) fn evaluate_constant_operands(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    if !ty.needs_constant_evaluation() {
        return None;
    }
    if !ty.contains_unevaluated_operator() {
        let evaluated = read_class_member_references(ty, ctx);
        return (evaluated != *ty).then_some(evaluated);
    }
    let subs = constant_operand_subs(std::iter::once(ty), &[], ctx);
    let evaluated = if subs.is_empty() {
        ty.clone()
    } else {
        ty.substitute(&subs)
    };
    let evaluated = if evaluated.contains_type_operator() {
        evaluate_enum_value_of(&evaluated, ctx)
    } else {
        evaluated
    };
    let evaluated = if evaluated.needs_constant_evaluation() {
        read_class_member_references(&evaluated, ctx)
    } else {
        evaluated
    };
    (evaluated != *ty).then_some(evaluated)
}

/// Replace every constant `ty` names as a type with the value it holds:
/// `Foo::BAR` with the constant's type, `Foo::BAR_*` with the union of every
/// constant whose name matches, `Foo::class` with the class's name, an
/// enum's `Suit::Hearts` with that case, and a global constant (`MY_CONST`)
/// that no class of the same name shadows with its value. A shape key
/// written as a class constant becomes the key the constant holds, and a
/// capitalised pseudo-type name no class answers to (`Number`) becomes the
/// pseudo-type.
///
/// The operand of a type operator is left alone: the operator reads the
/// constant through [`constant_operand_shape`], or through the enum it
/// names, and is not finished by the constant's value type.  A reference
/// nothing can read stays as written.
fn read_class_member_references(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> PhpType {
    match ty.raw_kind() {
        TypeKind::KeyOf(_) | TypeKind::ValueOf(_) | TypeKind::IndexAccess(..) => ty.clone(),
        TypeKind::Raw(text) => read_class_member_reference(text, ty, ctx),
        TypeKind::Named(name) => match crate::php_type::class_member_reference(name) {
            Some(_) => read_class_member_reference(name, ty, ctx),
            None => read_non_class_name(name, ctx).unwrap_or_else(|| ty.clone()),
        },
        _ if !ty.needs_constant_evaluation() => ty.clone(),
        TypeKind::ArrayShape(entries) => {
            let entries = entries
                .iter()
                .map(|entry| ShapeEntry {
                    key: entry
                        .key
                        .as_deref()
                        .map(|key| read_class_constant_shape_key(key, ctx)),
                    value_type: read_class_member_references(&entry.value_type, ctx),
                    optional: entry.optional,
                })
                .collect();
            PhpType::array_shape(entries)
        }
        _ => ty.map_children(&|child| read_class_member_references(child, ctx)),
    }
}

fn read_class_member_reference(text: &str, ty: &PhpType, ctx: &ResolutionCtx<'_>) -> PhpType {
    crate::php_type::class_member_reference(text)
        .and_then(|(class_part, member)| class_member_type(class_part, member, ctx))
        .unwrap_or_else(|| ty.clone())
}

/// The key a shape entry written under a class constant (`self::KEY`,
/// `Foo::KEY`) is stored under: the string or integer the constant holds.
/// Any other key, and a constant that holds no literal key, stays as written.
fn read_class_constant_shape_key(key: &str, ctx: &ResolutionCtx<'_>) -> String {
    let value = crate::php_type::class_constant_key(key)
        .filter(|(_, constant)| !constant.eq_ignore_ascii_case("class"))
        .and_then(|(class_part, constant)| class_member_type(class_part, constant, ctx));
    let literal_key = value.as_ref().and_then(|value| match value.as_literal()? {
        LiteralValue::Int(_) => Some(value.as_literal()?.parse_i64()?.to_string()),
        literal @ LiteralValue::String(_) => Some(literal.string_content()?.into_owned()),
        LiteralValue::Float(_) => None,
    });
    literal_key.unwrap_or_else(|| key.to_string())
}

/// The type a name the docblock reader qualified as a class stands for when
/// no class answers to it: the value of the constant of that name, or the
/// pseudo-type a capitalised `Number` or `Boolean` spells.
///
/// `None` when the name is a class, or is neither of the two. A constant is
/// looked up first, since that lookup is a map read and a class lookup that
/// misses is not; a class only has to be ruled out once a constant exists.
fn read_non_class_name(name: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let short = crate::util::short_name(name);
    if crate::php_type::is_constant_name_spelling(short)
        && let Some(value) = resolve_docblock_constant_value(name, ctx)
        && !names_class(name, ctx)
    {
        return super::global_constant_value_type(short, &value, ctx);
    }
    // A pseudo-type spelling is only read as one where it was written bare:
    // an imported `Number` names the class it was imported as.
    let pseudo = crate::php_type::capitalised_pseudo_type(short)?;
    let written_bare = short == name
        || docblock_namespace(ctx).is_some_and(|ns| {
            name.strip_suffix(short)
                .and_then(|prefix| prefix.strip_suffix('\\'))
                == Some(ns)
        });
    (written_bare && !names_class(name, ctx)).then(|| PhpType::parse(pseudo))
}

fn names_class(name: &str, ctx: &ResolutionCtx<'_>) -> bool {
    let name = name.strip_prefix('\\').unwrap_or(name);
    crate::class_lookup::find_class_by_fqn(ctx.file.all_classes, name).is_some()
        || (ctx.file.class_loader)(Fqn::new(name)).is_some()
}

/// The namespace a docblock read in `ctx` was written in: the current
/// class's, or outside a class the one in force at the cursor.
fn docblock_namespace<'a>(ctx: &'a ResolutionCtx<'_>) -> Option<&'a str> {
    match ctx.current_class.filter(|class| !class.name.is_empty()) {
        Some(class) => class.file_namespace.as_deref(),
        None => ctx.file.namespace.or_else(|| {
            crate::parser::namespace_at_offset(ctx.file.content, ctx.cursor_offset as usize)
        }),
    }
}

/// The type `Class::member` names in a type position.
fn class_member_type(class_part: &str, member: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    if member.eq_ignore_ascii_case("class") {
        // `static::class` is only known at the call.
        if class_part.eq_ignore_ascii_case("static") {
            return None;
        }
        let class_name = crate::class_lookup::resolve_class_keyword(class_part, ctx.current_class);
        let class_name = class_name
            .as_deref()
            .unwrap_or_else(|| class_part.strip_prefix('\\').unwrap_or(class_part));
        return Some(PhpType::class_name_literal(crate::atom::atom(class_name)));
    }
    let class = load_operand_class(class_part, ctx)?;
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &class,
        ctx.file.class_loader,
        ctx.resolved_class_cache,
    );
    let constant_type = |name: &str| -> Option<PhpType> {
        let constant = merged.constants.iter().find(|c| c.name == name)?;
        if constant.is_enum_case {
            return Some(PhpType::enum_case(merged.fqn(), constant.name));
        }
        super::return_types::class_constant_type(&merged, name, ctx)
    };
    if !member.contains('*') {
        return constant_type(member);
    }
    let mut values: Vec<PhpType> = Vec::new();
    for constant in merged.constants.iter() {
        if !glob_matches(member, &constant.name) {
            continue;
        }
        let value = constant_type(&constant.name)?;
        if !values.contains(&value) {
            values.push(value);
        }
    }
    match values.len() {
        0 => None,
        1 => values.pop(),
        _ => Some(PhpType::union(values)),
    }
}

/// Whether `name` matches `pattern`, where each `*` stands for any run of
/// characters.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = parts.collect();
    let last = parts.pop();
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    match last {
        Some(last) => rest.len() >= last.len() && rest.ends_with(last),
        None => rest.is_empty(),
    }
}

/// Finish every `value-of<…>` in `ty` whose operand is a backed enum or one
/// of its cases, re-evaluating the operators around it.
///
/// `value-of<Suit::Hearts>` is the case's backing value and `value-of<Suit>`
/// the union of every case's, which is what a template bound to an enum
/// case reads a shape through (`Data[value-of<T>]` with `T` bound to
/// `Target::DASHBOARD`).  An operand that is not a backed enum, or a case
/// with no backing value, leaves the operator standing.
fn evaluate_enum_value_of(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> PhpType {
    if !ty.contains_unevaluated_operator() {
        return ty.clone();
    }
    let recurse = |inner: &PhpType| evaluate_enum_value_of(inner, ctx);
    match ty.kind() {
        TypeKind::ValueOf(operand) => {
            enum_backing_values(operand, ctx).unwrap_or_else(|| PhpType::value_of(recurse(operand)))
        }
        TypeKind::KeyOf(operand) => crate::php_type::evaluate_key_of(&recurse(operand)),
        TypeKind::IndexAccess(base, index) => {
            crate::php_type::evaluate_index_access(&recurse(base), &recurse(index))
        }
        _ => ty.map_children(&recurse),
    }
}

/// The backing value of the enum case `operand` names (`Suit::Hearts`), or
/// the union of every case's for a backed enum named by itself.
fn enum_backing_values(operand: &PhpType, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    // A template bound to a union of cases reads each one's value.
    if let TypeKind::Union(members) = operand.kind()
        && members.iter().all(|member| member.as_enum_case().is_some())
    {
        let values = members
            .iter()
            .map(|member| enum_backing_values(member, ctx))
            .collect::<Option<Vec<_>>>()?;
        return Some(PhpType::union(values));
    }
    let (class_part, case) = match operand.as_enum_case() {
        Some((enum_name, case)) => (enum_name, Some(case)),
        None => {
            let name: &str = match operand.kind() {
                TypeKind::Named(name) => name,
                TypeKind::Raw(raw) => raw,
                _ => return None,
            };
            match name.rsplit_once("::") {
                Some((class_part, case)) => (class_part, Some(case)),
                None => (name, None),
            }
        }
    };
    let class = load_operand_class(class_part, ctx)?;
    if class.kind != crate::types::ClassLikeKind::Enum {
        return None;
    }
    let mut values: Vec<PhpType> = Vec::new();
    for constant in class.constants.iter().filter(|c| c.is_enum_case) {
        if case.is_some_and(|case| constant.name != case) {
            continue;
        }
        let value = crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
            constant.enum_value.as_deref()?,
        )?;
        if !values.contains(&value) {
            values.push(value);
        }
    }
    match values.len() {
        0 => None,
        1 => values.pop(),
        _ => Some(PhpType::union(values)),
    }
}

/// The class a type operand's `Foo` or `Foo::BAR` names.
///
/// The declaring file's parse already qualified the name against its own
/// imports and namespace, so it is read as a fully-qualified name: the call
/// site's imports have no say in what a callee's docblock meant. Only the
/// relative keywords are left, and those read the class in scope.
fn load_operand_class(class_part: &str, ctx: &ResolutionCtx<'_>) -> Option<Arc<ClassInfo>> {
    let class_name = crate::class_lookup::resolve_class_keyword(class_part, ctx.current_class);
    let class_name = class_name
        .as_deref()
        .unwrap_or_else(|| class_part.strip_prefix('\\').unwrap_or(class_part));
    crate::class_lookup::find_class_by_fqn(ctx.file.all_classes, class_name)
        .cloned()
        .or_else(|| (ctx.file.class_loader)(Fqn::new(class_name)))
}
