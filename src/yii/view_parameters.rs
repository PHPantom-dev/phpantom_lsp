//! Yii view parameters: the names a render call passes its view, read
//! against the `@var` block the template opens with.
//!
//! ```php
//! // Action.php
//! return $this->viewRenderer->render(__DIR__ . '/template', ['form' => $form]);
//!
//! // template.php
//! /**
//!  * @var WebView $this
//!  * @var Form $form
//!  */
//! ```
//!
//! A template declares the variables it expects in the docblocks before
//! its first statement, after the opening tag, `declare`, and `use`
//! statements.  That block is the template's signature: a render call's
//! keys complete from it, navigate to it, and are checked against it (see
//! `diagnostics/yii_view_parameters.rs`).  A parameter's key names the view
//! as well as the parameter (see [`parameter_key`]), since a name only
//! means something to the template it is passed to.
//!
//! A view is handed more than its render passes: the parameters the view
//! itself holds, set by `WebView::setParameters()` in the DI configuration
//! or by code calling `setParameter()`, and the common parameters the
//! renderer's injections add (`CsrfViewInjection` gives `$csrf`).  Those
//! are [`shared`](Backend::yii_view_shared_parameters) by every view, and
//! the code that sets them writes a key with no view.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::PhpType;
use crate::symbol_map::{StringKeyKind, SymbolKind};
use crate::text_position::LineIndex;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// The interface whose `getCommonParameters()` adds parameters to every
/// view a renderer renders.
const COMMON_PARAMETERS_INJECTION: &str =
    "Yiisoft\\Yii\\View\\Renderer\\CommonParametersInjectionInterface";

/// The parameter `CsrfViewInjection` adds unless it is given another name
/// with `withParameterName()`, which is read as a shared parameter of its
/// own.
const CSRF_PARAMETER: &str = "csrf";

/// The key of a [`StringKeyKind::YiiViewParameter`] span: the parameter
/// `name` passed to the template the view key `view` names.  A parameter
/// every view shares has an empty view.
pub(crate) fn parameter_key(view: &str, name: &str) -> String {
    format!("{view}#{name}")
}

/// The view key and the parameter name of a parameter's key.
pub(crate) fn split_parameter_key(key: &str) -> (&str, &str) {
    key.rsplit_once('#').unwrap_or(("", key))
}

/// Whether `method` sets a parameter every view the view renders shares,
/// by the name its first argument gives: `setParameter()`,
/// `addToParameter()`, and `CsrfViewInjection::withParameterName()`.
pub(crate) fn is_shared_parameter_method(method: &str) -> bool {
    ["setParameter", "addToParameter", "withParameterName"]
        .iter()
        .any(|name| method.eq_ignore_ascii_case(name))
}

/// Whether `method` sets the parameters every view shares by the keys of
/// the array its first argument is.
pub(crate) fn is_shared_parameters_method(method: &str) -> bool {
    method.eq_ignore_ascii_case("setParameters")
}

/// A variable a template declares in its `@var` block.
#[derive(Debug, Clone)]
pub(crate) struct DeclaredParameter {
    pub(crate) name: String,
    /// The declared type, its class names qualified by the template's own
    /// imports and namespace.
    pub(crate) ty: PhpType,
    /// The byte range of `$name` in the template.
    pub(crate) start: u32,
    pub(crate) end: u32,
}

/// What a template's source says about the parameters it takes.
#[derive(Debug, Default)]
pub(crate) struct ViewTemplate {
    /// The template file.
    pub(crate) path: PathBuf,
    /// The variables its `@var` block declares, `$this` left out, in the
    /// order they are written.
    pub(crate) declared: Vec<DeclaredParameter>,
    /// The variables the template reads, or `None` when it reads its scope
    /// wholesale (`get_defined_vars()`, `extract()`, `compact()`, a
    /// variable variable, or an `include` that shares it), so that any name
    /// could be one it uses.
    reads: Option<HashSet<String>>,
}

impl ViewTemplate {
    /// The declaration of the parameter `name`.
    pub(crate) fn declared(&self, name: &str) -> Option<&DeclaredParameter> {
        self.declared.iter().find(|declared| declared.name == name)
    }

    /// Whether the template has a use for a parameter named `name`: it
    /// declares it, or reads a variable of that name.  `None` when it reads
    /// its scope in a way that any name could reach.
    pub(crate) fn accepts(&self, name: &str) -> Option<bool> {
        if self.declared(name).is_some() {
            return Some(true);
        }
        Some(self.reads.as_ref()?.contains(name))
    }
}

/// Read the template `content`, at `path`, for its `@var` block and the
/// variables it reads.  `None` when it declares nothing.
fn read_template(path: &Path, content: &str) -> Option<ViewTemplate> {
    crate::parser::with_parsed_program(content, "yii_view_template", |program, content| {
        let mut statements: Vec<&Statement<'_>> = Vec::new();
        for statement in program.statements.iter() {
            match statement {
                Statement::Namespace(namespace) => statements.extend(namespace.statements().iter()),
                statement => statements.push(statement),
            }
        }
        // The block ends at the first statement that does something.
        let first = statements.iter().copied().find(|statement| {
            !matches!(
                statement,
                Statement::OpeningTag(_)
                    | Statement::Declare(_)
                    | Statement::Use(_)
                    | Statement::Noop(_)
            )
        });
        let end = first.map_or(content.len() as u32, |statement| {
            statement.span().start.offset
        });
        // A docblock right above an assignment describes what it assigns.
        let assigned = first.and_then(assigned_variable);
        let last_docblock = program
            .trivia
            .iter()
            .filter(|trivia| {
                trivia.kind == TriviaKind::DocBlockComment && trivia.span.end.offset <= end
            })
            .map(|trivia| trivia.span.start.offset)
            .max();

        let mut use_map = std::collections::HashMap::new();
        Backend::extract_use_statements_from_statements(
            program.statements.iter(),
            &mut use_map,
            None,
        );
        let namespace = Backend::extract_namespace_from_statements(program.statements.iter());

        let mut declared: Vec<DeclaredParameter> = Vec::new();
        for trivia in program.trivia.iter() {
            if trivia.kind != TriviaKind::DocBlockComment || trivia.span.end.offset > end {
                continue;
            }
            let start = trivia.span.start.offset;
            let text = bytes_to_str(trivia.value);
            if !text.contains("var") {
                continue;
            }
            let Some(info) = crate::docblock::parse_docblock_for_tags(text) else {
                continue;
            };
            for tag in info.tags_by_kind_vendor_first(crate::docblock::TagKind::Var) {
                let (Some(variable), Some(type_text)) = (tag.variable(), tag.type_text()) else {
                    continue;
                };
                let name = variable.trim_start_matches('$');
                if name.is_empty()
                    || name == "this"
                    || declared.iter().any(|declared| declared.name == name)
                    || (Some(start) == last_docblock && assigned == Some(name))
                {
                    continue;
                }
                let Some(ty) = crate::docblock::sanitise_and_parse_docblock_type(&type_text) else {
                    continue;
                };
                let from = tag.span.start.offset as usize;
                let spelled = format!("${name}");
                let Some(at) = text.get(from..).and_then(|rest| rest.find(&spelled)) else {
                    continue;
                };
                let name_start = start + (from + at) as u32;
                declared.push(DeclaredParameter {
                    name: name.to_string(),
                    ty: crate::names::qualify_written_type_names(
                        &ty,
                        namespace.as_deref(),
                        &use_map,
                    ),
                    start: name_start,
                    end: name_start + spelled.len() as u32,
                });
            }
        }
        if declared.is_empty() {
            return None;
        }
        declared.sort_by_key(|declared| declared.start);

        let mut reads = Reads::default();
        for statement in program.statements.iter() {
            mago_syntax::walker::Walker::walk_statement(&ReadWalker, statement, &mut reads);
        }
        Some(ViewTemplate {
            path: path.to_path_buf(),
            declared,
            reads: (!reads.open).then_some(reads.names),
        })
    })
}

/// The variable `statement` assigns, when it is an assignment to one.
fn assigned_variable<'a>(statement: &Statement<'a>) -> Option<&'a str> {
    let Statement::Expression(statement) = statement else {
        return None;
    };
    let Expression::Assignment(assignment) = statement.expression else {
        return None;
    };
    let Expression::Variable(Variable::Direct(variable)) = assignment.lhs else {
        return None;
    };
    Some(bytes_to_str(variable.name).trim_start_matches('$'))
}

/// The variables a template reads, and whether it reads its scope in a way
/// that reaches any name.
#[derive(Default)]
struct Reads {
    names: HashSet<String>,
    open: bool,
}

struct ReadWalker;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Reads> for ReadWalker {
    fn walk_in_variable(&self, node: &'ast Variable<'arena>, reads: &mut Reads) {
        match node {
            Variable::Direct(variable) => {
                let name = bytes_to_str(variable.name).trim_start_matches('$');
                if !reads.names.contains(name) {
                    reads.names.insert(name.to_string());
                }
            }
            Variable::Indirect(_) | Variable::Nested(_) => reads.open = true,
        }
    }

    fn walk_in_function_call(&self, node: &'ast FunctionCall<'arena>, reads: &mut Reads) {
        let Expression::Identifier(identifier) = node.function else {
            return;
        };
        let name = crate::util::strip_fqn_prefix(bytes_to_str(identifier.value()));
        if ["get_defined_vars", "extract", "compact"]
            .iter()
            .any(|scope_reader| name.eq_ignore_ascii_case(scope_reader))
        {
            reads.open = true;
        }
    }

    fn walk_in_construct(&self, node: &'ast Construct<'arena>, reads: &mut Reads) {
        if matches!(
            node,
            Construct::Include(_)
                | Construct::IncludeOnce(_)
                | Construct::Require(_)
                | Construct::RequireOnce(_)
                | Construct::Eval(_)
        ) {
            reads.open = true;
        }
    }
}

/// The keys of the arrays `getCommonParameters()` returns in the class
/// `class_name` the source `content` declares, or `None` when one of them
/// cannot be read.
fn common_parameter_names(content: &str, class_name: &str) -> Option<Vec<String>> {
    crate::parser::with_parsed_program(content, "yii_view_injection", |program, content| {
        let mut statements: Vec<&Statement<'_>> = Vec::new();
        for statement in program.statements.iter() {
            match statement {
                Statement::Namespace(namespace) => statements.extend(namespace.statements().iter()),
                statement => statements.push(statement),
            }
        }
        let class = statements
            .into_iter()
            .find_map(|statement| match statement {
                Statement::Class(class) if bytes_to_str(class.name.value) == class_name => {
                    Some(class)
                }
                _ => None,
            })?;
        let method = class.members.iter().find_map(|member| match member {
            ClassLikeMember::Method(method)
                if bytes_to_str(method.name.value).eq_ignore_ascii_case("getCommonParameters") =>
            {
                Some(method)
            }
            _ => None,
        })?;
        let MethodBody::Concrete(body) = &method.body else {
            return None;
        };
        let mut returns = Vec::new();
        crate::return_collection::collect_returns(body.statements.iter(), &mut returns);
        let mut names = Vec::new();
        for (value, ..) in returns {
            let elements = crate::parser::array_literal_elements(value?)?;
            for element in elements.iter() {
                let ArrayElement::KeyValue(entry) = element else {
                    return None;
                };
                let (key, _, _) = resolved_string_literal(entry.key, content)?;
                names.push(key.to_string());
            }
        }
        Some(names)
    })
}

impl Backend {
    /// What the template a view key names declares, when it is a PHP
    /// template that opens with a `@var` block.
    pub(crate) fn yii_view_template(&self, view: &str) -> Option<ViewTemplate> {
        let path = self.yii_view_file(view)?;
        if path.extension().is_none_or(|extension| extension != "php") {
            return None;
        }
        let uri = crate::util::path_to_url(&path)?;
        let content = self.get_file_content_arc(uri.as_str())?;
        read_template(&path, &content)
    }

    /// The parameters the template a view key names declares, each with its
    /// declared type, for completion.
    pub(crate) fn yii_view_parameter_names(&self, view: &str) -> Vec<(String, String)> {
        self.yii_view_template(view)
            .map(|template| {
                template
                    .declared
                    .into_iter()
                    .map(|declared| (declared.name, declared.ty.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Go-to-definition on a parameter key: the `@var` that declares it in
    /// the template.
    pub(crate) fn yii_view_parameter_locations(&self, key: &str) -> Vec<Location> {
        let (view, name) = split_parameter_key(key);
        let Some(template) = self.yii_view_template(view) else {
            return Vec::new();
        };
        let Some(declared) = template.declared(name) else {
            return Vec::new();
        };
        let Some(uri) = crate::util::path_to_url(&template.path) else {
            return Vec::new();
        };
        let Some(content) = self.get_file_content_arc(uri.as_str()) else {
            return Vec::new();
        };
        let lines = LineIndex::new(&content);
        vec![Location {
            uri,
            range: Range {
                start: lines.position(declared.start as usize),
                end: lines.position(declared.end as usize),
            },
        }]
    }

    /// The type the template a parameter key names declares the parameter
    /// with, for hover.
    pub(crate) fn yii_view_parameter_type(&self, key: &str) -> Option<PhpType> {
        let (view, name) = split_parameter_key(key);
        Some(self.yii_view_template(view)?.declared(name)?.ty.clone())
    }

    /// The names every view is handed besides what its render passes: the
    /// parameters the view holds, and the common parameters the renderer's
    /// injections add.  `None` when some of them cannot be read, so that any
    /// name could be one of them.
    ///
    /// The parameters are the ones code and the project's DI configuration
    /// set (see [`is_shared_parameter_method`]) and the ones under
    /// `params['yiisoft/view']['parameters']`, which the view's own package
    /// sets.  The injections are read from every class of the project that
    /// is one, whether the renderer is given it or not, and
    /// `CsrfViewInjection`'s from its default name and every name a
    /// `withParameterName()` gives.
    pub(crate) fn yii_view_shared_parameters(
        &self,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<HashSet<String>> {
        let mut names: HashSet<String> = HashSet::from([CSRF_PARAMETER.to_string()]);
        let mut unread = false;
        self.visit_declaring_symbol_maps(|_, map| {
            unread |= map.sets_unread_yii_view_parameters;
            for span in &map.spans {
                if let SymbolKind::StringKey {
                    kind: StringKeyKind::YiiViewParameter,
                    key,
                    is_write: true,
                    ..
                } = &span.kind
                {
                    let (view, name) = split_parameter_key(key);
                    if view.is_empty() {
                        names.insert(name.to_string());
                    }
                }
            }
        });
        if unread {
            return None;
        }
        let params = self.yii_params();
        if let Some(parameters) =
            params.get(&super::params::param_key(&["yiisoft/view", "parameters"]))
        {
            names.extend(parameters.child_names().map(str::to_string));
        }
        let injections = self.find_implementors(
            "CommonParametersInjectionInterface",
            COMMON_PARAMETERS_INJECTION,
            class_loader,
            false,
            false,
            true,
        );
        for injection in injections {
            let fqn = injection.fqn();
            let uri = self
                .symbols
                .fqn_uri_index
                .read()
                .get(fqn.trim_start_matches('\\'))
                .cloned()?;
            let content = self.get_file_content_arc(&uri)?;
            names.extend(common_parameter_names(&content, &injection.name)?);
        }
        Some(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(content: &str) -> Option<ViewTemplate> {
        read_template(Path::new("/app/views/template.php"), content)
    }

    fn declared(content: &str) -> Vec<(String, String)> {
        template(content)
            .map(|template| {
                template
                    .declared
                    .iter()
                    .map(|declared| {
                        assert_eq!(
                            &content[declared.start as usize..declared.end as usize],
                            format!("${}", declared.name)
                        );
                        (declared.name.clone(), declared.ty.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn the_block_after_the_imports_declares_the_parameters() {
        let php = r#"<?php

declare(strict_types=1);

use App\Blog\Post;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var Post $post
 * @var list<Post> $related
 * @var string $title
 */

$this->setTitle($title);
?>
<h1><?= $post->title ?></h1>
"#;
        assert_eq!(
            declared(php),
            [
                ("post".to_string(), "App\\Blog\\Post".to_string()),
                ("related".to_string(), "list<App\\Blog\\Post>".to_string()),
                ("title".to_string(), "string".to_string()),
            ]
        );
    }

    #[test]
    fn a_docblock_after_the_first_statement_declares_nothing() {
        let php = "<?php\n$this->setTitle('x');\n/** @var string $title */\necho $title;\n";
        assert!(declared(php).is_empty());
    }

    #[test]
    fn a_docblock_above_an_assignment_describes_what_it_assigns() {
        let php = "<?php\n/** @var string $title */\n$title = 'x';\n";
        assert!(declared(php).is_empty());
        let php = "<?php\n/** @var int $id */\n/** @var string $title */\n$title = 'x';\n";
        assert_eq!(declared(php), [("id".to_string(), "int".to_string())]);
    }

    #[test]
    fn a_namespaced_template_qualifies_by_its_namespace() {
        let php = "<?php\nnamespace App\\Views;\n/** @var Card $card */\necho $card;\n";
        assert_eq!(
            declared(php),
            [("card".to_string(), "App\\Views\\Card".to_string())]
        );
    }

    #[test]
    fn a_template_accepts_what_it_declares_and_what_it_reads() {
        let php = "<?php\n/** @var string $title */\necho $title . $subtitle;\n";
        let read = template(php).expect("declares");
        assert_eq!(read.accepts("title"), Some(true));
        assert_eq!(read.accepts("subtitle"), Some(true));
        assert_eq!(read.accepts("other"), Some(false));

        let php =
            "<?php\n/** @var string $title */\necho $this->render('x', get_defined_vars());\n";
        assert_eq!(template(php).expect("declares").accepts("other"), None);
    }

    #[test]
    fn an_injection_names_the_keys_it_returns() {
        let php = r#"<?php
namespace App;
final class Injection {
    public function getCommonParameters(): array {
        return ['user' => $this->user, 'locale' => 'en'];
    }
}
"#;
        assert_eq!(
            common_parameter_names(php, "Injection"),
            Some(vec!["user".to_string(), "locale".to_string()])
        );
        let php = "<?php\nfinal class Injection {\n    public function getCommonParameters(): array {\n        return $this->parameters;\n    }\n}\n";
        assert_eq!(common_parameter_names(php, "Injection"), None);
    }

    #[test]
    fn a_parameter_key_names_its_view() {
        let key = parameter_key("@views/blog/post", "post");
        assert_eq!(split_parameter_key(&key), ("@views/blog/post", "post"));
        assert_eq!(
            split_parameter_key(&parameter_key("", "csrf")),
            ("", "csrf")
        );
    }
}
