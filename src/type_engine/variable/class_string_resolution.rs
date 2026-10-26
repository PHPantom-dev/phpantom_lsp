//! Class-string variable resolution.
//!
//! Resolves a variable that holds a class name (`$cls = User::class`, a
//! `class-string<User>` parameter, a string literal naming a class, or a
//! union of them) to the classes it names, so that `$cls::method()`,
//! `$cls::` completion and `new $cls` resolve against those classes rather
//! than against `string`.
//!
//! The variable is read like any other, from the forward walker's scope, so
//! control flow, narrowing, and a `foreach` over an array of `::class`
//! literals all apply.

use std::sync::Arc;

use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

/// The classes `var_name` holds as class names at `cursor_offset`.
pub(in crate::type_engine) fn resolve_class_string_targets(
    var_name: &str,
    cursor_offset: u32,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<Arc<ClassInfo>> {
    let resolved = super::rhs_resolution::resolve_var_types(var_name, ctx, cursor_offset);
    classes_named_by(&resolved, ctx)
}

/// The classes a value of one of `resolved` names when PHP reads it as a
/// class name, in order and without repeats; see [`classes_named_by_type`].
pub(in crate::type_engine) fn classes_named_by(
    resolved: &[ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) -> Vec<Arc<ClassInfo>> {
    let owning_class = ctx.current_class.fqn();
    let mut classes: Vec<Arc<ClassInfo>> = Vec::new();
    for rt in resolved {
        ClassInfo::extend_unique_arc(
            &mut classes,
            classes_named_by_type(
                &rt.type_string,
                &owning_class,
                ctx.file.all_classes,
                ctx.file.class_loader,
            ),
        );
    }
    classes
}

/// What `new $x` builds when `$x` resolved to `operand`: an instance of each
/// class the value names.
///
/// A `class-string<T>` of a bounded template builds a `T`, whatever classes
/// its bound resolves to, so the result keeps `T` as its hint.
pub(crate) fn instances_built_by_new(
    operand: &[ResolvedType],
    owning_class: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<ResolvedType> {
    if let [only] = operand
        && let TypeKind::ClassString(Some(inner)) = only.type_string.kind()
        && inner.as_template_param().is_some()
    {
        let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            inner,
            owning_class,
            all_classes,
            class_loader,
        );
        if !classes.is_empty() {
            return ResolvedType::from_classes_with_hint(classes, inner.clone());
        }
    }

    let mut classes: Vec<Arc<ClassInfo>> = Vec::new();
    for rt in operand {
        ClassInfo::extend_unique_arc(
            &mut classes,
            classes_named_by_type(&rt.type_string, owning_class, all_classes, class_loader),
        );
    }
    ResolvedType::from_classes(classes)
}

/// The classes a value of type `ty` names where PHP reads a value as a
/// class name (`new $x`, `$x::method()`, `$x::CONST`): the `T` of a
/// `class-string<T>`, also when it is nullable or one member of a union,
/// and the class a string literal spells.
///
/// A literal string that names a class is as usable as `class-string<T>`
/// is: PHP resolves the class through whatever name the string holds at
/// runtime, whether or not the analyser wrapped it in `class-string`. This
/// is what lets a foreach variable bound to a `Foo::class` array element
/// (a literal `'Foo'`, not a `class-string<Foo>`, per PHPStan's own reading
/// of such a constant) still dispatch `$item::method()`.
///
/// `owning_class` is the FQN `self` and `static` inside `T` refer to.
pub(in crate::type_engine) fn classes_named_by_type(
    ty: &PhpType,
    owning_class: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    let mut results = Vec::new();
    collect_named_classes(ty, owning_class, all_classes, class_loader, &mut results);
    results
}

fn collect_named_classes(
    ty: &PhpType,
    owning_class: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
    results: &mut Vec<Arc<ClassInfo>>,
) {
    match ty.kind() {
        TypeKind::ClassString(Some(inner)) => {
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                inner,
                owning_class,
                all_classes,
                class_loader,
            );
            ClassInfo::extend_unique_arc(results, classes);
        }
        TypeKind::Nullable(inner) => {
            collect_named_classes(inner, owning_class, all_classes, class_loader, results);
        }
        TypeKind::Union(members) => {
            for member in members {
                collect_named_classes(member, owning_class, all_classes, class_loader, results);
            }
        }
        TypeKind::Literal(_) => {
            if let Some(cls) = class_spelled_by(ty, class_loader) {
                ClassInfo::extend_unique_arc(results, vec![cls]);
            }
        }
        _ => {}
    }
}

/// The classes a value of type `ty` names exactly, when every value it can
/// hold is a string literal spelling a loaded class (`'Foo'`, `'\\Foo'`).
///
/// `None` for anything else, `class-string<T>` included: that holds some
/// class extending `T`, so a fact about `T` itself (that it is abstract, or
/// what its constructor takes) says nothing about the class `new` builds.
pub(crate) fn classes_spelled_exactly_by(
    ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Vec<Arc<ClassInfo>>> {
    let mut results = Vec::new();
    for member in ty.union_members() {
        ClassInfo::extend_unique_arc(&mut results, vec![class_spelled_by(member, class_loader)?]);
    }
    Some(results)
}

/// The class a string literal type spells, if it spells a loaded one.
fn class_spelled_by(
    ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Arc<ClassInfo>> {
    let TypeKind::Literal(literal) = ty.kind() else {
        return None;
    };
    let content = literal.string_content()?;
    // PHP reads a class name held in a string as fully qualified.
    let name = content.strip_prefix('\\').unwrap_or(&content);
    let is_class_name = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b >= 0x80);
    if !is_class_name {
        return None;
    }
    class_loader(Fqn::new(name))
}
