//! Bundle configuration keys: the ones a project's own `TreeBuilder`
//! schemas declare, and the ones its `config/packages/` YAML sets.
//!
//! A bundle's `Configuration::getConfigTreeBuilder()` builds its schema as a
//! chain of node builders, `->children()->scalarNode('dsn')->end()`, so the
//! declared keys are read from the AST of that method by following the
//! chain the way the builders nest (see [`tree_builder_keys`]).  Each key is
//! a [`SymfonyConfigKey`](StringKeyKind::SymfonyConfigKey) span holding its
//! dotted path from the bundle's root, `acme_demo.mailer.dsn`.
//!
//! The YAML is read as text, line by line, by indentation (see [`KeyPath`]).
//! Every key under `config/packages/` becomes a span of the same kind, and
//! completion asks the same walk which key the line being typed is nested
//! in, so the two cannot disagree.
//!
//! The keys under a prototyped node (a map of named entries) are not
//! declared, since their path holds a name the schema leaves to the
//! configuration.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};

const TREE_BUILDER: &str = "Symfony\\Component\\Config\\Definition\\Builder\\TreeBuilder";

/// The top-level keys of a configuration file that configure the container
/// itself rather than a bundle.
const CONTAINER_ROOTS: [&str; 3] = ["imports", "parameters", "services"];

// ─── TreeBuilder schemas ────────────────────────────────────────────────────

/// One node builder a chain is on.
enum Frame {
    /// A node definition, `arrayNode('mailer')`; only an array node has
    /// children.  `declares` is unset under a prototype.
    Node {
        path: String,
        array: bool,
        declares: bool,
    },
    /// The children builder of an array node, `->children()`.
    Children { path: String, declares: bool },
    /// A normalization or validation rule, `->validate()`, which its own
    /// `end()` closes.
    Rule,
}

/// Each key `class`'s `getConfigTreeBuilder()` declares, as its dotted path
/// and the content range of the string that names it.  `resolve` gives the
/// class a name written at an offset names.
pub(crate) fn tree_builder_keys(
    class: &Class<'_>,
    resolve: &dyn Fn(&str, u32) -> String,
) -> Vec<(String, u32, u32)> {
    let body = class.members.iter().find_map(|member| match member {
        ClassLikeMember::Method(method)
            if method
                .name
                .value
                .eq_ignore_ascii_case(b"getConfigTreeBuilder") =>
        {
            match &method.body {
                MethodBody::Concrete(body) => Some(body),
                MethodBody::Abstract(_) => None,
            }
        }
        _ => None,
    });
    let Some(body) = body else {
        return Vec::new();
    };

    let mut walk = SchemaWalk {
        resolve,
        tree_builders: Vec::new(),
        root_nodes: Vec::new(),
        keys: Vec::new(),
    };
    for statement in body.statements.iter() {
        let Statement::Expression(statement) = statement else {
            continue;
        };
        match statement.expression {
            Expression::Assignment(assignment) => {
                let Expression::Variable(Variable::Direct(variable)) = assignment.lhs else {
                    continue;
                };
                if let Some(root) = walk.new_tree_builder(assignment.rhs) {
                    walk.tree_builders.push((variable.name, root));
                } else if let Some(root) = walk.chain(assignment.rhs) {
                    walk.root_nodes.push((variable.name, root));
                }
            }
            expression => {
                walk.chain(expression);
            }
        }
    }
    walk.keys
}

struct SchemaWalk<'r, 'a> {
    resolve: &'r dyn Fn(&str, u32) -> String,
    /// The variables holding a tree builder, with its root key.
    tree_builders: Vec<(&'a [u8], String)>,
    /// The variables holding a tree builder's root node, with its key.
    root_nodes: Vec<(&'a [u8], String)>,
    keys: Vec<(String, u32, u32)>,
}

impl<'a> SchemaWalk<'_, 'a> {
    /// The root key of a `new TreeBuilder('acme')`, declared on the way.
    fn new_tree_builder(&mut self, expr: &Expression<'a>) -> Option<String> {
        let expr = match expr {
            Expression::Parenthesized(inner) => inner.expression,
            expr => expr,
        };
        let Expression::Instantiation(instantiation) = expr else {
            return None;
        };
        let Expression::Identifier(class) = instantiation.class else {
            return None;
        };
        let class_name = (self.resolve)(bytes_to_str(class.value()), class.span().start.offset);
        if !class_name
            .trim_start_matches('\\')
            .eq_ignore_ascii_case(TREE_BUILDER)
        {
            return None;
        }
        let argument = instantiation.argument_list.as_ref()?.arguments.first()?;
        let (name, start, end) = config_key_literal(argument.value())?;
        self.keys.push((name.clone(), start, end));
        Some(name)
    }

    /// Follow a chain of builder calls from a root node, declaring the keys
    /// it adds.  Returns the root key when the chain is nothing but the
    /// root node itself, `$treeBuilder->getRootNode()`, so the variable it
    /// is assigned to can be followed too.
    fn chain(&mut self, expr: &Expression<'a>) -> Option<String> {
        let mut calls = Vec::new();
        let root = chain_root(expr, &mut calls)?;
        let mut calls = calls.into_iter();
        let root_key = match root {
            Expression::Variable(Variable::Direct(variable)) => {
                if let Some((_, key)) = self
                    .root_nodes
                    .iter()
                    .find(|(name, _)| *name == variable.name)
                {
                    key.clone()
                } else {
                    let (_, key) = self
                        .tree_builders
                        .iter()
                        .find(|(name, _)| *name == variable.name)?;
                    let key = key.clone();
                    let (method, _) = calls.next()?;
                    if !method.eq_ignore_ascii_case(b"getRootNode") {
                        return None;
                    }
                    key
                }
            }
            root => {
                let key = self.new_tree_builder(root)?;
                let (method, _) = calls.next()?;
                if !method.eq_ignore_ascii_case(b"getRootNode") {
                    return None;
                }
                key
            }
        };

        let mut stack = vec![Frame::Node {
            path: root_key.clone(),
            array: true,
            declares: true,
        }];
        let mut any_call = false;
        for (method, arguments) in calls {
            any_call = true;
            if !self.step(&mut stack, method, arguments) {
                break;
            }
        }
        (!any_call).then_some(root_key)
    }

    /// Apply one call to the builder the chain is on.  Returns whether the
    /// chain can still be followed.
    fn step(
        &mut self,
        stack: &mut Vec<Frame>,
        method: &[u8],
        arguments: &ArgumentList<'a>,
    ) -> bool {
        let is = |name: &str| method.eq_ignore_ascii_case(name.as_bytes());
        if is("end") {
            stack.pop();
            return !stack.is_empty();
        }
        let Some(frame) = stack.last() else {
            return false;
        };
        match frame {
            Frame::Rule => true,
            Frame::Node {
                path,
                array,
                declares,
            } => {
                let (path, array, declares) = (path.clone(), *array, *declares);
                if is("children") {
                    if !array {
                        return false;
                    }
                    stack.push(Frame::Children { path, declares });
                } else if is("validate") || is("beforeNormalization") {
                    stack.push(Frame::Rule);
                } else if let Some(prototype_is_array) = prototype_kind(method, arguments) {
                    stack.push(Frame::Node {
                        path: format!("{path}.*"),
                        array: prototype_is_array,
                        declares: false,
                    });
                }
                true
            }
            Frame::Children { path, declares } => {
                let Some(array) = child_node_kind(method, arguments) else {
                    // `append()` adds a node built elsewhere and returns the
                    // children builder; any other call is a node type this
                    // walk does not know the nesting of.
                    return is("append");
                };
                let (path, declares) = (path.clone(), *declares);
                let named = arguments
                    .arguments
                    .first()
                    .and_then(|argument| config_key_literal(argument.value()));
                let is_named = named.is_some();
                let path = match named {
                    Some((name, start, end)) => {
                        let path = format!("{path}.{name}");
                        if declares {
                            self.keys.push((path.clone(), start, end));
                        }
                        path
                    }
                    None => format!("{path}.?"),
                };
                stack.push(Frame::Node {
                    path,
                    array,
                    declares: declares && is_named,
                });
                true
            }
        }
    }
}

/// Whether a children builder's `method` adds a node, and whether that node
/// is an array node.
fn child_node_kind(method: &[u8], arguments: &ArgumentList<'_>) -> Option<bool> {
    let is = |name: &str| method.eq_ignore_ascii_case(name.as_bytes());
    if is("arrayNode") {
        return Some(true);
    }
    if is("node") {
        let node_type = arguments.arguments.iter().nth(1).map(Argument::value);
        return Some(node_type.is_some_and(|value| is_string(value, "array")));
    }
    let scalar = [
        "scalarNode",
        "booleanNode",
        "integerNode",
        "floatNode",
        "enumNode",
        "variableNode",
        "stringNode",
    ];
    scalar.iter().any(|name| is(name)).then_some(false)
}

/// Whether a node's `method` starts its prototype, and whether the
/// prototype is an array node.
fn prototype_kind(method: &[u8], arguments: &ArgumentList<'_>) -> Option<bool> {
    let is = |name: &str| method.eq_ignore_ascii_case(name.as_bytes());
    if is("arrayPrototype") {
        return Some(true);
    }
    if is("prototype") {
        let prototype_type = arguments.arguments.first().map(Argument::value);
        return Some(prototype_type.is_some_and(|value| is_string(value, "array")));
    }
    let scalar = [
        "scalarPrototype",
        "booleanPrototype",
        "integerPrototype",
        "floatPrototype",
        "enumPrototype",
        "variablePrototype",
        "stringPrototype",
    ];
    scalar.iter().any(|name| is(name)).then_some(false)
}

/// The key a string literal names, with its content range, when it can be
/// a configuration key.
fn config_key_literal(expr: &Expression<'_>) -> Option<(String, u32, u32)> {
    let Expression::Literal(Literal::String(literal)) = expr else {
        return None;
    };
    let value = literal.value.and_then(crate::atom::literal_bytes_to_str)?;
    is_key_segment(value).then(|| {
        (
            value.to_string(),
            literal.span.start.offset + 1,
            literal.span.end.offset - 1,
        )
    })
}

fn is_string(expr: &Expression<'_>, value: &str) -> bool {
    matches!(
        expr,
        Expression::Literal(Literal::String(literal))
            if literal.value.and_then(crate::atom::literal_bytes_to_str) == Some(value)
    )
}

/// The receiver a chain of method calls starts from, with the calls in the
/// order they run.
fn chain_root<'e, 'a>(
    expr: &'e Expression<'a>,
    calls: &mut Vec<(&'a [u8], &'e ArgumentList<'a>)>,
) -> Option<&'e Expression<'a>> {
    match expr {
        Expression::Call(Call::Method(call)) => {
            let root = chain_root(call.object, calls)?;
            let ClassLikeMemberSelector::Identifier(name) = &call.method else {
                return None;
            };
            calls.push((name.value, &call.argument_list));
            Some(root)
        }
        _ => Some(expr),
    }
}

/// Whether `name` can be one segment of a configuration key.
fn is_key_segment(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

// ─── YAML configuration ─────────────────────────────────────────────────────

/// Whether `uri` is a YAML file Symfony loads bundle configuration from,
/// under `config/packages/`, an environment's subdirectory included.
pub(crate) fn is_bundle_config_uri(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let lower = path.to_ascii_lowercase();
    let lower = lower.strip_suffix(".dist").unwrap_or(&lower);
    (lower.ends_with(".yaml") || lower.ends_with(".yml")) && lower.contains("/config/packages/")
}

/// One key of the mapping a YAML line is nested in.
struct Entry {
    indent: usize,
    key: String,
    /// Whether the key can be a segment of a configuration key path.
    valid: bool,
}

/// The keys a YAML document's lines are nested in, as a walk down the
/// document reads them.
#[derive(Default)]
struct KeyPath {
    entries: Vec<Entry>,
    /// Lines indented deeper than this belong to the value of the key
    /// before them: a block scalar, or a plain scalar running on.
    value_indent: Option<usize>,
}

impl KeyPath {
    /// Read one line, and return the content range of the key it sets,
    /// which [`Self::path`] then names.
    fn feed(&mut self, line: &str, line_start: usize) -> Option<(usize, usize)> {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() {
            return None;
        }
        let indent = crate::framework::leading_spaces(semantic);
        if let Some(value_indent) = self.value_indent {
            if indent > value_indent {
                return None;
            }
            self.value_indent = None;
        }
        if trimmed.starts_with('-') || trimmed.starts_with('%') || trimmed == "---" {
            // A sequence entry's keys are a list item's, not configuration
            // keys, so the whole entry is skipped.
            self.value_indent = Some(indent);
            return None;
        }
        let (raw_key, key_start) =
            crate::symfony::resources::yaml_mapping_key(semantic, line_start)?;
        let value = semantic[key_start - line_start + raw_key.len()..]
            .trim_start()
            .strip_prefix(':')?;
        if !(value.is_empty() || value.starts_with([' ', '\t'])) {
            return None;
        }
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
        self.pop_to(indent);
        self.entries.push(Entry {
            indent,
            key: key.to_string(),
            valid: is_key_segment(key) || (self.entries.is_empty() && key.starts_with("when@")),
        });
        if !value.trim().is_empty() {
            self.value_indent = Some(indent);
        }
        let start = key_start + quote;
        Some((start, start + key.len()))
    }

    /// Leave the keys a line at `indent` is not nested in.
    fn pop_to(&mut self, indent: usize) {
        while self
            .entries
            .last()
            .is_some_and(|entry| entry.indent >= indent)
        {
            self.entries.pop();
        }
    }

    /// The configuration key path of the keys the walk is in, when they
    /// configure a bundle: a `when@env` section holds configuration for
    /// one environment, and the container's own sections hold none.
    fn path(&self) -> Option<String> {
        let mut entries = self.entries.iter().peekable();
        if entries
            .peek()
            .is_some_and(|entry| entry.key.starts_with("when@"))
        {
            entries.next();
        }
        let mut path = String::new();
        for (index, entry) in entries.enumerate() {
            if !entry.valid || (index == 0 && CONTAINER_ROOTS.contains(&entry.key.as_str())) {
                return None;
            }
            if index > 0 {
                path.push('.');
            }
            path.push_str(&entry.key);
        }
        Some(path)
    }
}

/// The configuration key spans of one `config/packages/` YAML file.
pub(crate) fn config_key_spans(content: &str) -> Vec<SymbolSpan> {
    let mut walk = KeyPath::default();
    let mut spans = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let Some((start, end)) = walk.feed(line, line_start) else {
            continue;
        };
        let Some(key) = walk.path().filter(|path| !path.is_empty()) else {
            continue;
        };
        spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::SymfonyConfigKey,
                key,
                is_write: false,
                is_optional: false,
            },
        });
    }
    spans
}

/// The key being typed at `cursor`, as the path of the key it is nested in
/// (empty at the top level) and where the typed name starts.
pub(crate) fn config_key_completion(content: &str, cursor: usize) -> Option<(String, usize)> {
    let line_start = content[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let typed = &content[line_start..cursor];
    let indent = crate::framework::leading_spaces(typed);
    if !typed[indent..]
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }
    let mut walk = KeyPath::default();
    for (start, line) in crate::framework::line_offsets(&content[..line_start]) {
        walk.feed(line, start);
    }
    if walk
        .value_indent
        .is_some_and(|value_indent| indent > value_indent)
    {
        return None;
    }
    walk.pop_to(indent);
    Some((walk.path()?, line_start + indent))
}

/// The keys declared directly under `parent` (at the top level when it is
/// empty), among the declared key paths `declared`.
pub(crate) fn child_keys(declared: &[String], parent: &str) -> Vec<String> {
    let mut children: Vec<String> = declared
        .iter()
        .filter_map(|path| {
            let rest = if parent.is_empty() {
                path.as_str()
            } else {
                path.strip_prefix(parent)?.strip_prefix('.')?
            };
            (!rest.contains('.')).then(|| rest.to_string())
        })
        .collect();
    children.sort_unstable();
    children.dedup();
    children
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(php: &str) -> Vec<(String, String)> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let class = program
            .statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Class(class) => Some(class),
                _ => None,
            })
            .expect("a class");
        let resolve = |name: &str, _: u32| match name {
            "TreeBuilder" => TREE_BUILDER.to_string(),
            other => other.to_string(),
        };
        tree_builder_keys(class, &resolve)
            .into_iter()
            .map(|(path, start, end)| (path, php[start as usize..end as usize].to_string()))
            .collect()
    }

    #[test]
    fn a_tree_builder_declares_its_nested_keys() {
        let php = "<?php\nclass Configuration {\n    public function getConfigTreeBuilder() {\n        $treeBuilder = new TreeBuilder('acme');\n        $root = $treeBuilder->getRootNode();\n        $root\n            ->children()\n                ->scalarNode('api_key')->defaultValue('x')->end()\n                ->arrayNode('mailer')\n                    ->validate()->ifTrue(fn () => true)->thenInvalid('no')->end()\n                    ->children()\n                        ->scalarNode('dsn')->end()\n                    ->end()\n                ->end()\n                ->arrayNode('clients')\n                    ->useAttributeAsKey('name')\n                    ->arrayPrototype()\n                        ->children()->scalarNode('url')->end()->end()\n                    ->end()\n                ->end()\n                ->booleanNode('debug')->end()\n            ->end();\n        $treeBuilder->getRootNode()->children()->node('extra', 'array')->children()->enumNode('mode')->end();\n        return $treeBuilder;\n    }\n}\n";
        let keys: Vec<String> = declared(php).into_iter().map(|(path, _)| path).collect();
        assert_eq!(
            keys,
            vec![
                "acme",
                "acme.api_key",
                "acme.mailer",
                "acme.mailer.dsn",
                "acme.clients",
                "acme.debug",
                "acme.extra",
                "acme.extra.mode",
            ]
        );
    }

    #[test]
    fn only_symfony_tree_builders_declare_keys() {
        let php = "<?php\nclass Configuration {\n    public function getConfigTreeBuilder() {\n        $tb = new OtherBuilder('acme');\n        $tb->getRootNode()->children()->scalarNode('x')->end();\n    }\n}\n";
        assert!(declared(php).is_empty());
    }

    fn yaml_keys(yaml: &str) -> Vec<(String, String)> {
        config_key_spans(yaml)
            .into_iter()
            .map(|span| {
                let SymbolKind::StringKey { key, .. } = span.kind else {
                    unreachable!()
                };
                (
                    key,
                    yaml[span.start as usize..span.end as usize].to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn yaml_keys_are_read_as_paths() {
        let yaml = "acme:\n  api_key: '%env(KEY)%' # secret\n  intro: |\n    not: a key\n  mailer:\n    dsn: smtp://localhost\n  list:\n    - name: x\n  'quoted': 1\nparameters:\n  app.x: 1\nwhen@prod:\n  acme:\n    api_key: y\n";
        let keys: Vec<(String, String)> = yaml_keys(yaml);
        assert_eq!(
            keys.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
            vec![
                "acme",
                "acme.api_key",
                "acme.intro",
                "acme.mailer",
                "acme.mailer.dsn",
                "acme.list",
                "acme.quoted",
                "acme",
                "acme.api_key",
            ]
        );
        assert!(keys.iter().all(|(key, text)| key.ends_with(text.as_str())));
    }

    #[test]
    fn completion_is_offered_under_the_enclosing_key() {
        let at = |yaml: &str| {
            let cursor = yaml.find('|').unwrap();
            let yaml = yaml.replacen('|', "", 1);
            config_key_completion(&yaml, cursor)
                .map(|(parent, start)| (parent, yaml[start..cursor].to_string()))
        };
        assert_eq!(
            at("acme:\n  mailer:\n    ds|"),
            Some(("acme.mailer".to_string(), "ds".to_string()))
        );
        assert_eq!(
            at("acme:\n  mailer:\n    dsn: x\n  a|"),
            Some(("acme".to_string(), "a".to_string()))
        );
        assert_eq!(at("ac|"), Some((String::new(), "ac".to_string())));
        assert_eq!(at("acme:\n  dsn: x|"), None);
        assert_eq!(at("services:\n  ap|"), None);
        assert_eq!(at("acme:\n  intro: |\n    te|"), None);
    }

    #[test]
    fn children_are_one_segment_under_their_parent() {
        let declared = vec![
            "acme".to_string(),
            "acme.api_key".to_string(),
            "acme.mailer".to_string(),
            "acme.mailer.dsn".to_string(),
        ];
        assert_eq!(child_keys(&declared, ""), vec!["acme"]);
        assert_eq!(child_keys(&declared, "acme"), vec!["api_key", "mailer"]);
        assert_eq!(child_keys(&declared, "acme.mailer"), vec!["dsn"]);
    }
}
