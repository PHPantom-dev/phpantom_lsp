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
//! `SELECT u.id, u.name` names, or an entity at key `0` (or its result
//! variable) beside those fields.  A field's type is the type its entity
//! declares for the property; a field of a variable a `LEFT JOIN` declares
//! may be `null` too.  Anything else in a `SELECT` (an aggregate, a
//! function, arithmetic) is typed only when it names its result variable,
//! and then as `mixed`.  A fetch join adds nothing to a row: its entities
//! are hydrated into their owner's association.
//!
//! A query builder's query is the DQL its calls build, replayed by
//! [`QueryBuilderState`].  The calls that only filter or order the rows
//! leave the type alone whatever their arguments; the others need theirs
//! known.

use std::sync::Arc;

use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, ShapeEntry, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::ClassInfo;

use super::DoctrineCacheState;
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
    /// Anything else.
    Other,
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
    let mut entities: Vec<(Option<String>, PhpType)> = Vec::new();
    let mut scalars: Vec<(String, PhpType)> = Vec::new();
    let mut selected_entities: Vec<String> = Vec::new();
    let mut selected_fields: Vec<(String, String)> = Vec::new();
    for (start, end) in top_level_items(&reader, select_start, from)? {
        let (selected, result_variable, hidden) = select_expression(&reader, start, end)?;
        match &selected {
            Selected::Entity { alias } => selected_entities.push(alias.clone()),
            Selected::Field { alias, field } => {
                selected_fields.push((alias.clone(), field.clone()))
            }
            Selected::Other => {}
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
                entities.push((result_variable, PhpType::named(decl.entity?)));
            }
            Selected::Field { alias, field } => {
                let decl = decls.iter().find(|d| d.alias == alias)?;
                let Field::Column(mut ty) = model.field(decl.entity.as_ref()?, &field)? else {
                    return None;
                };
                if decl.optional && !ty.is_mixed() {
                    ty = PhpType::union(vec![ty, PhpType::null()]).simplified();
                }
                scalars.push((result_variable.unwrap_or(field), ty));
            }
            Selected::Other => scalars.push((result_variable?, PhpType::mixed())),
        }
    }

    let result = match (entities.len(), scalars.is_empty()) {
        (0, true) => return None,
        (_, true) => PhpType::union(entities.into_iter().map(|(_, ty)| ty).collect()),
        (0, false) => shape(scalars),
        (1, false) => {
            let (key, entity) = entities.pop()?;
            let mut entries = vec![(key.unwrap_or_else(|| "0".to_string()), entity)];
            entries.extend(scalars);
            shape(entries)
        }
        _ => return None,
    };

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
    let key = match index_by {
        None => PhpType::null(),
        Some((alias, field)) => {
            let decl = decls.iter().find(|d| d.alias == *alias)?;
            match model.field(decl.entity.as_ref()?, field)? {
                Field::Column(ty) => ty,
                Field::Association(_) => return None,
            }
        }
    };
    Some((key, result))
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
        } else if hidden || simple_expression_end(reader, start) == Some(before) {
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
        return None;
    } else {
        Selected::Other
    };
    Some((selected, result_variable, hidden))
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
    fn entity(&self, name: &str) -> Option<Atom> {
        let class = self.load(name)?;
        super::is_entity(&class, &self.state).then(|| class.fqn())
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
                ("App\\User", "address") => Field::Association(Some(atom("App\\Address"))),
                ("App\\Address", "city") => Field::Column(PhpType::string()),
                _ => return None,
            })
        }
    }

    fn query(dql: &str) -> String {
        query_type(dql, &Model).to_string()
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
            "Doctrine\\ORM\\Query<null, array{user: App\\User, total: mixed}>"
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
        assert_eq!(query("SELECT COUNT(u.id) FROM App\\User u"), mixed);
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
