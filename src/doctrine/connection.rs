//! The database connection a Doctrine project's queries run on.
//!
//! How a value comes back from the database depends on the driver: MySQL
//! returns `SUM()` of integers as a string, SQLite as an integer, and
//! PostgreSQL as either.  The driver, and the PDO options that make a PDO
//! driver return strings, are read from where the project configures its
//! connection:
//!
//! - the array `DriverManager::getConnection()` is given, by its `driver`,
//!   `url`, or `driverClass`, and its `driverOptions`;
//! - the `dbal` section of a Symfony project's Doctrine configuration in
//!   YAML, by the same keys (`driver_class` and `options` in YAML), with an
//!   `%env(…)%` URL read from the project's dotenv files.
//!
//! A project whose configurations name more than one driver, or one that is
//! not read, has no known driver: which connection a query runs on is not
//! known.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use mago_syntax::cst::*;
use parking_lot::RwLock;

use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;

/// The DBAL class whose `getConnection()` builds a connection.
const DRIVER_MANAGER: &str = "Doctrine\\DBAL\\DriverManager";

/// A PDO attribute a connection's options set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Attribute {
    /// `PDO::ATTR_STRINGIFY_FETCHES`.
    StringifyFetches,
    /// `PDO::ATTR_EMULATE_PREPARES`.
    EmulatePrepares,
    /// Any other attribute, which leaves fetching alone.
    Other,
}

impl Attribute {
    fn of_value(value: i64) -> Self {
        match value {
            17 => Attribute::StringifyFetches,
            20 => Attribute::EmulatePrepares,
            _ => Attribute::Other,
        }
    }

    /// The attribute the `PDO` constant `name` holds, `None` for one that
    /// may be any.
    fn of_constant(name: &str) -> Option<Self> {
        match name {
            "ATTR_STRINGIFY_FETCHES" => Some(Attribute::StringifyFetches),
            "ATTR_EMULATE_PREPARES" => Some(Attribute::EmulatePrepares),
            _ if name.starts_with("ATTR_") || name.starts_with("MYSQL_ATTR_") => {
                Some(Attribute::Other)
            }
            _ => None,
        }
    }
}

impl Connection {
    /// Set `attribute` to `value`; an attribute that is not known may be
    /// either of those fetching depends on.
    fn set(&mut self, attribute: Option<Attribute>, value: Option<bool>) {
        match attribute {
            Some(Attribute::StringifyFetches) => self.stringify_fetches = value,
            Some(Attribute::EmulatePrepares) => self.emulate_prepares = value,
            Some(Attribute::Other) => {}
            None => {
                self.stringify_fetches = None;
                self.emulate_prepares = None;
            }
        }
    }
}

/// A driver whose fetching is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Driver {
    Mysqli,
    PdoMysql,
    Pgsql,
    PdoPgsql,
    Sqlite3,
    PdoSqlite,
}

impl Driver {
    pub(crate) fn is_mysql(self) -> bool {
        matches!(self, Driver::Mysqli | Driver::PdoMysql)
    }

    pub(crate) fn is_pgsql(self) -> bool {
        matches!(self, Driver::Pgsql | Driver::PdoPgsql)
    }

    pub(crate) fn is_sqlite(self) -> bool {
        matches!(self, Driver::Sqlite3 | Driver::PdoSqlite)
    }
}

/// One configured connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Connection {
    /// The driver, or `None` for one whose fetching is not known or a
    /// configuration that is not read.
    pub(crate) driver: Option<Driver>,
    /// `PDO::ATTR_STRINGIFY_FETCHES`, `None` when not known.
    pub(crate) stringify_fetches: Option<bool>,
    /// `PDO::ATTR_EMULATE_PREPARES`, `None` when not known.
    pub(crate) emulate_prepares: Option<bool>,
}

impl Connection {
    fn new(driver: Option<Driver>) -> Self {
        Self {
            driver,
            stringify_fetches: Some(false),
            emulate_prepares: Some(true),
        }
    }

    /// A connection whose configuration is not read.
    fn unknown() -> Self {
        Self::new(None)
    }
}

/// The connections each file configures, keyed by URI.
pub(crate) type ConnectionIndex = Arc<RwLock<HashMap<String, Vec<Connection>>>>;

pub(crate) fn new_connection_index() -> ConnectionIndex {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Record the connections `uri` configures.  Returns whether that changed
/// what the index holds.
pub(crate) fn record(index: &ConnectionIndex, uri: &str, connections: Vec<Connection>) -> bool {
    if connections.is_empty() {
        if !index.read().contains_key(uri) {
            return false;
        }
        return index.write().remove(uri).is_some();
    }
    let mut index = index.write();
    if index.get(uri) == Some(&connections) {
        return false;
    }
    index.insert(uri.to_string(), connections);
    true
}

/// The connection the project's queries run on: the one every configured
/// connection agrees on.  An option the connections disagree on is not
/// known.
pub(crate) fn project_connection(index: &ConnectionIndex) -> Option<Connection> {
    let index = index.read();
    let mut connections = index.values().flatten();
    let mut project = *connections.next()?;
    project.driver?;
    for connection in connections {
        if connection.driver != project.driver {
            return None;
        }
        if connection.stringify_fetches != project.stringify_fetches {
            project.stringify_fetches = None;
        }
        if connection.emulate_prepares != project.emulate_prepares {
            project.emulate_prepares = None;
        }
    }
    Some(project)
}

/// The driver a DBAL driver name names.
fn driver_named(name: &str) -> Option<Driver> {
    Some(match name.to_ascii_lowercase().as_str() {
        "mysqli" => Driver::Mysqli,
        "pdo_mysql" => Driver::PdoMysql,
        "pgsql" => Driver::Pgsql,
        "pdo_pgsql" => Driver::PdoPgsql,
        "sqlite3" => Driver::Sqlite3,
        "pdo_sqlite" => Driver::PdoSqlite,
        _ => return None,
    })
}

/// The driver a DBAL driver class is.
fn driver_of_class(class: &str) -> Option<Driver> {
    let class = class.trim_start_matches('\\');
    let suffix = class.strip_prefix("Doctrine\\DBAL\\Driver\\")?;
    Some(match suffix {
        "Mysqli\\Driver" => Driver::Mysqli,
        "PDO\\MySQL\\Driver" => Driver::PdoMysql,
        "PgSQL\\Driver" => Driver::Pgsql,
        "PDO\\PgSQL\\Driver" => Driver::PdoPgsql,
        "SQLite3\\Driver" => Driver::Sqlite3,
        "PDO\\SQLite\\Driver" => Driver::PdoSqlite,
        _ => return None,
    })
}

/// The driver a database URL's scheme names, the way DBAL's DSN parser
/// reads it: one of its aliases, or a driver name with `-` for `_`.
fn driver_of_url(url: &str) -> Option<Driver> {
    let (scheme, _) = url.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    match scheme.as_str() {
        "mysql" | "mysql2" => Some(Driver::PdoMysql),
        "postgres" | "postgresql" | "pgsql" => Some(Driver::PdoPgsql),
        "sqlite" | "sqlite3" => Some(Driver::PdoSqlite),
        other => driver_named(&other.replace('-', "_")),
    }
}

// ─── DriverManager::getConnection() ─────────────────────────────────────────

/// Whether `content` may call `DriverManager::getConnection()`, which is
/// all [`php_connections`] reads.
pub(crate) fn may_configure_connection(content: &str) -> bool {
    memchr::memmem::find(content.as_bytes(), b"getConnection").is_some()
}

/// The connections the `DriverManager::getConnection()` calls in `program`
/// configure.
pub(crate) fn php_connections(
    program: &Program<'_>,
    names: &OwnedResolvedNames,
) -> Vec<Connection> {
    struct Calls<'n> {
        names: &'n OwnedResolvedNames,
    }
    impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<Connection>> for Calls<'_> {
        fn walk_in_static_method_call(
            &self,
            node: &'ast StaticMethodCall<'arena>,
            out: &mut Vec<Connection>,
        ) {
            let ClassLikeMemberSelector::Identifier(method) = &node.method else {
                return;
            };
            if !method.value.eq_ignore_ascii_case(b"getConnection") {
                return;
            }
            let Expression::Identifier(class) = node.class else {
                return;
            };
            if !self
                .names
                .class_fqn(class)
                .is_some_and(|fqn| fqn.eq_ignore_ascii_case(DRIVER_MANAGER))
            {
                return;
            }
            let params = node
                .argument_list
                .arguments
                .iter()
                .find(|argument| match argument {
                    Argument::Positional(_) => true,
                    Argument::Named(named) => named.name.value == b"params",
                })
                .map(Argument::value);
            out.push(params.map_or_else(Connection::unknown, |params| {
                connection_of_params(params, self.names)
            }));
        }
    }
    let mut out = Vec::new();
    mago_syntax::walker::Walker::walk_program(&Calls { names }, program, &mut out);
    out
}

/// The connection the parameters `params` configure.
fn connection_of_params(params: &Expression<'_>, names: &OwnedResolvedNames) -> Connection {
    let Some(entries) = array_entries(params) else {
        return Connection::unknown();
    };
    let mut driver = None;
    let mut url = None;
    let mut driver_class = None;
    let mut options = None;
    for (key, value) in entries {
        let Some(key) = key.and_then(string_of) else {
            // A key that is not read may be any of them.
            return Connection::unknown();
        };
        match key {
            "driver" => driver = Some(string_of(value)),
            "url" => url = Some(string_of(value)),
            "driverClass" => {
                driver_class = Some(
                    string_of(value)
                        .map(str::to_string)
                        .or_else(|| names.class_fetch_fqn(value)),
                )
            }
            "driverOptions" => options = Some(value),
            _ => {}
        }
    }
    // The driver class decides, then a URL's scheme, then the driver name.
    let driver = match (driver_class, url, driver) {
        (Some(class), _, _) => class.as_deref().and_then(driver_of_class),
        (None, Some(url), _) => url.and_then(driver_of_url),
        (None, None, Some(name)) => name.and_then(driver_named),
        (None, None, None) => None,
    };
    let mut connection = Connection::new(driver);
    if let Some(options) = options {
        read_php_options(options, names, &mut connection);
    }
    connection
}

/// Read the PDO attributes `driverOptions` sets.
fn read_php_options(options: &Expression<'_>, names: &OwnedResolvedNames, out: &mut Connection) {
    let Some(entries) = array_entries(options) else {
        out.stringify_fetches = None;
        out.emulate_prepares = None;
        return;
    };
    for (key, value) in entries {
        let attribute = match key {
            Some(Expression::Literal(Literal::Integer(integer))) => integer
                .value
                .and_then(|v| i64::try_from(v).ok())
                .map(Attribute::of_value),
            Some(Expression::Access(Access::ClassConstant(access))) => pdo_attribute(access, names),
            _ => None,
        };
        out.set(attribute, bool_of(value));
    }
}

/// The attribute the `PDO` constant `access` reads.
fn pdo_attribute(
    access: &ClassConstantAccess<'_>,
    names: &OwnedResolvedNames,
) -> Option<Attribute> {
    let Expression::Identifier(class) = access.class else {
        return None;
    };
    if !names.class_fqn(class)?.eq_ignore_ascii_case("PDO") {
        return None;
    }
    let ClassLikeConstantSelector::Identifier(constant) = &access.constant else {
        return None;
    };
    Attribute::of_constant(bytes_to_str(constant.value))
}

/// The entries of an array literal, each with its key when it has one.
/// `None` for anything else, or an array that spreads another.
#[allow(clippy::type_complexity)]
fn array_entries<'e, 'a>(
    expr: &'e Expression<'a>,
) -> Option<Vec<(Option<&'e Expression<'a>>, &'e Expression<'a>)>> {
    let elements = match expr {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        Expression::Parenthesized(inner) => return array_entries(inner.expression),
        _ => return None,
    };
    elements
        .iter()
        .map(|element| match element {
            ArrayElement::KeyValue(entry) => Some((Some(entry.key), entry.value)),
            ArrayElement::Value(entry) => Some((None, entry.value)),
            _ => None,
        })
        .collect()
}

fn string_of<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Literal(Literal::String(literal)) => literal.value.map(bytes_to_str),
        _ => None,
    }
}

fn bool_of(expr: &Expression<'_>) -> Option<bool> {
    match expr {
        Expression::Literal(Literal::True(_)) => Some(true),
        Expression::Literal(Literal::False(_)) => Some(false),
        Expression::Literal(Literal::Integer(integer)) => integer.value.map(|v| v != 0),
        _ => None,
    }
}

// ─── Symfony's Doctrine configuration ───────────────────────────────────────

/// One `key: value` line of a YAML file.
struct YamlLine<'a> {
    indent: usize,
    key: &'a str,
    value: &'a str,
}

/// The `key: value` lines of `content`, with their comments removed.  A
/// key ends at the first colon followed by a space or the line's end, so
/// `!php/const PDO::ATTR_STRINGIFY_FETCHES: true` keeps its whole key.
fn yaml_lines(content: &str) -> Vec<YamlLine<'_>> {
    let mut lines = Vec::new();
    for (_, line) in crate::framework::line_offsets(content) {
        let line = crate::symfony::resources::yaml_content_before_comment(line).trim_end();
        let indent = crate::framework::leading_spaces(line);
        let rest = &line[indent..];
        if rest.is_empty() || rest.starts_with('-') {
            continue;
        }
        let bytes = rest.as_bytes();
        let Some(colon) = (0..bytes.len())
            .find(|&i| bytes[i] == b':' && bytes.get(i + 1).is_none_or(|b| *b == b' '))
        else {
            continue;
        };
        lines.push(YamlLine {
            indent,
            key: crate::framework::strip_yaml_quotes(rest[..colon].trim()).0,
            value: crate::framework::strip_yaml_quotes(rest[colon + 1..].trim()).0,
        });
    }
    lines
}

/// The lines nested under `lines[at]`.
fn yaml_children<'l, 'a>(lines: &'l [YamlLine<'a>], at: usize) -> &'l [YamlLine<'a>] {
    let indent = lines[at].indent;
    let end = lines[at + 1..]
        .iter()
        .position(|line| line.indent <= indent)
        .map_or(lines.len(), |n| at + 1 + n);
    &lines[at + 1..end]
}

/// The direct children of the block `lines`, as indices into it.
fn yaml_direct(lines: &[YamlLine<'_>]) -> Vec<usize> {
    let Some(first) = lines.first() else {
        return Vec::new();
    };
    (0..lines.len())
        .filter(|&i| lines[i].indent == first.indent)
        .collect()
}

/// Whether the YAML file at `path` holds configuration Symfony loads in
/// every environment: one under `config/packages/<env>/` is loaded in that
/// environment only.
fn is_unconditional_config(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    parent
        .parent()
        .and_then(Path::file_name)
        .is_none_or(|name| name != "packages")
}

/// The connections the top-level `doctrine: dbal:` section of a YAML file
/// configures, `env` the value of an environment variable.  A `when@env:`
/// section only applies in that environment, and is left out.
pub(crate) fn yaml_connections(
    path: &Path,
    content: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<Connection> {
    if !content.contains("dbal") || !is_unconditional_config(path) {
        return Vec::new();
    }
    let lines = yaml_lines(content);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.indent != 0 || line.key != "doctrine" {
            continue;
        }
        let doctrine = yaml_children(&lines, i);
        for d in yaml_direct(doctrine) {
            if doctrine[d].key != "dbal" {
                continue;
            }
            let dbal = yaml_children(doctrine, d);
            let connections = yaml_direct(dbal)
                .into_iter()
                .find(|&c| dbal[c].key == "connections");
            match connections {
                Some(c) => {
                    let connections = yaml_children(dbal, c);
                    for n in yaml_direct(connections) {
                        out.push(yaml_connection(yaml_children(connections, n), env));
                    }
                }
                None => out.push(yaml_connection(dbal, env)),
            }
        }
    }
    out
}

/// The connection the YAML block `lines` configures.
fn yaml_connection(lines: &[YamlLine<'_>], env: &dyn Fn(&str) -> Option<String>) -> Connection {
    let mut driver = None;
    let mut url = None;
    let mut driver_class = None;
    let mut options = None;
    for i in yaml_direct(lines) {
        let line = &lines[i];
        match line.key {
            "driver" => driver = Some(line.value),
            "url" => url = Some(line.value),
            "driver_class" => driver_class = Some(line.value),
            "options" => options = Some(i),
            _ => {}
        }
    }
    let driver = match (driver_class, url, driver) {
        (Some(class), _, _) => driver_of_class(class),
        (None, Some(url), _) => driver_of_url(&resolve_parameters(url, env)),
        (None, None, Some(name)) => driver_named(&resolve_parameters(name, env)),
        (None, None, None) => None,
    };
    let mut connection = Connection::new(driver);
    if let Some(i) = options {
        let options = yaml_children(lines, i);
        if options.is_empty() && !lines[i].value.is_empty() {
            // An inline mapping or a parameter.
            connection.stringify_fetches = None;
            connection.emulate_prepares = None;
        }
        for o in yaml_direct(options) {
            let option = &options[o];
            let attribute = match option.key.strip_prefix("!php/const") {
                Some(constant) => constant
                    .trim()
                    .trim_start_matches('\\')
                    .strip_prefix("PDO::")
                    .and_then(Attribute::of_constant),
                None => option.key.parse().ok().map(Attribute::of_value),
            };
            let value = match option.value.to_ascii_lowercase().as_str() {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            };
            connection.set(attribute, value);
        }
    }
    connection
}

/// `value` with each `%env(…)%` parameter whose variable is known replaced
/// by its value.  Any other parameter is left as written: only a URL's
/// scheme names the driver, and a scheme that is a parameter names none.
fn resolve_parameters(value: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let parameter = &after[..end];
        // `resolve:`, `string:`, and the like process the variable; the
        // name is last.
        let resolved = parameter
            .strip_prefix("env(")
            .and_then(|inner| inner.strip_suffix(')'))
            .and_then(|inner| inner.rsplit(':').next())
            .and_then(env);
        match resolved {
            Some(value) => out.push_str(&value),
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The value the dotenv files of the Symfony project at `root` give `name`
/// in its default environment: `.env`, overridden by `.env.local`, then
/// `.env.dev` and `.env.dev.local`.
pub(crate) fn symfony_env(root: &Path, name: &str) -> Option<String> {
    [".env.dev.local", ".env.dev", ".env.local", ".env"]
        .iter()
        .find_map(|file| {
            let content = std::fs::read_to_string(root.join(file)).ok()?;
            crate::virtual_members::laravel::declared_env_value(&content, name)
        })
}

/// The root of the Symfony project the configuration file `path` belongs
/// to: the directory holding its `config/`.
pub(crate) fn symfony_root(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "config"))?
        .parent()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn php(source: &str) -> Vec<Connection> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, source.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        php_connections(program, &names)
    }

    fn driver(connections: &[Connection]) -> Vec<Option<Driver>> {
        connections.iter().map(|c| c.driver).collect()
    }

    #[test]
    fn reads_driver_manager_parameters() {
        let source = r#"<?php
use Doctrine\DBAL\DriverManager;
use Doctrine\DBAL\Driver\PDO\PgSQL\Driver;
DriverManager::getConnection(['driver' => 'pdo_sqlite', 'memory' => true]);
\Doctrine\DBAL\DriverManager::getConnection(['url' => 'mysql://u:p@localhost/db']);
DriverManager::getConnection(['driverClass' => Driver::class, 'driver' => 'mysqli']);
DriverManager::getConnection(['driver' => 'oci8']);
DriverManager::getConnection($params);
Other::getConnection(['driver' => 'mysqli']);
// DriverManager::getConnection(['driver' => 'sqlsrv']);
"#;
        assert_eq!(
            driver(&php(source)),
            vec![
                Some(Driver::PdoSqlite),
                Some(Driver::PdoMysql),
                Some(Driver::PdoPgsql),
                None,
                None,
            ]
        );
    }

    #[test]
    fn reads_pdo_options() {
        let source = r#"<?php
use Doctrine\DBAL\DriverManager;
DriverManager::getConnection([
    'driver' => 'pdo_mysql',
    'driverOptions' => [\PDO::ATTR_STRINGIFY_FETCHES => true, \PDO::ATTR_EMULATE_PREPARES => false],
]);
DriverManager::getConnection(['driver' => 'pdo_mysql', 'driverOptions' => [\PDO::ATTR_TIMEOUT => 5]]);
DriverManager::getConnection(['driver' => 'pdo_mysql', 'driverOptions' => $options]);
"#;
        let connections = php(source);
        assert_eq!(connections[0].stringify_fetches, Some(true));
        assert_eq!(connections[0].emulate_prepares, Some(false));
        assert_eq!(connections[1].stringify_fetches, Some(false));
        assert_eq!(connections[1].emulate_prepares, Some(true));
        assert_eq!(connections[2].stringify_fetches, None);
    }

    #[test]
    fn reads_symfony_dbal_configuration() {
        let env = |name: &str| (name == "DATABASE_URL").then(|| "postgresql://db/app".to_string());
        let path = Path::new("/app/config/packages/doctrine.yaml");
        let yaml = "doctrine:\n    dbal:\n        url: '%env(resolve:DATABASE_URL)%'\n        options:\n            !php/const PDO::ATTR_STRINGIFY_FETCHES: true\n    orm:\n        auto_mapping: true\nwhen@test:\n    doctrine:\n        dbal:\n            driver: pdo_sqlite\n";
        let connections = yaml_connections(path, yaml, &env);
        assert_eq!(driver(&connections), vec![Some(Driver::PdoPgsql)]);
        assert_eq!(connections[0].stringify_fetches, Some(true));

        let project_dir =
            "doctrine:\n  dbal:\n    url: 'sqlite:///%kernel.project_dir%/var/app.db'\n";
        assert_eq!(
            driver(&yaml_connections(path, project_dir, &env)),
            vec![Some(Driver::PdoSqlite)]
        );

        let several = "doctrine:\n  dbal:\n    default_connection: main\n    connections:\n      main:\n        driver: pdo_mysql\n      logs:\n        url: '%env(LOGS_URL)%'\n";
        assert_eq!(
            driver(&yaml_connections(path, several, &env)),
            vec![Some(Driver::PdoMysql), None]
        );

        let per_env = Path::new("/app/config/packages/test/doctrine.yaml");
        assert!(
            yaml_connections(
                per_env,
                "doctrine:\n  dbal:\n    driver: pdo_sqlite\n",
                &env
            )
            .is_empty()
        );
    }

    #[test]
    fn the_project_connection_is_the_one_every_configuration_agrees_on() {
        let index = new_connection_index();
        assert_eq!(project_connection(&index), None);
        record(&index, "a", vec![Connection::new(Some(Driver::Mysqli))]);
        record(&index, "b", vec![Connection::new(Some(Driver::Mysqli))]);
        assert_eq!(
            project_connection(&index).and_then(|c| c.driver),
            Some(Driver::Mysqli)
        );
        record(&index, "c", vec![Connection::new(Some(Driver::PdoPgsql))]);
        assert_eq!(project_connection(&index), None);
        assert!(record(&index, "c", Vec::new()));
        assert!(project_connection(&index).is_some());
        record(&index, "d", vec![Connection::unknown()]);
        assert_eq!(project_connection(&index), None);
    }
}
