//! Class lookup by name/offset and subtype-relationship checks.
//!
//! These helpers answer "which class is this?" and "is this class a
//! subtype of that one?" against an already-loaded slice of
//! [`ClassInfo`] values (or via a `class_loader` closure for
//! cross-file lookups). Cross-file/PSR-4/stub resolution itself lives
//! in [`crate::resolution`]; this module is the shared kernel that
//! resolution and the completion/diagnostics pipelines both call into.

use crate::atom::atom;
use crate::php_type::TypeKind;
use std::sync::Arc;

use crate::types::{ClassInfo, ClassLikeKind};

/// Find which class the cursor (byte offset) is inside.
///
/// When multiple classes contain the offset (e.g. an anonymous class
/// nested inside a named class's method), the smallest (most specific)
/// class is returned.  This ensures that `$this` inside an anonymous
/// class body resolves to the anonymous class, not the outer class.
///
/// The span runs from the declaration start (`decl_start_offset`, which
/// includes any leading attribute lists) to the closing brace, so a
/// `self::` reference inside a class-level attribute — which sits before
/// the body braces — still resolves to the class it decorates.
pub(crate) fn find_class_at_offset(classes: &[Arc<ClassInfo>], offset: u32) -> Option<&ClassInfo> {
    classes
        .iter()
        .map(|c| c.as_ref())
        .map(|c| {
            let start = if c.decl_start_offset != 0 {
                c.decl_start_offset
            } else {
                c.start_offset
            };
            (c, start)
        })
        .filter(|(c, start)| offset >= *start && offset <= c.end_offset)
        .min_by_key(|(c, start)| c.end_offset.saturating_sub(*start))
        .map(|(c, _)| c)
}

/// Find which class a docblock at `offset` documents.
///
/// A docblock on a *member* sits inside the class body, which
/// [`find_class_at_offset`] already resolves.  A docblock on the *class*
/// does not: it is trivia preceding the declaration, so it starts before
/// even `decl_start_offset` (which covers attribute lists but not
/// comments).  For that case the owner is the declaration the docblock
/// runs straight into, since a docblock binds to whatever follows it.
/// Anything else in between — a function, a `use` statement, another
/// docblock — means the block documents that instead, and the class is
/// none of its business.
pub(crate) fn find_documented_class_at_offset<'a>(
    classes: &'a [Arc<ClassInfo>],
    content: &str,
    offset: u32,
) -> Option<&'a ClassInfo> {
    if let Some(class) = find_class_at_offset(classes, offset) {
        return Some(class);
    }
    let rest = content.get(offset as usize..)?;
    let doc_end = offset as usize + rest.find("*/")? + 2;
    classes
        .iter()
        .map(|c| c.as_ref())
        .filter(|c| c.decl_start_offset as usize >= doc_end)
        .min_by_key(|c| c.decl_start_offset)
        .filter(|c| {
            content
                .get(doc_end..c.decl_start_offset as usize)
                .is_some_and(runs_into_declaration)
        })
}

/// Whether the source between a docblock's `*/` and a class declaration's
/// start holds nothing that would break the two apart.
///
/// `decl_start_offset` points at the leading attribute list when there is
/// one and at the `class` / `interface` / `trait` / `enum` keyword
/// otherwise, so the modifiers that may precede that keyword are all this
/// has to tolerate alongside whitespace.
fn runs_into_declaration(between: &str) -> bool {
    between.split_whitespace().all(|word| {
        ["final", "abstract", "readonly"]
            .iter()
            .any(|modifier| word.eq_ignore_ascii_case(modifier))
    })
}

/// Build the stand-in [`ClassInfo`] used when the cursor is not inside a
/// class body — a plain function, a closure at file scope, or top-level
/// code.  It has no name, so the checks that ask "are we in a class?"
/// (`current_class.name.is_empty()`) still answer no.
///
/// What it does carry is the namespace enclosing the cursor.  Source-level
/// class references resolve through
/// [`resolve_source_class_name`](crate::util::resolve_source_class_name),
/// which reads that namespace from `current_class.file_namespace` so a
/// same-namespace class outranks a global one of the same short name.  A
/// bare `ClassInfo::default()` reports no namespace, which would resolve
/// `Foo::bar()` inside `namespace App` to the global `\Foo` even when
/// `App\Foo` exists.
pub(crate) fn class_context_placeholder(content: &str, cursor_offset: u32) -> ClassInfo {
    placeholder_in_namespace(crate::text_scan::namespace_at_offset(
        content,
        cursor_offset as usize,
    ))
}

/// [`class_context_placeholder`] for callers that already know the
/// enclosing namespace (e.g. a walker recursing into a `namespace { … }`
/// block) and need not scan for it.
pub(crate) fn placeholder_in_namespace(namespace: Option<&str>) -> ClassInfo {
    ClassInfo {
        file_namespace: namespace.map(crate::atom::atom),
        ..ClassInfo::default()
    }
}

/// Find a class in a slice by name, preferring namespace-aware matching
/// when the name is fully qualified.
///
/// When `name` contains backslashes (e.g. `Illuminate\Database\Eloquent\Builder`),
/// the lookup checks each candidate's `file_namespace` field so that the
/// correct class is returned even when multiple classes share the same short
/// name but live in different namespace blocks within the same file (e.g.
/// `Demo\Builder` vs `Illuminate\Database\Eloquent\Builder`).
///
/// When `name` is a bare short name (no backslashes), the first class with
/// a matching `name` field is returned (preserving existing behavior).
pub(crate) fn find_class_by_name<'a>(
    all_classes: &'a [Arc<ClassInfo>],
    name: &str,
) -> Option<&'a Arc<ClassInfo>> {
    let short = crate::util::short_name(name);

    if name.contains('\\') {
        let expected_ns = name.rsplit_once('\\').map(|(ns, _)| ns);
        all_classes
            .iter()
            .find(|c| c.name == short && c.file_namespace.as_deref() == expected_ns)
    } else {
        all_classes.iter().find(|c| c.name == short)
    }
}

/// The class a class-position expression names, as written.
///
/// Covers the four spellings a `Foo::bar()`, `new Foo`, `Foo::CONST`, or
/// `Foo::$prop` can put on the left: a name, `self`, `static`, and
/// `parent`. `self` and `static` answer with the current class (late
/// static binding is settled by the caller, which knows whether the call
/// forwards it), and `parent` with its parent, or `None` when it has
/// none.
///
/// Every other expression kind (a variable holding a class-string, a call)
/// needs resolution this cannot do and yields `None`; a caller that
/// handles one matches it before reaching here.
pub(crate) fn class_expression_name(
    expr: &mago_syntax::cst::Expression<'_>,
    current_class: &ClassInfo,
) -> Option<String> {
    use mago_syntax::cst::Expression;
    match expr {
        Expression::Identifier(ident) => Some(crate::atom::bytes_to_str(ident.value()).to_string()),
        Expression::Self_(_) | Expression::Static(_) => Some(current_class.name.to_string()),
        Expression::Parent(_) => current_class.parent_class.map(|a| a.to_string()),
        _ => None,
    }
}

/// Returns `true` if `s` is one of the PHP keywords that refer to the
/// *current* class (not the parent): `self`, `static`, or `$this`.
///
/// Callers that also need to match `parent` should add a separate
/// `eq_ignore_ascii_case("parent")` check, because `parent` resolves
/// to the *parent* class rather than the current one.
///
/// The comparison is case-insensitive for `self` and `static`.
/// `$this` is matched literally (it is always lowercase in PHP).
pub(crate) fn is_self_or_static(s: &str) -> bool {
    s.eq_ignore_ascii_case("self") || s.eq_ignore_ascii_case("static") || s == "$this"
}

/// Returns `true` if `s` is one of the PHP class-keyword references:
/// `self`, `static`, `$this`, or `parent`.
///
/// Use this when you need a single guard that covers *all* class
/// keywords, including `parent`.  For the subset that resolves to the
/// *current* class only, use [`is_self_or_static`].
pub(crate) fn is_class_keyword(s: &str) -> bool {
    is_self_or_static(s) || s.eq_ignore_ascii_case("parent")
}

/// Resolve `self`, `static`, `$this`, or `parent` to a class name.
///
/// Returns `Some(class_name)` when the keyword can be resolved, or
/// `None` when:
/// - `keyword` is not a recognised class keyword, or
/// - there is no `current_class`, or
/// - `parent` is used but the class has no parent.
///
/// This centralises the keyword → class-name mapping that was
/// previously duplicated across 10+ call sites.
///
/// The name returned is fully qualified.  `parent` already is, and `self`
/// has to be: a namespaced class whose short name collides with a global
/// one (`PHPStan\Analyser\Error` against the built-in `\Error`) would
/// otherwise have every `new self(…)` and `self::` resolve to whichever
/// class the loader finds first, which is the global one.
pub(crate) fn resolve_class_keyword(
    keyword: &str,
    current_class: Option<&ClassInfo>,
) -> Option<String> {
    if is_self_or_static(keyword) {
        current_class.map(|cc| cc.fqn().to_string())
    } else if keyword.eq_ignore_ascii_case("parent") {
        current_class.and_then(|cc| cc.parent_class.map(|a| a.to_string()))
    } else {
        None
    }
}

/// Load a class's ancestor by the name stored on it, refusing an answer
/// that is the class itself.
///
/// `parent_class`, `interfaces`, and `used_traits` hold canonical FQNs, so
/// a backslash-free entry names a root-namespace class.  The class loader
/// applies the *consuming* file's `use` imports to bare names, which is
/// right for a name a user typed and wrong here: resolving
/// `Nette\Neon\Exception extends \Exception` while answering a question
/// asked from a file that says `use Nette\Neon\Exception;` turns the stored
/// `Exception` back into the subclass.  The re-entry guard then cuts the
/// chain and every member the real `\Exception` provides goes missing.
///
/// PHP has no self-inheritance, so an answer naming the class we started
/// from is proof that an import won a lookup it had no business in, and the
/// name is retried as the explicit global reference it stood for.  A stored
/// name that is already qualified and still resolves to the class itself is
/// a genuine cycle and yields nothing.
pub(crate) fn load_ancestor(
    class_fqn: &str,
    ancestor_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Arc<ClassInfo>> {
    let loaded = class_loader(ancestor_name);
    let is_self = |cls: &ClassInfo| cls.fqn().eq_ignore_ascii_case(class_fqn);
    match loaded {
        Some(cls) if is_self(&cls) => {
            if ancestor_name.contains('\\') {
                return None;
            }
            class_loader(&format!("\\{ancestor_name}")).filter(|c| !is_self(c))
        }
        other => other,
    }
}

/// Check whether `child` is a subclass (direct or transitive) of
/// `parent`, including implemented interfaces.
///
/// Returns `false` if `child` cannot be loaded or if there is no
/// inheritance relationship.  Delegates to the shared nominal subtype
/// walk ([`is_subtype_of`]), which handles
/// transitive interface extension, FQN normalisation, and cycle
/// detection.
pub(crate) fn is_subclass_of(
    child: &str,
    parent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if child.eq_ignore_ascii_case(parent) {
        return false; // same class, not a subclass
    }
    match class_loader(child) {
        Some(child_class) => is_subtype_of(&child_class, parent, class_loader),
        None => false,
    }
}

/// Check whether `class` is a subtype of the class identified by
/// `ancestor_name`.  Returns `true` when:
///
/// - `class.name` equals `ancestor_name` (same class), or
/// - walking the `parent_class` chain reaches `ancestor_name`, or
/// - `ancestor_name` appears in the `interfaces` list of `class` or any
///   of its ancestors.
///
/// Both short names and fully-qualified names are compared so that
/// cross-file relationships (where `parent_class` stores FQNs) work.
pub(crate) fn is_subtype_of(
    class: &ClassInfo,
    ancestor_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    // Every name handled by this function — the ancestor plus each
    // `interfaces`/`parent_class` entry pulled from a loaded class — is a
    // canonical FQN, not a user-typed reference.  A backslash-free name
    // (e.g. `Iterator`, `Traversable`, `Exception`) is therefore a
    // root-namespace class and must resolve in the global namespace.
    //
    // The raw `class_loader` applies the consuming file's use-map to
    // unqualified names.  That is correct for user-typed references but
    // wrong here: when a calling file has `use App\Iterator;`, loading the
    // global `\Iterator` interface node inside an SPL class's hierarchy
    // would return `App\Iterator` and break the walk (so subtype checks
    // against `Iterator`/`Traversable` spuriously fail).  Route
    // backslash-free names through the `__fqn__\` bypass, which skips the
    // use-map and falls back to a global short-name lookup.  The
    // `.or_else` preserves the raw loader for genuine short names that
    // have no global class (they resolve via namespace context as before).
    let load_fqn = |name: &str| -> Option<Arc<ClassInfo>> {
        if name.contains('\\') {
            class_loader(name)
        } else {
            class_loader(&format!("__fqn__\\{name}")).or_else(|| class_loader(name))
        }
    };

    // Resolve the ancestor to its FQN so that all comparisons below are
    // FQN-vs-FQN.  When `ancestor_name` is already a FQN (contains `\`)
    // we use it directly.  When it is a short name we try to load it
    // through `load_fqn` and use the loaded class's FQN.  For
    // root-namespace classes (e.g. `RuntimeException`) the FQN equals the
    // short name, so the fallback to `ancestor_name` is correct.
    let ancestor_fqn: String = if ancestor_name.contains('\\') {
        ancestor_name.to_string()
    } else if let Some(loaded) = load_fqn(ancestor_name) {
        loaded.fqn().to_string()
    } else {
        // Cannot resolve — keep the original name.  For root-namespace
        // classes this is already the FQN.
        ancestor_name.to_string()
    };
    let ancestor = ancestor_fqn.as_str();

    // Same class?  Always compare by FQN.
    if class.fqn() == ancestor {
        return true;
    }

    // Check interfaces on the class itself (stored as FQNs after
    // resolve_parent_class_names), walking the full interface
    // inheritance tree so that transitive relationships are found
    // (e.g. Response implements ResponseInterface extends MessageInterface).
    let mut iface_queue: Vec<String> = class.interfaces.iter().map(|a| a.to_string()).collect();
    let mut visited_ifaces: std::collections::HashSet<String> =
        iface_queue.iter().cloned().collect();
    while let Some(iface_name) = iface_queue.pop() {
        if iface_name == ancestor {
            return true;
        }
        // Load the interface and check its parents (interface extends).
        if let Some(iface_info) = load_fqn(&iface_name) {
            // Interface parents are stored in both `parent_class`
            // (first parent for single-extends compat) and
            // `interfaces` (all parents for multi-extends).
            for parent_iface in &iface_info.interfaces {
                if visited_ifaces.insert(parent_iface.to_string()) {
                    iface_queue.push(parent_iface.to_string());
                }
            }
            if let Some(ref pc) = iface_info.parent_class
                && visited_ifaces.insert(pc.to_string())
            {
                iface_queue.push(pc.to_string());
            }
        }
    }

    // Walk the parent class chain (parent_class is also a resolved FQN).
    let mut current_parent = class.parent_class.map(|a| a.to_string());
    let mut visited_parents: std::collections::HashSet<String> = std::collections::HashSet::new();
    visited_parents.insert(class.fqn().to_string());
    let mut depth = 0u32;
    while let Some(ref name) = current_parent {
        depth += 1;
        if depth > 20 {
            break;
        }
        if name == ancestor {
            return true;
        }
        // Load the parent to check its interfaces (transitively)
        // and continue the class chain.  `load_fqn` resolves
        // root-namespace names globally, so the use-map can no longer
        // shadow a global parent (e.g. a same-file `use App\Exception;`
        // does not hijack a stub class's global `\Exception` parent).
        if let Some(parent_info) = load_fqn(name) {
            let mut p_iface_queue: Vec<String> = parent_info
                .interfaces
                .iter()
                .map(|a| a.to_string())
                .collect();
            let mut p_visited: std::collections::HashSet<String> =
                p_iface_queue.iter().cloned().collect();
            while let Some(iface_name) = p_iface_queue.pop() {
                if iface_name == ancestor {
                    return true;
                }
                if let Some(iface_info) = load_fqn(&iface_name) {
                    for pi in &iface_info.interfaces {
                        if p_visited.insert(pi.to_string()) {
                            p_iface_queue.push(pi.to_string());
                        }
                    }
                    if let Some(ref pc) = iface_info.parent_class
                        && p_visited.insert(pc.to_string())
                    {
                        p_iface_queue.push(pc.to_string());
                    }
                }
            }
            // Cycle detection: stop if we have already visited this
            // parent's FQN (a malformed or self-referential hierarchy).
            if !visited_parents.insert(parent_info.fqn().to_string()) {
                break;
            }
            current_parent = parent_info.parent_class.map(|a| a.to_string());
        } else {
            break;
        }
    }

    false
}

/// Convenience wrapper around [`is_subtype_of_typed`] that accepts bare
/// class names instead of pre-constructed [`crate::php_type::PhpType`] values.
///
/// This avoids the boilerplate of wrapping each name in
/// `PhpType::named(atom(name.as_ref()))` at call sites that already have
/// `&str` class names.
pub(crate) fn is_subtype_of_names(
    subtype_name: &str,
    supertype_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    use crate::php_type::PhpType;
    is_subtype_of_typed(
        &PhpType::named(atom(subtype_name)),
        &PhpType::named(atom(supertype_name)),
        class_loader,
    )
}

/// Like [`is_subtype_of_typed`] but accepts a `&str` for the supertype,
/// avoiding `TypeKind::Named` wrapping at call sites that already have a
/// `&PhpType` subtype and a bare class name as supertype.
pub(crate) fn is_subtype_of_named(
    subtype: &crate::php_type::PhpType,
    supertype_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    use crate::php_type::PhpType;
    is_subtype_of_typed(subtype, &PhpType::named(atom(supertype_name)), class_loader)
}

/// Check whether `subtype` is a subtype of `supertype`, combining
/// structural subtyping ([`crate::php_type::PhpType::is_subtype_of`])
/// with nominal class-hierarchy walking ([`is_subtype_of`]).
///
/// This is the single entry point for all subtype checks that need
/// both layers:
///
/// - Scalars, unions, intersections, generics, callables, literals,
///   and other structural relationships are handled by
///   `PhpType::is_subtype_of`.
/// - Nominal class relationships (`Cat <: Animal`) are resolved by
///   loading the class via `class_loader` and walking its parent
///   chain and interface list.
///
/// Returns `true` when the structural check succeeds, or when both
/// types are named (class/interface) types and the class hierarchy
/// confirms the relationship.
pub(crate) fn is_subtype_of_typed(
    subtype: &crate::php_type::PhpType,
    supertype: &crate::php_type::PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    use crate::php_type::PhpType;

    // Fast path: structural subtyping covers scalars, unions,
    // intersections, generics, callables, literals, etc.
    if subtype.is_subtype_of(supertype) {
        return true;
    }

    if crate::virtual_members::laravel::has_model_type_operator(subtype)
        || crate::virtual_members::laravel::has_model_type_operator(supertype)
    {
        let sub = crate::virtual_members::laravel::expand_model_type(subtype, class_loader);
        let sup = crate::virtual_members::laravel::expand_model_type(supertype, class_loader);
        if &sub != subtype || &sup != supertype {
            return is_subtype_of_typed(&sub, &sup, class_loader);
        }
    }

    // ── Union subtype: every member must be a subtype ───────────
    if let TypeKind::Union(members) = subtype.kind() {
        return members
            .iter()
            .all(|m| is_subtype_of_typed(m, supertype, class_loader));
    }

    // ── Union supertype: at least one member must accept subtype ─
    if let TypeKind::Union(members) = supertype.kind() {
        return members
            .iter()
            .any(|m| is_subtype_of_typed(subtype, m, class_loader));
    }

    // ── Nullable normalisation ──────────────────────────────────
    if let TypeKind::Nullable(inner) = subtype.kind() {
        let as_union = PhpType::union(vec![inner.clone(), PhpType::null()]);
        return is_subtype_of_typed(&as_union, supertype, class_loader);
    }
    if let TypeKind::Nullable(inner) = supertype.kind() {
        let as_union = PhpType::union(vec![inner.clone(), PhpType::null()]);
        return is_subtype_of_typed(subtype, &as_union, class_loader);
    }

    // ── Intersection supertype: all members required ────────────
    // Checked before the subtype-intersection branch so that the
    // both-intersections case (e.g. `A&B&C` <: `A&B`) decomposes the
    // supertype first: every supertype member must be satisfied by
    // *some* subtype member.  Handling the subtype first would instead
    // demand that a single member satisfy the whole supertype, wrongly
    // rejecting a narrower intersection that carries extra constraints.
    if let TypeKind::Intersection(members) = supertype.kind() {
        return members
            .iter()
            .all(|m| is_subtype_of_typed(subtype, m, class_loader));
    }

    // ── Intersection subtype: at least one member suffices ──────
    if let TypeKind::Intersection(members) = subtype.kind() {
        return members
            .iter()
            .any(|m| is_subtype_of_typed(m, supertype, class_loader));
    }

    // ── Generic covariance with class-loader awareness ──────────
    // The structural `is_subtype_of` compares generic type params
    // by structural equality, which fails when one side uses a
    // namespace-qualified name and the other uses a short name
    // (e.g. `list<Pen>` vs `list<Demo\Pen>`).  Re-check with the
    // class loader so nominal hierarchy applies to inner params.
    if let (TypeKind::Generic(sub), TypeKind::Generic(sup)) = (subtype.kind(), supertype.kind()) {
        let (name_sub, args_sub) = (&sub.name, &sub.args);
        let (name_sup, args_sup) = (&sup.name, &sup.args);
        let base_sub = name_sub.to_ascii_lowercase();
        let base_sup = name_sup.to_ascii_lowercase();
        let bases_compatible = base_sub == base_sup
            || (crate::php_type::is_array_like_name(name_sub)
                && crate::php_type::is_array_like_name(name_sup));
        if bases_compatible && args_sub.len() == args_sup.len() {
            let is_array_like = crate::php_type::is_array_like_name(name_sub)
                || crate::php_type::is_array_like_name(name_sup);
            let all_params_ok = args_sub.iter().zip(args_sup.iter()).all(|(s, t)| {
                if is_array_like {
                    // Arrays are covariant in PHP (read-only semantics)
                    is_subtype_of_typed(s, t, class_loader)
                } else {
                    // Non-array generics are invariant by default
                    // (both directions must hold, or they must be equal)
                    s == t
                        || (is_subtype_of_typed(s, t, class_loader)
                            && is_subtype_of_typed(t, s, class_loader))
                }
            });
            if all_params_ok {
                return true;
            }
            // Type arguments that provably do not line up in *either*
            // direction settle the question here.  The nominal fallback at
            // the end of this function compares base names only, so it
            // would read `Box<string>` <: `Box<int>` as `Box` <: `Box` and
            // discard the arguments entirely.
            //
            // A pair where one direction does hold is still left to it:
            // variance is not recorded on the resolved type, so a wider
            // argument may be a `@template-contravariant` one, or simply
            // all we managed to infer.
            if !is_array_like
                && args_sub.iter().zip(args_sup.iter()).any(|(s, t)| {
                    !is_subtype_of_typed(s, t, class_loader)
                        && !is_subtype_of_typed(t, s, class_loader)
                })
            {
                return false;
            }
        }

        // ── Array-like generics written at different arities ────
        // The same array spells out at one, two, or no type arguments:
        // `list<V>` is `array<int, V>` and `array<V>` is
        // `array<array-key, V>`.  `is_subtype_of` normalises the pair
        // already, but compares the implied key and value structurally,
        // so a nominal inner type (`array<int, Cat>` <: `array<Animal>`)
        // never lines up.  Redo the comparison through the class loader.
        //
        // A `list` supertype is the one form that demands more than its
        // values: an `array` cannot promise the sequential integer keys,
        // so it is left to the checks below.
        if args_sub.len() != args_sup.len()
            && crate::php_type::is_array_like_name(name_sub)
            && crate::php_type::is_array_like_name(name_sup)
            && !(crate::php_type::is_list_name(name_sup)
                && !crate::php_type::is_list_name(name_sub))
            && let Some((sub_key, sub_val)) = crate::php_type::array_like_key_value(sub)
            && let Some((sup_key, sup_val)) = crate::php_type::array_like_key_value(sup)
            && is_subtype_of_typed(&sub_key, &sup_key, class_loader)
            && is_subtype_of_typed(sub_val, sup_val, class_loader)
        {
            return true;
        }
    }

    // ── Array-like generic <: `X[]` ─────────────────────────────
    // `X[]` is `array<mixed, X>`: it constrains the values and says
    // nothing about the keys, so any array-like generic whose value type
    // fits satisfies it — `array<int, Cat>` and `list<Cat>` are both
    // `Animal[]`.  `is_subtype_of` has no rule for this direction at all.
    if let TypeKind::Generic(sub) = subtype.kind()
        && crate::php_type::is_array_like_name(&sub.name)
        && let TypeKind::Array(inner_sup) = supertype.kind()
        && let Some(sub_val) = sub.args.last()
        && is_subtype_of_typed(sub_val, inner_sup, class_loader)
    {
        return true;
    }

    // ── Array slice covariance ──────────────────────────────────
    // The structural `is_subtype_of` compares `X[]` vs `Y[]` by
    // structural equality on the inner type, which misses nominal
    // subclass relationships (e.g. `Cat[]` <: `Animal[]` where
    // `Cat extends Animal`).  Re-check with the class loader so
    // the hierarchy walk applies to inner types.
    if let (TypeKind::Array(inner_sub), TypeKind::Array(inner_sup)) =
        (subtype.kind(), supertype.kind())
        && is_subtype_of_typed(inner_sub, inner_sup, class_loader)
    {
        return true;
    }

    // ── Callable specification <: Closure / object ──────────────
    // A `Closure(int): string` is a Closure instance, which is an
    // object.  The structural check only handles `callable` as
    // the named supertype; extend to `Closure` and `object`.
    if matches!(subtype.kind(), TypeKind::Callable { .. })
        && let Some(sup) = supertype.base_name()
        && (sup.eq_ignore_ascii_case("Closure") || sup.eq_ignore_ascii_case("object"))
    {
        return true;
    }

    // ── class-string covariance through nominal hierarchy ────────
    // The structural `is_subtype_of` handles `class-string<Cat> <:
    // class-string<Animal>` only when `Cat` and `Animal` are
    // structurally equal.  Extend to nominal hierarchy so that
    // `class-string<Cat>` is accepted where `class-string<Animal>`
    // is expected when `Cat extends Animal`.  A bare `class-string`
    // is treated as `class-string<object>`, so it satisfies
    // `class-string<object>` (and its `mixed` equivalent) — any class
    // name is a class-string of some object.
    if let (TypeKind::ClassString(sub_inner), TypeKind::ClassString(sup_inner)) =
        (subtype.kind(), supertype.kind())
    {
        let object_bound = PhpType::named(atom("object"));
        let sub = sub_inner.as_ref().unwrap_or(&object_bound);
        let sup = sup_inner.as_ref().unwrap_or(&object_bound);
        return is_subtype_of_typed(sub, sup, class_loader);
    }

    // ── class-string <: interface-string ────────────────────────
    // `interface-string` constrains the *name* a string holds, not the
    // type that name denotes: `SomeClass::class` is not one even when
    // `SomeClass implements SomeInterface`, while `SomeInterface::class`
    // is.  Only a name we can load and see is not an interface is
    // rejected — a bare `class-string`, and one whose class we cannot
    // load, stay silent.
    if let (TypeKind::ClassString(sub_inner), TypeKind::InterfaceString(sup_inner)) =
        (subtype.kind(), supertype.kind())
    {
        if let (Some(sub), Some(sup)) = (sub_inner, sup_inner)
            && !is_subtype_of_typed(sub, sup, class_loader)
        {
            return false;
        }
        return sub_inner
            .as_ref()
            .and_then(|inner| inner.base_name())
            .and_then(class_loader)
            .is_none_or(|cls| cls.kind == ClassLikeKind::Interface);
    }

    // ── interface-string <: class-string / interface-string ─────
    // The value names a class-like either way, so only the bounds are
    // left to line up.  A missing bound is `object`, which anything
    // satisfies.
    if let TypeKind::InterfaceString(sub_inner) = subtype.kind()
        && let TypeKind::ClassString(sup_inner) | TypeKind::InterfaceString(sup_inner) =
            supertype.kind()
    {
        let object_bound = PhpType::named(atom("object"));
        let sub = sub_inner.as_ref().unwrap_or(&object_bound);
        let sup = sup_inner.as_ref().unwrap_or(&object_bound);
        return is_subtype_of_typed(sub, sup, class_loader);
    }

    // ── String literal <: model-property<Model> ────────────────
    // The Laravel PHPStan extensions' `model-property<Model>` is a string subtype
    // representing the property names of an Eloquent model.  A
    // string literal is a subtype only if it names a known
    // property.  When the model class cannot be loaded, stay
    // permissive (return true) to avoid false positives.
    if let TypeKind::Literal(lit) = subtype.kind()
        && lit.string_content().is_some()
        && let TypeKind::Generic(g) = supertype.kind()
        && g.name.eq_ignore_ascii_case("model-property")
        && g.args.len() == 1
    {
        let prop_name = lit.string_content().unwrap();
        if let Some(model_name) = g.args[0].base_name()
            && let Some(cls) = class_loader(model_name)
        {
            let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                class_loader,
                crate::virtual_members::active_resolved_class_cache(),
            );
            return resolved.properties.iter().any(|p| *p.name == *prop_name)
                || crate::virtual_members::laravel::where_property::collect_column_names(
                    &resolved,
                )
                .iter()
                .any(|col| *col == *prop_name);
        }
        return true;
    }

    // ── String literal <: class-string<Bound> ────────────────────
    // A string literal that names an existing class satisfying the
    // bound is a valid `class-string<Bound>`, e.g. passing
    // `'RuntimeException'` where `class-string<Throwable>` is
    // expected.  Stay silent (return true) whenever the literal's
    // content cannot be resolved to a class — it may simply live in
    // a file we haven't indexed — and only reject when the resolved
    // class provably fails to satisfy the bound.
    if let Some(crate::php_type::LiteralValue::String(_)) = subtype.as_literal()
        && matches!(
            supertype.kind(),
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_)
        )
    {
        let TypeKind::Literal(lit) = subtype.kind() else {
            unreachable!()
        };
        let Some(class_name) = lit.string_content() else {
            return true;
        };
        let Some(cls) = class_loader(&class_name) else {
            return true;
        };
        if matches!(supertype.kind(), TypeKind::InterfaceString(_))
            && cls.kind != ClassLikeKind::Interface
        {
            return false;
        }
        return match supertype.kind() {
            TypeKind::ClassString(Some(bound)) | TypeKind::InterfaceString(Some(bound)) => {
                match bound.base_name() {
                    Some(bound_name) => is_subtype_of(&cls, bound_name, class_loader),
                    None => true,
                }
            }
            _ => true,
        };
    }

    // ── Class-like <: `object` / `iterable` ─────────────────────
    // Neither `object` nor `iterable` is a class name, so the nominal
    // walk below cannot speak for them: it needs a loadable name on both
    // sides.  `is_named_subtype` answers for a plain `Foo`, but not for a
    // `Foo<T>` — a generic carries the same instance, and its type
    // arguments have no say in whether it is an object.
    //
    // `iterable` is `array|Traversable`, so a class whose hierarchy
    // reaches `Traversable` satisfies it however it was spelled.
    if let Some(sub) = subtype.base_name()
        && let TypeKind::Named(sup) = supertype.kind()
    {
        if sup.eq_ignore_ascii_case("object") && crate::php_type::is_class_like_name(sub) {
            return true;
        }
        if sup.eq_ignore_ascii_case("iterable")
            && class_loader(sub).is_some_and(|cls| is_subtype_of(&cls, "Traversable", class_loader))
        {
            return true;
        }
    }

    // ── Nominal class hierarchy check ───────────────────────────
    // Both sides must resolve to a class name for the hierarchy walk.
    let sub_name = subtype.base_name();
    let sup_name = supertype.base_name();

    if let (Some(sub), Some(sup)) = (sub_name, sup_name) {
        // Try to load the subtype class and walk its hierarchy.
        if let Some(cls) = class_loader(sub) {
            return is_subtype_of(&cls, sup, class_loader);
        }
    }

    false
}

#[cfg(test)]
#[path = "class_lookup_tests.rs"]
mod tests;
