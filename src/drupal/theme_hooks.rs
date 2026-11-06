//! Drupal's theme hooks: the names `hook_theme()` implementations declare,
//! their variables, and the Twig templates that draw them.
//!
//! Every key of the array an implementation of the hook `theme` returns
//! declares a theme hook (see [`crate::drupal::hooks`] for how the
//! implementations are found).  The array is read when it is a literal, a
//! local variable assigned one or built key by key, or an `array_merge()` or
//! `+` of those; core's `ThemeCommonElements::commonElements()`, which
//! system's implementation merges in, is read as part of the implementation
//! that calls it, and so is `drupal_common_theme()`, which it merged in
//! before Drupal 11.1.  Arrays built in a loop or by other calls are not
//! read.  The
//! implementations are read the first time a theme hook is asked for.  An
//! edit to a file that implements the hook reads that file again; a change
//! to core's common hooks, or a file changed on disk, reads them all.
//!
//! A hook's template is `<template>.html.twig` under its `path`, else under
//! its extension's `templates/`.  A theme overrides it with a file of that
//! name anywhere under the theme, and supplies a suggestion's template
//! (`node--article.html.twig` for `node__article`) the same way.  Templates
//! are matched by name and place against the paths the Drupal walk records,
//! which reach the folders a site's `.gitignore` hides.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;
use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::{HookRole, StringKeyKind, SymbolKind, SymbolMap};
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// The extension every Twig template file name ends with.
pub(crate) const TEMPLATE_SUFFIX: &str = ".html.twig";

const COMMON_ELEMENTS_CLASS: &str = "Drupal\\Core\\Theme\\ThemeCommonElements";
const COMMON_ELEMENTS_METHOD: &str = "commonElements";

/// The function core's common theme hooks came from before
/// `ThemeCommonElements`, and the file of a web root it is in.
const COMMON_THEME_FUNCTION: &str = "drupal_common_theme";
const COMMON_THEME_FILE: &str = "core/includes/theme.inc";

/// The key of a [`StringKeyKind::DrupalThemeHookVariable`] span: the
/// variable `name` of the hook `hook`, as the render array writes it.
///
/// [`StringKeyKind::DrupalThemeHookVariable`]: crate::symbol_map::StringKeyKind::DrupalThemeHookVariable
pub(crate) fn theme_variable_key(hook: &str, name: &str) -> String {
    format!("{hook}#{name}")
}

/// The hook and the variable name of a theme hook variable's key.
pub(crate) fn split_theme_variable_key(key: &str) -> (&str, &str) {
    key.split_once('#').unwrap_or(("", key))
}

/// The name a template file at `path` is matched by: its file name without
/// [`TEMPLATE_SUFFIX`].
pub(crate) fn template_stem(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()?.strip_suffix(TEMPLATE_SUFFIX)
}

/// The hook name a template file at `path` spells: its
/// [`template_stem`] with `_` for `-`.
pub(crate) fn template_key(path: &Path) -> Option<String> {
    template_stem(path).map(|stem| stem.replace('-', "_"))
}

/// A name in a `hook_theme()` array and the byte range of its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Named {
    pub name: String,
    pub start: u32,
    pub end: u32,
}

/// One entry of the array a `hook_theme()` implementation returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThemeHookEntry {
    pub name: Named,
    /// The keys of its `variables`.
    pub variables: Vec<Named>,
    /// Its `render element`: the one variable its template receives.
    pub render_element: Option<String>,
    /// Its `base hook`, whose variables and preprocessing it shares.
    pub base_hook: Option<String>,
    /// Its `template`, the file name without [`TEMPLATE_SUFFIX`].
    pub template: Option<String>,
    /// Its literal `path`, relative to the web root.
    pub path: Option<String>,
}

/// What an implementation's array holds.
#[derive(Debug, Default)]
pub(crate) struct ImplementationRead {
    pub entries: Vec<ThemeHookEntry>,
    /// Whether it merges in `ThemeCommonElements::commonElements()`.
    pub common_elements: bool,
    /// Whether it merges in `drupal_common_theme()`.
    pub common_theme: bool,
}

/// Read the array the function or method whose hook is named at `at` of
/// `content` returns: by its `#[Hook]` attribute or by its own name.
pub(crate) fn read_implementation(content: &str, at: u32) -> ImplementationRead {
    let arena = LocalArena::new();
    let program = parse_file_content(&arena, FileId::new(b"theme.php"), content.as_bytes());
    let names = OwnedResolvedNames::from_resolved(&NameResolver::new(&arena).resolve(program));
    let mut body = None;
    find_body(Node::Program(program), at, content, &mut body);
    let mut read = ImplementationRead::default();
    if let Some(statements) = body {
        let reader = ArrayReader {
            content,
            names: &names,
            statements,
        };
        reader.read_returns(&mut read);
    }
    read
}

/// Read the array the function or method `name` of `content` returns: core's
/// `ThemeCommonElements::commonElements()` or `drupal_common_theme()`.
pub(crate) fn read_declared(content: &str, name: &str) -> ImplementationRead {
    let arena = LocalArena::new();
    let program = parse_file_content(&arena, FileId::new(b"common.php"), content.as_bytes());
    let names = OwnedResolvedNames::from_resolved(&NameResolver::new(&arena).resolve(program));
    let mut body = None;
    find_declared(Node::Program(program), name, &mut body);
    let mut read = ImplementationRead::default();
    if let Some(statements) = body {
        let reader = ArrayReader {
            content,
            names: &names,
            statements,
        };
        reader.read_returns(&mut read);
    }
    read
}

/// The statements of the function or method whose declaration holds `at`:
/// in its attributes, or in its name, or for a `#[Hook]` on a class, in the
/// class's attributes.
fn find_body<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    at: u32,
    content: &str,
    found: &mut Option<&'ast [Statement<'arena>]>,
) {
    if found.is_some() {
        return;
    }
    match node {
        Node::Class(class) => {
            if let Some(attribute) = class
                .attribute_lists
                .iter()
                .flat_map(|list| list.attributes.iter())
                .find(|attribute| {
                    let span = attribute.span();
                    span.start.offset <= at && at <= span.end.offset
                })
            {
                let name = hook_method(attribute, content);
                *found = class.members.iter().find_map(|member| match member {
                    ClassLikeMember::Method(method)
                        if bytes_to_str(method.name.value).eq_ignore_ascii_case(name) =>
                    {
                        match &method.body {
                            MethodBody::Concrete(block) => Some(block.statements.as_slice()),
                            MethodBody::Abstract(_) => None,
                        }
                    }
                    _ => None,
                });
                return;
            }
        }
        Node::Function(function) => {
            if declares_at(&function.attribute_lists, function.name.span, at) {
                *found = Some(function.body.statements.as_slice());
            }
            return;
        }
        Node::Method(method) => {
            if declares_at(&method.attribute_lists, method.name.span, at)
                && let MethodBody::Concrete(block) = &method.body
            {
                *found = Some(block.statements.as_slice());
            }
            return;
        }
        _ => {}
    }
    let span = node.span();
    if span.start.offset <= at && at <= span.end.offset {
        node.visit_children(|child| find_body(child, at, content, found));
    }
}

/// The method a class's `#[Hook]` runs: the one its `method:` argument
/// names, else `__invoke`.
fn hook_method<'arena>(attribute: &Attribute<'arena>, content: &str) -> &'arena str {
    attribute
        .argument_list
        .iter()
        .flat_map(|list| list.arguments.iter().enumerate())
        .find_map(|(index, argument)| match argument {
            PartialArgument::Named(named) if bytes_to_str(named.name.value) == "method" => {
                Some(named.value)
            }
            PartialArgument::Positional(positional) if index == 1 => Some(positional.value),
            _ => None,
        })
        .and_then(|value| resolved_string_literal(value, content))
        .map(|(name, _, _)| name)
        .filter(|name| !name.is_empty())
        .unwrap_or("__invoke")
}

/// Whether `at` lies in the attributes or the name of a declaration.
fn declares_at(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    name: mago_span::Span,
    at: u32,
) -> bool {
    let in_name = name.start.offset <= at && at <= name.end.offset;
    in_name
        || attribute_lists.iter().any(|list| {
            let span = list.span();
            span.start.offset <= at && at <= span.end.offset
        })
}

/// The statements of the first function or method named `name`.
fn find_declared<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    name: &str,
    found: &mut Option<&'ast [Statement<'arena>]>,
) {
    if found.is_some() {
        return;
    }
    match node {
        Node::Method(method) => {
            if bytes_to_str(method.name.value).eq_ignore_ascii_case(name)
                && let MethodBody::Concrete(block) = &method.body
            {
                *found = Some(block.statements.as_slice());
            }
            return;
        }
        Node::Function(function) => {
            if bytes_to_str(function.name.value).eq_ignore_ascii_case(name) {
                *found = Some(function.body.statements.as_slice());
            }
            return;
        }
        _ => {}
    }
    node.visit_children(|child| find_declared(child, name, found));
}

/// Whether `function` names the global function `name`.
fn is_function(function: &Expression<'_>, name: &str) -> bool {
    let Expression::Identifier(identifier) = function else {
        return false;
    };
    bytes_to_str(identifier.value())
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(name)
}

/// The implementations of the hook `theme` Drupal collects that `map`
/// records: where each names the hook, and its extension.
fn theme_implementations(map: &SymbolMap) -> Vec<(u32, String)> {
    map.spans
        .iter()
        .filter_map(|span| match &span.kind {
            SymbolKind::StringKey {
                kind: StringKeyKind::DrupalHook,
                key,
                is_write: false,
                ..
            } if key == "theme" => {
                let at = map
                    .drupal_hooks
                    .binary_search_by_key(&span.start, |site| site.start)
                    .ok()?;
                match &map.drupal_hooks[at].role {
                    HookRole::Implementation {
                        extension,
                        collected: true,
                        ..
                    } => Some((span.start, extension.to_string())),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// How deep variables and merges are followed into each other.
const MAX_DEPTH: usize = 8;

/// Reads the arrays a function body returns.
struct ArrayReader<'a, 'ast, 'arena> {
    content: &'a str,
    names: &'a OwnedResolvedNames,
    statements: &'ast [Statement<'arena>],
}

impl<'ast, 'arena> ArrayReader<'_, 'ast, 'arena> {
    /// Read every `return` written directly in the body.
    fn read_returns(&self, read: &mut ImplementationRead) {
        for statement in self.statements {
            if let Statement::Return(ret) = statement
                && let Some(value) = ret.value
            {
                self.read_array(value, ret.span().start.offset, 0, read);
            }
        }
    }

    /// Read `expression`, written before `before`, as a hook array.
    fn read_array(
        &self,
        expression: &'ast Expression<'arena>,
        before: u32,
        depth: usize,
        read: &mut ImplementationRead,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        match expression {
            Expression::Array(_) | Expression::LegacyArray(_) => {
                for element in crate::parser::array_literal_elements(expression)
                    .into_iter()
                    .flat_map(|elements| elements.iter())
                {
                    if let ArrayElement::KeyValue(entry) = element
                        && let Some(hook) = self.entry(entry.key, entry.value, before, depth)
                    {
                        read.entries.push(hook);
                    }
                }
            }
            Expression::Parenthesized(inner) => {
                self.read_array(inner.expression, before, depth + 1, read);
            }
            Expression::Variable(Variable::Direct(variable)) => {
                if let Some(value) = self.assigned(variable.name, before) {
                    self.read_array(value, value.span().start.offset, depth + 1, read);
                }
                for (key, value) in self.keyed_assignments(variable.name, before) {
                    if let Some(hook) = self.entry(key, value, before, depth) {
                        read.entries.push(hook);
                    }
                }
            }
            Expression::Binary(binary)
                if matches!(binary.operator, BinaryOperator::Addition(_)) =>
            {
                self.read_array(binary.lhs, before, depth + 1, read);
                self.read_array(binary.rhs, before, depth + 1, read);
            }
            Expression::Call(Call::Function(call))
                if is_function(call.function, COMMON_THEME_FUNCTION) =>
            {
                read.common_theme = true;
            }
            Expression::Call(Call::Function(call)) if is_function(call.function, "array_merge") => {
                for argument in call.argument_list.arguments.iter() {
                    if let Argument::Positional(argument) = argument {
                        self.read_array(argument.value, before, depth + 1, read);
                    }
                }
            }
            Expression::Call(Call::StaticMethod(call)) if self.is_common_elements(call) => {
                read.common_elements = true;
            }
            _ => {}
        }
    }

    /// The value last assigned to `$name` directly in the body before
    /// `before`.
    fn assigned(&self, name: &[u8], before: u32) -> Option<&'ast Expression<'arena>> {
        self.statements
            .iter()
            .take_while(|statement| statement.span().end.offset <= before)
            .filter_map(|statement| match statement {
                Statement::Expression(statement) => match statement.expression {
                    Expression::Assignment(assignment) if assignment.operator.is_assign() => {
                        match assignment.lhs {
                            Expression::Variable(Variable::Direct(variable))
                                if variable.name == name =>
                            {
                                Some(assignment.rhs)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                },
                _ => None,
            })
            .last()
    }

    /// Each `$name['key'] = value` written directly in the body before
    /// `before`, after the last value assigned to `$name` as a whole.
    fn keyed_assignments(
        &self,
        name: &[u8],
        before: u32,
    ) -> Vec<(&'ast Expression<'arena>, &'ast Expression<'arena>)> {
        let mut found = Vec::new();
        for statement in self
            .statements
            .iter()
            .take_while(|statement| statement.span().end.offset <= before)
        {
            let Statement::Expression(statement) = statement else {
                continue;
            };
            let Expression::Assignment(assignment) = statement.expression else {
                continue;
            };
            if !assignment.operator.is_assign() {
                continue;
            }
            match assignment.lhs {
                Expression::Variable(Variable::Direct(variable)) if variable.name == name => {
                    found.clear();
                }
                Expression::ArrayAccess(access)
                    if matches!(
                        access.array,
                        Expression::Variable(Variable::Direct(variable)) if variable.name == name
                    ) =>
                {
                    found.push((access.index, assignment.rhs));
                }
                _ => {}
            }
        }
        found
    }

    /// Whether `call` is core's `ThemeCommonElements::commonElements()`, by
    /// the class's resolved name.
    fn is_common_elements(&self, call: &StaticMethodCall<'_>) -> bool {
        let Expression::Identifier(class) = call.class else {
            return false;
        };
        let ClassLikeMemberSelector::Identifier(method) = &call.method else {
            return false;
        };
        bytes_to_str(method.value).eq_ignore_ascii_case(COMMON_ELEMENTS_METHOD)
            && self
                .names
                .class_fqn(class)
                .is_some_and(|fqn| fqn.eq_ignore_ascii_case(COMMON_ELEMENTS_CLASS))
    }

    /// The hook an entry `key => value` declares.
    fn entry(
        &self,
        key: &Expression<'_>,
        value: &'ast Expression<'arena>,
        before: u32,
        depth: usize,
    ) -> Option<ThemeHookEntry> {
        let (name, start, end) = resolved_string_literal(key, self.content)?;
        let mut hook = ThemeHookEntry {
            name: Named {
                name: name.to_string(),
                start: start as u32,
                end: end as u32,
            },
            variables: Vec::new(),
            render_element: None,
            base_hook: None,
            template: None,
            path: None,
        };
        // A hook whose settings are built some other way still declares
        // its name, and its template by that name.
        let Some(elements) = self
            .literal(value, before, depth)
            .and_then(crate::parser::array_literal_elements)
        else {
            return Some(hook);
        };
        for element in elements.iter() {
            let ArrayElement::KeyValue(entry) = element else {
                continue;
            };
            let Some((setting, _, _)) = resolved_string_literal(entry.key, self.content) else {
                continue;
            };
            let text = || {
                resolved_string_literal(entry.value, self.content)
                    .map(|(text, _, _)| text.to_string())
            };
            match setting {
                "variables" => {
                    let Some(variables) = self.literal(entry.value, before, depth) else {
                        continue;
                    };
                    for variable in crate::parser::array_literal_elements(variables)
                        .into_iter()
                        .flat_map(|elements| elements.iter())
                    {
                        if let ArrayElement::KeyValue(variable) = variable
                            && let Some((name, start, end)) =
                                resolved_string_literal(variable.key, self.content)
                        {
                            hook.variables.push(Named {
                                name: name.to_string(),
                                start: start as u32,
                                end: end as u32,
                            });
                        }
                    }
                }
                "render element" => hook.render_element = text(),
                "base hook" => hook.base_hook = text(),
                "template" => hook.template = text(),
                "path" => hook.path = text(),
                _ => {}
            }
        }
        Some(hook)
    }

    /// `expression` as an array literal, through a variable assigned one.
    fn literal(
        &self,
        expression: &'ast Expression<'arena>,
        before: u32,
        depth: usize,
    ) -> Option<&'ast Expression<'arena>> {
        match expression {
            Expression::Array(_) | Expression::LegacyArray(_) => Some(expression),
            Expression::Variable(Variable::Direct(variable)) if depth < MAX_DEPTH => {
                let value = self.assigned(variable.name, before)?;
                self.literal(value, value.span().start.offset, depth + 1)
            }
            _ => None,
        }
    }
}

/// A theme hook the site declares.
#[derive(Debug)]
pub(crate) struct ThemeHook {
    pub entry: ThemeHookEntry,
    /// The file the hook's key is written in.
    pub uri: String,
    /// The extension whose implementation declares it.
    pub extension: String,
    /// Where its default template is.
    pub template_path: PathBuf,
}

impl ThemeHook {
    pub(crate) fn name(&self) -> &str {
        &self.entry.name.name
    }

    /// The name of its template files: its `template`, else its name with
    /// `-` for `_`.
    pub(crate) fn template_name(&self) -> String {
        self.entry
            .template
            .clone()
            .unwrap_or_else(|| self.name().replace('_', "-"))
    }
}

/// The theme hooks the site declares, by name and by the file name of
/// their templates, and the files they were read from.
#[derive(Default)]
pub(crate) struct ThemeHookIndex {
    hooks: BTreeMap<String, Vec<Arc<ThemeHook>>>,
    /// Keyed by the last segment of each hook's template name, the
    /// [`template_stem`] of its template files.
    by_template: HashMap<String, Vec<Arc<ThemeHook>>>,
    /// The hash of each file the hooks were read from, as it was read.
    sources: HashMap<String, u64>,
    /// The files whose implementations merge core's common hooks in, and
    /// the files those are read from, whose hooks are credited to another
    /// file's extension.
    shared: HashSet<String>,
}

impl ThemeHookIndex {
    /// The same index without the hooks read from the file at `uri`.
    fn without(&self, uri: &str) -> ThemeHookIndex {
        let mut index = ThemeHookIndex {
            sources: self.sources.clone(),
            shared: self.shared.clone(),
            ..ThemeHookIndex::default()
        };
        index.sources.remove(uri);
        for hook in self.hooks.values().flatten() {
            if hook.uri != uri {
                index.add(Arc::clone(hook));
            }
        }
        index
    }

    fn add(&mut self, hook: Arc<ThemeHook>) {
        let template = hook.template_name();
        let file_name = template.rsplit('/').next().unwrap_or(&template);
        match self.by_template.get_mut(file_name) {
            Some(hooks) => hooks.push(Arc::clone(&hook)),
            None => {
                self.by_template
                    .insert(file_name.to_string(), vec![Arc::clone(&hook)]);
            }
        }
        self.hooks
            .entry(hook.name().to_string())
            .or_default()
            .push(hook);
    }

    /// The declarations of the hook a name written in a render array stands
    /// for: its own, else those of the hook a suggestion falls back to, by
    /// dropping `__<detail>` parts from the end.
    fn declarations(&self, written: &str) -> &[Arc<ThemeHook>] {
        let mut name = written;
        loop {
            if let Some(hooks) = self.hooks.get(name) {
                return hooks;
            }
            match name.rsplit_once("__") {
                Some((base, _)) if !base.is_empty() => name = base,
                _ => return &[],
            }
        }
    }
}

impl Backend {
    /// The theme hooks the site declares, read once until a file they come
    /// from changes (see [`Self::note_drupal_theme_hook_sources`]).
    fn theme_hook_index(&self) -> Arc<ThemeHookIndex> {
        if let Some(index) = self.drupal.theme_hook_index.read().as_ref() {
            return Arc::clone(index);
        }
        // The implementations in core's closed files are found through the
        // reference index.
        self.ensure_workspace_index_ready_for_request();
        let mut index = ThemeHookIndex::default();
        let add = |index: &mut ThemeHookIndex, uri: &str, extension: &str, entries| {
            for entry in entries {
                index.add(Arc::new(self.theme_hook(entry, uri, extension)));
            }
        };
        let mut common_elements: Option<(String, u64, ImplementationRead)> = None;
        let mut common_theme: Option<(String, u64, ImplementationRead)> = None;
        for (uri, start, extension) in self.drupal_hook_implementations("theme") {
            let Some(content) = self.get_file_content_arc(&uri) else {
                continue;
            };
            index
                .sources
                .insert(uri.clone(), SymbolMap::hash_source(&content));
            let mut read = read_implementation(&content, start);
            add(&mut index, &uri, &extension, read.entries);
            if read.common_theme {
                index.shared.insert(uri.clone());
                if common_theme.is_none() {
                    common_theme = Some(self.common_theme(&uri).unwrap_or_default());
                }
                if let Some((theme_uri, hash, theme_read)) = &common_theme
                    && !theme_uri.is_empty()
                {
                    index.sources.insert(theme_uri.clone(), *hash);
                    index.shared.insert(theme_uri.clone());
                    add(
                        &mut index,
                        theme_uri,
                        &extension,
                        theme_read.entries.clone(),
                    );
                    read.common_elements |= theme_read.common_elements;
                }
            }
            if read.common_elements {
                index.shared.insert(uri.clone());
                if common_elements.is_none() {
                    common_elements = Some(self.common_elements().unwrap_or_default());
                }
                if let Some((common_uri, hash, common_read)) = &common_elements
                    && !common_uri.is_empty()
                {
                    index.sources.insert(common_uri.clone(), *hash);
                    index.shared.insert(common_uri.clone());
                    add(
                        &mut index,
                        common_uri,
                        &extension,
                        common_read.entries.clone(),
                    );
                }
            }
        }
        let index = Arc::new(index);
        *self.drupal.theme_hook_index.write() = Some(Arc::clone(&index));
        index
    }

    /// The file of core's `ThemeCommonElements`, its hash, and what its
    /// `commonElements()` returns.
    fn common_elements(&self) -> Option<(String, u64, ImplementationRead)> {
        let uri = self
            .class_declaration_uri(COMMON_ELEMENTS_CLASS)
            .or_else(|| {
                self.find_or_load_class(COMMON_ELEMENTS_CLASS);
                self.class_declaration_uri(COMMON_ELEMENTS_CLASS)
            })?;
        let content = self.get_file_content_arc(&uri)?;
        let read = read_declared(&content, COMMON_ELEMENTS_METHOD);
        Some((uri, SymbolMap::hash_source(&content), read))
    }

    /// The file of `drupal_common_theme()` in the web root of the file at
    /// `uri`, its hash, and what the function returns.
    fn common_theme(&self, uri: &str) -> Option<(String, u64, ImplementationRead)> {
        let file = crate::util::uri_to_path(uri)?;
        let path = self
            .drupal
            .web_roots
            .read()
            .iter()
            .find(|root| file.starts_with(root))?
            .join(COMMON_THEME_FILE);
        let uri = crate::util::path_to_uri(&path);
        let content = self.get_file_content_arc(&uri)?;
        let read = read_declared(&content, COMMON_THEME_FUNCTION);
        Some((uri, SymbolMap::hash_source(&content), read))
    }

    /// The hook `entry` declares, by `extension`'s implementation in `uri`,
    /// with where its default template is.
    fn theme_hook(&self, entry: ThemeHookEntry, uri: &str, extension: &str) -> ThemeHook {
        let file = crate::util::uri_to_path(uri).unwrap_or_default();
        let dir = match &entry.path {
            Some(path) => self
                .drupal
                .web_roots
                .read()
                .iter()
                .find(|root| file.starts_with(root))
                .map(|root| root.join(path)),
            None => self
                .drupal
                .extensions
                .read()
                .dir_of(extension)
                .map(|dir| dir.join("templates")),
        };
        let mut hook = ThemeHook {
            entry,
            uri: uri.to_string(),
            extension: extension.to_string(),
            template_path: PathBuf::new(),
        };
        if let Some(dir) = dir {
            hook.template_path = dir.join(format!("{}{TEMPLATE_SUFFIX}", hook.template_name()));
        }
        hook
    }

    /// Follow the file at `uri`, now holding `map`, when the theme hooks
    /// [`Self::theme_hook_index`] holds were read from it or it now
    /// implements the hook `theme`: read its hooks again when its text
    /// changed.  A file the common hooks come through makes every hook be
    /// read again, and so does a file whose map is dropped (`None`) when its
    /// text on disk is no longer the text read.
    pub(crate) fn note_drupal_theme_hook_sources(&self, uri: &str, map: Option<&SymbolMap>) {
        let Some(index) = self.drupal.theme_hook_index.read().clone() else {
            return;
        };
        let known = index.sources.get(uri).copied();
        let Some(map) = map else {
            // A map is dropped while its file is read again, as well as
            // when the file is gone.
            let unchanged = crate::util::uri_to_path(uri)
                .and_then(|path| crate::util::read_php_source(&path))
                .is_some_and(|content| Some(SymbolMap::hash_source(&content)) == known);
            if known.is_some() && !unchanged {
                self.forget_drupal_theme_hooks();
            }
            return;
        };
        let implementations = theme_implementations(map);
        if known.is_none() && implementations.is_empty() {
            return;
        }
        // A map of older text than the buffer's is followed by the newer
        // text's own.
        let Some(content) = self
            .get_file_content_arc(uri)
            .filter(|content| map.matches_source(content))
        else {
            return;
        };
        let hash = SymbolMap::hash_source(&content);
        if known == Some(hash) {
            return;
        }
        if index.shared.contains(uri) {
            return self.forget_drupal_theme_hooks();
        }
        let mut updated = index.without(uri);
        if !implementations.is_empty() {
            for (start, extension) in implementations {
                let read = read_implementation(&content, start);
                if read.common_elements || read.common_theme {
                    return self.forget_drupal_theme_hooks();
                }
                for entry in read.entries {
                    updated.add(Arc::new(self.theme_hook(entry, uri, &extension)));
                }
            }
            updated.sources.insert(uri.to_string(), hash);
        }
        let mut slot = self.drupal.theme_hook_index.write();
        // Another change landed meanwhile: read everything again.
        *slot = slot
            .as_ref()
            .filter(|current| Arc::ptr_eq(current, &index))
            .map(|_| Arc::new(updated));
    }

    /// Drop the theme hooks, for every implementation to be read again.
    fn forget_drupal_theme_hooks(&self) {
        *self.drupal.theme_hook_index.write() = None;
    }

    /// Every theme hook the site declares, sorted.
    pub(crate) fn drupal_theme_hook_names(&self) -> Vec<String> {
        self.theme_hook_index().hooks.keys().cloned().collect()
    }

    /// The declarations of the hook `written` names in a render array: its
    /// own, or for a suggestion those of the hook it falls back to.
    pub(crate) fn drupal_theme_hooks(&self, written: &str) -> Vec<Arc<ThemeHook>> {
        if !self.is_drupal_project() {
            return Vec::new();
        }
        self.theme_hook_index().declarations(written).to_vec()
    }

    /// The hook whose variables `hook`'s template receives: the one its
    /// `base hook` names, when it has one, else itself.
    pub(crate) fn drupal_theme_hook_variable_source(
        &self,
        hook: &Arc<ThemeHook>,
    ) -> Arc<ThemeHook> {
        hook.entry
            .base_hook
            .as_deref()
            .and_then(|base| self.theme_hook_index().hooks.get(base)?.first().cloned())
            .unwrap_or_else(|| Arc::clone(hook))
    }

    /// The template files of the hook `hook`, written in a render array as
    /// `written`: its default template, then the files of every theme that
    /// overrides it, or for a suggestion, that supplies the suggestion's.
    pub(crate) fn drupal_theme_hook_templates(
        &self,
        hook: &ThemeHook,
        written: &str,
    ) -> Vec<PathBuf> {
        let templates = self.drupal.templates.read();
        let mut found: Vec<PathBuf> = Vec::new();
        if template_key(&hook.template_path)
            .and_then(|key| templates.get(&key))
            .is_some_and(|paths| paths.contains(&hook.template_path))
        {
            found.push(hook.template_path.clone());
        }
        // The files spelling the name written, and for the hook itself
        // those named by its `template`.
        let template_name = hook.template_name();
        let by_template = (written == hook.name())
            .then(|| templates.get(&template_name.replace('-', "_")))
            .flatten()
            .into_iter()
            .flatten()
            .filter(|path| template_stem(path) == Some(template_name.as_str()));
        let extensions = self.drupal.extensions.read();
        let mut overrides: Vec<PathBuf> = templates
            .get(written)
            .into_iter()
            .flatten()
            .chain(by_template)
            .filter(|path| **path != hook.template_path)
            .filter(|path| extensions.owner_is_theme(path))
            .cloned()
            .collect();
        overrides.sort();
        overrides.dedup();
        found.extend(overrides);
        found
    }

    /// The hooks whose template the file at `path` is: the default template
    /// of each, a theme's override of it, or a theme's template for one of
    /// its suggestions.
    pub(crate) fn drupal_template_theme_hooks(&self, path: &Path) -> Vec<Arc<ThemeHook>> {
        if !self.is_drupal_project() {
            return Vec::new();
        }
        let Some(stem) = template_stem(path) else {
            return Vec::new();
        };
        let index = self.theme_hook_index();
        let in_theme = self.drupal.extensions.read().owner_is_theme(path);
        let hook_name = stem.replace('-', "_");
        // A theme overrides a hook's template by its file name, which a
        // `template` naming a subfolder does not match.
        let overrides = |hook: &&Arc<ThemeHook>| {
            in_theme
                && hook
                    .entry
                    .template
                    .as_deref()
                    .is_none_or(|template| !template.contains('/'))
        };
        let mut found: Vec<Arc<ThemeHook>> = index
            .by_template
            .get(stem)
            .into_iter()
            .flatten()
            .filter(|hook| hook.template_path == path || overrides(hook))
            .cloned()
            .collect();
        if in_theme {
            for hook in index.hooks.get(&hook_name).into_iter().flatten() {
                if !found.iter().any(|known| Arc::ptr_eq(known, hook)) {
                    found.push(Arc::clone(hook));
                }
            }
        }
        found.sort_by(|a, b| a.name().cmp(b.name()));
        if found.is_empty() && in_theme {
            found.extend(
                index
                    .declarations(&hook_name)
                    .iter()
                    .filter(|hook| hook.entry.base_hook.is_none())
                    .cloned(),
            );
        }
        found
    }

    /// Where go-to-definition on the hook `written` leads: its entries in
    /// `hook_theme()`, then its templates.
    pub(crate) fn drupal_theme_hook_definitions(&self, written: &str) -> Vec<Location> {
        let hooks = self.drupal_theme_hooks(written);
        let mut locations: Vec<Location> = hooks
            .iter()
            .filter_map(|hook| {
                self.theme_location(&hook.uri, hook.entry.name.start, hook.entry.name.end)
            })
            .collect();
        let mut templates: Vec<PathBuf> = Vec::new();
        for hook in &hooks {
            for path in self.drupal_theme_hook_templates(hook, written) {
                if !templates.contains(&path) {
                    templates.push(path);
                }
            }
        }
        locations.extend(templates.iter().filter_map(|path| {
            Some(Location {
                uri: crate::util::path_to_url(path)?,
                range: Range::default(),
            })
        }));
        locations
    }

    /// Where the variable of the theme hook variable key `key` is declared:
    /// its key in the `variables` of the hook its render array names.
    pub(crate) fn drupal_theme_variable_definitions(&self, key: &str) -> Vec<Location> {
        let (written, name) = split_theme_variable_key(key);
        let name = name.trim_start_matches('#');
        self.drupal_theme_hooks(written)
            .iter()
            .map(|hook| self.drupal_theme_hook_variable_source(hook))
            .flat_map(|hook| {
                hook.entry
                    .variables
                    .iter()
                    .filter(|variable| variable.name == name)
                    .filter_map(|variable| {
                        self.theme_location(&hook.uri, variable.start, variable.end)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The hover text of the hook `written`: what declares it, its
    /// template, and what its template receives.
    pub(crate) fn drupal_theme_hook_detail(&self, written: &str) -> String {
        let hooks = self.drupal_theme_hooks(written);
        let Some(hook) = hooks.first() else {
            return "No `hook_theme()` array the project's code writes out declares it".to_string();
        };
        let mut lines = Vec::new();
        if written != hook.name() {
            lines.push(format!("Suggestion of `{}`", hook.name()));
        }
        lines.push(format!("Declared by `{}`", hook.extension));
        let template = self
            .drupal_theme_hook_templates(hook, hook.name())
            .first()
            .cloned()
            .unwrap_or_else(|| hook.template_path.clone());
        if let Some(relative) = crate::util::path_to_url(&template)
            .and_then(|uri| self.workspace_relative_path(uri.as_str()))
        {
            lines.push(format!("Template `{relative}`"));
        }
        let source = self.drupal_theme_hook_variable_source(hook);
        if let Some(element) = &source.entry.render_element {
            lines.push(format!("Render element `{element}`"));
        } else if !source.entry.variables.is_empty() {
            let names: Vec<String> = source
                .entry
                .variables
                .iter()
                .map(|variable| format!("`{}`", variable.name))
                .collect();
            lines.push(format!("Variables {}", names.join(", ")));
        }
        lines.join("\n\n")
    }

    /// The variables a render array whose `#theme` is `written` can pass,
    /// as their `#` keys; none for a hook with a render element.
    pub(crate) fn drupal_theme_hook_variable_names(&self, written: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for hook in self.drupal_theme_hooks(written) {
            let source = self.drupal_theme_hook_variable_source(&hook);
            for variable in &source.entry.variables {
                let key = format!("#{}", variable.name);
                if !names.contains(&key) {
                    names.push(key);
                }
            }
        }
        names
    }

    fn theme_location(&self, uri: &str, start: u32, end: u32) -> Option<Location> {
        let parsed = crate::util::parse_url(uri)?;
        let content = self.get_file_content_arc(uri)?;
        let lines = crate::text_position::LineIndex::new(&content);
        Some(Location {
            uri: parsed,
            range: Range {
                start: lines.position(start as usize),
                end: lines.position(end as usize),
            },
        })
    }

    /// Remember the templates the Drupal walk found.
    pub(crate) fn note_drupal_templates(&self, templates: Vec<PathBuf>) {
        let mut known = self.drupal.templates.write();
        for path in templates {
            if let Some(key) = template_key(&path) {
                known.entry(key).or_default().push(path);
            }
        }
    }

    /// Follow the watched-file events that change the theme hooks: a
    /// template created or deleted, and a file the hooks were read from
    /// created, deleted, or changed to other text than was read, unless it
    /// is open.  Whether any did.
    pub(crate) fn note_drupal_theme_watched_changes(
        &self,
        changes: &[tower_lsp_server::gen_lsp_types::FileEvent],
    ) -> bool {
        use tower_lsp_server::gen_lsp_types::FileChangeType;
        if !self.is_drupal_project() {
            return false;
        }
        let mut touched = false;
        for change in changes {
            let Some(path) = crate::util::url_to_path(&change.uri) else {
                continue;
            };
            if template_stem(&path).is_some() {
                if change.kind != FileChangeType::Changed {
                    touched |= self
                        .note_drupal_template_change(&path, change.kind == FileChangeType::Created);
                }
                continue;
            }
            let Some(index) = self.drupal.theme_hook_index.read().clone() else {
                continue;
            };
            let Some(read) = index
                .sources
                .get(change.uri.as_str())
                .or_else(|| index.sources.get(&crate::util::path_to_uri(&path)))
                .copied()
            else {
                continue;
            };
            // An open file follows its buffer, and an editor regaining focus
            // reports every file as changed.
            if change.kind == FileChangeType::Changed
                && (self.open_files.read().contains_key(change.uri.as_str())
                    || crate::util::read_php_source(&path)
                        .is_some_and(|content| SymbolMap::hash_source(&content) == read))
            {
                continue;
            }
            self.forget_drupal_theme_hooks();
            touched = true;
        }
        touched
    }

    /// Follow a template file created (`exists`) or deleted on disk:
    /// whether the recorded templates changed.
    fn note_drupal_template_change(&self, path: &Path, exists: bool) -> bool {
        if !self
            .drupal
            .web_roots
            .read()
            .iter()
            .any(|root| path.starts_with(root) && super::is_template_file(path, root))
        {
            return false;
        }
        let Some(key) = template_key(path) else {
            return false;
        };
        let mut templates = self.drupal.templates.write();
        let paths = templates.entry(key).or_default();
        let known = paths.iter().position(|known| known == path);
        match (known, exists) {
            (None, true) => paths.push(path.to_path_buf()),
            (Some(at), false) => {
                paths.swap_remove(at);
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[ThemeHookEntry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.name.name.as_str())
            .collect()
    }

    #[test]
    fn a_literal_a_variable_and_their_merges_are_read() {
        let php = r#"<?php
namespace Drupal\acme\Hook;
use Drupal\Core\Hook\Attribute\Hook;
use Drupal\Core\Theme\ThemeCommonElements;
class AcmeHooks {
  #[Hook('theme')]
  public function theme(): array {
    $common = ThemeCommonElements::commonElements();
    $box = ['variables' => ['label' => NULL, 'tone' => 'calm']];
    $hooks = [
      'acme_box' => $box,
      'acme_form' => ['render element' => 'form', 'template' => 'form/acme-form'],
    ];
    return array_merge($common, $hooks) + ['acme_card' => ['base hook' => 'acme_box', 'path' => 'modules/custom/acme/cards']];
  }
  #[Hook('cron')]
  public function cron(): array {
    return ['not_a_hook' => []];
  }
}
"#;
        let read = read_implementation(php, php.find("'theme'").unwrap() as u32 + 1);
        assert!(read.common_elements);
        assert_eq!(names(&read.entries), ["acme_box", "acme_form", "acme_card"]);
        let box_ = &read.entries[0];
        assert_eq!(
            box_.variables
                .iter()
                .map(|variable| variable.name.as_str())
                .collect::<Vec<_>>(),
            ["label", "tone"]
        );
        let label = &box_.variables[0];
        assert_eq!(&php[label.start as usize..label.end as usize], "label");
        let form = &read.entries[1];
        assert_eq!(form.render_element.as_deref(), Some("form"));
        assert_eq!(form.template.as_deref(), Some("form/acme-form"));
        let card = &read.entries[2];
        assert_eq!(card.base_hook.as_deref(), Some("acme_box"));
        assert_eq!(card.path.as_deref(), Some("modules/custom/acme/cards"));
        let name = &read.entries[0].name;
        assert_eq!(&php[name.start as usize..name.end as usize], "acme_box");
    }

    #[test]
    fn a_class_hook_runs_its_method_or_invoke() {
        let php = r#"<?php
namespace Drupal\acme\Hook;
use Drupal\Core\Hook\Attribute\Hook;
#[Hook('cron'), Hook('theme', method: 'themes')]
class AcmeThemes {
  public function cron(): array { return ['not_a_hook' => []]; }
  public function themes(): array { return ['acme_named' => []]; }
}
#[Hook('theme')]
class AcmeInvoked {
  #[Hook('help')]
  public function help(): array { return ['not_a_hook' => []]; }
  public function __invoke(): array { return ['acme_invoked' => []]; }
}
#[Hook('theme', 'positional')]
class AcmePositional {
  public function positional(): array { return ['acme_positional' => []]; }
}
"#;
        let read = |needle: &str| {
            names(&read_implementation(php, php.find(needle).unwrap() as u32 + 1).entries)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(read("'theme', method"), ["acme_named"]);
        assert_eq!(read("'theme')]\nclass AcmeInvoked"), ["acme_invoked"]);
        assert_eq!(read("'theme', 'positional'"), ["acme_positional"]);
    }

    #[test]
    fn a_procedural_implementation_is_found_by_its_name() {
        let php = "<?php\nfunction example_theme($existing) {\n  return ['example_box' => ['variables' => []]];\n}\nfunction example_help() {\n  return ['nope' => []];\n}\n";
        let at = php.find("_theme").unwrap() as u32 + 1;
        assert_eq!(
            names(&read_implementation(php, at).entries),
            ["example_box"]
        );
    }

    #[test]
    fn an_array_built_key_by_key_is_read_after_its_last_assignment() {
        let php = "<?php\nfunction example_theme() {\n  $items['dropped'] = [];\n  $items = ['example_box' => []];\n  $items['example_card'] = ['variables' => ['label' => NULL]];\n  $other['nope'] = [];\n  return $items;\n}\n";
        let at = php.find("_theme").unwrap() as u32 + 1;
        let read = read_implementation(php, at);
        assert_eq!(names(&read.entries), ["example_box", "example_card"]);
        assert_eq!(read.entries[1].variables[0].name, "label");
    }

    #[test]
    fn a_hook_whose_settings_are_not_a_literal_keeps_its_name() {
        let php = "<?php\nfunction example_theme() {\n  $base = ['variables' => ['label' => NULL]];\n  return [\n    'example_box' => $base + ['template' => 'box'],\n    'example_card' => self::definition(),\n    'example_list' => ['variables' => ['items' => []]],\n  ];\n}\n";
        let at = php.find("_theme").unwrap() as u32 + 1;
        let read = read_implementation(php, at);
        assert_eq!(
            names(&read.entries),
            ["example_box", "example_card", "example_list"]
        );
        assert!(read.entries[0].variables.is_empty());
        assert!(read.entries[1].variables.is_empty());
        assert_eq!(read.entries[2].variables[0].name, "items");
    }

    #[test]
    fn a_common_elements_class_of_another_namespace_is_not_cores() {
        let php = r#"<?php
namespace Drupal\acme\Hook;
use Drupal\acme\ThemeCommonElements;
class AcmeHooks {
  #[Hook('theme')]
  public function theme(): array {
    return ThemeCommonElements::commonElements();
  }
}
"#;
        let read = read_implementation(php, php.find("'theme'").unwrap() as u32 + 1);
        assert!(!read.common_elements);
    }

    #[test]
    fn core_common_elements_are_read_from_their_method() {
        let php = "<?php\nnamespace Drupal\\Core\\Theme;\nclass ThemeCommonElements {\n  public static function commonElements(): array {\n    return ['item_list' => ['variables' => ['items' => []]], 'html' => ['render element' => 'html']];\n  }\n}\n";
        let entries = read_declared(php, COMMON_ELEMENTS_METHOD).entries;
        assert_eq!(names(&entries), ["item_list", "html"]);
    }

    /// `drupal_common_theme()` returns its hooks before Drupal 11.1, and
    /// `ThemeCommonElements::commonElements()` from then on.
    #[test]
    fn a_merged_drupal_common_theme_is_followed() {
        let php = "<?php\nfunction system_theme() {\n  return array_merge(drupal_common_theme(), ['system_box' => []]);\n}\n";
        let read = read_implementation(php, php.find("_theme").unwrap() as u32 + 1);
        assert!(read.common_theme);
        assert_eq!(names(&read.entries), ["system_box"]);

        let theme_10 = "<?php\nfunction drupal_common_theme() {\n  return ['links' => ['variables' => ['links' => []]], 'html' => ['render element' => 'html']];\n}\n";
        let read = read_declared(theme_10, COMMON_THEME_FUNCTION);
        assert_eq!(names(&read.entries), ["links", "html"]);
        assert!(!read.common_elements);

        let theme_11 = "<?php\nuse Drupal\\Core\\Theme\\ThemeCommonElements;\nfunction drupal_common_theme(): array {\n  return ThemeCommonElements::commonElements();\n}\n";
        let read = read_declared(theme_11, COMMON_THEME_FUNCTION);
        assert!(read.entries.is_empty());
        assert!(read.common_elements);
    }

    #[test]
    fn a_suggestion_falls_back_to_the_hook_its_name_starts_with() {
        let hook = |name: &str| ThemeHook {
            entry: ThemeHookEntry {
                name: Named {
                    name: name.to_string(),
                    start: 0,
                    end: 0,
                },
                variables: Vec::new(),
                render_element: None,
                base_hook: None,
                template: None,
                path: None,
            },
            uri: String::new(),
            extension: "acme".to_string(),
            template_path: PathBuf::new(),
        };
        let mut index = ThemeHookIndex::default();
        for name in ["node", "block__system_branding_block", "block"] {
            index.add(Arc::new(hook(name)));
        }
        let found = |written: &str| {
            index
                .declarations(written)
                .iter()
                .map(|hook| hook.name().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(found("node__article__teaser"), ["node"]);
        assert_eq!(
            found("block__system_branding_block"),
            ["block__system_branding_block"]
        );
        assert_eq!(
            found("block__system_branding_block__x"),
            ["block__system_branding_block"]
        );
        assert!(found("acme__x").is_empty());
        assert!(found("__x").is_empty());
    }

    /// The watcher records the templates the Drupal walk would.
    #[test]
    fn a_watched_template_counts_where_the_walk_reads_templates() {
        let dir = tempfile::tempdir().unwrap();
        let web = dir.path().join("web");
        let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
        backend.mark_drupal_project();
        backend.note_drupal_web_root(&web);
        for (path, counts) in [
            ("modules/custom/acme/templates/acme-box.html.twig", true),
            (
                "modules/custom/acme/tests/templates/acme-box.html.twig",
                false,
            ),
            ("core/assets/vendor/x/acme-box.html.twig", false),
            ("sites/simpletest/1/templates/acme-box.html.twig", false),
            ("modules/custom/acme/templates/acme-box.twig", false),
        ] {
            assert_eq!(
                backend.note_drupal_template_change(&web.join(path), true),
                counts,
                "{path}"
            );
        }
        assert!(!backend.note_drupal_template_change(&dir.path().join("x.html.twig"), true));
    }

    /// A template is claimed by the hook it is the default template of, and
    /// in a theme by the hook it overrides or a suggestion of it; a module's
    /// file of the same name is no override.
    #[test]
    fn a_template_finds_the_hooks_that_claim_it() {
        let dir = tempfile::tempdir().unwrap();
        let web = dir.path().join("web");
        let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
        backend.mark_drupal_project();
        backend.note_drupal_web_root(&web);
        let extension = |path: &str, name: &str, is_theme| crate::drupal::hooks::Extension {
            dir: web.join(path),
            name: name.to_string(),
            is_theme,
        };
        backend.note_drupal_extensions(vec![
            extension("modules/custom/acme", "acme", false),
            extension("modules/custom/other", "other", false),
            extension("themes/custom/olivo", "olivo", true),
        ]);
        let hook = |name: &str, base_hook: Option<&str>, template_path: &str| ThemeHook {
            entry: ThemeHookEntry {
                name: Named {
                    name: name.to_string(),
                    start: 0,
                    end: 0,
                },
                variables: Vec::new(),
                render_element: None,
                base_hook: base_hook.map(str::to_string),
                template: None,
                path: None,
            },
            uri: String::new(),
            extension: "acme".to_string(),
            template_path: web.join(template_path),
        };
        let mut index = ThemeHookIndex::default();
        for hook in [
            hook(
                "acme_greeting",
                None,
                "modules/custom/acme/templates/acme-greeting.html.twig",
            ),
            hook(
                "acme_greeting__wide",
                Some("acme_greeting"),
                "modules/custom/acme/templates/acme-greeting--wide.html.twig",
            ),
        ] {
            index.add(Arc::new(hook));
        }
        *backend.drupal.theme_hook_index.write() = Some(Arc::new(index));

        let claims = |path: &str| {
            backend
                .drupal_template_theme_hooks(&web.join(path))
                .iter()
                .map(|hook| hook.name().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            claims("modules/custom/acme/templates/acme-greeting.html.twig"),
            ["acme_greeting"]
        );
        assert_eq!(
            claims("themes/custom/olivo/templates/content/acme-greeting.html.twig"),
            ["acme_greeting"]
        );
        assert_eq!(
            claims("themes/custom/olivo/templates/acme-greeting--compact.html.twig"),
            ["acme_greeting"]
        );
        assert_eq!(
            claims("themes/custom/olivo/templates/acme-greeting--wide.html.twig"),
            ["acme_greeting__wide"]
        );
        assert!(claims("modules/custom/other/templates/acme-greeting.html.twig").is_empty());
        assert!(claims("themes/custom/olivo/templates/page.html.twig").is_empty());
    }
}
