//! Names written in a file, read the way PHP reads them, and resolved
//! names written back the way the file would spell them.
//!
//! A class name written in code or in a docblock follows PHP's rule: the
//! block's imports, else the current namespace, never a global class.  A
//! name no class answers to still comes out as the FQN PHP reads it, so a
//! later lookup cannot land on a global class of the same short name.

use std::collections::HashMap;
use std::sync::Arc;

/// Resolve an unqualified or partially-qualified PHP class/function name
/// to a fully-qualified name using the file's `use` map and namespace.
///
/// Rules:
///   - Leading `\` — strip it and return (already fully-qualified).
///   - Unqualified (no `\`):
///     1. Check the `use_map` for a direct mapping.
///     2. Prefix with the current namespace.
///     3. Fall back to the bare name (global namespace).
///   - Qualified (contains `\`, no leading `\`):
///     1. Check if the first segment is in the `use_map`; if so, expand it.
///     2. Prefix with the current namespace.
///     3. Fall back to the bare name.
///
/// The result never has a leading `\`.  Import aliases match
/// case-insensitively (see [`lookup_import`]).
pub(crate) fn resolve_to_fqn(
    name: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> String {
    if let Some(stripped) = name.strip_prefix('\\') {
        return stripped.to_string();
    }

    if !name.contains('\\') {
        if let Some(fqn) = lookup_import(use_map, name) {
            return fqn.clone();
        }
        if let Some(ns) = namespace {
            return format!("{}\\{}", ns, name);
        }
        return name.to_string();
    }

    let first_segment = name.split('\\').next().unwrap_or(name);
    if let Some(fqn_prefix) = lookup_import(use_map, first_segment) {
        let rest = &name[first_segment.len()..];
        return format!("{}{}", fqn_prefix, rest);
    }
    if let Some(ns) = namespace {
        return format!("{}\\{}", ns, name);
    }
    name.to_string()
}

/// The function a call by `name`, as written, reaches: the FQN PHP looks
/// up first, and the global name it falls back to when that one does not
/// exist.
///
/// A qualified name's first segment goes through the class imports
/// (`class_imports`) and never falls back.  A bare name is a `use function`
/// alias, which never falls back either, or else local to the namespace,
/// falling back to the global function.
pub(crate) fn function_lookup_names<'n>(
    name: &'n str,
    class_imports: &HashMap<String, String>,
    function_imports: &HashMap<String, String>,
    namespace: &Option<String>,
) -> (String, Option<&'n str>) {
    if name.contains('\\') {
        return (resolve_to_fqn(name, class_imports, namespace), None);
    }
    if let Some(fqn) = lookup_import(function_imports, name) {
        return (fqn.clone(), None);
    }
    match namespace {
        Some(ns) => (format!("{ns}\\{name}"), Some(name)),
        None => (name.to_string(), None),
    }
}

/// Reverse of [`resolve_to_fqn`]: shorten a resolved name to the form the
/// file would reference it by: the `use`-map alias when one imports it,
/// or the bare name when it lives in the file's own namespace.  A
/// qualified name that matches neither gets a leading `\` inside a
/// namespace, which would otherwise read it relative to itself.  A bare
/// name that matches neither comes back as is, since it may be a
/// template parameter rather than a global class; use
/// [`shorten_class_fqn`] for a name known to be a class.
pub(crate) fn shorten_from_fqn(
    fqn: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> String {
    let fqn = fqn.trim_start_matches('\\');
    shorten_to_import_or_namespace(fqn, use_map, namespace).unwrap_or_else(|| {
        if namespace.is_some() && fqn.contains('\\') {
            format!("\\{fqn}")
        } else {
            fqn.to_string()
        }
    })
}

/// [`shorten_from_fqn`] for a name known to be a class: one the file can
/// neither import-alias nor reach through its namespace is written fully
/// qualified with a leading `\`, global classes included.
pub(crate) fn shorten_class_fqn(
    fqn: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> String {
    let fqn = fqn.trim_start_matches('\\');
    shorten_to_import_or_namespace(fqn, use_map, namespace).unwrap_or_else(|| {
        if namespace.is_some() {
            format!("\\{fqn}")
        } else {
            fqn.to_string()
        }
    })
}

/// Write a resolved type as source text for a file with `use_map` and
/// `namespace`: every name `class_loader` knows as a class is shortened
/// with [`shorten_class_fqn`], and anything else (a template parameter)
/// with [`shorten_from_fqn`].
pub(crate) fn shorten_type_for_file(
    ty: &crate::php_type::PhpType,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> crate::php_type::PhpType {
    ty.resolve_names(&|name| {
        if crate::class_lookup::load_resolved_name(name, class_loader).is_some() {
            shorten_class_fqn(name, use_map, namespace)
        } else {
            shorten_from_fqn(name, use_map, namespace)
        }
    })
}

fn shorten_to_import_or_namespace(
    fqn: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> Option<String> {
    for (alias, imported) in use_map {
        if imported.trim_start_matches('\\') == fqn {
            return Some(alias.clone());
        }
    }
    let rest = fqn
        .strip_prefix(namespace.as_deref()?)
        .and_then(|rest| rest.strip_prefix('\\'))?;
    (!rest.contains('\\')).then(|| rest.to_string())
}

/// Resolve a class name to its FQN via the class loader.
///
/// Returns the FQN from the loaded `ClassInfo` when the loader can find
/// the class, or falls back to the original `name` unchanged.
///
/// **Caveat:** when the loader cannot resolve `name`, the original string
/// is returned as-is.  If `name` is a short (unqualified) class name,
/// the returned value is *not* a FQN — it is the same short name.
/// Callers that need a guaranteed FQN should use [`resolve_to_fqn`]
/// with the file's use-map and namespace instead, falling back to this
/// function only for names that are already expected to be resolvable
/// by the class loader (e.g. names extracted from `::class` expressions
/// or already-resolved type hints).
pub(crate) fn resolve_name_via_loader(
    name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> String {
    class_loader(name)
        .map(|cls| cls.fqn().to_string())
        .unwrap_or_else(|| name.to_string())
}

fn find_source_class_name(
    name: &str,
    namespace: Option<&str>,
    local_classes: &[Arc<crate::types::ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> Option<String> {
    if !name.contains('\\')
        && let Some(local) = local_classes.iter().find(|c| {
            c.name.eq_ignore_ascii_case(name)
                && match (c.file_namespace.as_deref(), namespace) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    (None, None) => true,
                    _ => false,
                }
        })
    {
        return Some(local.fqn().to_string());
    }
    class_loader(name).map(|cls| cls.fqn().to_string())
}

/// Resolve a source-level (user-typed) class reference for a class named
/// in code (`new Foo`, `Foo::`, `instanceof Foo`, `catch (Foo $e)`) or in a
/// docblock, to the FQN of the class it names, through the source loader.
///
/// The source loader applies PHP's rule (the block's imports, else the
/// current namespace, never a global class), so this only adds a check of
/// the file's own classes: PHP refuses to compile an import that collides
/// with a class declared in the same namespace block, so an unqualified
/// name that matches one always means it.  This is what keeps a file with
/// several `namespace` blocks from resolving a short name in a later block
/// to the same-named class of the first, which is the namespace the
/// file-wide class loader was built for.
///
/// A written class name never falls back to the global scope: inside
/// `namespace App`, an unimported `Foo` is `App\Foo` whether or not that
/// class exists, even when a global `Foo` does.  A name no class answers
/// to is qualified that way too, when nothing in `imports` (the namespace
/// block's `use` table) claims its first segment, so a later lookup of the
/// result cannot land on the global class.
///
/// `imports` is `None` where the caller has no file context to read them
/// from; a miss then comes back as written.
pub(crate) fn resolve_code_class_name(
    name: &str,
    namespace: Option<&str>,
    imports: Option<&HashMap<String, String>>,
    local_classes: &[Arc<crate::types::ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> String {
    find_source_class_name(name, namespace, local_classes, class_loader)
        .or_else(|| qualify_unknown_code_name(name, namespace, imports))
        .unwrap_or_else(|| name.to_string())
}

/// The name PHP reads a class name no class answers to as: expanded
/// through the import that claims its first segment, else qualified with
/// the current namespace.  `None` when PHP reads it as written.
fn qualify_unknown_code_name(
    name: &str,
    namespace: Option<&str>,
    imports: Option<&HashMap<String, String>>,
) -> Option<String> {
    expand_import(name, imports).or_else(|| {
        namespace_relative_code_name(name, namespace, imports).map(|ns| format!("{ns}\\{name}"))
    })
}

/// `name` with its first segment replaced by the import that claims it,
/// for a relative name an entry of `imports` claims.
fn expand_import(name: &str, imports: Option<&HashMap<String, String>>) -> Option<String> {
    if name.starts_with('\\') {
        return None;
    }
    let (first_segment, rest) = match name.find('\\') {
        Some(pos) => (&name[..pos], &name[pos..]),
        None => (name, ""),
    };
    lookup_import(imports?, first_segment).map(|target| format!("{target}{rest}"))
}

/// The import `alias` names in `imports`.  Aliases are case-insensitive,
/// like class names, so a `use Lib\Bar;` answers to `bar` too.
pub(crate) fn lookup_import<'m>(
    imports: &'m HashMap<String, String>,
    alias: &str,
) -> Option<&'m String> {
    imports.get(alias).or_else(|| {
        imports
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(alias))
            .map(|(_, target)| target)
    })
}

/// The namespace PHP qualifies a class named in code with, when that is
/// all it can mean: a relative name inside a namespace whose first segment
/// no import claims.  `None` for a fully qualified name, a name in the
/// global namespace, an imported name, or unknown `imports`.
fn namespace_relative_code_name<'a>(
    name: &str,
    namespace: Option<&'a str>,
    imports: Option<&HashMap<String, String>>,
) -> Option<&'a str> {
    let (ns, imports) = (namespace?, imports?);
    if name.starts_with('\\') {
        return None;
    }
    let first_segment = name.split('\\').next().unwrap_or(name);
    lookup_import(imports, first_segment)
        .is_none()
        .then_some(ns)
}

/// Resolve all class names inside a [`PhpType`] to their fully-qualified
/// forms using the class loader.  Scalar/keyword types are left untouched.
///
/// This should be called on any `PhpType` that originates from raw source
/// text (docblock annotations, AST identifiers) before it is stored in a
/// [`ResolvedType`](crate::types::ResolvedType).
pub(crate) fn resolve_php_type_names(
    ty: &crate::php_type::PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> crate::php_type::PhpType {
    let resolved = ty.resolve_names(&|name| resolve_name_via_loader(name, class_loader));
    fill_generic_type_defaults(&resolved, class_loader)
}

/// Spell out generic type arguments a declared type left to their
/// `@template` default.
///
/// `@param Test<false> $one` on a class declaring
/// `@template T1 = true  @template T2 = true` binds only `T1`; member
/// lookups already see `T2`'s default because [`build_generic_subs`] fills
/// omitted trailing parameters in, but the annotation's own `PhpType` still
/// carries just the one argument written. Left alone, hover shows
/// `Test<false>` and a comparison against the fully spelled `Test<false,
/// true>` sees two different arities for what should be the same type.
///
/// This walks the type and rebuilds every `Generic` node whose argument
/// count falls short of its class's template parameters, spelling out each
/// omitted parameter the way [`build_generic_subs`] binds it for member
/// substitution.
///
/// Only a parameter that is actually left to something is filled: one with
/// a declared default, or a leading key parameter a short list skips
/// (`Collection<User>` is `Collection<array-key, User>`, `Iterator<Foo>` is
/// `Iterator<mixed, Foo>`). A trailing parameter with neither is left out,
/// as PHPStan leaves it, rather than spelled `mixed`: writing
/// `Iterator<Foo>` out as `Iterator<Foo, mixed>` would move `Foo` into the
/// key slot for every consumer that reads the arguments positionally.
///
/// [`build_generic_subs`]: crate::inheritance::build_generic_subs
fn fill_generic_type_defaults(
    ty: &crate::php_type::PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> crate::php_type::PhpType {
    use crate::php_type::{PhpType, TypeKind};

    match ty.raw_kind() {
        TypeKind::Generic(g) => {
            let args: Vec<PhpType> = g
                .args
                .iter()
                .map(|a| fill_generic_type_defaults(a, class_loader))
                .collect();
            // The name is already resolved, so a source-name loader must not
            // resolve a global FQN such as `Iterator` against the file's
            // namespace again.
            let fq_class = crate::class_lookup::load_resolved_name(g.name.as_str(), class_loader);
            let filled_args = fq_class.and_then(|cls| {
                let written = args.len();
                let offset = crate::inheritance::generic_arg_offset(&cls, written);
                let every_omitted_is_left_to_something = cls
                    .template_params
                    .iter()
                    .skip(offset + written)
                    .all(|param| cls.template_param_defaults.contains_key(param));
                (cls.template_params.len() > written && every_omitted_is_left_to_something).then(
                    || {
                        let subs = crate::inheritance::build_generic_subs(&cls, &args);
                        cls.template_params
                            .iter()
                            .map(|param| {
                                subs.get(param.as_str())
                                    .cloned()
                                    .unwrap_or_else(PhpType::mixed)
                            })
                            .collect::<Vec<_>>()
                    },
                )
            });
            PhpType::generic_atom(g.name, filled_args.unwrap_or(args))
        }
        _ => ty.map_children(&|t| fill_generic_type_defaults(t, class_loader)),
    }
}

/// [`resolve_php_type_names`] for a native type hint written in code (a
/// parameter, a closure's return type, a `catch`), where every name is a
/// class: each one follows [`resolve_code_class_name`], so a name no class
/// answers to is still qualified the way PHP reads it.
pub(crate) fn resolve_code_php_type_names(
    ty: &crate::php_type::PhpType,
    namespace: Option<&str>,
    imports: Option<&HashMap<String, String>>,
    local_classes: &[Arc<crate::types::ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> crate::php_type::PhpType {
    let resolved = ty.resolve_names(&|name| {
        resolve_code_class_name(name, namespace, imports, local_classes, class_loader)
    });
    fill_generic_type_defaults(&resolved, class_loader)
}

/// [`resolve_php_type_names`] for a type written as source the reader
/// resolves relative to where it stands: a docblock annotation inside a
/// body.
///
/// Every class name follows PHP's rule through the source loader, the same
/// as a `@param` tag (which the parser resolves through the file's use-map
/// and namespace): `@var list<Error>` inside `namespace App` means
/// `App\Error` unless `Error` is imported, never the global `\Error`.
///
/// A name no class answers to is qualified the way
/// [`resolve_code_class_name`] qualifies it, so the global class of the
/// same short name is not read in its place, unless a docblock of
/// `content` (the file the type is written in) declares it as a template
/// parameter or a type alias for a declaration that reaches `region`
/// (see [`written_name_region`]).  Those keep their spelling for the
/// declaration in scope to read.
pub(crate) fn resolve_source_php_type_names(
    ty: &crate::php_type::PhpType,
    namespace: Option<&str>,
    imports: Option<&HashMap<String, String>>,
    content: &str,
    region: (u32, u32),
    local_classes: &[Arc<crate::types::ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> crate::php_type::PhpType {
    let resolved = ty.resolve_names(&|name| {
        find_source_class_name(name, namespace, local_classes, class_loader)
            .or_else(|| {
                qualify_unknown_code_name(name, namespace, imports)
                    .filter(|_| !declares_docblock_type_name(content, name, region))
            })
            .unwrap_or_else(|| name.to_string())
    });
    fill_generic_type_defaults(&resolved, class_loader)
}

/// The byte range of `content` a type written at `cursor_offset` inside
/// `class` stands in, for deciding which `@template` and type alias
/// declarations reach it.
///
/// Inside a class this is the whole class: a type read at the cursor may
/// have been written in another of the class's members (a callee's
/// parameter, a bound), so the templates of every member stay in scope.
/// Outside a class it is the cursor itself, and a walk that runs without
/// one (`u32::MAX`) gets the whole file.
pub(crate) fn written_name_region(
    content: &str,
    class: Option<&crate::types::ClassInfo>,
    cursor_offset: u32,
) -> (u32, u32) {
    match class {
        Some(cls) if cls.end_offset > cls.start_offset => (cls.start_offset, cls.end_offset),
        _ if (cursor_offset as usize) <= content.len() => (cursor_offset, cursor_offset),
        _ => (0, u32::MAX),
    }
}

/// Whether a docblock in `content` declares `name` as a `@template`
/// parameter or a type alias (`@phpstan-type`, `@psalm-type`, or either
/// side of an `@phpstan-import-type … as …`) on a declaration whose span
/// overlaps `region`.  A file in the active parse cache collects its
/// declarations once per pass (see
/// [`declares_docblock_type_name`](crate::parser::declares_docblock_type_name)).
fn declares_docblock_type_name(content: &str, name: &str, region: (u32, u32)) -> bool {
    !name.contains('\\') && crate::parser::declares_docblock_type_name(content, name, region)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_aliases_match_case_insensitively() {
        let imports = HashMap::from([("Bar".to_string(), r"Lib\Bar".to_string())]);
        let namespace = Some("App".to_string());
        assert_eq!(resolve_to_fqn("bar", &imports, &namespace), r"Lib\Bar");
        assert_eq!(
            resolve_to_fqn(r"BAR\Baz", &imports, &namespace),
            r"Lib\Bar\Baz"
        );
        assert_eq!(resolve_to_fqn("Baz", &imports, &namespace), r"App\Baz");
        assert_eq!(resolve_to_fqn(r"\bar", &imports, &namespace), "bar");
    }

    /// `ty` written inside `namespace App` of `content`, with `imports` and
    /// no class answering to any name.
    fn resolve_unknown_docblock_type(ty: &str, imports: &[(&str, &str)], content: &str) -> String {
        resolve_unknown_docblock_type_at(ty, imports, content, (0, 0))
    }

    /// [`resolve_unknown_docblock_type`] with the type standing in
    /// `region` of `content`.
    fn resolve_unknown_docblock_type_at(
        ty: &str,
        imports: &[(&str, &str)],
        content: &str,
        region: (u32, u32),
    ) -> String {
        let imports: HashMap<String, String> = imports
            .iter()
            .map(|(alias, fqn)| (alias.to_string(), fqn.to_string()))
            .collect();
        resolve_source_php_type_names(
            &crate::php_type::PhpType::parse(ty),
            Some("App"),
            Some(&imports),
            content,
            region,
            &[],
            &|_| None,
        )
        .to_string()
    }

    #[test]
    fn unknown_docblock_name_is_qualified_like_code() {
        assert_eq!(
            resolve_unknown_docblock_type("Bar", &[], "<?php"),
            r"App\Bar"
        );
        assert_eq!(
            resolve_unknown_docblock_type("list<Bar>", &[("Bar", r"Lib\Bar")], "<?php"),
            r"list<Lib\Bar>"
        );
        assert_eq!(
            resolve_unknown_docblock_type(r"\Bar", &[], "<?php"),
            r"\Bar"
        );
    }

    #[test]
    fn docblock_templates_and_aliases_keep_their_spelling() {
        for content in [
            "/**\n * @template T\n */",
            "/** @template-covariant T of object */",
            "/** @psalm-template T as object */",
            "/** @phpstan-type T array{a: int} */",
            "/** @psalm-type T=array{a: int} */",
            "/** @phpstan-import-type T from Other */",
            "/** @phpstan-import-type Shape from Other as T */",
        ] {
            assert_eq!(
                resolve_unknown_docblock_type("T", &[], content),
                "T",
                "{content}"
            );
        }
        for content in [
            "/** @template-extends T<int> */",
            "/** @var T $t */",
            "/** @template TT */",
        ] {
            assert_eq!(
                resolve_unknown_docblock_type("T", &[], content),
                r"App\T",
                "{content}"
            );
        }
    }

    #[test]
    fn docblock_templates_reach_only_their_own_declaration() {
        let content = "<?php\nnamespace App;\n\n/** @template Node */\nclass Box {}\n\n\
                       class Walker {\n    /** @template Item */\n    public function run() {}\n}\n\n\
                       $f = /** @template Leaf */ fn() => 1;\n";
        let at = |needle: &str| content.find(needle).unwrap() as u32;
        let in_box = (at("class Box"), at("class Box") + 1);
        let in_walker = (at("class Walker"), at("$f") - 2);
        let at_top = (at("$f"), at("$f"));
        let in_arrow = (at("=> 1"), at("=> 1"));

        let resolve = |ty: &str, region| resolve_unknown_docblock_type_at(ty, &[], content, region);
        assert_eq!(resolve("Node", in_box), "Node");
        assert_eq!(resolve("Node", in_walker), r"App\Node");
        assert_eq!(resolve("Item", in_walker), "Item");
        assert_eq!(resolve("Item", in_box), r"App\Item");
        assert_eq!(resolve("Leaf", in_arrow), "Leaf");
        assert_eq!(resolve("Leaf", at_top), r"App\Leaf");
    }
}
