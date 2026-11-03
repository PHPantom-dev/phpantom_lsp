//! Static scan of Laravel's middleware aliases and groups.
//!
//! A route names its middleware by a short alias (`->middleware('auth')`)
//! or by a group (`'web'`), which the router maps to a class when the
//! application boots.  Every one of those maps is a literal in source:
//!
//! * the framework's defaults, in
//!   `Illuminate\Foundation\Configuration\Middleware::defaultAliases()` and
//!   `getMiddlewareGroups()`;
//! * the application's own, through the `Middleware` configurator
//!   `bootstrap/app.php` hands to `withMiddleware()` (`->alias([…])`,
//!   `->group()`, `->appendToGroup()`, …) on Laravel 11+, or an HTTP kernel's
//!   `$middlewareAliases` / `$routeMiddleware` / `$middlewareGroups` before
//!   that;
//! * a package's, through the router's `aliasMiddleware()` and
//!   `middlewareGroup()` in its service provider.
//!
//! This module reads all of them into one alias → class and group → members
//! table.  A registration whose name is not a literal makes the table
//! [incomplete](LaravelMiddlewareIndex::is_incomplete): some name exists
//! that the scan cannot know, so none can be told unknown.

use std::collections::HashMap;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use super::file_contributions::{Contribution, FileContributions};
use super::helpers::string_literal_at;
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;

/// The class holding the framework's default aliases and groups, and the
/// type of the configurator `withMiddleware()` passes its callback.
pub(crate) const MIDDLEWARE_CONFIGURATION_FQN: &str =
    "Illuminate\\Foundation\\Configuration\\Middleware";

/// The base class of an application's HTTP kernel.
const HTTP_KERNEL_FQN: &str = "Illuminate\\Foundation\\Http\\Kernel";

/// One `'alias' => Middleware::class` registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MiddlewareAliasEntry {
    pub name: String,
    /// FQN of the middleware class, when the registration names one.
    pub class: Option<String>,
    /// Byte offset of the alias literal's content.
    pub offset: u32,
}

/// One registration of a middleware group's members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MiddlewareGroupEntry {
    pub name: String,
    /// Each member as the group lists it: a class FQN or another alias,
    /// with any `:parameters`.
    pub members: Vec<String>,
    /// Whether the registration replaces the group (`group()`,
    /// `middlewareGroup()`, a kernel's `$middlewareGroups`) rather than
    /// adding to it (`appendToGroup()`, `web(append: …)`).
    pub defines: bool,
    /// Byte offset of the group name's literal content, or of the call
    /// that names the group implicitly (`->web(…)`).
    pub offset: u32,
}

/// Everything one file contributes to the middleware table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MiddlewareScan {
    pub aliases: Vec<MiddlewareAliasEntry>,
    pub groups: Vec<MiddlewareGroupEntry>,
    /// The file registers an alias or group under a name the scan cannot
    /// read.
    pub incomplete: bool,
    /// The file is the framework's own configurator, whose defaults the
    /// application's registrations override.
    pub framework: bool,
}

impl Contribution for MiddlewareScan {
    fn is_empty(&self) -> bool {
        self.aliases.is_empty() && self.groups.is_empty() && !self.incomplete
    }
}

/// Whether a file's content may register middleware, as the cheap
/// pre-filter ahead of [`scan_middleware`].
///
/// Every registration spells one of these: the router's methods, a
/// kernel's properties, and the configurator's class name, which a
/// `withMiddleware()` closure has to import or write out to type its
/// parameter (or the `withMiddleware` call itself, for an import that
/// groups it with others).  A route file that only uses middleware has
/// none of them.
pub(crate) fn may_register_middleware(content: &str) -> bool {
    const TOKENS: [&[u8]; 7] = [
        b"aliasMiddleware",
        b"iddlewareGroup",
        b"MiddlewareToGroup",
        b"$middlewareAliases",
        b"$routeMiddleware",
        b"Configuration\\Middleware",
        b"withMiddleware",
    ];
    let bytes = content.as_bytes();
    TOKENS
        .iter()
        .any(|token| memchr::memmem::find(bytes, token).is_some())
}

/// Extract every middleware alias and group registration from a file.
pub(crate) fn scan_middleware(content: &str) -> MiddlewareScan {
    if !may_register_middleware(content) {
        return MiddlewareScan::default();
    }

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let names = OwnedResolvedNames::from_resolved(&resolved);

    let mut scanner = Scanner {
        names: &names,
        content,
        configurator_vars: Vec::new(),
        scan: MiddlewareScan::default(),
    };
    scanner.walk(Node::Program(program));
    scanner.scan
}

struct Scanner<'s> {
    names: &'s OwnedResolvedNames,
    content: &'s str,
    /// The variables a closure receives the `Middleware` configurator as.
    configurator_vars: Vec<String>,
    scan: MiddlewareScan,
}

impl Scanner<'_> {
    /// Walk the tree with an explicit worklist: a deep method chain nests
    /// one node per link, which a recursive walk can overflow on.
    fn walk<'ast, 'arena>(&mut self, root: Node<'ast, 'arena>) {
        let mut pending: Vec<Node<'ast, 'arena>> = vec![root];
        while let Some(node) = pending.pop() {
            match node {
                Node::Closure(closure) => self.note_configurator(&closure.parameter_list),
                Node::ArrowFunction(arrow) => self.note_configurator(&arrow.parameter_list),
                Node::Class(class) => self.class(class),
                Node::MethodCall(call) => {
                    if let ClassLikeMemberSelector::Identifier(method) = &call.method {
                        self.call(
                            Some(call.object),
                            bytes_to_str(method.value),
                            method.span().start.offset,
                            &call.argument_list,
                        );
                    }
                }
                Node::NullSafeMethodCall(call) => {
                    if let ClassLikeMemberSelector::Identifier(method) = &call.method {
                        self.call(
                            Some(call.object),
                            bytes_to_str(method.value),
                            method.span().start.offset,
                            &call.argument_list,
                        );
                    }
                }
                Node::StaticMethodCall(call) => {
                    if let ClassLikeMemberSelector::Identifier(method) = &call.method {
                        self.call(
                            None,
                            bytes_to_str(method.value),
                            method.span().start.offset,
                            &call.argument_list,
                        );
                    }
                }
                _ => {}
            }
            node.visit_children(|child| pending.push(child));
        }
    }

    fn class_fqn(&self, ident: &Identifier<'_>) -> String {
        self.names
            .get(ident.span().start.offset)
            .unwrap_or_else(|| bytes_to_str(ident.value()))
            .trim_start_matches('\\')
            .to_string()
    }

    fn note_configurator(&mut self, parameters: &FunctionLikeParameterList<'_>) {
        for parameter in parameters.parameters.iter() {
            if let Some(Hint::Identifier(hint)) = &parameter.hint
                && self
                    .class_fqn(hint)
                    .eq_ignore_ascii_case(MIDDLEWARE_CONFIGURATION_FQN)
            {
                self.configurator_vars
                    .push(bytes_to_str(parameter.variable.name).to_string());
            }
        }
    }

    fn is_configurator(&self, object: &Expression<'_>) -> bool {
        matches!(
            object,
            Expression::Variable(Variable::Direct(variable))
                if self.configurator_vars.iter().any(|name| name.as_bytes() == variable.name)
        )
    }

    /// A kernel's middleware properties, and the framework's defaults.
    fn class(&mut self, class: &Class<'_>) {
        let fqn = self.class_fqn_of_declaration(class);
        if fqn.eq_ignore_ascii_case(MIDDLEWARE_CONFIGURATION_FQN) {
            self.scan.framework = true;
            self.framework_defaults(class);
            return;
        }
        if !self.extends_kernel(class) {
            return;
        }
        for member in class.members.iter() {
            let ClassLikeMember::Property(Property::Plain(property)) = member else {
                continue;
            };
            for item in property.items.iter() {
                let PropertyItem::Concrete(concrete) = item else {
                    continue;
                };
                match bytes_to_str(concrete.variable.name) {
                    "$middlewareAliases" | "$routeMiddleware" => self.alias_map(concrete.value),
                    "$middlewareGroups" => self.group_map(concrete.value),
                    _ => {}
                }
            }
        }
    }

    fn class_fqn_of_declaration(&self, class: &Class<'_>) -> String {
        self.names
            .get(class.name.span.start.offset)
            .map(|name| name.trim_start_matches('\\').to_string())
            .unwrap_or_else(|| bytes_to_str(class.name.value).to_string())
    }

    /// Whether a class extends the framework's HTTP kernel, or a class of
    /// the application's own named like one.
    fn extends_kernel(&self, class: &Class<'_>) -> bool {
        class.extends.as_ref().is_some_and(|extends| {
            extends.types.iter().any(|parent| {
                let fqn = self.class_fqn(parent);
                fqn.eq_ignore_ascii_case(HTTP_KERNEL_FQN)
                    || fqn
                        .rsplit('\\')
                        .next()
                        .is_some_and(|short| short.ends_with("Kernel"))
            })
        })
    }

    /// The `$aliases = […]` of `defaultAliases()` and the first
    /// `$middleware = […]` of `getMiddlewareGroups()`.
    fn framework_defaults(&mut self, class: &Class<'_>) {
        for member in class.members.iter() {
            let ClassLikeMember::Method(method) = member else {
                continue;
            };
            let MethodBody::Concrete(body) = &method.body else {
                continue;
            };
            let (variable, aliases): (&[u8], bool) = match bytes_to_str(method.name.value) {
                "defaultAliases" => (b"$aliases", true),
                "getMiddlewareGroups" => (b"$middleware", false),
                _ => continue,
            };
            let mut pending: Vec<Node<'_, '_>> = vec![Node::Block(body)];
            while let Some(node) = pending.pop() {
                if let Node::Assignment(assignment) = node
                    && assignment.operator.is_assign()
                    && let Expression::Variable(Variable::Direct(target)) = assignment.lhs
                    && target.name == variable
                    && crate::parser::array_literal_elements(assignment.rhs).is_some()
                {
                    if aliases {
                        self.alias_map(assignment.rhs);
                    } else {
                        self.group_map(assignment.rhs);
                    }
                    break;
                }
                node.visit_children(|child| pending.push(child));
            }
        }
    }

    /// A method call that may register middleware: the router's own
    /// registrations on any receiver (their names are distinctive enough),
    /// and the configurator's on the variable a `withMiddleware()` closure
    /// receives it as.
    fn call(
        &mut self,
        object: Option<&Expression<'_>>,
        method: &str,
        method_offset: u32,
        arguments: &ArgumentList<'_>,
    ) {
        let positional = |index: usize| {
            arguments
                .arguments
                .iter()
                .filter(|argument| matches!(argument, Argument::Positional(_)))
                .nth(index)
                .map(|argument| argument.value())
        };
        let named = |name: &str| {
            arguments
                .arguments
                .iter()
                .find_map(|argument| match argument {
                    Argument::Named(named) if bytes_to_str(named.name.value) == name => {
                        Some(named.value)
                    }
                    _ => None,
                })
        };
        let lower = method.to_ascii_lowercase();
        match lower.as_str() {
            "aliasmiddleware" => {
                let Some(name) = positional(0) else {
                    return;
                };
                let Some((alias, offset)) = string_literal_at(name, self.content) else {
                    self.scan.incomplete = true;
                    return;
                };
                let class = positional(1).and_then(|class| self.class_of(class));
                self.scan.aliases.push(MiddlewareAliasEntry {
                    name: alias.to_string(),
                    class,
                    offset,
                });
                return;
            }
            "middlewaregroup" => {
                self.group_call(positional(0), positional(1), true);
                return;
            }
            "pushmiddlewaretogroup" | "prependmiddlewaretogroup" => {
                self.group_call(positional(0), positional(1), false);
                return;
            }
            _ => {}
        }

        if !object.is_some_and(|object| self.is_configurator(object)) {
            return;
        }
        match lower.as_str() {
            "alias" => match positional(0).or_else(|| named("aliases")) {
                Some(map) if crate::parser::array_literal_elements(map).is_some() => {
                    self.alias_map(map)
                }
                Some(_) => self.scan.incomplete = true,
                None => {}
            },
            "group" => self.group_call(
                positional(0).or_else(|| named("group")),
                positional(1).or_else(|| named("middleware")),
                true,
            ),
            "appendtogroup" | "prependtogroup" => self.group_call(
                positional(0).or_else(|| named("group")),
                positional(1).or_else(|| named("middleware")),
                false,
            ),
            "web" | "api" => {
                let mut members = Vec::new();
                for (index, name) in [(0, "append"), (1, "prepend")] {
                    if let Some(value) = named(name).or_else(|| positional(index)) {
                        self.members_of(value, &mut members);
                    }
                }
                if !members.is_empty() {
                    self.scan.groups.push(MiddlewareGroupEntry {
                        name: lower,
                        members,
                        defines: false,
                        offset: method_offset,
                    });
                }
            }
            _ => {}
        }
    }

    fn group_call(
        &mut self,
        name: Option<&Expression<'_>>,
        members: Option<&Expression<'_>>,
        defines: bool,
    ) {
        let Some(name) = name else {
            return;
        };
        let Some((group, offset)) = string_literal_at(name, self.content) else {
            self.scan.incomplete = true;
            return;
        };
        let mut listed = Vec::new();
        if let Some(members) = members {
            self.members_of(members, &mut listed);
        }
        self.scan.groups.push(MiddlewareGroupEntry {
            name: group.to_string(),
            members: listed,
            defines,
            offset,
        });
    }

    /// An `['alias' => Middleware::class, …]` literal.
    fn alias_map(&mut self, map: &Expression<'_>) {
        let Some(elements) = crate::parser::array_literal_elements(map) else {
            return;
        };
        for element in elements.iter() {
            match element {
                ArrayElement::KeyValue(pair) => {
                    let Some((alias, offset)) = string_literal_at(pair.key, self.content) else {
                        self.scan.incomplete = true;
                        continue;
                    };
                    let class = self.class_of(pair.value);
                    self.scan.aliases.push(MiddlewareAliasEntry {
                        name: alias.to_string(),
                        class,
                        offset,
                    });
                }
                ArrayElement::Variadic(_) => self.scan.incomplete = true,
                ArrayElement::Value(_) | ArrayElement::Missing(_) => {}
            }
        }
    }

    /// A `['web' => [...], …]` literal.
    fn group_map(&mut self, map: &Expression<'_>) {
        let Some(elements) = crate::parser::array_literal_elements(map) else {
            return;
        };
        for element in elements.iter() {
            match element {
                ArrayElement::KeyValue(pair) => {
                    let Some((group, offset)) = string_literal_at(pair.key, self.content) else {
                        self.scan.incomplete = true;
                        continue;
                    };
                    let mut members = Vec::new();
                    self.members_of(pair.value, &mut members);
                    self.scan.groups.push(MiddlewareGroupEntry {
                        name: group.to_string(),
                        members,
                        defines: true,
                        offset,
                    });
                }
                ArrayElement::Variadic(_) => self.scan.incomplete = true,
                ArrayElement::Value(_) | ArrayElement::Missing(_) => {}
            }
        }
    }

    /// The class a registration maps an alias to.  The framework picks
    /// between two classes for `throttle`; the first stands for both.
    fn class_of(&self, expr: &Expression<'_>) -> Option<String> {
        match crate::parser::unwrap_parens(expr) {
            Expression::Conditional(conditional) => conditional
                .then
                .and_then(|then| self.class_of(then))
                .or_else(|| self.class_of(conditional.r#else)),
            expr => {
                if let Some(fqn) = self.names.class_fetch_fqn(expr) {
                    return Some(fqn);
                }
                let (text, _) = string_literal_at(expr, self.content)?;
                let fqn = text.replace("\\\\", "\\");
                let fqn = fqn.trim_start_matches('\\');
                (!fqn.is_empty()).then(|| fqn.to_string())
            }
        }
    }

    /// The members a group lists, through the `array_values(array_filter([
    /// …]))` and `$flag ? A::class : null` the framework builds its own
    /// defaults with.
    fn members_of(&self, expr: &Expression<'_>, members: &mut Vec<String>) {
        let expr = crate::parser::unwrap_parens(expr);
        if let Some(elements) = crate::parser::array_literal_elements(expr) {
            for element in elements.iter() {
                match element {
                    ArrayElement::KeyValue(pair) => self.members_of(pair.value, members),
                    ArrayElement::Value(value) => self.members_of(value.value, members),
                    ArrayElement::Variadic(_) | ArrayElement::Missing(_) => {}
                }
            }
            return;
        }
        match expr {
            Expression::Conditional(conditional) => {
                if let Some(then) = conditional.then {
                    self.members_of(then, members);
                }
                self.members_of(conditional.r#else, members);
            }
            Expression::Call(Call::Function(call)) => {
                if let Some(first) = call.argument_list.arguments.first() {
                    self.members_of(first.value(), members);
                }
            }
            _ => {
                if let Some(member) = self.class_of(expr)
                    && !members.contains(&member)
                {
                    members.push(member);
                }
            }
        }
    }
}

/// Where a middleware alias is registered, and the class it maps to.
#[derive(Clone, Debug)]
pub(crate) struct MiddlewareAlias {
    pub class: Option<String>,
    pub uri: String,
    pub offset: u32,
}

/// Where a middleware group is registered, and what it runs.
#[derive(Clone, Debug)]
pub(crate) struct MiddlewareGroup {
    pub members: Vec<String>,
    pub uri: String,
    pub offset: u32,
}

/// What a middleware name stands for.
pub(crate) enum MiddlewareTarget<'a> {
    Alias(&'a MiddlewareAlias),
    Group(&'a MiddlewareGroup),
}

/// Project-wide table of middleware aliases and groups.
///
/// `files` is the source of truth, one scan per contributing file; the
/// maps are derived from it by [`Self::rebuild`].
#[derive(Default)]
pub(crate) struct LaravelMiddlewareIndex {
    pub(crate) files: FileContributions<MiddlewareScan>,
    aliases: HashMap<String, MiddlewareAlias>,
    groups: HashMap<String, MiddlewareGroup>,
    incomplete: bool,
}

impl LaravelMiddlewareIndex {
    /// Rebuild the derived maps from the per-file scans.
    ///
    /// The application's registrations override the framework's defaults,
    /// the way `getMiddlewareAliases()` merges its custom aliases over
    /// them; among the application's own, the first file in URI order wins,
    /// since scan order is not boot order.  Additions to a group apply
    /// after every definition, whichever file makes them.
    pub(crate) fn rebuild(&mut self) {
        let mut aliases: HashMap<String, MiddlewareAlias> = HashMap::new();
        let mut groups: HashMap<String, MiddlewareGroup> = HashMap::new();
        let mut incomplete = false;

        let mut files = self.files.iter_sorted();
        // Application files first, so their definitions win.
        files.sort_by_key(|(_, scan)| scan.framework);

        for (uri, scan) in &files {
            incomplete |= scan.incomplete;
            for alias in &scan.aliases {
                aliases
                    .entry(alias.name.clone())
                    .or_insert_with(|| MiddlewareAlias {
                        class: alias.class.clone(),
                        uri: (*uri).clone(),
                        offset: alias.offset,
                    });
            }
            for group in scan.groups.iter().filter(|group| group.defines) {
                groups
                    .entry(group.name.clone())
                    .or_insert_with(|| MiddlewareGroup {
                        members: group.members.clone(),
                        uri: (*uri).clone(),
                        offset: group.offset,
                    });
            }
        }
        for (uri, scan) in &files {
            for group in scan.groups.iter().filter(|group| !group.defines) {
                let target = groups
                    .entry(group.name.clone())
                    .or_insert_with(|| MiddlewareGroup {
                        members: Vec::new(),
                        uri: (*uri).clone(),
                        offset: group.offset,
                    });
                for member in &group.members {
                    if !target.members.contains(member) {
                        target.members.push(member.clone());
                    }
                }
            }
        }

        self.aliases = aliases;
        self.groups = groups;
        self.incomplete = incomplete;
    }

    /// What `name` is registered as, an alias before a group.
    pub(crate) fn get(&self, name: &str) -> Option<MiddlewareTarget<'_>> {
        self.aliases
            .get(name)
            .map(MiddlewareTarget::Alias)
            .or_else(|| self.groups.get(name).map(MiddlewareTarget::Group))
    }

    /// Whether `name` is a registered alias or group.
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.aliases.contains_key(name) || self.groups.contains_key(name)
    }

    /// Every registered alias and group name, sorted.
    pub(crate) fn all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .aliases
            .keys()
            .chain(
                self.groups
                    .keys()
                    .filter(|name| !self.aliases.contains_key(*name)),
            )
            .cloned()
            .collect();
        names.sort_unstable();
        names
    }

    /// Whether nothing is registered at all, which means the scan found
    /// none of the framework's defaults either.
    pub(crate) fn is_empty(&self) -> bool {
        self.aliases.is_empty() && self.groups.is_empty()
    }

    /// Whether some registration names an alias or group the scan could
    /// not read, so an unknown name may still be a real one.
    pub(crate) fn is_incomplete(&self) -> bool {
        self.incomplete
    }
}

#[cfg(test)]
#[path = "middleware_tests.rs"]
mod tests;
