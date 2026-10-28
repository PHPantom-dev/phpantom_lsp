//! The test case a Pest test closure is bound to.
//!
//! Pest runs every `it()`/`test()`/`beforeEach()`/`afterEach()` closure
//! bound to an instance of a class it generates per test file, which
//! extends a PHPUnit test case and uses the traits bound to the file:
//! `PHPUnit\Framework\TestCase` and nothing else unless the `Pest.php` of
//! the test directory binds more to the file's directory with
//! `pest()->extends(…)->use(…)->in(…)` (or the older `uses(…)->in(…)`), or
//! the test file binds them to itself with the same chain without
//! `->in(…)`.  Pest's own functions cannot know which, so their
//! `@param-closure-this` names the base class, or `Pest\PendingCalls\TestCall`
//! in newer releases, which is what the call returns rather than what the
//! closure is bound to.
//!
//! [`Backend::file_context`] looks up the classes bound to the file, and
//! the function loader rewrites the tag on Pest's functions to the bound
//! test case (see [`Backend::bind_pest_closure_this`]), so every consumer
//! that reads the tag sees the class the closure actually runs as.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use crate::Backend;
use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::names::OwnedResolvedNames;
use crate::php_type::PhpType;
use crate::types::{ClassLikeKind, FunctionInfo};

/// The test case Pest binds a closure to when nothing else is configured.
const PHPUNIT_TEST_CASE: &str = "PHPUnit\\Framework\\TestCase";

/// What newer Pest releases declare as the closure's `$this`.
const PEST_TEST_CALL: &str = "Pest\\PendingCalls\\TestCall";

/// The Pest functions whose closure runs bound to the test case.
const PEST_TEST_FUNCTIONS: [&str; 4] = ["it", "test", "beforeEach", "afterEach"];

/// The `Pest.php` files of a workspace, found by walking up from the
/// test files that ask for them.
pub(crate) struct PestConfigs {
    /// URI of the workspace root, with a trailing `/`.  The walk stops
    /// there, and files outside it have no config.
    root_uri: String,
    /// Every directory looked at so far, keyed by its URI with a trailing
    /// `/`: its parsed `Pest.php`, or `None` when it has none.
    dirs: HashMap<String, Option<Arc<PestConfig>>>,
}

/// The `->in(…)` bindings declared in a test directory's `Pest.php`.
pub(crate) struct PestConfig {
    bindings: Vec<Binding>,
}

/// One `pest()->extends(…)->in(…)` chain.
struct Binding {
    /// The classes and traits, as FQNs, in the order they were written.
    uses: Vec<String>,
    targets: Vec<Target>,
}

/// A path passed to `->in(…)`.  Pest expands it with `glob()` and binds
/// the files it matches and everything under the directories it matches.
enum Target {
    Path(PathBuf),
    Glob(globset::GlobMatcher),
}

impl Target {
    fn new(path: PathBuf) -> Option<Self> {
        let text = path.to_str()?;
        if !text.contains(['*', '?', '[', '{']) {
            return Some(Target::Path(path));
        }
        let glob = globset::GlobBuilder::new(text)
            .literal_separator(true)
            .build()
            .ok()?;
        Some(Target::Glob(glob.compile_matcher()))
    }

    fn contains(&self, file: &Path) -> bool {
        match self {
            Target::Path(target) => file.starts_with(target),
            Target::Glob(glob) => file.ancestors().any(|dir| glob.is_match(dir)),
        }
    }
}

impl PestConfigs {
    /// The config of the nearest directory above `uri` with a `Pest.php`,
    /// `Ok(None)` when none has one, or `Err(dir)` with the URI of the
    /// nearest directory not looked at yet.
    fn lookup<'a>(&self, uri: &'a str) -> Result<Option<Arc<PestConfig>>, &'a str> {
        if !uri.starts_with(&self.root_uri) {
            return Ok(None);
        }
        let mut end = uri.len();
        while let Some(slash) = uri[..end].rfind('/') {
            let dir = &uri[..=slash];
            if dir.len() < self.root_uri.len() {
                break;
            }
            match self.dirs.get(dir) {
                Some(Some(config)) => return Ok(Some(Arc::clone(config))),
                Some(None) => {}
                None => return Err(dir),
            }
            end = slash;
        }
        Ok(None)
    }
}

impl PestConfig {
    /// Read the `Pest.php` in the directory at `dir_uri`, or `None` when
    /// there is none.  Pest only reads the one in its test directory
    /// (`tests/` unless `--test-directory` names another), and test files
    /// live under it, so the nearest one above a test file is it.
    fn load(backend: &Backend, dir_uri: &str) -> Option<Self> {
        let uri = format!("{dir_uri}Pest.php");
        let tests_dir = crate::util::uri_to_path(dir_uri)?;
        // Every directory above a file is probed and almost none has one,
        // so a missing file is the normal answer, not a read failure.
        if !backend.open_files.read().contains_key(&uri) && !tests_dir.join("Pest.php").is_file() {
            return None;
        }
        let content = backend.get_file_content(&uri)?;
        Some(Self {
            bindings: parse_bindings(&content, &tests_dir),
        })
    }

    /// The classes and traits bound to the file at `uri`, in the order
    /// Pest applies them.
    fn uses_for(&self, uri: &str, out: &mut Vec<String>) {
        if self.bindings.is_empty() {
            return;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return;
        };
        out.extend(
            self.bindings
                .iter()
                .filter(|binding| binding.targets.iter().any(|t| t.contains(&path)))
                .flat_map(|binding| binding.uses.iter().cloned()),
        );
    }
}

impl Backend {
    /// The classes and traits bound to the test file at `uri`, by its own
    /// `uses()` and by its test directory's `Pest.php`, or `None` when
    /// nothing is.
    pub(crate) fn pest_uses_for(&self, uri: &str) -> Option<Arc<[String]>> {
        let own = self
            .files
            .read()
            .get(uri)
            .and_then(|state| state.pest_uses.clone());
        let Some(config) = self.pest_config_for(uri) else {
            return own;
        };
        let mut uses: Vec<String> = own.as_deref().map(<[String]>::to_vec).unwrap_or_default();
        config.uses_for(uri, &mut uses);
        (!uses.is_empty()).then(|| uses.into())
    }

    /// The `Pest.php` of the test directory `uri` is in.
    fn pest_config_for(&self, uri: &str) -> Option<Arc<PestConfig>> {
        loop {
            let unchecked = {
                let configs = self.pest_configs.read();
                match configs.as_ref().map(|configs| configs.lookup(uri)) {
                    Some(Ok(config)) => return config,
                    Some(Err(dir)) => Some(dir),
                    None => None,
                }
            };
            let Some(dir) = unchecked else {
                let root = self.workspace.workspace_root.read().clone()?;
                let mut root_uri = crate::util::path_to_uri(&root);
                root_uri.push('/');
                self.pest_configs
                    .write()
                    .get_or_insert_with(|| PestConfigs {
                        root_uri,
                        dirs: HashMap::new(),
                    });
                continue;
            };
            let config = PestConfig::load(self, dir).map(Arc::new);
            if let Some(configs) = self.pest_configs.write().as_mut() {
                configs.dirs.insert(dir.to_string(), config);
            }
        }
    }

    /// Forget every `Pest.php` read so far, and which directories had
    /// none, so the next lookup reads them again.
    pub(crate) fn invalidate_pest_config(&self) {
        *self.pest_configs.write() = None;
    }

    /// The directory of the `Pest.php` at `uri` when a lookup has looked
    /// there, so a change to it on disk has a cached answer to forget.
    pub(crate) fn looked_up_pest_dir<'a>(&self, uri: &'a str) -> Option<&'a str> {
        let dir = uri.strip_suffix("Pest.php")?;
        if !dir.ends_with('/') {
            return None;
        }
        let configs = self.pest_configs.read();
        configs
            .as_ref()
            .is_some_and(|configs| configs.dirs.contains_key(dir))
            .then_some(dir)
    }

    /// Forget what the directory at `dir` was found to have, so the next
    /// lookup through it reads its `Pest.php` again.
    pub(crate) fn forget_pest_dir(&self, dir: &str) {
        if let Some(configs) = self.pest_configs.write().as_mut() {
            configs.dirs.remove(dir);
        }
    }

    /// Point the `@param-closure-this` of Pest's test functions at the
    /// test case the closure is bound to in this file: the class among
    /// `uses` (what is bound here), or PHPUnit's base test case when none
    /// is, intersected with the traits among `uses`.
    ///
    /// Only Pest's own declarations are rewritten, recognised by the
    /// global name and the class their tag names, so a project's own
    /// `test()` helper keeps whatever it declares.
    pub(crate) fn bind_pest_closure_this(&self, fi: &mut FunctionInfo, uses: Option<&[String]>) {
        if fi.namespace.is_some()
            || !PEST_TEST_FUNCTIONS
                .iter()
                .any(|name| fi.name.eq_ignore_ascii_case(name))
        {
            return;
        }
        let Some(index) = fi.parameters.iter().position(|param| {
            param
                .closure_this_type
                .as_ref()
                .and_then(|ty| ty.base_name())
                .is_some_and(|name| name == PHPUNIT_TEST_CASE || name == PEST_TEST_CALL)
        }) else {
            return;
        };
        let mut test_case = None;
        let mut traits = Vec::new();
        for name in uses.into_iter().flatten() {
            match self.find_or_load_class(name).map(|class| class.kind) {
                Some(ClassLikeKind::Class) if test_case.is_none() => {
                    test_case = Some(name.as_str())
                }
                Some(ClassLikeKind::Trait) => traits.push(PhpType::named(atom(name))),
                _ => {}
            }
        }
        let test_case = PhpType::named(atom(test_case.unwrap_or(PHPUNIT_TEST_CASE)));
        let this = if traits.is_empty() {
            test_case
        } else {
            traits.insert(0, test_case);
            PhpType::intersection(traits)
        };
        fi.parameters.make_mut()[index].closure_this_type = Some(this);
    }
}

/// The classes and traits a test file binds to itself: every
/// `pest()->extends(…)` or `uses(…)` chain written as a statement of it
/// without `->in(…)`.
pub(crate) fn file_uses<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
    names: &OwnedResolvedNames,
) -> Option<Arc<[String]>> {
    let mut uses = Vec::new();
    collect_chains(statements, names, &mut |chain| {
        if !chain.has_in {
            uses.extend(chain.uses);
        }
    });
    (!uses.is_empty()).then(|| uses.into())
}

/// The bindings of every `pest()->…->in(…)` and `uses(…)->in(…)` chain
/// written as a statement of `content`.  Relative targets are resolved
/// against `tests_dir`, the directory `Pest.php` lives in.
fn parse_bindings(content: &str, tests_dir: &Path) -> Vec<Binding> {
    let arena = LocalArena::new();
    let program = parse_file_content(&arena, FileId::new(b"Pest.php"), content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let names = OwnedResolvedNames::from_resolved(&resolved);
    let mut bindings = Vec::new();
    // A chain without `->in(…)` configures the file it is written in,
    // which for `Pest.php` is no test at all.
    collect_chains(program.statements.iter(), &names, &mut |chain| {
        let targets: Vec<Target> = chain
            .targets
            .iter()
            .filter_map(|arg| target(arg, tests_dir))
            .collect();
        if !targets.is_empty() {
            bindings.push(Binding {
                uses: chain.uses,
                targets,
            });
        }
    });
    bindings
}

/// One `pest()->…` or `uses(…)->…` call chain that binds something.
struct Chain<'e, 'a> {
    /// The classes and traits, as FQNs, in the order they were written.
    uses: Vec<String>,
    /// The arguments of its `->in(…)` calls.
    targets: Vec<&'e Expression<'a>>,
    /// Whether it calls `->in(…)` at all.
    has_in: bool,
}

fn collect_chains<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
    names: &OwnedResolvedNames,
    on_chain: &mut impl FnMut(Chain<'a, 'a>),
) {
    for statement in statements {
        match statement {
            Statement::Expression(expr) => {
                if let Some(chain) = read_chain(expr.expression, names) {
                    on_chain(chain);
                }
            }
            Statement::Namespace(ns) => {
                collect_chains(ns.statements().iter(), names, on_chain);
            }
            _ => {}
        }
    }
}

/// Read one call chain rooted at `pest()` or `uses()`.  `None` when the
/// chain has another root or binds no class or trait.
fn read_chain<'e, 'a>(
    expr: &'e Expression<'a>,
    names: &OwnedResolvedNames,
) -> Option<Chain<'e, 'a>> {
    let mut uses = Vec::new();
    let mut targets = Vec::new();
    let mut has_in = false;
    let mut current = expr;
    loop {
        match current {
            Expression::Call(Call::Method(call)) => {
                let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                    return None;
                };
                let method = bytes_to_str(method.value);
                let args = call.argument_list.arguments.iter().map(|arg| arg.value());
                if method.eq_ignore_ascii_case("in") {
                    has_in = true;
                    targets.extend(args);
                } else if ["extends", "extend", "uses", "use"]
                    .iter()
                    .any(|name| method.eq_ignore_ascii_case(name))
                {
                    // Walking outward-in, so a later call's classes go first
                    // and the whole list is reversed at the end.
                    let mut classes: Vec<String> =
                        args.filter_map(|arg| class_name(arg, names)).collect();
                    classes.reverse();
                    uses.extend(classes);
                }
                current = call.object;
            }
            Expression::Call(Call::Function(call)) => {
                let Expression::Identifier(function) = call.function else {
                    return None;
                };
                let function = bytes_to_str(function.value()).trim_start_matches('\\');
                if function.eq_ignore_ascii_case("uses") {
                    let mut classes: Vec<String> = call
                        .argument_list
                        .arguments
                        .iter()
                        .filter_map(|arg| class_name(arg.value(), names))
                        .collect();
                    classes.reverse();
                    uses.extend(classes);
                } else if !function.eq_ignore_ascii_case("pest") {
                    return None;
                }
                break;
            }
            _ => return None,
        }
    }
    if uses.is_empty() {
        return None;
    }
    uses.reverse();
    Some(Chain {
        uses,
        targets,
        has_in,
    })
}

/// The FQN a `Foo::class` or `'Foo'` argument names.
fn class_name(expr: &Expression<'_>, names: &OwnedResolvedNames) -> Option<String> {
    if let Some(class) = crate::parser::class_fetch_identifier(expr) {
        return names.class_fqn(class);
    }
    let name = string_value(expr)?;
    Some(name.trim_start_matches('\\').to_string())
}

/// The path an `->in(…)` argument names: a string relative to the tests
/// directory (or absolute), or `__DIR__ . '/…'`.
fn target(expr: &Expression<'_>, tests_dir: &Path) -> Option<Target> {
    if let Expression::Binary(binary) = expr
        && binary.operator.is_concatenation()
        && matches!(
            binary.lhs,
            Expression::MagicConstant(MagicConstant::Directory(_))
        )
    {
        let rest = string_value(binary.rhs)?;
        return Target::new(tests_dir.join(rest.trim_start_matches('/')));
    }
    let path = string_value(expr)?;
    Target::new(tests_dir.join(path.trim_end_matches('/')))
}

fn string_value<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Literal(Literal::String(s)) => literal_bytes_to_str(s.value?),
        _ => None,
    }
}
