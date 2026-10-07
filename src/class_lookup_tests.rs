use super::*;

#[test]
fn is_self_or_static_matches_three() {
    assert!(is_self_or_static("self"));
    assert!(is_self_or_static("static"));
    assert!(is_self_or_static("$this"));
}

#[test]
fn is_self_or_static_excludes_parent() {
    assert!(!is_self_or_static("parent"));
    assert!(!is_self_or_static("Parent"));
    assert!(!is_self_or_static("PARENT"));
}

#[test]
fn is_self_or_static_case_insensitive() {
    assert!(is_self_or_static("Self"));
    assert!(is_self_or_static("SELF"));
    assert!(is_self_or_static("Static"));
    assert!(is_self_or_static("STATIC"));
}

#[test]
fn is_self_or_static_rejects_others() {
    assert!(!is_self_or_static(""));
    assert!(!is_self_or_static("this"));
    assert!(!is_self_or_static("Foo"));
}

/// Helper to build a minimal `ClassInfo` for hierarchy tests.
fn make_class(
    name: &str,
    namespace: Option<&str>,
    parent: Option<&str>,
    interfaces: &[&str],
) -> Arc<ClassInfo> {
    Arc::new(ClassInfo {
        name: crate::atom::atom(name),
        file_namespace: namespace.map(crate::atom::atom),
        parent_class: parent.map(crate::atom::atom),
        interfaces: interfaces.iter().map(|s| crate::atom::atom(s)).collect(),
        ..Default::default()
    })
}

/// Build a class loader from a slice of `Arc<ClassInfo>`.
fn loader_from(classes: &[Arc<ClassInfo>]) -> impl Fn(&str) -> Option<Arc<ClassInfo>> + '_ {
    move |name: &str| classes.iter().find(|c| c.fqn() == name).cloned()
}

// ── is_subtype_of: FQN self-check ──────────────────────────

#[test]
fn subtype_of_self_by_fqn() {
    let cls = make_class("User", Some("App\\Models"), None, &[]);
    let classes = [cls.clone()];
    let loader = loader_from(&classes);
    assert!(is_subtype_of(&cls, "App\\Models\\User", &loader));
}

#[test]
fn subtype_of_self_root_namespace() {
    // Root-namespace class: FQN == short name.
    let cls = make_class("RuntimeException", None, None, &[]);
    let classes = [cls.clone()];
    let loader = loader_from(&classes);
    assert!(is_subtype_of(&cls, "RuntimeException", &loader));
}

#[test]
fn subtype_of_self_short_name_resolves_via_loader() {
    // Passing a short name that the loader can resolve to a FQN.
    let cls = make_class("User", Some("App\\Models"), None, &[]);
    let classes = [cls.clone()];
    let loader = loader_from(&classes);
    // The loader finds "User" → no, it only matches on fqn().
    // So passing just "User" when the class is App\Models\User
    // should NOT match (different FQN).
    assert!(!is_subtype_of(&cls, "User", &loader));
}

// ── is_subtype_of: interface matching by FQN ────────────────

#[test]
fn subtype_of_interface_fqn_match() {
    let cls = make_class(
        "UserRepo",
        Some("App\\Repos"),
        None,
        &["App\\Contracts\\Repository"],
    );
    let iface = make_class("Repository", Some("App\\Contracts"), None, &[]);
    let classes = [cls.clone(), iface];
    let loader = loader_from(&classes);
    assert!(is_subtype_of(&cls, "App\\Contracts\\Repository", &loader));
}

#[test]
fn subtype_of_interface_short_name_does_not_match_different_namespace() {
    // Two unrelated classes that share the short name "Carbon".
    // `is_subtype_of` must NOT treat them as the same type.
    let vendor_carbon = make_class("Carbon", Some("Vendor\\DateTime"), None, &[]);
    let cls = make_class("MyDate", Some("App"), None, &["Vendor\\DateTime\\Carbon"]);
    let app_carbon = make_class("Carbon", Some("App\\DateTime"), None, &[]);
    let classes = [cls.clone(), vendor_carbon, app_carbon.clone()];
    let loader = loader_from(&classes);

    // The class implements Vendor\DateTime\Carbon, NOT App\DateTime\Carbon.
    assert!(is_subtype_of(&cls, "Vendor\\DateTime\\Carbon", &loader));
    assert!(!is_subtype_of(&cls, "App\\DateTime\\Carbon", &loader));
}

// ── is_subtype_of: parent chain by FQN ──────────────────────

#[test]
fn subtype_of_parent_fqn() {
    let parent = make_class("BaseModel", Some("App\\Models"), None, &[]);
    let child = make_class(
        "User",
        Some("App\\Models"),
        Some("App\\Models\\BaseModel"),
        &[],
    );
    let classes = [parent, child.clone()];
    let loader = loader_from(&classes);
    assert!(is_subtype_of(&child, "App\\Models\\BaseModel", &loader));
}

#[test]
fn subtype_of_grandparent_fqn() {
    let grandparent = make_class("Model", Some("Illuminate"), None, &[]);
    let parent = make_class("BaseModel", Some("App"), Some("Illuminate\\Model"), &[]);
    let child = make_class("User", Some("App"), Some("App\\BaseModel"), &[]);
    let classes = [grandparent, parent, child.clone()];
    let loader = loader_from(&classes);
    assert!(is_subtype_of(&child, "Illuminate\\Model", &loader));
}

#[test]
fn subtype_of_parent_interface_fqn() {
    // Parent implements an interface; child should also be a subtype.
    let iface = make_class("Countable", None, None, &[]);
    let parent = make_class("Collection", Some("App"), None, &["Countable"]);
    let child = make_class("UserCollection", Some("App"), Some("App\\Collection"), &[]);
    let classes = [iface, parent, child.clone()];
    let loader = loader_from(&classes);
    assert!(is_subtype_of(&child, "Countable", &loader));
}

#[test]
fn subtype_of_unrelated_class_returns_false() {
    let cls = make_class("User", Some("App"), None, &[]);
    let other = make_class("Order", Some("App"), None, &[]);
    let classes = [cls.clone(), other];
    let loader = loader_from(&classes);
    assert!(!is_subtype_of(&cls, "App\\Order", &loader));
}

// ── is_subtype_of: short ancestor resolves through loader ───

#[test]
fn subtype_of_short_ancestor_resolved_by_loader() {
    // The ancestor name "RuntimeException" is a root-namespace class.
    // The loader can resolve it, and the comparison should work.
    let exc = make_class("RuntimeException", None, None, &["Exception"]);
    let cls = make_class("AppException", Some("App"), Some("RuntimeException"), &[]);
    let classes = [exc, cls.clone()];
    let loader = loader_from(&classes);
    assert!(is_subtype_of(&cls, "RuntimeException", &loader));
}

// ── is_subtype_of: global name shadowed by consuming file's use-map ─

#[test]
fn subtype_of_global_interface_not_broken_by_shadowing_use_import() {
    // A project class shares the short name of a global interface
    // (`Iterator`).  The consuming file imports the project class
    // (`use App\Input\Iterator;`), so its use-map maps the unqualified
    // name `Iterator` to `App\Input\Iterator`.  Subtype checks against
    // the global `\Iterator` / `\Traversable` — reached while walking a
    // stub class's hierarchy — must still succeed.
    let traversable = make_class("Traversable", None, None, &[]);
    let iterator = make_class("Iterator", None, None, &["Traversable"]);
    let recursive_iterator = make_class("RecursiveIterator", None, None, &["Iterator"]);
    let rec_dir_iterator = make_class(
        "RecursiveDirectoryIterator",
        None,
        None,
        &["RecursiveIterator"],
    );
    // The shadowing project class (imported into the consuming file).
    let app_iterator = make_class("Iterator", Some("App\\Input"), None, &[]);

    let classes = [
        traversable,
        iterator,
        recursive_iterator,
        rec_dir_iterator.clone(),
        app_iterator.clone(),
    ];

    // A loader that mirrors `class_loader_with`: an unqualified name in
    // the use-map resolves to the imported class first; the `__fqn__\`
    // bypass skips the use-map and resolves the global short name.
    let loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        let stripped = name.strip_prefix('\\').unwrap_or(name);
        if !stripped.contains('\\') && stripped == "Iterator" {
            // use-map shadow: unqualified `Iterator` → App\Input\Iterator
            return Some(app_iterator.clone());
        }
        if let Some(cls) = classes.iter().find(|c| c.fqn() == stripped).cloned() {
            return Some(cls);
        }
        if stripped.contains('\\') {
            let short = crate::util::short_name(stripped);
            return classes.iter().find(|c| c.fqn() == short).cloned();
        }
        None
    };

    // Without the fix, the intermediate global `Iterator` node resolves
    // to `App\Input\Iterator`, so the walk never reaches `Traversable`
    // and the ancestor `Iterator` itself mis-resolves.
    assert!(is_subtype_of(&rec_dir_iterator, "Iterator", &loader));
    assert!(is_subtype_of(&rec_dir_iterator, "Traversable", &loader));
    // A genuinely unrelated global class is still rejected.
    assert!(!is_subtype_of(&rec_dir_iterator, "Countable", &loader));
}

// ── is_subtype_of_typed: generic type arguments ─────────────

#[test]
fn subtype_of_typed_rejects_disjoint_generic_arguments() {
    use crate::php_type::PhpType;

    let animal = make_class("Animal", None, None, &[]);
    let cat = make_class("Cat", None, Some("Animal"), &[]);
    let plant = make_class("Plant", None, None, &[]);
    let boxed = make_class("Box", None, None, &[]);
    let classes = [animal, cat, plant, boxed];
    let loader = loader_from(&classes);
    let parse = |src: &str| PhpType::parse(src);

    // Neither argument is the other, so the two boxes are unrelated.
    // The nominal fallback would compare the bases alone and call them
    // the same class.
    assert!(!is_subtype_of_typed(
        &parse("Box<string>"),
        &parse("Box<int>"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("Box<Cat>"),
        &parse("Box<Plant>"),
        &loader
    ));

    // A narrower or wider argument leaves the question to the nominal
    // check as before: variance is not recorded on the resolved type.
    assert!(is_subtype_of_typed(
        &parse("Box<Cat>"),
        &parse("Box<Animal>"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("Box<Animal>"),
        &parse("Box<Cat>"),
        &loader
    ));
}

// ── is_subtype_of_typed: object and iterable supertypes ─────

#[test]
fn subtype_of_typed_accepts_any_class_like_as_object() {
    use crate::php_type::PhpType;

    let collection = make_class("Collection", Some("App"), None, &[]);
    let cat = make_class("Cat", None, None, &[]);
    let classes = [collection, cat];
    let loader = loader_from(&classes);
    let parse = |src: &str| PhpType::parse(src);

    // A plain name and a generic both name an instance; the type
    // arguments have no say in it.
    assert!(is_subtype_of_typed(
        &parse("Cat"),
        &parse("object"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("App\\Collection<int, Cat>"),
        &parse("object"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("Cat"),
        &parse("?object"),
        &loader
    ));
    // A class the project cannot load is still an object.
    assert!(is_subtype_of_typed(
        &parse("Unindexed<Cat>"),
        &parse("object"),
        &loader
    ));
    // Scalars and arrays are not.
    assert!(!is_subtype_of_typed(
        &parse("string"),
        &parse("object"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array<int, Cat>"),
        &parse("object"),
        &loader
    ));
}

#[test]
fn subtype_of_typed_accepts_traversable_class_as_iterable() {
    use crate::php_type::PhpType;

    let traversable = make_class("Traversable", None, None, &[]);
    let iterator = make_class("Iterator", None, None, &["Traversable"]);
    let collection = make_class("Collection", Some("App"), None, &["Iterator"]);
    let plain = make_class("Plain", Some("App"), None, &[]);
    let classes = [traversable, iterator, collection, plain];
    let loader = loader_from(&classes);
    let parse = |src: &str| PhpType::parse(src);

    assert!(is_subtype_of_typed(
        &parse("App\\Collection"),
        &parse("iterable"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("App\\Collection<Cat>"),
        &parse("iterable"),
        &loader
    ));
    // A class with no Traversable in its hierarchy cannot be iterated.
    assert!(!is_subtype_of_typed(
        &parse("App\\Plain"),
        &parse("iterable"),
        &loader
    ));
}

// ── is_subtype_of_typed: array-like cross-form rules ────────

#[test]
fn subtype_of_typed_array_forms_resolve_nominal_value_types() {
    use crate::php_type::PhpType;

    let animal = make_class("Animal", None, None, &[]);
    let cat = make_class("Cat", None, Some("Animal"), &[]);
    let classes = [animal, cat];
    let loader = loader_from(&classes);
    let parse = |src: &str| PhpType::parse(src);

    // The value type is only a subtype nominally, and the two sides are
    // written at different arities, so neither the structural check nor
    // the equal-arity branch can answer.
    assert!(is_subtype_of_typed(
        &parse("array<int, Cat>"),
        &parse("array<Animal>"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("list<Cat>"),
        &parse("array<int, Animal>"),
        &loader
    ));
    // `X[]` constrains the values and says nothing about the keys.
    assert!(is_subtype_of_typed(
        &parse("array<string, Cat>"),
        &parse("Animal[]"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("list<Cat>"),
        &parse("Animal[]"),
        &loader
    ));

    // A wider value type does not fit a narrower slice.
    assert!(!is_subtype_of_typed(
        &parse("array<int, Animal>"),
        &parse("Cat[]"),
        &loader
    ));
    // Only a list promises the sequential keys a `list` supertype wants.
    assert!(!is_subtype_of_typed(
        &parse("array<int, Cat>"),
        &parse("list<Animal>"),
        &loader
    ));
}

// ── is_subtype_of_typed: array shapes ───────────────────────

/// Two shapes are settled by the entries they list. An unsealed one reads
/// as the generic array it widens to everywhere else, and every
/// `array{…, ...}` widens to the same `non-empty-array<array-key, mixed>`.
#[test]
fn subtype_of_typed_compares_unsealed_shapes_by_the_entries_they_list() {
    use crate::php_type::PhpType;

    let loader = loader_from(&[]);
    let parse = |src: &str| PhpType::parse(src);

    assert!(!is_subtype_of_typed(
        &parse("array{foo: int, ...}"),
        &parse("array{foo: string, ...}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{bar: int, ...}"),
        &parse("array{foo: int, ...}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{foo?: int, ...}"),
        &parse("array{foo: int, ...}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{foo: int, ...<string, string>}"),
        &parse("array{foo: int, ...<string, int>}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{foo: int, ...}"),
        &parse("array{foo: int}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("list{string, ...<int>}"),
        &parse("list{int, ...<int>}"),
        &loader
    ));

    assert!(is_subtype_of_typed(
        &parse("array{foo: int, bar: string, ...}"),
        &parse("array{foo: int, ...}"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("array{foo: int, ...<string, int>}"),
        &parse("array{foo: int|string, ...<string, int|string>}"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("array{foo: int}"),
        &parse("array{foo: int, ...}"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("list{string, ...<int>}"),
        &parse("list{string, ...<int>}"),
        &loader
    ));
}

/// The class hierarchy reaches into the entries of a shape, sealed or
/// unsealed, and into the tail of an unsealed one.
#[test]
fn subtype_of_typed_follows_the_class_hierarchy_into_the_entries_of_a_shape() {
    use crate::php_type::PhpType;

    let animal = make_class("Animal", None, None, &[]);
    let cat = make_class("Cat", None, Some("Animal"), &[]);
    let plant = make_class("Plant", None, None, &[]);
    let classes = [animal, cat, plant];
    let loader = loader_from(&classes);
    let parse = |src: &str| PhpType::parse(src);

    assert!(is_subtype_of_typed(
        &parse("array{pet: Cat, ...}"),
        &parse("array{pet: Animal, ...}"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("array{pet: Cat}"),
        &parse("array{pet: Animal}"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("array{pet: Cat, ...<string, Cat>}"),
        &parse("array{pet: Animal, ...<string, Animal>}"),
        &loader
    ));
    assert!(is_subtype_of_typed(
        &parse("list{Cat, ...<Cat>}"),
        &parse("list{Animal, ...<Animal>}"),
        &loader
    ));

    assert!(!is_subtype_of_typed(
        &parse("array{pet: Plant, ...}"),
        &parse("array{pet: Animal, ...}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{pet: Animal, ...}"),
        &parse("array{pet: Cat, ...}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{pet: Cat}"),
        &parse("array{pet: Plant}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{pet: Cat, ...<string, Plant>}"),
        &parse("array{pet: Animal, ...<string, Animal>}"),
        &loader
    ));
    assert!(!is_subtype_of_typed(
        &parse("array{pet: Cat, ...<string, Animal>}"),
        &parse("array{pet: Animal, ...<string, Cat>}"),
        &loader
    ));
}
