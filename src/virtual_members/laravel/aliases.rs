//! Laravel alias tables parsed from the *installed framework source* and the
//! project's `config/app.php`.
//!
//! Laravel registers two kinds of string aliases that neither PHP nor a naive
//! static analyzer can follow, because both are wired up at runtime through the
//! service container:
//!
//! * **Container string aliases** — `resolve('blade.compiler')` /
//!   `app('cache')` bind a short string to a concrete class. The mapping lives
//!   in [`Application::registerCoreContainerAliases()`][core] as a literal
//!   `'alias' => [Concrete::class, Contract::class, …]` array.
//! * **Global facade class aliases** — a bare `\App`, `\DB`, `\Route`, … refers
//!   to a facade class that is `class_alias()`-ed into the global namespace.
//!   The base set is the literal collection in
//!   [`Facade::defaultAliases()`][def]; a project may add to (or override) it in
//!   `config/app.php`'s `aliases` value.
//!
//! Both tables are read by *parsing the framework the project actually has
//! installed* — never a version-specific list baked into PHPantom. If Laravel
//! changes either shape to something we do not recognize we simply bail and the
//! name stays unresolved (intended degradation, no guessing). Only the
//! *concrete* class of each container entry is used (string → class); the
//! contract-typed entries are deliberately ignored.
//!
//! [core]: https://github.com/laravel/framework/blob/master/src/Illuminate/Foundation/Application.php
//! [def]: https://github.com/laravel/framework/blob/master/src/Illuminate/Support/Facades/Facade.php

use std::collections::HashMap;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;
use tower_lsp::lsp_types::Url;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::types::ClassInfo;

/// FQN of the framework class that declares the core container aliases.
const APPLICATION_FQN: &str = "Illuminate\\Foundation\\Application";
/// FQN of the base facade whose `defaultAliases()` declares the global set.
const FACADE_FQN: &str = "Illuminate\\Support\\Facades\\Facade";

/// The two alias tables PHPantom resolves, both keyed by the string as written
/// in source (case-sensitive; Laravel's alias keys are declared exactly once).
#[derive(Debug, Default)]
pub(crate) struct LaravelAliases {
    /// Container string alias → concrete class FQN
    /// (e.g. `"blade.compiler"` → `"Illuminate\\View\\Compilers\\BladeCompiler"`).
    pub(crate) container: HashMap<String, String>,
    /// Global facade class alias → facade class FQN
    /// (e.g. `"App"` → `"Illuminate\\Support\\Facades\\App"`).
    pub(crate) facade: HashMap<String, String>,
}

impl LaravelAliases {
    /// Whether both tables are empty (a non-Laravel project, or a Laravel whose
    /// framework source we could not parse). Used to skip the alias fallback
    /// entirely on the hot class-resolution path.
    fn is_empty(&self) -> bool {
        self.container.is_empty() && self.facade.is_empty()
    }
}

impl Backend {
    /// Resolve a name through Laravel's alias tables, loading the target class.
    ///
    /// Consulted by [`find_or_load_class`](Backend::find_or_load_class) only as
    /// a *fallback*, after every ordinary resolution phase has missed — so a
    /// project class named `App` (or any collision) always wins over the
    /// facade, and a plain string like `blade.compiler` (never a valid class
    /// name) resolves to its bound concrete class.
    ///
    /// Container aliases are checked before facade aliases; the two key spaces
    /// do not overlap in practice (`blade.compiler` vs `Blade`).
    pub(crate) fn resolve_laravel_alias(&self, name: &str) -> Option<Arc<ClassInfo>> {
        let aliases = self.laravel_aliases();
        if aliases.is_empty() {
            return None;
        }
        let fqn = aliases
            .container
            .get(name)
            .or_else(|| aliases.facade.get(name))?;
        self.find_or_load_class(fqn)
    }

    /// Expand a file's macro registrations so a macro registered through a
    /// facade also attaches to the facade's concrete container-bound class.
    ///
    /// `View::macro('extends', …)` lands, at runtime, on the object the `View`
    /// facade proxies (the view factory) — not on the facade class itself. The
    /// scan records the written target (the facade FQN), which is correct for
    /// static facade calls (`View::extends()`). This appends a second
    /// registration targeting the concrete class so an instance call
    /// (`$factory->extends()`) resolves too. Both registrations share the same
    /// source location, so go-to-definition still lands on the `::macro(...)`
    /// call regardless of which subject the call was made on.
    ///
    /// A no-op for the common case (targets like `Str`, `Collection` are not
    /// facades): [`facade_macro_concrete`](Self::facade_macro_concrete) gates on
    /// the facade alias table before reading any source.
    pub(crate) fn expand_facade_macros(&self, regs: &mut Vec<super::macros::MacroRegistration>) {
        let mut extra = Vec::new();
        for reg in regs.iter() {
            if let Some(concrete) = self.facade_macro_concrete(&reg.target)
                && concrete != reg.target
            {
                let mut cloned = reg.clone();
                cloned.target = concrete;
                extra.push(cloned);
            }
        }
        regs.extend(extra);
    }

    /// The concrete container-bound class a Laravel facade proxies to, resolved
    /// statically without booting the application.
    ///
    /// Returns `None` unless `target` is a known facade (a value in the global
    /// facade alias table), so a non-facade macro target never pays a source
    /// read. For a genuine facade it parses `getFacadeAccessor()`: a string
    /// return (`'view'`) is looked up in the core container alias table, and a
    /// `::class` return resolves directly to that FQN. A string accessor that
    /// is not in the container table (a binding registered only at runtime by a
    /// service provider) yields `None`.
    fn facade_macro_concrete(&self, target: &str) -> Option<String> {
        let aliases = self.laravel_aliases();
        // Gate on the facade table so only real facades trigger a source read.
        if !aliases.facade.values().any(|fqn| fqn == target) {
            return None;
        }
        let source = read_source_by_fqn(self, target)?;
        match parse_facade_accessor(&source)? {
            FacadeAccessor::Alias(key) => aliases.container.get(&key).cloned(),
            FacadeAccessor::Class(fqn) => Some(fqn),
        }
    }

    /// The memoized Laravel alias tables, built on first use from the installed
    /// framework source and cached on the [`Backend`]. Rebuilt after a
    /// reindex (the cache is cleared alongside the other resolution caches).
    fn laravel_aliases(&self) -> Arc<LaravelAliases> {
        if let Some(cached) = self.laravel_aliases.read().clone() {
            return cached;
        }
        // Seed with an empty table to break re-entry: the build reads framework
        // source via `find_or_load_class`, whose miss path falls back to
        // `resolve_laravel_alias`, which would otherwise re-enter this builder.
        *self.laravel_aliases.write() = Some(Arc::new(LaravelAliases::default()));
        let built = Arc::new(build_laravel_aliases(self));
        *self.laravel_aliases.write() = Some(Arc::clone(&built));
        built
    }
}

/// Build both alias tables by parsing the framework source the project has
/// installed. Returns empty tables for a non-Laravel project (the framework
/// classes are absent from the class index).
fn build_laravel_aliases(backend: &Backend) -> LaravelAliases {
    let container = read_source_by_fqn(backend, APPLICATION_FQN)
        .and_then(|src| parse_container_aliases(&src))
        .unwrap_or_default();

    // Facade aliases: the framework defaults, then the project's config
    // overlay (which may add to or override individual entries).
    let mut facade = read_source_by_fqn(backend, FACADE_FQN)
        .and_then(|src| parse_facade_default_aliases(&src))
        .unwrap_or_default();
    if let Some(config) = read_project_config(backend, "app.php") {
        for (alias, fqn) in parse_config_facade_aliases(&config) {
            facade.insert(alias, fqn);
        }
    }

    LaravelAliases { container, facade }
}

// ─── Source access ────────────────────────────────────────────────────────

/// Read a framework class's source, preferring an open editor buffer over the
/// on-disk copy. Located via the class index (`fqn_uri_index`), which already
/// covers every vendor class from the Composer classmap.
fn read_source_by_fqn(backend: &Backend, fqn: &str) -> Option<String> {
    // Bind the lookup to a `let` so the `fqn_uri_index` read guard is
    // released before the `None` arm runs.  Holding it across
    // `find_or_load_class` would self-deadlock: parsing the class takes a
    // `write()` on the same index, which blocks forever waiting for this
    // thread's own outstanding reader (a temporary in a `match` scrutinee
    // lives to the end of the `match`).
    let indexed = backend.fqn_uri_index.read().get(fqn).cloned();
    let uri = match indexed {
        Some(uri) => uri,
        None => {
            // Not yet in the class index (e.g. a lazily-loaded PSR-4 project).
            // Parsing the class populates its FQN → URI entry.
            backend.find_or_load_class(fqn)?;
            backend.fqn_uri_index.read().get(fqn).cloned()?
        }
    };
    if let Some(content) = backend.get_file_content(&uri) {
        return Some(content);
    }
    let path = Url::parse(&uri).ok()?.to_file_path().ok()?;
    std::fs::read_to_string(&path).ok()
}

/// Read a `config/*.php` file, preferring an open editor buffer over disk.
/// Mirrors the auth-config reader so in-editor edits take effect immediately.
fn read_project_config(backend: &Backend, file_name: &str) -> Option<String> {
    let root = backend.workspace_root.read().clone()?;
    let path = root.join("config").join(file_name);
    if !path.is_file() {
        return None;
    }
    if let Ok(uri) = Url::from_file_path(&path)
        && let Some(content) = backend.get_file_content(uri.as_ref())
    {
        return Some(content);
    }
    std::fs::read_to_string(&path).ok()
}

// ─── Parsers ──────────────────────────────────────────────────────────────

/// Run the parser and mago-names resolver over `content`, then hand the parsed
/// program and resolved-name table to `f` (which must extract owned data before
/// the arena is dropped).
fn with_parsed<R>(content: &str, f: impl FnOnce(&Program<'_>, &OwnedResolvedNames) -> R) -> R {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);
    f(program, &owned)
}

/// Parse the core container aliases out of `registerCoreContainerAliases()`.
///
/// Shape: a `'alias' => [Concrete::class, Contract::class, …]` array. We take
/// the *concrete* class (first element) of each entry and map the string key to
/// it. Entries whose concrete class we cannot resolve (e.g. `self::class`) are
/// skipped. Returns `None` when no array of that shape is found.
fn parse_container_aliases(content: &str) -> Option<HashMap<String, String>> {
    with_parsed(content, |program, resolved| {
        let mut arrays = Vec::new();
        collect_arrays(Node::Program(program), &mut arrays);
        // The alias array is the one whose values are themselves arrays.
        let array = arrays
            .into_iter()
            .find(|arr| array_values_are_arrays(arr))?;

        let mut out = HashMap::new();
        for (key, value) in key_value_elements(array) {
            let Some(alias) = string_literal(key, content) else {
                continue;
            };
            // Concrete class = first element of the `[Concrete, Contract, …]`.
            let Some(concrete) = first_array_element(value) else {
                continue;
            };
            if let Some(fqn) = class_const_fqn(concrete, resolved) {
                out.insert(alias, fqn);
            }
        }
        (!out.is_empty()).then_some(out)
    })
}

/// Parse the global facade defaults out of `Facade::defaultAliases()`.
///
/// Shape: an `'Alias' => Facade::class` array (short class names resolved
/// against the file's namespace and use statements). Returns `None` when no
/// array of that shape is found.
fn parse_facade_default_aliases(content: &str) -> Option<HashMap<String, String>> {
    with_parsed(content, |program, resolved| {
        let mut arrays = Vec::new();
        collect_arrays(Node::Program(program), &mut arrays);
        // The defaults are the largest array whose values are all `::class`
        // constants keyed by strings.
        let array = arrays
            .into_iter()
            .filter(|arr| array_values_are_class_consts(arr))
            .max_by_key(|arr| arr.elements.len())?;

        let out = collect_string_class_pairs(array, content, resolved);
        (!out.is_empty()).then_some(out)
    })
}

/// Parse the `aliases` overlay from `config/app.php`.
///
/// Handles both the plain-array shape and the modern
/// `Facade::defaultAliases()->merge([…])` shape by collecting every
/// `'Alias' => Class::class` pair found anywhere inside the `aliases` value.
fn parse_config_facade_aliases(content: &str) -> HashMap<String, String> {
    with_parsed(content, |program, resolved| {
        let Some(aliases_value) = find_return_array_entry(program, "aliases") else {
            return HashMap::new();
        };
        let mut arrays = Vec::new();
        collect_arrays(node_of_expression(aliases_value), &mut arrays);
        let mut out = HashMap::new();
        for array in arrays {
            for (alias, fqn) in collect_string_class_pairs(array, content, resolved) {
                out.insert(alias, fqn);
            }
        }
        out
    })
}

/// What a facade's `getFacadeAccessor()` returns.
enum FacadeAccessor {
    /// A container-binding string (`return 'view';`), looked up in the core
    /// container alias table to find the concrete class.
    Alias(String),
    /// A direct class reference (`return Factory::class;`), already the FQN of
    /// the concrete class.
    Class(String),
}

/// Parse the return value of a facade's `getFacadeAccessor()` method.
///
/// Facades declare `protected static function getFacadeAccessor()` returning
/// either a container-binding string or a `::class` reference. Returns `None`
/// when the method is absent or returns anything else (e.g. a computed value).
fn parse_facade_accessor(content: &str) -> Option<FacadeAccessor> {
    with_parsed(content, |program, resolved| {
        let return_value = find_facade_accessor_return(Node::Program(program))?;
        if let Some((text, _, _)) = super::helpers::extract_string_literal(return_value, content) {
            return Some(FacadeAccessor::Alias(text.to_string()));
        }
        class_const_fqn(return_value, resolved).map(FacadeAccessor::Class)
    })
}

/// Find the first return-statement value inside a `getFacadeAccessor()` method
/// reachable from `node`.
fn find_facade_accessor_return<'ast, 'arena>(
    node: Node<'ast, 'arena>,
) -> Option<&'ast Expression<'arena>> {
    use mago_syntax::cst::class_like::method::MethodBody;

    if let Node::Method(method) = node
        && bytes_to_str(method.name.value).eq_ignore_ascii_case("getFacadeAccessor")
        && let MethodBody::Concrete(block) = &method.body
    {
        return block.statements.iter().find_map(|stmt| match stmt {
            Statement::Return(ret) => ret.value,
            _ => None,
        });
    }
    let mut found = None;
    node.visit_children(|child| {
        if found.is_none() {
            found = find_facade_accessor_return(child);
        }
    });
    found
}

// ─── AST helpers ────────────────────────────────────────────────────────────

/// Recursively collect every array literal (`[…]` and `array(…)`) reachable
/// from `node`.
fn collect_arrays<'ast, 'arena>(node: Node<'ast, 'arena>, out: &mut Vec<&'ast Array<'arena>>) {
    if let Node::Array(arr) = node {
        out.push(arr);
    }
    node.visit_children(|child| collect_arrays(child, out));
}

/// Iterate the key/value pairs of an array literal, skipping non-keyed and
/// spread elements.
fn key_value_elements<'ast, 'arena>(
    array: &'ast Array<'arena>,
) -> impl Iterator<Item = (&'ast Expression<'arena>, &'ast Expression<'arena>)> {
    array.elements.iter().filter_map(|element| match element {
        ArrayElement::KeyValue(kv) => Some((kv.key, kv.value)),
        _ => None,
    })
}

/// Whether every keyed element of `array` has an array-literal value
/// (the `'alias' => [Concrete, …]` container shape).
fn array_values_are_arrays(array: &Array<'_>) -> bool {
    let mut any = false;
    for (_, value) in key_value_elements(array) {
        any = true;
        if !matches!(value, Expression::Array(_) | Expression::LegacyArray(_)) {
            return false;
        }
    }
    any
}

/// Whether every keyed element of `array` has a `::class` value
/// (the `'Alias' => Facade::class` facade shape).
fn array_values_are_class_consts(array: &Array<'_>) -> bool {
    let mut any = false;
    for (_, value) in key_value_elements(array) {
        any = true;
        if !is_class_const(value) {
            return false;
        }
    }
    any
}

/// The first element expression of an array literal, if any.
fn first_array_element<'ast, 'arena>(
    expr: &'ast Expression<'arena>,
) -> Option<&'ast Expression<'arena>> {
    let elements = match expr {
        Expression::Array(arr) => &arr.elements,
        Expression::LegacyArray(arr) => &arr.elements,
        _ => return None,
    };
    elements.iter().find_map(|element| match element {
        ArrayElement::Value(v) => Some(v.value),
        _ => None,
    })
}

/// Collect the `'Alias' => Class::class` pairs of an array, resolving each
/// class name to its FQN via the file's resolved-name table.
fn collect_string_class_pairs(
    array: &Array<'_>,
    content: &str,
    resolved: &OwnedResolvedNames,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (key, value) in key_value_elements(array) {
        let Some(alias) = string_literal(key, content) else {
            continue;
        };
        if let Some(fqn) = class_const_fqn(value, resolved) {
            out.insert(alias, fqn);
        }
    }
    out
}

/// Whether `expr` is a `Something::class` constant access.
fn is_class_const(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::Access(Access::ClassConstant(cca))
            if matches!(
                &cca.constant,
                ClassLikeConstantSelector::Identifier(id)
                    if bytes_to_str(id.value).eq_ignore_ascii_case("class")
            )
    )
}

/// Extract the fully-qualified name from a `Something::class` expression,
/// resolving short names through the file's namespace and use statements.
/// Returns `None` for non-`::class` expressions and for `self::class` /
/// dynamic class expressions we cannot pin to a name.
fn class_const_fqn(expr: &Expression<'_>, resolved: &OwnedResolvedNames) -> Option<String> {
    let Expression::Access(Access::ClassConstant(cca)) = expr else {
        return None;
    };
    let ClassLikeConstantSelector::Identifier(constant) = &cca.constant else {
        return None;
    };
    if !bytes_to_str(constant.value).eq_ignore_ascii_case("class") {
        return None;
    }
    let Expression::Identifier(ident) = cca.class else {
        return None;
    };
    // `self::class` / `static::class` / `parent::class` are relative and carry
    // no useful concrete for an alias table; skip them.
    let raw = bytes_to_str(ident.value());
    if matches!(
        raw.to_ascii_lowercase().as_str(),
        "self" | "static" | "parent"
    ) {
        return None;
    }
    let offset = ident.span().start.offset;
    if let Some(fqn) = resolved.get(offset) {
        return Some(fqn.trim_start_matches('\\').to_string());
    }
    // Fall back to the written name when the resolver did not track it.
    (!raw.is_empty()).then(|| raw.trim_start_matches('\\').to_string())
}

/// The value expression for a top-level `return [ 'key' => … ]` entry.
fn find_return_array_entry<'ast, 'arena>(
    program: &'ast Program<'arena>,
    key: &str,
) -> Option<&'ast Expression<'arena>> {
    for stmt in program.statements.iter() {
        if let Statement::Return(ret) = stmt
            && let Some(value) = ret.value
        {
            let elements = match value {
                Expression::Array(arr) => &arr.elements,
                Expression::LegacyArray(arr) => &arr.elements,
                _ => continue,
            };
            for (k, v) in elements.iter().filter_map(|e| match e {
                ArrayElement::KeyValue(kv) => Some((kv.key, kv.value)),
                _ => None,
            }) {
                if string_literal_matches(k, key) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The [`Node`] for an expression, so it can be fed to [`collect_arrays`].
fn node_of_expression<'ast, 'arena>(expr: &'ast Expression<'arena>) -> Node<'ast, 'arena> {
    Node::Expression(expr)
}

/// Extract a string-literal's content (without quotes) from an expression.
fn string_literal(expr: &Expression<'_>, content: &str) -> Option<String> {
    super::helpers::extract_string_literal(expr, content).map(|(text, _, _)| text.to_string())
}

/// Whether `expr` is a string literal equal to `expected`.
fn string_literal_matches(expr: &Expression<'_>, expected: &str) -> bool {
    // Only the raw source text is available without content here, so match via
    // the literal's own value.
    matches!(
        expr,
        Expression::Literal(literal::Literal::String(s))
            if s.value.is_some_and(|v| bytes_to_str(v) == expected)
    )
}

#[cfg(test)]
#[path = "aliases_tests.rs"]
mod tests;
