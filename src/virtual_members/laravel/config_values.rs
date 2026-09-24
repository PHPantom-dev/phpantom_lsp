//! Static reader for Laravel `config/*.php` **values**.
//!
//! The sibling [`config_keys`](super::config_keys) module walks a config
//! file to record *key* spans (powering go-to-definition and references on
//! `config('a.b.c')` strings).  This module answers a different question:
//! *what value sits at a given dotted config path?*  It parses the `return
//! [...]` array literal into an owned [`ConfigNode`] tree, then lets callers
//! navigate by path and classify leaf expressions.
//!
//! Values are deliberately kept as a small, honest set of shapes rather than
//! being evaluated.  A config value can be a string literal, a `::class`
//! constant, a ternary over several literals, an `env('KEY', <default>)` read
//! whose default is known but which a runtime environment variable may
//! override, or something we cannot resolve statically at all.  Consumers
//! decide how much uncertainty they can tolerate (see the auth-user model
//! resolver, which anchors on `env()` defaults but records that the value
//! could be overridden so it can widen the result to the framework contract).
//!
//! This is the first consumer of static config-value reading; it is written
//! to be reusable (e.g. `Storage::disk()` reading `config/filesystems.php`).

use std::collections::HashMap;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;

use super::config_keys::ConfigSourceKind;
use crate::Backend;
use crate::atom::{atom, bytes_to_str};
use crate::php_type::{PhpType, ShapeEntry};

/// A statically-classified Laravel config value.
///
/// Values are never evaluated.  `env()` conditions, variables, and arbitrary
/// function calls are opaque at analysis time, so anything we cannot pin to a
/// literal collapses to [`ConfigValue::Dynamic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigValue {
    /// A string literal, e.g. `'web'` or `'users'`.
    Str(String),
    /// An integer literal, e.g. `3306`.
    Int,
    /// A float literal, e.g. `0.5`.
    Float,
    /// A boolean literal (`true` or `false`).
    Bool,
    /// A `null` literal.
    Null,
    /// A `::class` constant, e.g. `App\Models\User::class`.  The stored name
    /// is resolved against the config file's `use` statements at parse time,
    /// so an imported short name (`use App\Models\User; User::class`) is kept
    /// fully-qualified; callers still normalise it against the classmap.
    ClassString(String),
    /// A ternary or short-ternary over two or more sub-values, e.g.
    /// `env('is_admin') ? User::class : Admin::class`.  We do not evaluate the
    /// condition; the honest value is *one of* the arms.
    OneOf(Vec<ConfigValue>),
    /// `env('KEY', <default>)`: the default argument is statically known, but
    /// an environment variable may override it at runtime.
    EnvDefault(Box<ConfigValue>),
    /// Not statically resolvable (bare `env('KEY')`, a variable, a call, or a
    /// shape we do not recognize).
    Dynamic,
}

impl ConfigValue {
    /// Flatten to the set of string literals this value may take, plus a flag
    /// indicating whether a runtime-dynamic branch was encountered (an
    /// `env()` override, a bare `env()`, or an unrecognized expression).
    ///
    /// Used for scalar config such as guard and provider names.
    pub(crate) fn as_strings(&self) -> (Vec<String>, bool) {
        let mut out = Vec::new();
        let mut dynamic = false;
        self.collect_strings(&mut out, &mut dynamic);
        (out, dynamic)
    }

    /// Flatten to the set of class names this value may take, plus a flag
    /// indicating whether a runtime-dynamic branch was encountered.
    ///
    /// Used for the `providers.*.model` config.  A string literal that looks
    /// like a class reference (contains a namespace separator) is accepted as
    /// a class too, since a handful of configs write the model as a string.
    pub(crate) fn as_classes(&self) -> (Vec<String>, bool) {
        let mut out = Vec::new();
        let mut dynamic = false;
        self.collect_classes(&mut out, &mut dynamic);
        (out, dynamic)
    }

    fn collect_strings(&self, out: &mut Vec<String>, dynamic: &mut bool) {
        match self {
            ConfigValue::Str(s) => push_unique(out, s),
            ConfigValue::ClassString(_)
            | ConfigValue::Int
            | ConfigValue::Float
            | ConfigValue::Bool
            | ConfigValue::Null => *dynamic = true,
            ConfigValue::OneOf(arms) => {
                for arm in arms {
                    arm.collect_strings(out, dynamic);
                }
            }
            ConfigValue::EnvDefault(inner) => {
                inner.collect_strings(out, dynamic);
                *dynamic = true;
            }
            ConfigValue::Dynamic => *dynamic = true,
        }
    }

    fn collect_classes(&self, out: &mut Vec<String>, dynamic: &mut bool) {
        match self {
            ConfigValue::ClassString(name) => push_unique(out, name),
            ConfigValue::Str(s) if s.contains('\\') => push_unique(out, s),
            ConfigValue::Str(_)
            | ConfigValue::Int
            | ConfigValue::Float
            | ConfigValue::Bool
            | ConfigValue::Null => *dynamic = true,
            ConfigValue::OneOf(arms) => {
                for arm in arms {
                    arm.collect_classes(out, dynamic);
                }
            }
            ConfigValue::EnvDefault(inner) => {
                inner.collect_classes(out, dynamic);
                *dynamic = true;
            }
            ConfigValue::Dynamic => *dynamic = true,
        }
    }
}

impl ConfigValue {
    pub(crate) fn to_php_type(&self) -> PhpType {
        match self {
            ConfigValue::Str(_) => PhpType::string(),
            ConfigValue::Int => PhpType::int(),
            ConfigValue::Float => PhpType::float(),
            ConfigValue::Bool => PhpType::bool(),
            ConfigValue::Null => PhpType::null(),
            ConfigValue::ClassString(name) => {
                PhpType::class_string(Some(PhpType::named(atom(name))))
            }
            ConfigValue::OneOf(arms) => {
                let mut members: Vec<PhpType> = Vec::new();
                for arm in arms {
                    let ty = arm.to_php_type();
                    if !members.iter().any(|m| m == &ty) {
                        members.push(ty);
                    }
                }
                if members.is_empty() {
                    PhpType::mixed()
                } else {
                    PhpType::union(members)
                }
            }
            ConfigValue::EnvDefault(inner) => inner.to_php_type(),
            ConfigValue::Dynamic => PhpType::mixed(),
        }
    }
}

impl ConfigNode {
    pub(crate) fn to_php_type(&self) -> PhpType {
        match self {
            ConfigNode::Leaf(value) => value.to_php_type(),
            ConfigNode::Array(entries) => {
                let shape_entries: Vec<ShapeEntry> = entries
                    .iter()
                    .map(|(key, node)| ShapeEntry {
                        key: Some(key.clone()),
                        value_type: node.to_php_type(),
                        optional: false,
                    })
                    .collect();
                PhpType::array_shape(shape_entries)
            }
            ConfigNode::List(items) => {
                let mut members: Vec<PhpType> = Vec::new();
                for item in items {
                    let ty = item.to_php_type();
                    if !members.contains(&ty) {
                        members.push(ty);
                    }
                }
                let element = if members.len() == 1 {
                    members.pop().unwrap_or_else(PhpType::mixed)
                } else {
                    PhpType::union(members)
                };
                PhpType::list(element)
            }
        }
    }
}

fn push_unique(out: &mut Vec<String>, value: &str) {
    if !out.iter().any(|existing| existing == value) {
        out.push(value.to_string());
    }
}

/// An owned tree of a parsed `config/*.php` array literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigNode {
    /// A nested array, ordered by declaration so callers can enumerate keys
    /// for fan-out (e.g. every configured guard).
    Array(Vec<(String, ConfigNode)>),
    /// An array whose entries have no string keys, e.g. a list of handler
    /// classes.  Its positions are not config keys.
    List(Vec<ConfigNode>),
    /// A leaf value.
    Leaf(ConfigValue),
}

impl ConfigNode {
    /// Navigate to a nested node by dotted path segments.
    pub(crate) fn get(&self, path: &[&str]) -> Option<&ConfigNode> {
        let mut node = self;
        for segment in path {
            let ConfigNode::Array(entries) = node else {
                return None;
            };
            node = entries
                .iter()
                .find(|(key, _)| key == segment)
                .map(|(_, child)| child)?;
        }
        Some(node)
    }

    /// The immediate child keys of an array node (empty for leaves).  Used to
    /// fan out over every configured guard or provider when an intermediate
    /// hop cannot be resolved to a single choice.
    pub(crate) fn child_keys(&self) -> Vec<String> {
        match self {
            ConfigNode::Array(entries) => entries.iter().map(|(key, _)| key.clone()).collect(),
            ConfigNode::List(_) | ConfigNode::Leaf(_) => Vec::new(),
        }
    }

    /// The [`ConfigValue`] at a path, if it resolves to a leaf.
    pub(crate) fn value_at(&self, path: &[&str]) -> Option<&ConfigValue> {
        match self.get(path)? {
            ConfigNode::Leaf(value) => Some(value),
            ConfigNode::Array(_) | ConfigNode::List(_) => None,
        }
    }

    /// Every key beneath this node, spelled as a dotted path under `prefix`:
    /// groups and leaves alike, but not the positions of a list.
    pub(crate) fn collect_keys(&self, prefix: &str, out: &mut Vec<String>) {
        let ConfigNode::Array(entries) = self else {
            return;
        };
        for (key, child) in entries {
            let dotted = format!("{prefix}.{key}");
            child.collect_keys(&dotted, out);
            out.push(dotted);
        }
    }

    /// Merge a lower-precedence config beneath this one the way
    /// `array_merge($lower, $this)` does: a top-level key this config
    /// declares keeps its whole value, however much of it `lower` spells
    /// out, and only the keys it leaves out are taken from `lower`.  The
    /// keys named in `deep` are merged one level further, which is what
    /// `LoadConfiguration` does for the framework's mergeable options.
    fn merge_beneath(&mut self, lower: ConfigNode, deep: &[&str]) {
        let ConfigNode::Array(target) = self else {
            return;
        };
        let ConfigNode::Array(source) = lower else {
            return;
        };
        for (key, lower_child) in source {
            match target.iter_mut().find(|(k, _)| *k == key) {
                Some((_, existing)) => {
                    if deep.contains(&key.as_str()) {
                        existing.merge_beneath(lower_child, &[]);
                    }
                }
                None => target.push((key, lower_child)),
            }
        }
    }
}

/// Parse a `config/*.php` file's returned array into an owned [`ConfigNode`].
///
/// Handles both `return [...]` and the `$config = [...]; return $config;`
/// pattern, mirroring [`config_keys`](super::config_keys)'s declaration
/// walker.
pub(crate) fn parse_config_tree(content: &str) -> Option<ConfigNode> {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());

    // A file that builds its array up over several assignments is read
    // from the first: the tree is a snapshot of the value, not a merge of
    // every write to it.
    let expr = super::array_file::returned_exprs(program)
        .into_iter()
        .next()?;

    // Resolve `::class` references against the config file's own `use`
    // statements, so `use App\Models\User; ... User::class` yields the
    // fully-qualified `App\Models\User` rather than the bare short name.
    let mut use_map: HashMap<String, String> = HashMap::new();
    Backend::extract_use_statements_from_statements(program.statements.iter(), &mut use_map);

    Some(node_from_expr(expr, content, &use_map))
}

/// Resolve a class name written in a config file against its `use` statements.
///
/// Config files live in the global namespace, so a name is either already
/// fully-qualified (a leading `\`, or a multi-segment name with no matching
/// import) or an imported short name / aliased prefix. PHP resolves the first
/// segment against the use-map and appends any trailing segments, exactly as
/// `use App\Models\User; User::class` yields `App\Models\User`.
fn resolve_config_class_name(name: &str, use_map: &HashMap<String, String>) -> String {
    // A leading `\` marks an explicit fully-qualified name; strip it and use
    // it verbatim without consulting imports.
    if let Some(rest) = name.strip_prefix('\\') {
        return rest.to_string();
    }
    let (first, rest) = match name.split_once('\\') {
        Some((first, rest)) => (first, Some(rest)),
        None => (name, None),
    };
    match use_map.get(first) {
        Some(fqn) => match rest {
            Some(rest) => format!("{fqn}\\{rest}"),
            None => fqn.clone(),
        },
        // No import matches the first segment: the name is already relative to
        // the global namespace, so it is its own FQN.
        None => name.to_string(),
    }
}

/// Build a [`ConfigNode`] from an arbitrary expression: arrays become
/// [`ConfigNode::Array`], everything else is classified as a leaf value.
fn node_from_expr(
    expr: &Expression<'_>,
    content: &str,
    use_map: &HashMap<String, String>,
) -> ConfigNode {
    match expr {
        Expression::Parenthesized(p) => node_from_expr(p.expression, content, use_map),
        Expression::Array(arr) => array_node(arr.elements.iter(), content, use_map),
        Expression::LegacyArray(arr) => array_node(arr.elements.iter(), content, use_map),
        Expression::Call(Call::Function(fc)) if matches!(fc.function, Expression::Identifier(ident) if ident.value().eq_ignore_ascii_case(b"array_merge")) => {
            array_merge_node(fc, content, use_map)
        }
        other => ConfigNode::Leaf(classify_value(other, content, use_map)),
    }
}

/// `array_merge()` over config arrays: a later string key replaces an
/// earlier one, and positional entries are appended.  An argument that is
/// not an array literal contributes keys we cannot see, so only the ones
/// spelled out are kept.
fn array_merge_node(
    fc: &FunctionCall<'_>,
    content: &str,
    use_map: &HashMap<String, String>,
) -> ConfigNode {
    let mut entries: Vec<(String, ConfigNode)> = Vec::new();
    let mut items = Vec::new();
    for arg in fc.argument_list.arguments.iter() {
        match node_from_expr(arg.value(), content, use_map) {
            ConfigNode::Array(arg_entries) => {
                for (key, node) in arg_entries {
                    set_entry(&mut entries, key, node);
                }
            }
            ConfigNode::List(arg_items) => items.extend(arg_items),
            ConfigNode::Leaf(_) => {}
        }
    }
    array_or_list(entries, items)
}

/// Store `node` under `key`, replacing an earlier entry of the same key in
/// place the way PHP does for a duplicate array key.
fn set_entry(entries: &mut Vec<(String, ConfigNode)>, key: String, node: ConfigNode) {
    match entries.iter_mut().find(|(k, _)| *k == key) {
        Some((_, existing)) => *existing = node,
        None => entries.push((key, node)),
    }
}

/// An array with only positional entries is a list; one with any string
/// key is read by its keys.
fn array_or_list(entries: Vec<(String, ConfigNode)>, items: Vec<ConfigNode>) -> ConfigNode {
    if entries.is_empty() && !items.is_empty() {
        ConfigNode::List(items)
    } else {
        ConfigNode::Array(entries)
    }
}

fn array_node<'a>(
    elements: impl Iterator<Item = &'a ArrayElement<'a>>,
    content: &str,
    use_map: &HashMap<String, String>,
) -> ConfigNode {
    let mut entries = Vec::new();
    let mut items = Vec::new();
    for element in elements {
        let kv = match element {
            ArrayElement::KeyValue(kv) => kv,
            ArrayElement::Value(value) => {
                items.push(node_from_expr(value.value, content, use_map));
                continue;
            }
            _ => continue,
        };
        // The literal's unescaped value, as PHP reads it: `'it\'s'` is the
        // key `it's`.
        let key_text = match kv.key {
            Expression::Literal(literal::Literal::String(key)) => {
                match key.value.and_then(|v| std::str::from_utf8(v).ok()) {
                    Some(text) => text,
                    None => continue,
                }
            }
            // `0 => …` is a position like any other.
            Expression::Literal(literal::Literal::Integer(_)) => {
                items.push(node_from_expr(kv.value, content, use_map));
                continue;
            }
            _ => continue,
        };
        set_entry(
            &mut entries,
            key_text.to_string(),
            node_from_expr(kv.value, content, use_map),
        );
    }
    array_or_list(entries, items)
}

/// Classify a leaf value expression into a [`ConfigValue`].
fn classify_value(
    expr: &Expression<'_>,
    content: &str,
    use_map: &HashMap<String, String>,
) -> ConfigValue {
    match expr {
        Expression::Parenthesized(p) => classify_value(p.expression, content, use_map),
        Expression::Literal(literal::Literal::String(s)) => {
            match s.value.and_then(|v| std::str::from_utf8(v).ok()) {
                Some(text) => ConfigValue::Str(text.to_string()),
                None => ConfigValue::Dynamic,
            }
        }
        Expression::Literal(literal::Literal::Integer(_)) => ConfigValue::Int,
        Expression::Literal(literal::Literal::Float(_)) => ConfigValue::Float,
        Expression::Literal(literal::Literal::True(_) | literal::Literal::False(_)) => {
            ConfigValue::Bool
        }
        Expression::Literal(literal::Literal::Null(_)) => ConfigValue::Null,
        Expression::Access(Access::ClassConstant(cca)) => classify_class_constant(cca, use_map),
        Expression::Conditional(cond) => {
            // `a ? b : c` and short `a ?: c`.  We never evaluate the
            // condition; the value is one of the branches.  For `?:` the
            // "then" branch is the condition itself.
            let then_expr = cond.then.unwrap_or(cond.condition);
            let mut arms = Vec::new();
            flatten_one_of(classify_value(then_expr, content, use_map), &mut arms);
            flatten_one_of(classify_value(cond.r#else, content, use_map), &mut arms);
            ConfigValue::OneOf(arms)
        }
        Expression::Call(Call::Function(fc)) => classify_call(fc, content, use_map),
        _ => ConfigValue::Dynamic,
    }
}

fn classify_class_constant(
    cca: &ClassConstantAccess<'_>,
    use_map: &HashMap<String, String>,
) -> ConfigValue {
    let is_class = matches!(
        &cca.constant,
        ClassLikeConstantSelector::Identifier(ident)
            if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
    );
    if is_class && let Expression::Identifier(id) = cca.class {
        let name = bytes_to_str(id.value());
        if !name.is_empty() {
            return ConfigValue::ClassString(resolve_config_class_name(name, use_map));
        }
    }
    ConfigValue::Dynamic
}

fn classify_call(
    fc: &FunctionCall<'_>,
    content: &str,
    use_map: &HashMap<String, String>,
) -> ConfigValue {
    let Expression::Identifier(ident) = fc.function else {
        return ConfigValue::Dynamic;
    };
    if !ident.value().eq_ignore_ascii_case(b"env") {
        return ConfigValue::Dynamic;
    }
    // `env('KEY', <default>)` anchors on the default argument; a bare
    // `env('KEY')` has no static value.
    let default_arg = fc.argument_list.arguments.iter().nth(1);
    match default_arg {
        Some(arg) => {
            ConfigValue::EnvDefault(Box::new(classify_value(arg.value(), content, use_map)))
        }
        None => ConfigValue::Dynamic,
    }
}

/// The options of a framework config file that `LoadConfiguration` merges
/// entry by entry rather than letting the application's value replace
/// them whole, as listed in its `mergeableOptions()`.
fn framework_mergeable_options(file: &str) -> &'static [&'static str] {
    match file {
        "auth" => &["guards", "providers", "passwords"],
        "broadcasting" => &["connections"],
        "cache" => &["stores"],
        "database" => &["connections"],
        "filesystems" => &["disks"],
        "logging" => &["channels"],
        "mail" => &["mailers"],
        "queue" => &["connections"],
        _ => &[],
    }
}

/// Append a classified value into a `OneOf` accumulator, flattening nested
/// `OneOf`s so ternary chains stay a single flat set of arms.
fn flatten_one_of(value: ConfigValue, out: &mut Vec<ConfigValue>) {
    match value {
        ConfigValue::OneOf(arms) => {
            for arm in arms {
                flatten_one_of(arm, out);
            }
        }
        other => out.push(other),
    }
}

impl Backend {
    pub(crate) fn resolve_config_type(&self, dotted_key: &str) -> Option<PhpType> {
        let parts: Vec<&str> = dotted_key.split('.').collect();
        if parts.is_empty() {
            return None;
        }
        let trees = self.cached_config_trees();
        for (prefix, tree) in trees.iter() {
            let prefix_parts: Vec<&str> = prefix.split('.').collect();
            if parts.len() < prefix_parts.len() {
                continue;
            }
            if parts[..prefix_parts.len()] != prefix_parts[..] {
                continue;
            }
            let remaining = &parts[prefix_parts.len()..];
            if remaining.is_empty() {
                return Some(tree.to_php_type());
            }
            let node = tree.get(remaining)?;
            return Some(node.to_php_type());
        }
        None
    }

    pub(crate) fn cached_config_trees(&self) -> std::sync::Arc<Vec<(String, ConfigNode)>> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.config_trees,
            |cache| cache.config_trees.clone(),
            |cache, trees| cache.config_trees = Some(trees),
            || std::sync::Arc::new(self.enumerate_config_trees()),
        )
    }

    fn enumerate_config_trees(&self) -> Vec<(String, ConfigNode)> {
        // Lower-precedence sources only fill the top-level keys the
        // higher-precedence tree for the same prefix leaves unset, so a
        // project `config/app.php` that publishes just a handful of keys
        // still inherits the framework defaults for everything it does not
        // override, while a group it does publish is its own whole value.
        let mut trees: Vec<(String, ConfigNode)> = Vec::new();
        self.for_each_config_source(|prefix, kind, content| {
            let Some(tree) = parse_config_tree(content) else {
                return;
            };
            match trees.iter_mut().find(|(p, _)| p == prefix) {
                Some((_, existing)) => {
                    let deep = match kind {
                        ConfigSourceKind::Framework => framework_mergeable_options(prefix),
                        ConfigSourceKind::Project | ConfigSourceKind::Package => &[],
                    };
                    existing.merge_beneath(tree, deep);
                }
                None => trees.push((prefix.to_string(), tree)),
            }
        });
        trees
    }
}

#[cfg(test)]
#[path = "config_values_tests.rs"]
mod tests;
