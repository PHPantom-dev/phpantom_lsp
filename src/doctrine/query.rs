//! The type of a Doctrine query, read from its DQL.
//!
//! ORM's `Query` declares no type parameters, so a query carries what its
//! DQL returns the way PHPStan's Doctrine extension has it carry them:
//! `Query<TKey, TResult>`, where `TResult` is one row as `getResult()`
//! hydrates it into objects and `TKey` the type of the field the root
//! entity is indexed by (`null` without `INDEX BY`).  An `UPDATE` or
//! `DELETE` is `Query<void, void>`, and DQL this reader cannot type is
//! `Query<mixed, mixed>`.
//!
//! A row is the entity a `SELECT u` names, the shape of the fields a
//! `SELECT u.id, u.name` names, the object a lone `NEW Dto(...)` makes, or
//! an entity at key `0` (or its result variable) beside those fields.  A
//! field's type is the type its entity declares for the property; a field
//! of a variable a `LEFT JOIN` declares may be `null` too, and so may
//! everything a query that aggregates without a `GROUP BY` selects.  A fetch
//! join adds nothing to a row: its entities are hydrated into their owner's
//! association.
//!
//! Any other expression is keyed by its result variable, or by a counter
//! from `1`.  `COUNT()` and `LENGTH()` are converted to `int` by Doctrine
//! itself; every other expression is what the database gives for it, as the
//! driver fetches it.  The string functions, string literals, and the
//! `CASE`, `COALESCE()`, and `NULLIF()` of those give strings on every
//! driver.  The rest (`SUM()`, arithmetic, a number) differs between MySQL,
//! PostgreSQL, and SQLite, and with the PDO options and the PHP version:
//! it is typed the way PHPStan's Doctrine extension types it for the
//! project's connection (see [`super::connection`]), and is `mixed` when
//! the connection is not known.
//!
//! A query builder's query is the DQL its calls build, replayed by
//! [`QueryBuilderState`].  The calls that only filter or order the rows
//! leave the type alone whatever their arguments; the others need theirs
//! known.

use std::sync::Arc;

use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{LiteralValue, PhpType, ShapeEntry, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{ClassInfo, PhpVersion};

use super::DoctrineCacheState;
use super::connection::{Connection, Driver};
use super::dql::{Tok, Token, is_keyword, tokenize};

/// ORM's query class.
pub(crate) const QUERY: &str = "Doctrine\\ORM\\Query";

/// ORM's query base class, which declares the result methods.
pub(crate) const ABSTRACT_QUERY: &str = "Doctrine\\ORM\\AbstractQuery";

/// ORM's query builder.
pub(crate) const QUERY_BUILDER: &str = "Doctrine\\ORM\\QueryBuilder";

/// The entity manager interface, which declares `createQuery()` and
/// `createQueryBuilder()`.
pub(crate) const ENTITY_MANAGER: &str = "Doctrine\\ORM\\EntityManagerInterface";

/// `AbstractQuery::HYDRATE_OBJECT`.
const HYDRATE_OBJECT: i64 = 1;
/// `AbstractQuery::HYDRATE_SIMPLEOBJECT`.
const HYDRATE_SIMPLEOBJECT: i64 = 5;

/// What a field of an entity maps.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Field {
    /// A column, with the type the entity declares for its property.
    Column(PhpType),
    /// An association, with the entity it targets when that is known.
    Association(Option<Atom>),
}

/// The mapping the DQL is read against.
pub(crate) trait EntityModel {
    /// The fully-qualified name of the entity `name` names, when it is a
    /// mapped entity.
    fn entity(&self, name: &str) -> Option<Atom>;
    /// What `field` of `entity` maps, when it maps it.
    fn field(&self, entity: &str, field: &str) -> Option<Field>;
    /// The fully-qualified name of the class `name` names, when it is one:
    /// the class `NEW` constructs.
    fn class(&self, name: &str) -> Option<Atom>;
    /// The connection the queries run on, when its driver is known.
    fn platform(&self) -> Option<Platform> {
        None
    }
}

/// `Query<key, result>`.
pub(crate) fn query_of(key: PhpType, result: PhpType) -> PhpType {
    PhpType::generic(QUERY, vec![key, result])
}

/// The type of a query whose DQL is `dql`.
pub(crate) fn query_type(dql: &str, model: &dyn EntityModel) -> PhpType {
    let (key, result) =
        result_types(dql, model).unwrap_or_else(|| (PhpType::mixed(), PhpType::mixed()));
    query_of(key, result)
}

/// The type of the query `createQuery()` gives, `dql` the type of its
/// argument (`None` when it is left out, `Some(None)` when the type is not
/// known): a query for each string the argument may be, or
/// `Query<mixed, mixed>` when it may be any other.
pub(crate) fn create_query_type(dql: Option<Option<PhpType>>, model: &dyn EntityModel) -> PhpType {
    let dqls = match dql {
        // The default is an empty query, which is no DQL.
        None => vec![String::new()],
        Some(ty) => ty.as_ref().and_then(string_literals).unwrap_or_default(),
    };
    if dqls.is_empty() {
        return query_of(PhpType::mixed(), PhpType::mixed());
    }
    PhpType::union(dqls.iter().map(|dql| query_type(dql, model)).collect())
}

/// The string literals `ty` is one of, or `None` when it admits any other
/// value.
fn string_literals(ty: &PhpType) -> Option<Vec<String>> {
    ty.union_members()
        .into_iter()
        .map(|member| {
            member
                .as_literal()?
                .string_content()
                .map(|s| s.into_owned())
        })
        .collect()
}

/// The hydration mode a value of type `ty` is, when it is one.
pub(crate) fn hydration_mode(ty: &PhpType) -> Option<i64> {
    ty.as_literal()?.parse_i64()
}

/// The `TKey` and `TResult` of a query a builder in `state` builds.
pub(crate) fn builder_query_type(state: &QueryBuilderState, model: &dyn EntityModel) -> PhpType {
    match state.kind {
        StatementKind::Update | StatementKind::Delete => query_of(PhpType::void(), PhpType::void()),
        StatementKind::Select => query_type(&state.dql(), model),
    }
}

// ─── Reading the DQL ────────────────────────────────────────────────────────

/// What an identification variable stands for.
struct Decl {
    alias: String,
    /// The entity, when known.
    entity: Option<Atom>,
    /// Whether a `SELECT` of the variable puts its entity in the row: a
    /// root or an arbitrary join, as opposed to a fetch join.
    in_row: bool,
    /// Whether the variable may stand for no entity: a `LEFT JOIN`.
    optional: bool,
    /// The field `INDEX BY` names.
    index_by: Option<(String, String)>,
    /// Whether the variable is a root, declared by `FROM`.
    root: bool,
}

struct Reader<'a> {
    text: &'a str,
    tokens: Vec<Token>,
}

impl Reader<'_> {
    fn kind(&self, i: usize) -> Option<Tok> {
        self.tokens.get(i).map(|t| t.kind)
    }

    fn text(&self, i: usize) -> &str {
        let token = &self.tokens[i];
        &self.text[token.start..token.end]
    }

    fn word(&self, i: usize) -> Option<&str> {
        (self.kind(i)? == Tok::Ident).then(|| self.text(i))
    }

    fn is_word(&self, i: usize, keyword: &str) -> bool {
        self.word(i)
            .is_some_and(|w| w.eq_ignore_ascii_case(keyword))
    }

    /// The variable at `i`: a name that is no keyword or class name.
    fn variable(&self, i: usize) -> Option<&str> {
        self.word(i).filter(|w| !is_keyword(w) && !w.contains('\\'))
    }

    fn adjacent(&self, i: usize) -> bool {
        match (self.tokens.get(i), self.tokens.get(i + 1)) {
            (Some(a), Some(b)) => a.end == b.start,
            _ => false,
        }
    }

    /// The path `alias.field` at `i`, with nothing after the field.
    fn path(&self, i: usize) -> Option<(String, String)> {
        let alias = self.variable(i)?;
        if self.kind(i + 1) != Some(Tok::Dot) || !self.adjacent(i) || !self.adjacent(i + 1) {
            return None;
        }
        let field = self.word(i + 2)?;
        if self.kind(i + 3) == Some(Tok::Dot) && self.adjacent(i + 2) {
            return None;
        }
        Some((alias.to_string(), field.to_string()))
    }

    /// The index past the top-level clause starting at `from`, which ends
    /// at the first of `keywords` outside parentheses.
    fn clause_end(&self, from: usize, keywords: &[&str]) -> Option<usize> {
        let mut depth = 0usize;
        for i in from..self.tokens.len() {
            match self.tokens[i].kind {
                Tok::LParen => depth += 1,
                Tok::RParen => depth = depth.checked_sub(1)?,
                Tok::Ident if depth == 0 && keywords.iter().any(|k| self.is_word(i, k)) => {
                    return Some(i);
                }
                _ => {}
            }
        }
        (depth == 0).then_some(self.tokens.len())
    }

    /// Read `[AS] alias [INDEX BY path]` at `i` into a declaration of
    /// `entity`.  Returns where the reading stopped.
    fn declaration(
        &self,
        mut i: usize,
        entity: Option<Atom>,
        in_row: bool,
        optional: bool,
        root: bool,
    ) -> Option<(Decl, usize)> {
        if self.is_word(i, "AS") {
            i += 1;
        }
        let alias = self.variable(i)?.to_string();
        i += 1;
        let mut index_by = None;
        if self.is_word(i, "INDEX") && self.is_word(i + 1, "BY") {
            index_by = Some(self.path(i + 2)?);
            i += 5;
        }
        Some((
            Decl {
                alias,
                entity,
                in_row,
                optional,
                index_by,
                root,
            },
            i,
        ))
    }

    /// Read the `FROM` clause in `start..end`: its range declarations and
    /// the joins after each.
    fn range_declarations(
        &self,
        start: usize,
        end: usize,
        model: &dyn EntityModel,
    ) -> Option<Vec<Decl>> {
        let mut decls: Vec<Decl> = Vec::new();
        let mut i = start;
        loop {
            let entity = model.entity(self.word(i).filter(|w| !is_keyword(w))?)?;
            let (decl, next) = self.declaration(i + 1, Some(entity), true, false, true)?;
            if decls.iter().any(|d| d.alias == decl.alias) {
                return None;
            }
            decls.push(decl);
            i = next;
            loop {
                let optional = self.is_word(i, "LEFT");
                if optional || self.is_word(i, "INNER") {
                    i += 1;
                    if optional && self.is_word(i, "OUTER") {
                        i += 1;
                    }
                }
                if !self.is_word(i, "JOIN") {
                    break;
                }
                i += 1;
                let (entity, in_row, next) = if let Some((alias, field)) = self.path(i) {
                    let owner = decls.iter().find(|d| d.alias == alias)?;
                    let target = match &owner.entity {
                        Some(owner) => match model.field(owner, &field)? {
                            Field::Association(target) => target,
                            Field::Column(_) => return None,
                        },
                        None => None,
                    };
                    (target, false, i + 3)
                } else {
                    let name = self.word(i).filter(|w| !is_keyword(w))?;
                    (Some(model.entity(name)?), true, i + 1)
                };
                let (decl, next) = self.declaration(next, entity, in_row, optional, false)?;
                if decls.iter().any(|d| d.alias == decl.alias) {
                    return None;
                }
                decls.push(decl);
                i = next;
                if self.is_word(i, "WITH") || self.is_word(i, "ON") {
                    // The condition filters, and runs to the next join or
                    // range.
                    let mut depth = 0usize;
                    i += 1;
                    while i < end {
                        match self.tokens[i].kind {
                            Tok::LParen => depth += 1,
                            Tok::RParen => depth = depth.checked_sub(1)?,
                            Tok::Comma if depth == 0 => break,
                            Tok::Ident
                                if depth == 0
                                    && ["LEFT", "INNER", "JOIN"]
                                        .iter()
                                        .any(|k| self.is_word(i, k)) =>
                            {
                                break;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
            }
            if i == end {
                return Some(decls);
            }
            if self.kind(i) != Some(Tok::Comma) {
                return None;
            }
            i += 1;
        }
    }
}

/// One `SELECT` expression, read.
enum Selected {
    /// The entity a variable stands for.
    Entity { alias: String },
    /// A field, `alias.field`.
    Field { alias: String, field: String },
    /// `NEW class(...)`, with the number of arguments it passes.
    New { class: String, args: usize },
    /// Any other expression, the tokens `start..end`.
    Other { start: usize, end: usize },
}

/// The words that name an aggregate function.
const AGGREGATES: &[&str] = &["COUNT", "SUM", "AVG", "MIN", "MAX"];

/// The connection a query runs on, as far as it decides what the database
/// gives back for an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Platform {
    pub(crate) driver: Driver,
    /// `PDO::ATTR_STRINGIFY_FETCHES`, `None` when not known.
    pub(crate) stringify_fetches: Option<bool>,
    /// `PDO::ATTR_EMULATE_PREPARES`, `None` when not known.
    pub(crate) emulate_prepares: Option<bool>,
    /// The PHP version the project runs on, whose PDO drivers changed what
    /// they return in 8.1 and 8.4.
    pub(crate) php: PhpVersion,
}

/// The kinds of value a PDO driver may return as a string instead.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fetch {
    Int,
    Float,
    Bool,
}

impl Platform {
    /// The platform of `connection` on `php`, when its driver is known.
    pub(crate) fn of(connection: Connection, php: PhpVersion) -> Option<Self> {
        Some(Self {
            driver: connection.driver?,
            stringify_fetches: connection.stringify_fetches,
            emulate_prepares: connection.emulate_prepares,
            php,
        })
    }

    /// Whether the driver returns a value of kind `fetch` as a string:
    /// `Some(true)` when it does, `Some(false)` when it does not, `None`
    /// when either may happen.  The table is PHPStan's Doctrine extension's,
    /// which comes from fetching every kind from every driver.
    fn stringifies(&self, fetch: Fetch) -> Option<bool> {
        let at_least = |minor: u8| self.php >= PhpVersion::new(8, minor);
        let stringify = self.stringify_fetches;
        match self.driver {
            Driver::PdoMysql => match stringify {
                Some(true) => Some(true),
                None => None,
                Some(false) if at_least(1) => Some(false),
                Some(false) => self.emulate_prepares,
            },
            Driver::PdoSqlite => match stringify {
                Some(true) => Some(true),
                _ if !at_least(1) => Some(true),
                other => other,
            },
            Driver::PdoPgsql => match fetch {
                Fetch::Bool if !at_least(1) => Some(false),
                Fetch::Float if !at_least(4) => Some(true),
                _ => stringify,
            },
            Driver::Mysqli | Driver::Pgsql | Driver::Sqlite3 => Some(false),
        }
    }

    /// What the value the database gives as `fetched` is once the driver
    /// has fetched it.
    fn hydrate(&self, fetched: Fetched) -> Fetched {
        let mut members = Vec::new();
        for member in fetched.members {
            let fetch = match member {
                Scalar::Int(_) | Scalar::NonNegativeInt => Fetch::Int,
                Scalar::Float(_) => Fetch::Float,
                Scalar::Bool(_) => Fetch::Bool,
                Scalar::NumericString | Scalar::String { .. } => {
                    members.push(member);
                    continue;
                }
            };
            let stringify = self.stringifies(fetch);
            if stringify == Some(false) {
                members.push(member);
                continue;
            }
            let strings = fetched_as_string(&member);
            if stringify.is_none() {
                members.push(member);
            }
            members.extend(strings);
        }
        Fetched {
            members,
            nullable: fetched.nullable,
        }
    }
}

/// What a PDO driver gives for `member` when it returns it as a string.
fn fetched_as_string(member: &Scalar) -> Vec<Scalar> {
    match member {
        Scalar::Int(Some(n)) => vec![Scalar::string(n.to_string())],
        Scalar::Bool(Some(b)) => vec![Scalar::string(if *b { "1" } else { "0" })],
        Scalar::Bool(None) => vec![Scalar::string("0"), Scalar::string("1")],
        // A float's digits differ between PHP versions, so only its being
        // numeric is known.
        _ => vec![Scalar::NumericString],
    }
}

/// A value the database gives for an expression, as PHP sees it before
/// Doctrine converts anything.
#[derive(Debug, Clone, PartialEq)]
enum Scalar {
    /// An `int`, or the one int when known.
    Int(Option<i64>),
    /// An `int<0, max>`.
    NonNegativeInt,
    /// A `float`, or the one float when known.
    Float(Option<f64>),
    /// A `numeric-string`.
    NumericString,
    /// A `string`, or the one string when known.  `written` says whether
    /// it is a string literal of the DQL, which arithmetic reads as the
    /// number it spells.
    String {
        value: Option<String>,
        written: bool,
    },
    /// A `bool`, or the one bool when known.
    Bool(Option<bool>),
}

impl Scalar {
    fn string(value: impl Into<String>) -> Self {
        Scalar::String {
            value: Some(value.into()),
            written: false,
        }
    }

    fn any_string() -> Self {
        Scalar::String {
            value: None,
            written: false,
        }
    }

    fn is_int(&self) -> bool {
        matches!(self, Scalar::Int(_) | Scalar::NonNegativeInt)
    }

    fn is_float(&self) -> bool {
        matches!(self, Scalar::Float(_))
    }

    fn is_string(&self) -> bool {
        matches!(self, Scalar::NumericString | Scalar::String { .. })
    }

    fn is_numeric_string(&self) -> bool {
        match self {
            Scalar::NumericString => true,
            Scalar::String {
                value: Some(value), ..
            } => LiteralValue::string_value(value).is_numeric_string(),
            _ => false,
        }
    }

    fn is_numeric(&self) -> bool {
        self.is_int() || self.is_float() || self.is_numeric_string()
    }

    fn is_constant(&self) -> bool {
        matches!(
            self,
            Scalar::Int(Some(_))
                | Scalar::Float(Some(_))
                | Scalar::String { value: Some(_), .. }
                | Scalar::Bool(Some(_))
        )
    }

    fn to_php(&self) -> PhpType {
        match self {
            Scalar::Int(Some(n)) => PhpType::literal_int(n.to_string()),
            Scalar::Int(None) => PhpType::int(),
            Scalar::NonNegativeInt => PhpType::int_range("0", "max"),
            Scalar::Float(Some(f)) => PhpType::literal_float(float_literal(*f)),
            Scalar::Float(None) => PhpType::float(),
            Scalar::NumericString => PhpType::numeric_string(),
            Scalar::String {
                value: Some(value), ..
            } => PhpType::literal_string_value(value),
            Scalar::String { value: None, .. } => PhpType::string(),
            Scalar::Bool(Some(true)) => PhpType::true_(),
            Scalar::Bool(Some(false)) => PhpType::false_(),
            Scalar::Bool(None) => PhpType::bool(),
        }
    }

    /// The scalar PHP's string cast makes of this one.
    fn cast_to_string(&self) -> Vec<Scalar> {
        match self {
            Scalar::Int(Some(n)) => vec![Scalar::string(n.to_string())],
            Scalar::Float(Some(f)) => vec![Scalar::string(php_float_string(*f))],
            Scalar::Int(None) | Scalar::NonNegativeInt | Scalar::Float(None) => {
                vec![Scalar::NumericString]
            }
            Scalar::Bool(Some(b)) => vec![Scalar::string(if *b { "1" } else { "" })],
            Scalar::Bool(None) => vec![Scalar::string(""), Scalar::string("1")],
            Scalar::NumericString | Scalar::String { .. } => vec![self.clone()],
        }
    }

    /// The scalar PHP's float cast makes of this one.
    fn cast_to_float(&self) -> Scalar {
        match self {
            Scalar::Int(Some(n)) => Scalar::Float(Some(*n as f64)),
            Scalar::Bool(Some(b)) => Scalar::Float(Some(if *b { 1.0 } else { 0.0 })),
            Scalar::String {
                value: Some(value), ..
            } => Scalar::Float(Some(value.trim().parse().unwrap_or(0.0))),
            Scalar::Float(f) => Scalar::Float(*f),
            _ => Scalar::Float(None),
        }
    }
}

/// A float as a PHPDoc literal spells it, with a fraction.
fn float_literal(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// A float as PHP's string cast spells it.
fn php_float_string(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// What the database gives for an expression: one of `members`, or `null`
/// when `nullable`.
#[derive(Debug, Clone, PartialEq)]
struct Fetched {
    members: Vec<Scalar>,
    nullable: bool,
}

impl Fetched {
    fn of(member: Scalar, nullable: bool) -> Self {
        Self {
            members: vec![member],
            nullable,
        }
    }

    fn null() -> Self {
        Self {
            members: Vec::new(),
            nullable: true,
        }
    }

    fn union(values: impl IntoIterator<Item = Fetched>) -> Self {
        let mut out = Fetched {
            members: Vec::new(),
            nullable: false,
        };
        for value in values {
            out.nullable |= value.nullable;
            for member in value.members {
                if !out.members.contains(&member) {
                    out.members.push(member);
                }
            }
        }
        out
    }

    fn or_null(mut self, nullable: bool) -> Self {
        self.nullable |= nullable;
        self
    }

    /// Whether every member satisfies `predicate`, there being at least
    /// one.
    fn all(&self, predicate: impl Fn(&Scalar) -> bool) -> bool {
        !self.members.is_empty() && self.members.iter().all(predicate)
    }

    fn is_integer(&self) -> bool {
        self.all(Scalar::is_int)
    }

    fn is_float(&self) -> bool {
        self.all(Scalar::is_float)
    }

    fn is_string(&self) -> bool {
        self.all(Scalar::is_string)
    }

    fn is_numeric_string(&self) -> bool {
        self.all(Scalar::is_numeric_string)
    }

    fn is_numeric(&self) -> bool {
        self.all(Scalar::is_numeric)
    }

    /// Whether the value is a string literal of the DQL.
    fn is_written_string(&self) -> bool {
        matches!(
            self.members.as_slice(),
            [Scalar::String {
                value: Some(_),
                written: true
            }]
        )
    }

    /// The type a constant is one of: `SUM(1)` is an `int`, not `1`.
    fn generalized(&self, nullable: bool) -> Fetched {
        let nullable = self.nullable || nullable;
        if !self.all(Scalar::is_constant) {
            return Fetched {
                members: self.members.clone(),
                nullable,
            };
        }
        let member = if self.is_integer() {
            Scalar::Int(None)
        } else if self.is_float() {
            Scalar::Float(None)
        } else if self.is_numeric_string() {
            Scalar::NumericString
        } else if self.is_string() {
            Scalar::any_string()
        } else {
            return Fetched {
                members: self.members.clone(),
                nullable,
            };
        };
        Fetched::of(member, nullable)
    }

    fn map(self, f: impl Fn(&Scalar) -> Vec<Scalar>) -> Fetched {
        Fetched::union(
            self.members
                .iter()
                .map(|member| Fetched {
                    members: f(member),
                    nullable: false,
                })
                .chain(std::iter::once(Fetched {
                    members: Vec::new(),
                    nullable: self.nullable,
                })),
        )
    }

    fn to_php(&self) -> PhpType {
        let mut types: Vec<PhpType> = self.members.iter().map(Scalar::to_php).collect();
        if self.nullable {
            types.push(PhpType::null());
        }
        if types.is_empty() {
            return PhpType::mixed();
        }
        PhpType::union(types).simplified()
    }
}

/// What the database gives for a scalar expression.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Known(Fetched),
    /// A value whose type is not known, and whether it may be `null`.
    Unknown {
        nullable: bool,
    },
}

impl Value {
    fn nullable(&self) -> bool {
        match self {
            Value::Known(fetched) => fetched.nullable,
            Value::Unknown { nullable } => *nullable,
        }
    }

    fn unknown() -> Self {
        Value::Unknown { nullable: true }
    }

    fn of(member: Scalar, nullable: bool) -> Self {
        Value::Known(Fetched::of(member, nullable))
    }

    /// A numeric string literal of the DQL, read as the number it spells,
    /// the way the database reads it in arithmetic: `1 * '1'` is `1`.
    /// MySQL reads every one as a float.
    fn read_as_number(self, driver: Option<Driver>) -> Self {
        let Value::Known(fetched) = &self else {
            return self;
        };
        let [
            Scalar::String {
                value: Some(text),
                written: true,
            },
        ] = fetched.members.as_slice()
        else {
            return self;
        };
        if fetched.nullable || !LiteralValue::string_value(text).is_numeric_string() {
            return self;
        }
        let mysql = driver.is_some_and(Driver::is_mysql);
        if !mysql
            && !text.contains(['.', 'e'])
            && let Ok(n) = text.trim().parse::<i64>()
        {
            return Value::of(Scalar::Int(Some(n)), false);
        }
        Value::of(Scalar::Float(text.trim().parse().ok()), false)
    }

    /// A numeric string literal of the DQL, read as a float.
    fn read_as_float(self) -> Self {
        match &self {
            Value::Known(fetched)
                if fetched.is_written_string() && !fetched.nullable && fetched.is_numeric() =>
            {
                Value::Known(fetched.clone().map(|m| vec![m.cast_to_float()]))
            }
            _ => self,
        }
    }
}

/// What the scalar values `values` are, each known.
fn all_known(values: Vec<Value>) -> Result<Vec<Fetched>, Value> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        match value {
            Value::Known(fetched) => out.push(fetched),
            Value::Unknown { .. } => return Err(Value::unknown()),
        }
    }
    Ok(out)
}

/// What the database gives for `+`, `-`, or `*` of `terms`.
fn plus_minus_times(terms: Vec<Value>, driver: Option<Driver>) -> Value {
    let terms = match all_known(terms) {
        Ok(terms) => terms,
        Err(unknown) => return unknown,
    };
    let terms: Vec<Fetched> = terms.iter().map(|t| t.generalized(false)).collect();
    let union = Fetched::union(terms.iter().cloned());
    let nullable = union.nullable;
    if union.is_integer() {
        return Value::of(Scalar::Int(None), nullable);
    }
    match driver {
        Some(driver) if driver.is_sqlite() => sqlite_arithmetic(&terms, nullable),
        Some(_) => {
            if union.all(|m| m.is_int() || m.is_float()) {
                Value::of(Scalar::Float(None), nullable)
            } else if union.all(|m| m.is_int() || m.is_numeric_string()) {
                Value::of(Scalar::NumericString, nullable)
            } else if union.is_numeric() {
                Value::of(Scalar::Float(None), nullable)
            } else {
                Value::unknown()
            }
        }
        None => Value::unknown(),
    }
}

/// What the database gives for `/` of `factors`.
fn division(factors: Vec<Value>, driver: Option<Driver>) -> Value {
    let factors = match all_known(factors) {
        Ok(factors) => factors,
        Err(unknown) => return unknown,
    };
    let factors: Vec<Fetched> = factors.iter().map(|t| t.generalized(false)).collect();
    let union = Fetched::union(factors.iter().cloned());
    let nullable = union.nullable;
    let Some(driver) = driver else {
        return Value::unknown();
    };
    if union.is_integer() {
        let member = if driver.is_mysql() {
            Scalar::NumericString
        } else {
            Scalar::Int(None)
        };
        return Value::of(member, nullable);
    }
    if driver.is_sqlite() {
        return sqlite_arithmetic(&factors, nullable);
    }
    if union.all(|m| m.is_int() || m.is_numeric_string()) {
        Value::of(Scalar::NumericString, nullable)
    } else if union.is_numeric() {
        Value::of(Scalar::Float(None), nullable)
    } else {
        Value::unknown()
    }
}

/// What SQLite gives for arithmetic on the generalized `operands`, which
/// are not all integers.
fn sqlite_arithmetic(operands: &[Fetched], nullable: bool) -> Value {
    if !operands.iter().all(Fetched::is_numeric) {
        return Value::unknown();
    }
    if operands.iter().any(Fetched::is_float) {
        return Value::of(Scalar::Float(None), nullable);
    }
    Value::Known(Fetched {
        members: vec![Scalar::Float(None), Scalar::Int(None)],
        nullable,
    })
}

fn or_null(ty: PhpType, nullable: bool) -> PhpType {
    if nullable {
        PhpType::union(vec![ty, PhpType::null()])
    } else {
        ty
    }
}

/// What the `SELECT` expressions of a query are read against.
struct Scope<'a> {
    reader: &'a Reader<'a>,
    decls: &'a [Decl],
    model: &'a dyn EntityModel,
    /// Whether the query aggregates without a `GROUP BY`: its one row may
    /// come from no row at all, so every column may be `null`.
    aggregate_without_group_by: bool,
    /// The connection the query runs on, when known.
    platform: Option<Platform>,
}

impl Scope<'_> {
    fn driver(&self) -> Option<Driver> {
        self.platform.map(|p| p.driver)
    }

    /// The type of the selected field `field` of what `alias` stands for.
    fn field(&self, alias: &str, field: &str) -> Option<PhpType> {
        let decl = self.decls.iter().find(|d| d.alias == alias)?;
        let Field::Column(ty) = self.model.field(decl.entity.as_ref()?, field)? else {
            return None;
        };
        if (decl.optional || self.aggregate_without_group_by) && !ty.is_mixed() {
            return Some(PhpType::union(vec![ty, PhpType::null()]).simplified());
        }
        Some(ty)
    }

    /// The type of the top-level `SELECT` expression in `start..end`.
    ///
    /// Doctrine converts the result of `COUNT()` and `LENGTH()` through its
    /// `integer` type, which makes it an `int` whatever the driver gave.
    /// Every other expression is hydrated as the driver fetched it: a
    /// string stays one on every driver, and anything else is known once
    /// the driver is.
    fn select_type(&self, start: usize, end: usize) -> PhpType {
        if let Some((name, args)) = self.function(start, end) {
            if name.eq_ignore_ascii_case("COUNT") {
                return PhpType::int_range("0", "max");
            }
            if name.eq_ignore_ascii_case("LENGTH") {
                let nullable = self.aggregate_without_group_by
                    || args
                        .first()
                        .is_none_or(|&(s, e)| self.value(s, e).nullable());
                return or_null(PhpType::int_range("0", "max"), nullable);
            }
        }
        let Value::Known(fetched) = self.value(start, end) else {
            return PhpType::mixed();
        };
        match self.platform {
            Some(platform) => platform.hydrate(fetched).to_php(),
            None if fetched.members.iter().all(Scalar::is_string) => fetched.to_php(),
            None => PhpType::mixed(),
        }
    }

    /// The function call `name(args)` the tokens `start..end` are.
    fn function(&self, start: usize, end: usize) -> Option<(&str, Vec<(usize, usize)>)> {
        let name = self.reader.word(start)?;
        if self.reader.kind(start + 1) != Some(Tok::LParen)
            || matching(self.reader, start + 1)? + 1 != end
        {
            return None;
        }
        let args = if start + 2 == end - 1 {
            Vec::new()
        } else {
            top_level_items(self.reader, start + 2, end - 1)?
        };
        Some((name, args))
    }

    /// The type of the expression in `start..end`.
    fn value(&self, start: usize, end: usize) -> Value {
        self.value_of(start, end).unwrap_or_else(Value::unknown)
    }

    fn value_of(&self, start: usize, end: usize) -> Option<Value> {
        let reader = self.reader;
        if start >= end {
            return None;
        }
        if reader.kind(start) == Some(Tok::LParen) && matching(reader, start)? + 1 == end {
            if reader.is_word(start + 1, "SELECT") {
                return None;
            }
            return self.value_of(start + 1, end - 1);
        }
        if end - start == 1 {
            return self.literal(start);
        }
        if end - start == 3
            && let Some((alias, field)) = reader.path(start)
        {
            return self.path_value(&alias, &field);
        }
        if reader.is_word(start, "CASE") && reader.is_word(end - 1, "END") {
            return self.case(start, end);
        }
        if let Some((name, args)) = self.function(start, end) {
            return self.call(name, &args);
        }
        self.arithmetic(start, end)
    }

    /// The literal at `i`.
    fn literal(&self, i: usize) -> Option<Value> {
        let reader = self.reader;
        if reader.is_word(i, "NULL") {
            return Some(Value::Known(Fetched::null()));
        }
        if reader.is_word(i, "TRUE") || reader.is_word(i, "FALSE") {
            let value = reader.is_word(i, "TRUE");
            return Some(match self.driver() {
                Some(driver) if driver.is_pgsql() => Value::of(Scalar::Bool(Some(value)), false),
                Some(_) => Value::of(Scalar::Int(Some(i64::from(value))), false),
                None => Value::Unknown { nullable: false },
            });
        }
        let text = reader.text(i);
        if let Some(inner) = text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
            return Some(Value::of(
                Scalar::String {
                    value: Some(inner.replace("''", "'")),
                    written: true,
                },
                false,
            ));
        }
        if !text.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        if !text.contains(['.', 'e', 'E']) {
            return text
                .parse()
                .ok()
                .map(|n| Value::of(Scalar::Int(Some(n)), false));
        }
        // MySQL gives a decimal literal as the string it is, and a float
        // only for one with an exponent; PostgreSQL gives a string either
        // way.
        let float: f64 = text.parse().ok()?;
        let has_exponent = text.contains(['e', 'E']);
        Some(match self.driver() {
            Some(driver) if driver.is_mysql() && !has_exponent => {
                Value::of(Scalar::string(text), false)
            }
            Some(driver) if driver.is_pgsql() => {
                let text = if has_exponent {
                    php_float_string(float)
                } else {
                    text.to_string()
                };
                Value::of(Scalar::string(text), false)
            }
            Some(_) => Value::of(Scalar::Float(Some(float)), false),
            None => Value::Unknown { nullable: false },
        })
    }

    /// The field `alias.field` as the database gives it, which is the
    /// column's type rather than the property's: a string column is a
    /// string on every driver, an integer one an integer, and a boolean
    /// one an integer except on PostgreSQL.
    fn path_value(&self, alias: &str, field: &str) -> Option<Value> {
        let decl = self.decls.iter().find(|d| d.alias == alias)?;
        let Field::Column(ty) = self.model.field(decl.entity.as_ref()?, field)? else {
            return None;
        };
        let nullable = decl.optional || self.aggregate_without_group_by || ty.accepts_null();
        let non_null = ty.non_null_type().unwrap_or(ty);
        Some(if non_null.is_string_type() {
            Value::of(Scalar::any_string(), nullable)
        } else if non_null.is_int() {
            Value::of(Scalar::Int(None), nullable)
        } else if non_null.is_bool() {
            match self.driver() {
                Some(driver) if driver.is_pgsql() => Value::of(Scalar::Bool(None), nullable),
                Some(_) => Value::Known(Fetched {
                    members: vec![Scalar::Int(Some(0)), Scalar::Int(Some(1))],
                    nullable,
                }),
                None => Value::Unknown { nullable },
            }
        } else {
            Value::Unknown { nullable }
        })
    }

    /// The function call `name(args)`.
    fn call(&self, name: &str, args: &[(usize, usize)]) -> Option<Value> {
        let reader = self.reader;
        let is = |n: &str| name.eq_ignore_ascii_case(n);
        let any_nullable =
            |args: &[(usize, usize)]| args.iter().any(|&(s, e)| self.value(s, e).nullable());
        let string = |nullable: bool| Some(Value::of(Scalar::any_string(), nullable));
        let non_negative = |nullable: bool| Some(Value::of(Scalar::NonNegativeInt, nullable));
        let driver = self.driver();
        if is("COUNT") {
            non_negative(false)
        } else if is("SUM") || is("AVG") || is("MIN") || is("MAX") {
            let &[(mut s, e)] = args else {
                return None;
            };
            if reader.is_word(s, "DISTINCT") {
                s += 1;
            }
            Some(self.aggregate(name, self.value(s, e)))
        } else if is("LENGTH") || is("BIT_AND") || is("BIT_OR") {
            non_negative(any_nullable(args))
        } else if is("LOCATE") {
            non_negative(any_nullable(args.get(..2)?))
        } else if is("CONCAT") || is("LOWER") || is("UPPER") || is("SUBSTRING") {
            string(any_nullable(args))
        } else if is("TRIM") {
            // `TRIM([[LEADING | TRAILING | BOTH] [char] FROM] string)`.
            let &[(s, e)] = args else {
                return None;
            };
            let from = (s..e)
                .rev()
                .find(|&i| reader.is_word(i, "FROM"))
                .map_or(s, |i| i + 1);
            string(self.value(from, e).nullable())
        } else if is("CURRENT_DATE") || is("CURRENT_TIME") || is("CURRENT_TIMESTAMP") {
            args.is_empty()
                .then(|| Value::of(Scalar::any_string(), false))
        } else if is("DATE_ADD") || is("DATE_SUB") {
            string(any_nullable(args.get(..2)?))
        } else if is("DATE_DIFF") {
            let nullable = any_nullable(args.get(..2)?);
            Some(match driver {
                Some(driver) if driver.is_sqlite() => Value::of(Scalar::Float(None), nullable),
                Some(_) => Value::of(Scalar::Int(None), nullable),
                None => Value::Unknown { nullable },
            })
        } else if is("ABS") {
            let &[(s, e)] = args else {
                return None;
            };
            let Value::Known(fetched) = self.value(s, e).read_as_float() else {
                return Some(Value::unknown());
            };
            let fetched = fetched.generalized(false);
            Some(if fetched.is_integer() {
                Value::of(Scalar::NonNegativeInt, fetched.nullable)
            } else if fetched.is_numeric() {
                Value::Known(fetched)
            } else {
                Value::unknown()
            })
        } else if is("MOD") {
            let &[(s1, e1), (s2, e2)] = args else {
                return None;
            };
            let (Value::Known(a), Value::Known(b)) = (self.value(s1, e1), self.value(s2, e2))
            else {
                return Some(Value::unknown());
            };
            if !Fetched::union([a.clone(), b.clone()]).is_integer() {
                return Some(Value::unknown());
            }
            // `MOD(x, 0)` is `NULL` where the database does not refuse it.
            let may_be_zero = b
                .members
                .iter()
                .any(|m| !matches!(m, Scalar::Int(Some(n)) if *n != 0));
            let by_zero = may_be_zero && !driver.is_some_and(Driver::is_pgsql);
            Some(Value::of(
                Scalar::Int(None),
                a.nullable || b.nullable || by_zero,
            ))
        } else if is("SQRT") {
            let &[(s, e)] = args else {
                return None;
            };
            Some(self.sqrt(self.value(s, e)))
        } else if is("COALESCE") {
            Some(self.coalesce(args))
        } else if is("NULLIF") {
            let &[(s, e), _] = args else {
                return None;
            };
            Some(match self.value(s, e) {
                Value::Known(fetched) => Value::Known(fetched.or_null(true)),
                Value::Unknown { .. } => Value::unknown(),
            })
        } else {
            None
        }
    }

    /// What the aggregate function `name` gives for `arg`.
    fn aggregate(&self, name: &str, arg: Value) -> Value {
        let without_group_by = self.aggregate_without_group_by;
        if name.eq_ignore_ascii_case("MIN") || name.eq_ignore_ascii_case("MAX") {
            // The column's own type.
            return match arg {
                Value::Known(fetched) => Value::Known(fetched.generalized(without_group_by)),
                Value::Unknown { .. } => Value::unknown(),
            };
        }
        let Value::Known(fetched) = arg.read_as_number(self.driver()) else {
            return Value::unknown();
        };
        let nullable = fetched.nullable || without_group_by;
        let Some(driver) = self.driver() else {
            return Value::unknown();
        };
        let integer = fetched.is_integer();
        let non_numeric_string = fetched.is_string() && !fetched.is_numeric_string();
        let general = Value::Known(fetched.generalized(nullable));
        if name.eq_ignore_ascii_case("AVG") {
            if driver.is_sqlite() {
                return Value::of(Scalar::Float(None), nullable);
            }
            if integer {
                return Value::of(Scalar::NumericString, nullable);
            }
            if driver.is_mysql() && non_numeric_string {
                return Value::of(Scalar::Float(None), nullable);
            }
            return general;
        }
        // `SUM`.
        if integer && driver.is_mysql() {
            Value::of(Scalar::NumericString, nullable)
        } else if integer && driver.is_pgsql() {
            Value::Known(Fetched {
                members: vec![Scalar::Int(None), Scalar::NumericString],
                nullable,
            })
        } else if non_numeric_string && !driver.is_pgsql() {
            Value::of(Scalar::Float(None), nullable)
        } else {
            general
        }
    }

    /// What `SQRT()` gives for `arg`.
    fn sqrt(&self, arg: Value) -> Value {
        let Value::Known(fetched) = arg else {
            return Value::unknown();
        };
        if !fetched.is_numeric() {
            return Value::unknown();
        }
        let Some(driver) = self.driver() else {
            return Value::unknown();
        };
        if !driver.is_pgsql() {
            // The square root of a negative number is `NULL`.
            let may_be_negative = fetched.members.iter().any(|m| match m {
                Scalar::NonNegativeInt => false,
                Scalar::Int(Some(n)) => *n < 0,
                Scalar::Float(Some(f)) => *f < 0.0,
                Scalar::String {
                    value: Some(value), ..
                } => value.trim().parse::<f64>().is_ok_and(|f| f < 0.0),
                _ => true,
            });
            return Value::of(Scalar::Float(None), fetched.nullable || may_be_negative);
        }
        let nullable = fetched.nullable;
        let number = match Value::Known(Fetched {
            members: fetched.members,
            nullable: false,
        })
        .read_as_number(Some(driver))
        {
            Value::Known(number) => number,
            Value::Unknown { .. } => return Value::unknown(),
        };
        let members = if number.is_integer() || number.is_float() {
            vec![Scalar::Float(None)]
        } else if number.is_numeric_string() {
            vec![Scalar::NumericString]
        } else {
            vec![Scalar::Float(None), Scalar::NumericString]
        };
        Value::Known(Fetched { members, nullable })
    }

    /// What `COALESCE()` of `args` gives: one of its arguments, `null` only
    /// when every one may be.  MySQL converts the arguments to one type: a
    /// float when one is, and a string when one is.
    fn coalesce(&self, args: &[(usize, usize)]) -> Value {
        let values: Vec<Value> = args.iter().map(|&(s, e)| self.value(s, e)).collect();
        let all_nullable = values.iter().all(Value::nullable);
        let raw = match all_known(values) {
            Ok(raw) => raw,
            Err(_) => {
                return Value::Unknown {
                    nullable: all_nullable,
                };
            }
        };
        let mut result = Fetched::union(raw.iter().map(|r| r.generalized(false)));
        result.nullable = all_nullable;
        if self.driver().is_some_and(Driver::is_mysql) {
            let contains_float = raw.iter().any(Fetched::is_float);
            let all_numbers = raw.iter().all(|r| r.is_numeric() && !r.is_written_string());
            let contains_string = raw.iter().any(Fetched::is_string);
            if contains_float && all_numbers {
                result = result.map(|m| vec![m.cast_to_float()]);
            } else if contains_string {
                result = result.map(|m| match m {
                    Scalar::Float(None) => vec![Scalar::NumericString],
                    other => other.cast_to_string(),
                });
            }
        }
        Value::Known(result)
    }

    /// The arithmetic expression in `start..end`: terms joined by `+` and
    /// `-`, factors joined by `*` and `/`, or a signed factor.
    fn arithmetic(&self, start: usize, end: usize) -> Option<Value> {
        let driver = self.driver();
        let plus_minus = self.binary_operators(start, end, &["+", "-"])?;
        if !plus_minus.is_empty() {
            let terms = split_at(start, end, &plus_minus)
                .into_iter()
                .map(|(s, e)| self.value(s, e).read_as_number(driver))
                .collect();
            return Some(plus_minus_times(terms, driver));
        }
        let times_divide = self.binary_operators(start, end, &["*", "/"])?;
        if !times_divide.is_empty() {
            let factors = split_at(start, end, &times_divide)
                .into_iter()
                .map(|(s, e)| self.value(s, e).read_as_number(driver))
                .collect();
            let divides = times_divide.iter().any(|&i| self.reader.text(i) == "/");
            return Some(if divides {
                division(factors, driver)
            } else {
                plus_minus_times(factors, driver)
            });
        }
        let sign = self.reader.text(start);
        if sign != "-" && sign != "+" {
            return None;
        }
        let value = self.value_of(start + 1, end)?;
        if sign == "+" {
            return Some(value);
        }
        Some(match value {
            Value::Known(fetched) => Value::Known(fetched.map(|m| {
                vec![match m {
                    Scalar::Int(Some(n)) => Scalar::Int(n.checked_neg()),
                    Scalar::NonNegativeInt => Scalar::Int(None),
                    Scalar::Float(Some(f)) => Scalar::Float(Some(-f)),
                    other => other.clone(),
                }]
            })),
            unknown => unknown,
        })
    }

    /// The top-level binary operators among `operators` in `start..end`.
    /// `None` when the range holds an operator that is no arithmetic.
    fn binary_operators(&self, start: usize, end: usize, operators: &[&str]) -> Option<Vec<usize>> {
        let reader = self.reader;
        let mut found = Vec::new();
        let mut depth = 0usize;
        for i in start..end {
            match reader.tokens[i].kind {
                Tok::LParen => depth += 1,
                Tok::RParen => depth = depth.checked_sub(1)?,
                Tok::Ident if reader.is_word(i, "CASE") => depth += 1,
                Tok::Ident if reader.is_word(i, "END") => depth = depth.checked_sub(1)?,
                Tok::Other if depth == 0 => {
                    let text = reader.text(i);
                    if text
                        .starts_with(|c: char| c.is_ascii_digit() || matches!(c, '\'' | ':' | '?'))
                    {
                        continue;
                    }
                    if !["+", "-", "*", "/"].contains(&text) {
                        return None;
                    }
                    // A sign, rather than an operator, follows another
                    // operator or opens the expression.
                    let binary = i > start
                        && match reader.kind(i - 1) {
                            Some(Tok::Other) => !["+", "-", "*", "/"].contains(&reader.text(i - 1)),
                            Some(Tok::RParen | Tok::Ident) => true,
                            _ => false,
                        };
                    if binary && operators.contains(&text) {
                        found.push(i);
                    }
                }
                _ => {}
            }
        }
        Some(found)
    }

    /// The type of `CASE ... END` in `start..end`: one of its results, or
    /// `null` when it has no `ELSE`.
    fn case(&self, start: usize, end: usize) -> Option<Value> {
        let reader = self.reader;
        // The `WHEN`, `THEN`, and `ELSE` of this `CASE`, outside the
        // parentheses and the `CASE`s nested in it.
        let mut marks = Vec::new();
        let mut depth = 0usize;
        for i in start + 1..end - 1 {
            match reader.tokens[i].kind {
                Tok::LParen => depth += 1,
                Tok::RParen => depth = depth.checked_sub(1)?,
                Tok::Ident if reader.is_word(i, "CASE") => depth += 1,
                Tok::Ident if reader.is_word(i, "END") => depth = depth.checked_sub(1)?,
                Tok::Ident
                    if depth == 0
                        && ["WHEN", "THEN", "ELSE"]
                            .iter()
                            .any(|k| reader.is_word(i, k)) =>
                {
                    marks.push(i);
                }
                _ => {}
            }
        }
        if depth != 0 {
            return None;
        }
        marks.push(end - 1);
        let mut results = Vec::new();
        let mut has_else = false;
        for pair in marks.windows(2) {
            let (mark, next) = (pair[0], pair[1]);
            let is_else = reader.is_word(mark, "ELSE");
            if !is_else && !reader.is_word(mark, "THEN") {
                continue;
            }
            has_else |= is_else;
            results.push(self.value(mark + 1, next));
        }
        if results.is_empty() {
            return None;
        }
        Some(match all_known(results) {
            Ok(results) => Value::Known(Fetched::union(results).or_null(!has_else)),
            Err(unknown) => unknown,
        })
    }
}

/// The ranges between the operators at `operators` in `start..end`.
fn split_at(start: usize, end: usize, operators: &[usize]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(operators.len() + 1);
    let mut from = start;
    for &op in operators {
        out.push((from, op));
        from = op + 1;
    }
    out.push((from, end));
    out
}

/// The index of the `)` that closes the `(` at `open`.
fn matching(reader: &Reader<'_>, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in open..reader.tokens.len() {
        match reader.tokens[i].kind {
            Tok::LParen => depth += 1,
            Tok::RParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the `SELECT` clause in `start..end` calls an aggregate function
/// outside a subquery.
fn selects_aggregate(reader: &Reader<'_>, start: usize, end: usize) -> bool {
    let mut i = start;
    while i < end {
        if reader.kind(i) == Some(Tok::LParen) && reader.is_word(i + 1, "SELECT") {
            match matching(reader, i) {
                Some(close) => i = close,
                None => return false,
            }
        } else if reader.kind(i + 1) == Some(Tok::LParen)
            && AGGREGATES.iter().any(|name| reader.is_word(i, name))
        {
            return true;
        }
        i += 1;
    }
    false
}

/// The `TKey` and `TResult` of a query whose DQL is `text`, or `None` when
/// the DQL is not read.
fn result_types(text: &str, model: &dyn EntityModel) -> Option<(PhpType, PhpType)> {
    let reader = Reader {
        text,
        tokens: tokenize(text),
    };
    if reader.tokens.iter().any(|t| t.kind == Tok::Dynamic) {
        return None;
    }
    if reader.is_word(0, "UPDATE") || reader.is_word(0, "DELETE") {
        let at = if reader.is_word(1, "FROM") { 2 } else { 1 };
        model.entity(reader.word(at)?)?;
        return Some((PhpType::void(), PhpType::void()));
    }
    if !reader.is_word(0, "SELECT") {
        return None;
    }
    let from = reader.clause_end(1, &["FROM"])?;
    if from == reader.tokens.len() {
        return None;
    }
    let from_end = reader.clause_end(from + 1, &["WHERE", "GROUP", "HAVING", "ORDER"])?;
    let decls = reader.range_declarations(from + 1, from_end, model)?;

    let mut select_start = 1;
    if reader.is_word(1, "DISTINCT") {
        select_start = 2;
    }
    let group_by = (from_end..reader.tokens.len().saturating_sub(1))
        .any(|i| reader.is_word(i, "GROUP") && reader.is_word(i + 1, "BY"));
    let scope = Scope {
        reader: &reader,
        decls: &decls,
        model,
        aggregate_without_group_by: !group_by && selects_aggregate(&reader, select_start, from),
        platform: model.platform(),
    };
    let mut entities: Vec<(Option<String>, PhpType)> = Vec::new();
    let mut scalars: Vec<(String, PhpType)> = Vec::new();
    let mut new_objects: Vec<(String, PhpType)> = Vec::new();
    let mut selected_entities: Vec<String> = Vec::new();
    let mut selected_fields: Vec<(String, String)> = Vec::new();
    // Doctrine keys a scalar without a result variable by a counter from 1,
    // which each argument of a `NEW` advances too, and a `NEW` without one
    // by a counter from 0.
    let mut scalar_counter = 1usize;
    let mut new_counter = 0usize;
    for (start, end) in top_level_items(&reader, select_start, from)? {
        let (selected, result_variable, hidden) = select_expression(&reader, start, end)?;
        match &selected {
            Selected::Entity { alias } => selected_entities.push(alias.clone()),
            Selected::Field { alias, field } => {
                selected_fields.push((alias.clone(), field.clone()))
            }
            Selected::New { .. } | Selected::Other { .. } => {}
        }
        if hidden {
            continue;
        }
        match selected {
            Selected::Entity { alias } => {
                let decl = decls.iter().find(|d| d.alias == alias)?;
                if !decl.in_row {
                    continue;
                }
                let entity = PhpType::named(decl.entity?);
                let nullable = decl.optional || scope.aggregate_without_group_by;
                entities.push((result_variable, or_null(entity, nullable)));
            }
            Selected::Field { alias, field } => {
                let ty = scope.field(&alias, &field)?;
                scalars.push((result_variable.unwrap_or(field), ty));
            }
            Selected::New { class, args } => {
                let class = model.class(&class)?;
                scalar_counter += args;
                let key = result_variable.unwrap_or_else(|| {
                    new_counter += 1;
                    (new_counter - 1).to_string()
                });
                new_objects.push((key, PhpType::named(class)));
            }
            Selected::Other { start, end } => {
                let key = result_variable.unwrap_or_else(|| {
                    scalar_counter += 1;
                    (scalar_counter - 1).to_string()
                });
                scalars.push((key, scope.select_type(start, end)));
            }
        }
    }

    // How Doctrine's `ObjectHydrator` lays out a row, as PHPStan's
    // `QueryResultTypeBuilder` has it: a lone `NEW` object is the row; an
    // entity is the row unless a scalar, another `NEW` object, or a result
    // variable of an entity makes it a shape; and each entity selected
    // beside others is a row of its own, the last carrying the scalars.
    let key = index_key(&decls, model, &selected_entities, &selected_fields)?;
    if new_objects.len() == 1 && scalars.is_empty() {
        let (_, object) = new_objects.pop()?;
        return Some((key, object));
    }
    let is_shape = !scalars.is_empty()
        || new_objects.len() > 1
        || entities.iter().any(|(key, _)| key.is_some());
    scalars.extend(new_objects);
    let result = if entities.is_empty() {
        if scalars.is_empty() {
            return None;
        }
        shape(scalars)
    } else if !is_shape {
        PhpType::union(entities.into_iter().map(|(_, ty)| ty).collect())
    } else {
        let last = entities.len() - 1;
        let mut rows = Vec::new();
        for (i, (key, entity)) in entities.into_iter().enumerate() {
            let mut entries = vec![(key.unwrap_or_else(|| "0".to_string()), entity)];
            if i == last {
                entries.extend(std::mem::take(&mut scalars));
            }
            rows.push(shape(entries));
        }
        PhpType::union(rows)
    };
    Some((key, result))
}

/// The `TKey` of a query whose root's `INDEX BY` is read in `decls`, given
/// the entities and fields its `SELECT` names.
fn index_key(
    decls: &[Decl],
    model: &dyn EntityModel,
    selected_entities: &[String],
    selected_fields: &[(String, String)],
) -> Option<PhpType> {
    // `INDEX BY` keys the rows by the field when the entity it indexes is
    // hydrated, or when the field is selected itself; otherwise the rows
    // are a list (Doctrine's `SqlWalker::walkIndexBy()`).
    let root = decls.iter().find(|d| d.root)?;
    let index_by = root.index_by.as_ref().filter(|(alias, field)| {
        selected_entities.contains(alias)
            || selected_fields
                .iter()
                .any(|(a, f)| a == alias && f == field)
    });
    match index_by {
        None => Some(PhpType::null()),
        Some((alias, field)) => {
            let decl = decls.iter().find(|d| d.alias == *alias)?;
            match model.field(decl.entity.as_ref()?, field)? {
                Field::Column(ty) => Some(ty),
                Field::Association(_) => None,
            }
        }
    }
}

/// The token ranges of the comma-separated items in `start..end`.
fn top_level_items(reader: &Reader<'_>, start: usize, end: usize) -> Option<Vec<(usize, usize)>> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut item_start = start;
    for i in start..end {
        match reader.tokens[i].kind {
            Tok::LParen | Tok::LBrace => depth += 1,
            Tok::RParen | Tok::RBrace => depth = depth.checked_sub(1)?,
            Tok::Comma if depth == 0 => {
                items.push((item_start, i));
                item_start = i + 1;
            }
            _ => {}
        }
    }
    items.push((item_start, end));
    items.iter().all(|(s, e)| s < e).then_some(items)
}

/// Read the `SELECT` expression in `start..end`: what it selects, the
/// result variable it names, and whether it is `HIDDEN`.
fn select_expression(
    reader: &Reader<'_>,
    start: usize,
    end: usize,
) -> Option<(Selected, Option<String>, bool)> {
    // `[AS] [HIDDEN] name` ends the expression.
    let mut core_end = end;
    let mut result_variable = None;
    let mut hidden = false;
    if end - start >= 2
        && let Some(name) = reader.variable(end - 1)
    {
        let mut before = end - 1;
        if reader.is_word(before - 1, "HIDDEN") {
            hidden = true;
            before -= 1;
        }
        if before > start && reader.is_word(before - 1, "AS") {
            result_variable = Some(name.to_string());
            core_end = before - 1;
        } else if hidden
            || simple_expression_end(reader, start) == Some(before)
            || ends_expression(reader, before - 1)
        {
            result_variable = Some(name.to_string());
            core_end = before;
        }
    }
    if core_end <= start {
        return None;
    }

    let selected = if core_end - start == 1
        && let Some(alias) = reader.variable(start)
    {
        Selected::Entity {
            alias: alias.to_string(),
        }
    } else if reader.is_word(start, "PARTIAL") {
        let alias = reader.variable(start + 1)?;
        if reader.kind(start + 2) != Some(Tok::Dot) || reader.kind(start + 3) != Some(Tok::LBrace) {
            return None;
        }
        Selected::Entity {
            alias: alias.to_string(),
        }
    } else if core_end - start == 3
        && let Some((alias, field)) = reader.path(start)
    {
        Selected::Field { alias, field }
    } else if reader.is_word(start, "NEW") {
        let class = reader.word(start + 1)?;
        if reader.kind(start + 2) != Some(Tok::LParen)
            || matching(reader, start + 2)? + 1 != core_end
        {
            return None;
        }
        let args = if start + 3 == core_end - 1 {
            0
        } else {
            top_level_items(reader, start + 3, core_end - 1)?.len()
        };
        Selected::New {
            class: class.to_string(),
            args,
        }
    } else {
        Selected::Other {
            start,
            end: core_end,
        }
    };
    Some((selected, result_variable, hidden))
}

/// Whether the token at `i` can end a scalar expression, so that a name
/// after it is a result variable: a `)`, a literal, or `END`.
fn ends_expression(reader: &Reader<'_>, i: usize) -> bool {
    match reader.kind(i) {
        Some(Tok::RParen) => true,
        Some(Tok::Other) => {
            let text = reader.text(i);
            text.starts_with('\'') || text.starts_with(|c: char| c.is_ascii_digit())
        }
        Some(Tok::Ident) => ["END", "TRUE", "FALSE", "NULL"]
            .iter()
            .any(|k| reader.is_word(i, k)),
        _ => false,
    }
}

/// Where the variable or path at `start` ends, when one starts there.
fn simple_expression_end(reader: &Reader<'_>, start: usize) -> Option<usize> {
    if reader.path(start).is_some() {
        Some(start + 3)
    } else {
        reader.variable(start).map(|_| start + 1)
    }
}

fn shape(entries: Vec<(String, PhpType)>) -> PhpType {
    PhpType::array_shape(
        entries
            .into_iter()
            .map(|(key, value_type)| ShapeEntry {
                key: Some(key),
                value_type,
                optional: false,
            })
            .collect(),
    )
}

// ─── Hydrating the result ───────────────────────────────────────────────────

/// The position of the hydration mode argument of each result method, and
/// whether the method hydrates objects when it is left out.
fn hydration_argument(method: &str) -> Option<(usize, bool)> {
    const METHODS: &[(&str, usize, bool)] = &[
        ("getResult", 0, true),
        ("toIterable", 1, true),
        ("execute", 1, false),
        ("executeIgnoreQueryCache", 1, false),
        ("executeUsingQueryCache", 1, false),
        ("getOneOrNullResult", 0, false),
        ("getSingleResult", 0, false),
    ];
    METHODS
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(method))
        .map(|&(_, index, defaults_to_object)| (index, defaults_to_object))
}

/// The parameter name of each result method's hydration mode argument.
pub(crate) const HYDRATION_MODE_PARAMETER: &str = "hydrationMode";

/// Whether `method` is a result method whose type the query's `TResult`
/// decides, and where its hydration mode argument is.
pub(crate) fn result_method(method: &str) -> Option<usize> {
    hydration_argument(method).map(|(index, _)| index)
}

/// The `TKey` and `TResult` a query type carries.  A single type argument
/// is `TResult` of a query with no `INDEX BY`, the way PHPStan's Doctrine
/// extension reads `Query<T>`.
fn query_arguments(ty: &PhpType) -> Option<(PhpType, PhpType)> {
    let TypeKind::Generic(generic) = ty.kind() else {
        return None;
    };
    if !generic.name.eq_ignore_ascii_case(QUERY)
        && !generic.name.eq_ignore_ascii_case(ABSTRACT_QUERY)
    {
        return None;
    }
    match generic.args.as_slice() {
        [result] => Some((PhpType::null(), result.clone())),
        [key, result] => Some((key.clone(), result.clone())),
        _ => None,
    }
}

/// Whether `receiver` is a query that carries its `TKey` and `TResult`,
/// which a result method's type can be read from.
pub(crate) fn carries_result(receiver: &PhpType) -> bool {
    receiver
        .union_members()
        .iter()
        .all(|member| query_arguments(member).is_some())
}

/// What the result method `method` returns on a query of type `receiver`
/// in the hydration mode `mode` (`None` when the call leaves it out,
/// `Some(None)` when it passes one that is not known), or `None` when the
/// method's declared type is all that is known.  `is_class` says whether a
/// name is a class, as opposed to a template parameter with no bound.
pub(crate) fn hydrated_result(
    method: &str,
    receiver: &PhpType,
    mode: Option<Option<i64>>,
    is_class: &dyn Fn(&str) -> bool,
) -> Option<PhpType> {
    let (_, defaults_to_object) = hydration_argument(method)?;
    let mut types = Vec::new();
    for member in receiver.union_members() {
        let (key, result) = query_arguments(member)?;
        if !result.is_void() && !is_known_result(&result, is_class) {
            return None;
        }
        types.push(hydrated_member(
            method,
            key,
            result,
            mode,
            defaults_to_object,
        )?);
    }
    (!types.is_empty()).then(|| PhpType::union(types))
}

fn hydrated_member(
    method: &str,
    key: PhpType,
    result: PhpType,
    mode: Option<Option<i64>>,
    defaults_to_object: bool,
) -> Option<PhpType> {
    if result.is_void() {
        // An `UPDATE` or `DELETE`: every method gives the affected rows.
        return Some(PhpType::int_range("0", "max"));
    }
    let mode = match mode {
        None if defaults_to_object => HYDRATE_OBJECT,
        None => return None,
        Some(mode) => mode?,
    };
    match mode {
        HYDRATE_OBJECT => {}
        HYDRATE_SIMPLEOBJECT if is_objects(&result) => {}
        _ => return None,
    }
    let is = |name: &str| method.eq_ignore_ascii_case(name);
    Some(if is("getSingleResult") {
        result
    } else if is("getOneOrNullResult") {
        PhpType::union(vec![result, PhpType::null()])
    } else if is("toIterable") {
        let key = if key.is_null() { PhpType::int() } else { key };
        PhpType::generic("iterable", vec![key, result])
    } else if key.is_null() {
        PhpType::list(result)
    } else {
        PhpType::generic("array", vec![key, result])
    })
}

/// Whether `result` says what a row is: not `mixed`, and not a template
/// parameter that could stand for the `void` of an `UPDATE` or `DELETE`.
/// A template parameter bounded by rows reads as its bound.
fn is_known_result(result: &PhpType, is_class: &dyn Fn(&str) -> bool) -> bool {
    result.union_members().iter().all(|member| {
        if let Some((_, bound)) = member.as_template_param() {
            return is_known_result(bound, is_class);
        }
        is_known_row(member, is_class)
    })
}

fn is_known_row(member: &PhpType, is_class: &dyn Fn(&str) -> bool) -> bool {
    match member.kind() {
        TypeKind::Named(name) if crate::php_type::is_keyword_type(name.as_str()) => {
            !name.eq_ignore_ascii_case("mixed") && !name.eq_ignore_ascii_case("void")
        }
        TypeKind::Named(name) => is_class(name.as_str()),
        TypeKind::ArrayShape(_) | TypeKind::Generic(_) => true,
        _ => false,
    }
}

/// Whether every member of `result` is an object, which is all
/// `HYDRATE_SIMPLEOBJECT` hydrates.
fn is_objects(result: &PhpType) -> bool {
    result.union_members().iter().all(|member| {
        matches!(member.kind(), TypeKind::Named(name)
            if !crate::php_type::is_keyword_type(name.as_str()))
    })
}

// ─── Replaying a query builder ──────────────────────────────────────────────

/// What a query builder's statement does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum StatementKind {
    #[default]
    Select,
    Update,
    Delete,
}

/// What a variable a query builder declares stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AliasRoot<'a> {
    /// The entity a range declaration, or a join of an entity, names.
    Entity(&'a str),
    /// The target of the association `field` of what the variable `alias`
    /// stands for.
    Association { alias: &'a str, field: &'a str },
}

/// A range declaration a builder's `from()` adds, with the joins attached
/// to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
struct FromPart {
    entity: String,
    alias: String,
    index_by: Option<String>,
    joins: Vec<String>,
    /// The variable each join declares, with the association path or
    /// entity it joins.
    join_aliases: Vec<(String, String)>,
}

/// An argument a builder method is called with, as far as its value is
/// known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BuilderArg {
    /// The strings it holds: one for a string, each element for an array.
    Strings(Vec<String>),
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An object `new` makes of the class `class` with the arguments
    /// `args`: an expression object `add()` takes.
    New { class: String, args: Vec<CallArg> },
    /// A value that is not known.
    Unknown,
}

impl BuilderArg {
    /// What a value of type `ty` holds: `null`, the class a class string
    /// names, or a string.
    pub(crate) fn of_type(ty: &PhpType) -> Self {
        if ty.is_null() {
            return BuilderArg::Null;
        }
        if ty.is_true() || ty.is_false() {
            return BuilderArg::Bool(ty.is_true());
        }
        if let TypeKind::ClassString(Some(inner)) = ty.kind()
            && let TypeKind::Named(class) = inner.kind()
        {
            return BuilderArg::Strings(vec![class.to_string()]);
        }
        match string_literals(ty) {
            Some(strings) if strings.len() == 1 => BuilderArg::Strings(strings),
            _ => BuilderArg::Unknown,
        }
    }

    /// The one string the argument holds.
    fn single_string(&self) -> Option<String> {
        self.string().map(str::to_string)
    }

    fn string(&self) -> Option<&str> {
        match self {
            BuilderArg::Strings(strings) => match strings.as_slice() {
                [only] => Some(only),
                _ => None,
            },
            _ => None,
        }
    }

    /// The string an optional argument holds: `Some(None)` when left out or
    /// `null`, `None` when not known.
    fn optional_string(arg: Option<&Self>) -> Option<Option<String>> {
        match arg {
            None | Some(BuilderArg::Null) => Some(None),
            Some(arg) => arg.string().map(|s| Some(s.to_string())),
        }
    }
}

/// An argument of a builder call as written: its name when it is named,
/// whether it is spread, and what it holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CallArg {
    pub(crate) name: Option<String>,
    pub(crate) spread: bool,
    pub(crate) value: BuilderArg,
}

/// `args` bound to the parameters `params`, in order.  `None` when an
/// argument is spread, or names no parameter.
fn bind(args: Vec<CallArg>, params: &[&str]) -> Option<Vec<Option<BuilderArg>>> {
    let mut bound: Vec<Option<BuilderArg>> = vec![None; params.len()];
    let mut position = 0;
    for arg in args {
        if arg.spread {
            return None;
        }
        let index = match &arg.name {
            Some(name) => params.iter().position(|p| p == name)?,
            None => {
                position += 1;
                position - 1
            }
        };
        *bound.get_mut(index)? = Some(arg.value);
    }
    Some(bound)
}

/// The arguments of a call of the builder method `method`, bound to its
/// parameters (every argument, in order, for a variadic method).  `None`
/// for a method whose effect is not known.
pub(crate) fn builder_args(method: &str, args: Vec<CallArg>) -> Option<Vec<Option<BuilderArg>>> {
    match builder_parameters(method)? {
        Some(params) => bind(args, params),
        // A spread argument holds values that are not known.
        None => Some(
            args.into_iter()
                .map(|arg| {
                    Some(if arg.spread || arg.name.is_some() {
                        BuilderArg::Unknown
                    } else {
                        arg.value
                    })
                })
                .collect(),
        ),
    }
}

/// The parts of the DQL a query builder has built so far that decide its
/// result.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct QueryBuilderState {
    kind: StatementKind,
    select: Vec<String>,
    from: Vec<FromPart>,
}

/// The builder methods that only filter, order, or page the rows, or set
/// the query's parameters or caching: the result's type is the same
/// whatever their arguments.
const RESULT_NEUTRAL_METHODS: &[&str] = &[
    "where",
    "andWhere",
    "orWhere",
    "having",
    "andHaving",
    "orHaving",
    "orderBy",
    "addOrderBy",
    "groupBy",
    "addGroupBy",
    "set",
    "setParameter",
    "setParameters",
    "setFirstResult",
    "setMaxResults",
    "addCriteria",
    "distinct",
    "setCacheable",
    "setCacheRegion",
    "setLifetime",
    "setCacheMode",
];

/// The parameter names of the builder methods that shape the result, in
/// order; a variadic method's names are `None`.
pub(crate) fn builder_parameters(method: &str) -> Option<Option<&'static [&'static str]>> {
    const METHODS: &[(&str, Option<&[&str]>)] = &[
        ("select", None),
        ("addSelect", None),
        ("from", Some(&["from", "alias", "indexBy"])),
        (
            "join",
            Some(&["join", "alias", "conditionType", "condition", "indexBy"]),
        ),
        (
            "innerJoin",
            Some(&["join", "alias", "conditionType", "condition", "indexBy"]),
        ),
        (
            "leftJoin",
            Some(&["join", "alias", "conditionType", "condition", "indexBy"]),
        ),
        ("indexBy", Some(&["alias", "indexBy"])),
        ("delete", Some(&["delete", "alias"])),
        ("update", Some(&["update", "alias"])),
        ("add", Some(&["dqlPartName", "dqlPart", "append"])),
    ];
    METHODS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(method))
        .map(|&(_, params)| params)
}

/// Whether a call of `method` leaves the result's type as it was.
pub(crate) fn is_result_neutral(method: &str) -> bool {
    RESULT_NEUTRAL_METHODS
        .iter()
        .any(|name| name.eq_ignore_ascii_case(method))
}

/// Whether `method` is a builder method [`QueryBuilderState::apply`]
/// replays: one that returns the builder it was called on, changed or not.
pub(crate) fn is_replayed(method: &str) -> bool {
    is_result_neutral(method) || builder_parameters(method).is_some()
}

/// The DQL parts `add()` sets that only filter, order, or group the rows.
const RESULT_NEUTRAL_PARTS: &[&str] = &["distinct", "set", "where", "groupBy", "having", "orderBy"];

/// `Doctrine\ORM\Query\Expr\Select`, which `add('select', …)` takes.
const EXPR_SELECT: &str = "Doctrine\\ORM\\Query\\Expr\\Select";

/// `Doctrine\ORM\Query\Expr\From`, which `add('from', …)` takes.
const EXPR_FROM: &str = "Doctrine\\ORM\\Query\\Expr\\From";

/// Whether `arg` is an object `new` makes of `class`, and with which
/// arguments.
fn new_of<'a>(arg: &'a BuilderArg, class: &str) -> Option<&'a [CallArg]> {
    match arg {
        BuilderArg::New { class: name, args }
            if name
                .strip_prefix('\\')
                .unwrap_or(name)
                .eq_ignore_ascii_case(class) =>
        {
            Some(args)
        }
        _ => None,
    }
}

impl QueryBuilderState {
    /// The builder `EntityRepository::createQueryBuilder($alias, $indexBy)`
    /// returns for `entity`, given the call's arguments.
    pub(crate) fn for_repository(entity: &str, args: Vec<CallArg>) -> Option<Self> {
        let args = bind(args, &["alias", "indexBy"])?;
        let alias = args.first()?.as_ref()?.single_string()?;
        let index_by = BuilderArg::optional_string(args.get(1).and_then(Option::as_ref))?;
        Some(Self {
            kind: StatementKind::Select,
            select: vec![alias.clone()],
            from: vec![FromPart {
                entity: entity.to_string(),
                alias,
                index_by,
                ..FromPart::default()
            }],
        })
    }

    /// Apply a call of `method`, given its arguments bound to the
    /// parameters [`builder_parameters`] lists (every positional argument
    /// for a variadic method).  `None` when the call's effect on the result
    /// is not known.
    pub(crate) fn apply(&mut self, method: &str, args: &[Option<BuilderArg>]) -> Option<()> {
        if is_result_neutral(method) {
            return Some(());
        }
        let is = |name: &str| method.eq_ignore_ascii_case(name);
        let arg = |i: usize| args.get(i).and_then(Option::as_ref);
        let required = |i: usize| arg(i).and_then(BuilderArg::string).map(str::to_string);
        if is("select") || is("addSelect") {
            self.kind = StatementKind::Select;
            let mut selects = Vec::new();
            for arg in args {
                match arg.as_ref()? {
                    BuilderArg::Strings(strings) => selects.extend(strings.iter().cloned()),
                    _ => return None,
                }
            }
            if is("select") {
                if selects.is_empty() {
                    return Some(());
                }
                self.select = selects;
            } else {
                self.select.extend(selects);
            }
        } else if is("from") {
            self.from.push(FromPart {
                entity: required(0)?,
                alias: required(1)?,
                index_by: BuilderArg::optional_string(arg(2))?,
                ..FromPart::default()
            });
        } else if is("join") || is("innerJoin") || is("leftJoin") {
            let join = required(0)?;
            let alias = required(1)?;
            let index_by = BuilderArg::optional_string(arg(4))?;
            let parent = join.split_once('.').map(|(parent, _)| parent);
            let root = self
                .from
                .iter()
                .position(|f| Some(f.alias.as_str()) == parent)
                .or_else(|| {
                    self.from.iter().position(|f| {
                        f.join_aliases
                            .iter()
                            .any(|(a, _)| Some(a.as_str()) == parent)
                    })
                })
                .or_else(|| (!self.from.is_empty()).then_some(0))?;
            let kind = if is("leftJoin") { "LEFT" } else { "INNER" };
            let mut text = format!("{kind} JOIN {join} {alias}");
            if let Some(index_by) = index_by {
                text.push_str(" INDEX BY ");
                text.push_str(&index_by);
            }
            let part = &mut self.from[root];
            part.joins.push(text);
            part.join_aliases.push((alias, join));
        } else if is("indexBy") {
            let alias = required(0)?;
            let index_by = required(1)?;
            self.from.iter_mut().find(|f| f.alias == alias)?.index_by = Some(index_by);
        } else if is("delete") || is("update") {
            self.kind = if is("delete") {
                StatementKind::Delete
            } else {
                StatementKind::Update
            };
        } else if is("add") {
            return self.add(&required(0)?, arg(1)?, arg(2));
        } else {
            return None;
        }
        Some(())
    }

    /// Apply `add($part, $dqlPart, $append)`: an expression object, or the
    /// text of a select expression, for the DQL part `part`.
    fn add(
        &mut self,
        part: &str,
        dql_part: &BuilderArg,
        append: Option<&BuilderArg>,
    ) -> Option<()> {
        if RESULT_NEUTRAL_PARTS
            .iter()
            .any(|name| name.eq_ignore_ascii_case(part))
        {
            return Some(());
        }
        let append = match append {
            None | Some(BuilderArg::Null) => false,
            Some(BuilderArg::Bool(append)) => *append,
            Some(_) => return None,
        };
        if part == "select" {
            let selects = match dql_part {
                BuilderArg::Strings(strings) => strings.clone(),
                _ => {
                    let args = bind(new_of(dql_part, EXPR_SELECT)?.to_vec(), &["args"])?;
                    match args.into_iter().next().flatten() {
                        None => Vec::new(),
                        Some(BuilderArg::Strings(strings)) => strings,
                        Some(_) => return None,
                    }
                }
            };
            self.kind = StatementKind::Select;
            if !append {
                self.select.clear();
            }
            self.select.extend(selects);
        } else if part == "from" {
            let args = bind(
                new_of(dql_part, EXPR_FROM)?.to_vec(),
                &["from", "alias", "indexBy"],
            )?;
            let arg = |i: usize| args.get(i).and_then(Option::as_ref);
            let from = FromPart {
                entity: arg(0)?.single_string()?,
                alias: arg(1)?.single_string()?,
                index_by: BuilderArg::optional_string(arg(2))?,
                ..FromPart::default()
            };
            if !append {
                // The joins are kept apart from the ranges they hang off,
                // so replacing the ranges under them is not followed.
                if self.from.iter().any(|f| !f.joins.is_empty()) {
                    return None;
                }
                self.from.clear();
            }
            self.from.push(from);
        } else {
            return None;
        }
        Some(())
    }

    /// What the variable `alias` the builder has declared stands for.
    pub(crate) fn alias_root(&self, alias: &str) -> Option<AliasRoot<'_>> {
        for from in &self.from {
            if from.alias == alias {
                return Some(AliasRoot::Entity(&from.entity));
            }
            for (joined, join) in &from.join_aliases {
                if joined == alias {
                    return Some(match join.split_once('.') {
                        Some((alias, field)) => AliasRoot::Association { alias, field },
                        None => AliasRoot::Entity(join),
                    });
                }
            }
        }
        None
    }

    /// How many variables the builder has declared.
    pub(crate) fn alias_count(&self) -> usize {
        self.from.iter().map(|f| 1 + f.join_aliases.len()).sum()
    }

    /// The DQL of the `SELECT` built so far.
    fn dql(&self) -> String {
        let mut dql = format!("SELECT {} FROM ", self.select.join(", "));
        for (i, from) in self.from.iter().enumerate() {
            if i > 0 {
                dql.push_str(", ");
            }
            dql.push_str(&from.entity);
            dql.push(' ');
            dql.push_str(&from.alias);
            if let Some(index_by) = &from.index_by {
                dql.push_str(" INDEX BY ");
                dql.push_str(index_by);
            }
            for join in &from.joins {
                dql.push(' ');
                dql.push_str(join);
            }
        }
        dql
    }
}

/// The state of the builder `createQueryBuilder()` returns on `owner`,
/// `generic_args` the type arguments the receiver carries, given the call's
/// arguments: an empty builder from an entity manager, or one selecting the
/// entity from a repository.
pub(crate) fn root_state(
    owner: &ClassInfo,
    generic_args: &[PhpType],
    args: Vec<CallArg>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<QueryBuilderState> {
    if crate::class_lookup::is_subtype_of(owner, ENTITY_MANAGER, class_loader) {
        return args.is_empty().then(QueryBuilderState::default);
    }
    let entity = super::repository_entity_type(owner, generic_args, class_loader)?;
    let TypeKind::Named(entity) = entity.kind() else {
        return None;
    };
    QueryBuilderState::for_repository(entity, args)
}

// ─── The project's mapping ──────────────────────────────────────────────────

/// The [`EntityModel`] of the project a resolution runs in: the entities
/// its mapping knows, and the types their properties declare.
pub(crate) struct ResolverModel<'a, 'c> {
    ctx: &'a ResolutionCtx<'c>,
    state: DoctrineCacheState,
}

impl<'a, 'c> ResolverModel<'a, 'c> {
    pub(crate) fn new(ctx: &'a ResolutionCtx<'c>) -> Self {
        let state = ctx
            .resolved_class_cache
            .map(|cache| cache.read().doctrine().clone())
            .unwrap_or_default();
        Self { ctx, state }
    }

    fn load(&self, name: &str) -> Option<Arc<ClassInfo>> {
        (self.ctx.file.class_loader)(Fqn::new(name.strip_prefix('\\').unwrap_or(name)))
    }
}

impl EntityModel for ResolverModel<'_, '_> {
    fn platform(&self) -> Option<Platform> {
        let backend = self.ctx.file.backend?;
        let connection = super::connection::project_connection(&backend.doctrine_connections)?;
        Platform::of(connection, backend.php_version())
    }

    fn entity(&self, name: &str) -> Option<Atom> {
        let class = self.load(name)?;
        super::is_entity(&class, &self.state).then(|| class.fqn())
    }

    fn class(&self, name: &str) -> Option<Atom> {
        self.load(name).map(|class| class.fqn())
    }

    fn field(&self, entity: &str, field: &str) -> Option<Field> {
        let class = self.load(entity)?;
        let fields = super::entity_fields(&class, self.ctx.file.class_loader, &self.state)?;
        if !fields.iter().any(|f| f == field) {
            return None;
        }
        let class_loader = self.ctx.file.class_loader;
        let ty = super::field_type(&class, field, class_loader, self.ctx.resolved_class_cache);
        Some(
            match super::association(&class, field, &ty, class_loader, &self.state) {
                Some(target) => Field::Association(target),
                None => Field::Column(ty),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;

    struct Model;

    impl EntityModel for Model {
        fn entity(&self, name: &str) -> Option<Atom> {
            let name = name.strip_prefix('\\').unwrap_or(name);
            ["App\\User", "App\\Address", "App\\Post"]
                .contains(&name)
                .then(|| atom(name))
        }

        fn field(&self, entity: &str, field: &str) -> Option<Field> {
            Some(match (entity, field) {
                (_, "id") => Field::Column(PhpType::int()),
                ("App\\User", "name") => Field::Column(PhpType::parse("string|null")),
                ("App\\User", "email") => Field::Column(PhpType::string()),
                ("App\\User", "score") => Field::Column(PhpType::parse("int|null")),
                ("App\\User", "active") => Field::Column(PhpType::bool()),
                ("App\\User", "address") => Field::Association(Some(atom("App\\Address"))),
                ("App\\Address", "city") => Field::Column(PhpType::string()),
                _ => return None,
            })
        }

        fn class(&self, name: &str) -> Option<Atom> {
            let name = name.strip_prefix('\\').unwrap_or(name);
            (self.entity(name).is_some() || name == "App\\Dto").then(|| atom(name))
        }
    }

    fn query(dql: &str) -> String {
        query_type(dql, &Model).to_string()
    }

    /// [`Model`] on a connection to `driver`.
    struct OnPlatform(Platform);

    impl EntityModel for OnPlatform {
        fn entity(&self, name: &str) -> Option<Atom> {
            Model.entity(name)
        }

        fn field(&self, entity: &str, field: &str) -> Option<Field> {
            Model.field(entity, field)
        }

        fn class(&self, name: &str) -> Option<Atom> {
            Model.class(name)
        }

        fn platform(&self) -> Option<Platform> {
            Some(self.0)
        }
    }

    fn platform(driver: Driver) -> Platform {
        Platform {
            driver,
            stringify_fetches: Some(false),
            emulate_prepares: Some(true),
            php: PhpVersion::new(8, 4),
        }
    }

    /// The row type of `SELECT <select> FROM App\User u <rest>` on
    /// `platform`.
    fn row_on(platform: Platform, select: &str, rest: &str) -> String {
        let dql = format!("SELECT {select} FROM App\\User u {rest}");
        let ty = query_type(&dql, &OnPlatform(platform)).to_string();
        ty.strip_prefix("Doctrine\\ORM\\Query<null, ")
            .and_then(|t| t.strip_suffix('>'))
            .unwrap_or(&ty)
            .to_string()
    }

    /// The type of the one expression `expr` selects, on each of pdo_mysql,
    /// pdo_sqlite, and pdo_pgsql.
    fn on_each(expr: &str, rest: &str) -> [String; 3] {
        [Driver::PdoMysql, Driver::PdoSqlite, Driver::PdoPgsql].map(|driver| {
            let row = row_on(platform(driver), &format!("{expr} AS v"), rest);
            row.strip_prefix("array{v: ")
                .and_then(|t| t.strip_suffix('}'))
                .unwrap_or(&row)
                .to_string()
        })
    }

    #[test]
    fn aggregates_are_typed_by_the_driver() {
        let group = "GROUP BY u.id";
        assert_eq!(
            on_each("SUM(u.id)", group),
            ["numeric-string", "int", "int|numeric-string"]
        );
        assert_eq!(
            on_each("AVG(u.id)", group),
            ["numeric-string", "float", "numeric-string"]
        );
        assert_eq!(on_each("MAX(u.id)", group), ["int", "int", "int"]);
        // One row from no row at all.
        assert_eq!(
            on_each("MIN(u.id)", ""),
            ["int|null", "int|null", "int|null"]
        );
        assert_eq!(
            on_each("SUM(u.score)", group),
            ["numeric-string|null", "int|null", "int|numeric-string|null"]
        );
        assert_eq!(
            on_each("MAX(u.email)", ""),
            ["string|null", "string|null", "string|null"]
        );
    }

    #[test]
    fn literals_and_arithmetic_are_typed_by_the_driver() {
        assert_eq!(on_each("u.id + 1", ""), ["int", "int", "int"]);
        assert_eq!(on_each("u.id / 2", ""), ["numeric-string", "int", "int"]);
        assert_eq!(on_each("1.5", ""), ["'1.5'", "1.5", "'1.5'"]);
        assert_eq!(on_each("TRUE", ""), ["1", "1", "true"]);
        assert_eq!(on_each("-2 * 3", ""), ["int", "int", "int"]);
        assert_eq!(
            on_each("u.id * 1.5", ""),
            ["numeric-string", "float", "numeric-string"]
        );
        assert_eq!(on_each("u.id = 1", ""), ["mixed", "mixed", "mixed"]);
    }

    #[test]
    fn functions_are_typed_by_the_driver() {
        assert_eq!(
            on_each("ABS(u.id)", ""),
            ["int<0, max>", "int<0, max>", "int<0, max>"]
        );
        assert_eq!(on_each("MOD(u.id, 3)", ""), ["int", "int", "int"]);
        assert_eq!(
            on_each("MOD(u.id, u.id)", ""),
            ["int|null", "int|null", "int"]
        );
        assert_eq!(on_each("SQRT(4)", ""), ["float", "float", "float"]);
        assert_eq!(
            on_each("SQRT(u.id)", ""),
            ["float|null", "float|null", "float"]
        );
        assert_eq!(
            on_each("DATE_DIFF(u.email, u.email)", ""),
            ["int", "float", "int"]
        );
        assert_eq!(on_each("COALESCE(u.score, 0)", ""), ["int", "int", "int"]);
        assert_eq!(
            on_each("COALESCE(u.score, u.email)", ""),
            ["string", "int|string", "int|string"]
        );
        assert_eq!(
            on_each("CASE WHEN u.id > 1 THEN 1 ELSE 0 END", ""),
            ["1|0", "1|0", "1|0"]
        );
        assert_eq!(on_each("u.active + 0", ""), ["int", "int", "mixed"]);
    }

    #[test]
    fn a_pdo_driver_may_give_numbers_as_strings() {
        let old = Platform {
            php: PhpVersion::new(8, 0),
            ..platform(Driver::PdoMysql)
        };
        assert_eq!(row_on(old, "u.id + 1 AS v", ""), "array{v: numeric-string}");
        let unknown = Platform {
            stringify_fetches: None,
            ..platform(Driver::PdoMysql)
        };
        assert_eq!(
            row_on(unknown, "u.id + 1 AS v", ""),
            "array{v: int|numeric-string}"
        );
        assert_eq!(row_on(unknown, "1 AS v", ""), "array{v: 1|'1'}");
        // pdo_pgsql gives floats as strings before PHP 8.4.
        let pgsql = Platform {
            php: PhpVersion::new(8, 3),
            ..platform(Driver::PdoPgsql)
        };
        assert_eq!(
            row_on(pgsql, "SQRT(u.id) AS v", ""),
            "array{v: numeric-string}"
        );
        // mysqli never does.
        let mysqli = Platform {
            php: PhpVersion::new(8, 0),
            ..platform(Driver::Mysqli)
        };
        assert_eq!(row_on(mysqli, "u.id + 1 AS v", ""), "array{v: int}");
    }

    #[test]
    fn entity_and_field_selects() {
        assert_eq!(
            query("SELECT u FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, App\\User>"
        );
        assert_eq!(
            query("SELECT u.id, u.name AS n FROM App\\User u WHERE u.id = 1"),
            "Doctrine\\ORM\\Query<null, array{id: int, n: string|null}>"
        );
        assert_eq!(
            query("SELECT u, u.email FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, array{0: App\\User, email: string}>"
        );
        assert_eq!(
            query("SELECT u AS user, COUNT(u.id) AS total FROM App\\User u GROUP BY u"),
            "Doctrine\\ORM\\Query<null, array{user: App\\User, total: int<0, max>}>"
        );
    }

    #[test]
    fn functions_and_aggregates() {
        assert_eq!(
            query("SELECT COUNT(u.id) FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, array{1: int<0, max>}>"
        );
        assert_eq!(
            query("SELECT u, COUNT(u.id) total, SUM(u.id) AS s FROM App\\User u GROUP BY u"),
            "Doctrine\\ORM\\Query<null, array{0: App\\User, total: int<0, max>, s: mixed}>"
        );
        assert_eq!(
            query(
                "SELECT LENGTH(u.email) AS a, LENGTH(u.name) AS b, LOWER(u.email), \
                 CONCAT(u.email, u.name) c FROM App\\User u"
            ),
            "Doctrine\\ORM\\Query<null, array{a: int<0, max>, b: int<0, max>|null, 1: string, c: string|null}>"
        );
        assert_eq!(
            query(
                "SELECT TRIM(LEADING 'x' FROM u.email) t, CURRENT_DATE() d, 'it''s' AS l, \
                 u.id + 1 AS plus FROM App\\User u"
            ),
            "Doctrine\\ORM\\Query<null, array{t: string, d: string, l: 'it\\'s', plus: mixed}>"
        );
        // Aggregating without `GROUP BY` gives one row, from no row at all
        // when nothing matches.
        assert_eq!(
            query("SELECT COUNT(u.id) AS n, MAX(u.id) AS m, u.email FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, array{n: int<0, max>, m: mixed, email: string|null}>"
        );
        assert_eq!(
            query("SELECT (SELECT COUNT(p.id) FROM App\\Post p) AS n, u.email FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, array{n: mixed, email: string}>"
        );
    }

    #[test]
    fn conditional_expressions() {
        assert_eq!(
            query(
                "SELECT CASE WHEN u.id > 1 THEN 'big' ELSE 'small' END AS size, \
                 CASE u.id WHEN 1 THEN 'one' END one FROM App\\User u"
            ),
            "Doctrine\\ORM\\Query<null, array{size: 'big'|'small', one: 'one'|null}>"
        );
        assert_eq!(
            query(
                "SELECT COALESCE(u.name, '') AS a, COALESCE(u.name, NULL) AS b, \
                 NULLIF(u.email, 'x') AS c, COALESCE(u.id, 0) AS d FROM App\\User u"
            ),
            "Doctrine\\ORM\\Query<null, array{a: string, b: string|null, c: string|null, d: mixed}>"
        );
        assert_eq!(
            query("SELECT CASE WHEN u.id > 1 THEN 1 ELSE 0 END AS flag FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, array{flag: mixed}>"
        );
    }

    #[test]
    fn new_objects() {
        assert_eq!(
            query("SELECT NEW App\\Dto(u.id, u.email) FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, App\\Dto>"
        );
        assert_eq!(
            query("SELECT NEW App\\Dto(u.id, u.email), COUNT(u.id) FROM App\\User u GROUP BY u.id"),
            "Doctrine\\ORM\\Query<null, array{3: int<0, max>, 0: App\\Dto}>"
        );
        assert_eq!(
            query("SELECT NEW App\\Dto(u.id) AS a, NEW App\\Dto() FROM App\\User u"),
            "Doctrine\\ORM\\Query<null, array{a: App\\Dto, 0: App\\Dto}>"
        );
        assert_eq!(
            query("SELECT NEW App\\Missing(u.id) FROM App\\User u"),
            "Doctrine\\ORM\\Query<mixed, mixed>"
        );
    }

    #[test]
    fn several_entities_beside_scalars() {
        assert_eq!(
            query("SELECT u, p, u.email FROM App\\User u JOIN App\\Post p WITH p.id = u.id"),
            "Doctrine\\ORM\\Query<null, array{0: App\\User}|array{0: App\\Post, email: string}>"
        );
        assert_eq!(
            query("SELECT u, p FROM App\\User u LEFT JOIN App\\Post p WITH p.id = u.id"),
            "Doctrine\\ORM\\Query<null, App\\User|App\\Post|null>"
        );
    }

    #[test]
    fn joins() {
        assert_eq!(
            query("SELECT u, a FROM App\\User u JOIN u.address a"),
            "Doctrine\\ORM\\Query<null, App\\User>"
        );
        assert_eq!(
            query("SELECT u.id, a.city FROM App\\User u LEFT JOIN u.address a WITH a.id > 1"),
            "Doctrine\\ORM\\Query<null, array{id: int, city: string|null}>"
        );
        assert_eq!(
            query("SELECT u, p FROM App\\User u JOIN App\\Post p WITH p.id = u.id"),
            "Doctrine\\ORM\\Query<null, App\\User|App\\Post>"
        );
    }

    #[test]
    fn index_by_and_statements() {
        assert_eq!(
            query("SELECT u FROM App\\User u INDEX BY u.email"),
            "Doctrine\\ORM\\Query<string, App\\User>"
        );
        assert_eq!(
            query("SELECT u.name FROM App\\User u INDEX BY u.email"),
            "Doctrine\\ORM\\Query<null, array{name: string|null}>"
        );
        assert_eq!(
            query("SELECT u.email, u.name FROM App\\User u INDEX BY u.email"),
            "Doctrine\\ORM\\Query<string, array{email: string, name: string|null}>"
        );
        assert_eq!(
            query("UPDATE App\\User u SET u.name = 'x'"),
            "Doctrine\\ORM\\Query<void, void>"
        );
        assert_eq!(
            query("DELETE FROM App\\User u"),
            "Doctrine\\ORM\\Query<void, void>"
        );
    }

    #[test]
    fn unread_dql_is_mixed() {
        let mixed = "Doctrine\\ORM\\Query<mixed, mixed>";
        assert_eq!(query("invalid"), mixed);
        assert_eq!(query("SELECT u.missing FROM App\\User u"), mixed);
        assert_eq!(query("SELECT u.address FROM App\\User u"), mixed);
        assert_eq!(query("SELECT x FROM App\\Unknown x"), mixed);
        assert_eq!(query("SELECT u FROM App\\User u, App\\Post u"), mixed);
    }

    #[test]
    fn hydration() {
        let query = PhpType::parse("Doctrine\\ORM\\Query<null, App\\User>");
        let class = |_: &str| true;
        let get = |method: &str, mode| {
            hydrated_result(method, &query, mode, &class).map(|t| t.to_string())
        };
        assert_eq!(get("getResult", None).as_deref(), Some("list<App\\User>"));
        assert_eq!(get("getSingleResult", None), None);
        assert_eq!(
            get("getOneOrNullResult", Some(Some(1))).as_deref(),
            Some("App\\User|null")
        );
        assert_eq!(get("getResult", Some(Some(2))), None);
        assert_eq!(get("getResult", Some(None)), None);
        let indexed = PhpType::parse("Doctrine\\ORM\\Query<string, App\\User>");
        assert_eq!(
            hydrated_result("toIterable", &indexed, None, &class).map(|t| t.to_string()),
            Some("iterable<string, App\\User>".to_string())
        );
        let void = PhpType::parse("Doctrine\\ORM\\Query<void, void>");
        assert_eq!(
            hydrated_result("execute", &void, None, &class).map(|t| t.to_string()),
            Some("int<0, max>".to_string())
        );
        let mixed = PhpType::parse("Doctrine\\ORM\\Query<mixed>");
        assert_eq!(hydrated_result("getResult", &mixed, None, &class), None);
        let template = PhpType::parse("Doctrine\\ORM\\Query<T>");
        assert_eq!(
            hydrated_result("getResult", &template, None, &|_| false),
            None
        );
    }

    #[test]
    fn builder_replay() {
        let mut state = QueryBuilderState::default();
        let s = |v: &str| Some(BuilderArg::Strings(vec![v.to_string()]));
        state
            .apply(
                "select",
                &[Some(BuilderArg::Strings(vec![
                    "u.id".into(),
                    "a.city".into(),
                ]))],
            )
            .unwrap();
        state.apply("from", &[s("App\\User"), s("u")]).unwrap();
        state.apply("leftJoin", &[s("u.address"), s("a")]).unwrap();
        state
            .apply("andWhere", &[Some(BuilderArg::Unknown)])
            .unwrap();
        state.apply("indexBy", &[s("u"), s("u.email")]).unwrap();
        assert_eq!(
            builder_query_type(&state, &Model).to_string(),
            "Doctrine\\ORM\\Query<null, array{id: int, city: string|null}>"
        );
        assert!(
            state
                .apply("from", &[Some(BuilderArg::Unknown), s("x")])
                .is_none()
        );
        assert!(state.apply("expr", &[]).is_none());
    }

    #[test]
    fn builder_add_replay() {
        let s = |v: &str| Some(BuilderArg::Strings(vec![v.to_string()]));
        let arg = |v: &str| CallArg {
            name: None,
            spread: false,
            value: BuilderArg::Strings(vec![v.to_string()]),
        };
        let mut state = QueryBuilderState::default();
        state.apply("select", &[s("u.email")]).unwrap();
        let from = BuilderArg::New {
            class: format!("\\{EXPR_FROM}"),
            args: vec![arg("App\\User"), arg("u")],
        };
        state
            .apply(
                "add",
                &[s("from"), Some(from), Some(BuilderArg::Bool(true))],
            )
            .unwrap();
        state
            .apply("add", &[s("where"), Some(BuilderArg::Unknown), None])
            .unwrap();
        assert_eq!(
            builder_query_type(&state, &Model).to_string(),
            "Doctrine\\ORM\\Query<null, array{email: string}>"
        );
        state.apply("add", &[s("select"), s("u"), None]).unwrap();
        assert_eq!(
            builder_query_type(&state, &Model).to_string(),
            "Doctrine\\ORM\\Query<null, App\\User>"
        );
        assert!(
            state
                .apply("add", &[s("join"), Some(BuilderArg::Unknown), None])
                .is_none()
        );
        assert!(
            state
                .apply("add", &[s("select"), s("u"), Some(BuilderArg::Unknown)])
                .is_none()
        );
    }

    #[test]
    fn builders_of_different_dql_are_different_values() {
        let class = PhpType::named(atom(QUERY_BUILDER));
        let s = |v: &str| Some(BuilderArg::Strings(vec![v.to_string()]));
        let mut first = QueryBuilderState::default();
        first.apply("select", &[s("u")]).unwrap();
        let mut second = first.clone();
        second.apply("from", &[s("App\\User"), s("u")]).unwrap();
        let a = PhpType::dql_builder(class.clone(), first.clone());
        let b = PhpType::dql_builder(class.clone(), second);

        assert_eq!(a.to_string(), QUERY_BUILDER);
        assert_eq!(a, PhpType::dql_builder(class.clone(), first));
        assert_eq!(
            PhpType::union(vec![a.clone(), b.clone()])
                .union_members()
                .len(),
            2
        );
        assert!(a.is_subset_of(&class));
        assert!(!class.is_subset_of(&a));
        assert!(!a.is_subset_of(&b));
    }
}
