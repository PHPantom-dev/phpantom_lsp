//! Symbol spans for the entity classes and fields Doctrine Query Language
//! names, in a query (`$em->createQuery('SELECT u FROM App\Entity\User u
//! WHERE u.email = :e')`) or in the fragments a query builder joins into
//! one (`$qb->from(User::class, 'u')->where('u.email = :e')`).
//!
//! An entity class written in the text becomes a
//! [`SymbolKind::ClassReference`]; DQL names classes by their
//! fully-qualified name, so it is one.  A field becomes a
//! [`DocblockMemberRef::EntityField`] access whose subject is what its
//! identification variable stands for: the class an entity name or
//! `X::class` gives, or the repository whose `createQueryBuilder('u')`
//! declared it, which consumers swap for the entity it manages.
//!
//! A query's variables are its own.  A query builder's are declared by one
//! call and used by another, often in another statement, so the variables
//! of every builder call in a function body are read together: a variable
//! declared alike throughout the body stands for what it is declared as,
//! and one declared differently stands for nothing.  A variable joined
//! through an association, and one a dynamic part of the text declares,
//! stand for nothing either.
//!
//! Only a Doctrine project's files are read: builder methods such as
//! `where()` and `from()` are common names elsewhere.

use std::ops::Range;

use crate::doctrine::dql::{self, DYNAMIC, DeclRoot, DqlRef};

use super::*;

/// What an identification variable stands for, as the extractor can name
/// it.
#[derive(Debug, Clone, PartialEq)]
enum Root {
    /// A class or repository PHP code names: its subject text and the span
    /// of the expression it was read from, resolved like any subject.
    Php { text: String, start: u32, end: u32 },
    /// An entity class the DQL text names, fully qualified.
    Fqn(String),
    /// Something no text names.
    Unknown,
}

impl Root {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Php { text: a, .. }, Self::Php { text: b, .. }) => a == b,
            (Self::Fqn(a), Self::Fqn(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

/// Which variables a piece of DQL shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DqlTable {
    /// A whole query, starting at the offset.
    Query(u32),
    /// The query builder calls of the function body starting at the
    /// offset.
    Scope(u32),
}

#[derive(Debug)]
struct PendingDecl {
    table: DqlTable,
    alias: String,
    root: Root,
}

#[derive(Debug)]
struct PendingField {
    table: DqlTable,
    alias: String,
    start: u32,
    end: u32,
}

/// The variables and fields read so far, settled once the whole file is.
#[derive(Debug, Default)]
pub(super) struct DqlPending {
    decls: Vec<PendingDecl>,
    fields: Vec<PendingField>,
    /// The source range each query covers.
    queries: Vec<(u32, u32)>,
}

/// DQL text assembled from a PHP expression, with where each byte came
/// from.
struct DqlSource {
    text: String,
    /// The source range of each byte of `text`; `None` for a byte no
    /// source text spells.
    origin: Vec<Option<(u32, u32)>>,
    /// The parts of the text PHP code names a class or repository by,
    /// with what it names.
    pieces: Vec<(Range<usize>, Root)>,
    /// Whether any of the text is written out.
    literal: bool,
}

impl DqlSource {
    fn new() -> Self {
        Self {
            text: String::new(),
            origin: Vec::new(),
            pieces: Vec::new(),
            literal: false,
        }
    }

    fn push_synthetic(&mut self, text: &str) {
        self.text.push_str(text);
        self.origin.extend(std::iter::repeat_n(None, text.len()));
    }

    fn push_dynamic(&mut self) {
        self.text.push(DYNAMIC);
        self.origin.push(None);
    }

    fn push_char(&mut self, c: char, start: u32, end: u32) {
        self.text.push(c);
        self.origin
            .extend(std::iter::repeat_n(Some((start, end)), c.len_utf8()));
        self.literal = true;
    }

    fn push_piece(&mut self, root: Root) {
        let start = self.text.len();
        // Any name reads as the class; the piece says which one it is.
        self.push_synthetic(" Piece ");
        self.pieces.push((start + 1..start + 6, root));
    }

    /// Append the text of a literal's body `raw`, starting at `base` in the
    /// source, reading the escapes `quote` gives meaning to: `'` for a
    /// single-quoted string, `"` for a double-quoted string or a heredoc,
    /// and none for a nowdoc.
    fn push_literal(&mut self, raw: &str, base: u32, quote: Option<char>) {
        let mut chars = raw.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            let start = base + i as u32;
            if c != '\\' || quote.is_none() {
                self.push_char(c, start, start + c.len_utf8() as u32);
                continue;
            }
            let Some(&(j, next)) = chars.peek() else {
                self.push_char(c, start, start + 1);
                continue;
            };
            let end = base + (j + next.len_utf8()) as u32;
            let decoded = match (quote, next) {
                (_, '\\') => Some('\\'),
                (Some('\''), '\'') => Some('\''),
                (Some('"'), '"' | '$') => Some(next),
                (Some('"'), 'n' | 't' | 'r' | 'v' | 'e' | 'f') => Some(' '),
                (Some('"'), '0'..='7' | 'x' | 'u') => None,
                _ => {
                    // Not an escape: the backslash stays, as in a class
                    // name.
                    self.push_char(c, start, start + 1);
                    continue;
                }
            };
            chars.next();
            match decoded {
                Some(d) => self.push_char(d, start, end),
                None => self.push_dynamic(),
            }
        }
    }

    /// Append what `expr` evaluates to, as far as it can be read.
    fn push_expr(&mut self, expr: &Expression<'_>, content: &str) {
        // A concatenation chain is a left-leaning tree as deep as the chain
        // is long, so it is walked with a stack of its own.
        let mut pending: Vec<&Expression<'_>> = vec![expr];
        while let Some(expr) = pending.pop() {
            match expr {
                Expression::Parenthesized(inner) => pending.push(inner.expression),
                Expression::Binary(binary) if binary.operator.is_concatenation() => {
                    pending.push(binary.rhs);
                    pending.push(binary.lhs);
                }
                Expression::Literal(literal::Literal::String(string)) => {
                    let span = string.span;
                    let Some(raw) =
                        content.get(span.start.offset as usize..span.end.offset as usize)
                    else {
                        self.push_dynamic();
                        continue;
                    };
                    let quote = raw.chars().next().filter(|c| *c == '\'' || *c == '"');
                    let Some(quote) = quote else {
                        self.push_dynamic();
                        continue;
                    };
                    let body = &raw[1..raw.len().saturating_sub(1).max(1)];
                    self.push_literal(body, span.start.offset + 1, Some(quote));
                }
                Expression::CompositeString(string)
                    if !matches!(string, CompositeString::ShellExecute(_)) =>
                {
                    let quote = match string {
                        CompositeString::Document(document)
                            if matches!(document.kind, DocumentKind::Nowdoc) =>
                        {
                            None
                        }
                        _ => Some('"'),
                    };
                    for part in string.parts().iter() {
                        match part {
                            StringPart::Literal(literal) => {
                                let span = literal.span;
                                match content
                                    .get(span.start.offset as usize..span.end.offset as usize)
                                {
                                    Some(raw) => self.push_literal(raw, span.start.offset, quote),
                                    None => self.push_dynamic(),
                                }
                            }
                            _ => self.push_dynamic(),
                        }
                    }
                }
                Expression::Access(Access::ClassConstant(access))
                    if matches!(&access.constant, ClassLikeConstantSelector::Identifier(ident)
                        if ident.value.eq_ignore_ascii_case(b"class")) =>
                {
                    let root = match access.class {
                        Expression::Identifier(_)
                        | Expression::Self_(_)
                        | Expression::Static(_) => php_root(access.class),
                        _ => Root::Unknown,
                    };
                    self.push_piece(root);
                }
                // A repository's own entity, which is the one it manages.
                Expression::Call(Call::Method(call))
                    if matches!(call.object, Expression::Variable(Variable::Direct(v)) if v.name == b"$this")
                        && matches!(&call.method, ClassLikeMemberSelector::Identifier(ident)
                            if ident.value.eq_ignore_ascii_case(b"getEntityName")
                                || ident.value.eq_ignore_ascii_case(b"getClassName"))
                        && call.argument_list.arguments.is_empty() =>
                {
                    self.push_piece(php_root(call.object));
                }
                _ => self.push_dynamic(),
            }
        }
    }

    /// The source range the text `range` was read from, when all of it was
    /// written out.
    fn source_range(&self, range: &Range<usize>) -> Option<(u32, u32)> {
        let origin = self.origin.get(range.clone())?;
        let first = (*origin.first()?)?;
        let last = (*origin.last()?)?;
        origin
            .iter()
            .all(Option::is_some)
            .then_some((first.0, last.1))
    }

    /// What the name at `range` stands for: the piece it is, or the class
    /// the text writes out.
    fn root(&self, range: &Range<usize>) -> Root {
        for (piece, root) in &self.pieces {
            if piece == range {
                return root.clone();
            }
            if piece.start < range.end && range.start < piece.end {
                return Root::Unknown;
            }
        }
        let name = &self.text[range.clone()];
        if self.source_range(range).is_some() && dql::is_class_name(name) {
            Root::Fqn(name.trim_start_matches('\\').to_string())
        } else {
            Root::Unknown
        }
    }
}

/// The root a subject PHP code writes stands for.
fn php_root(expr: &Expression<'_>) -> Root {
    let text = expr_to_subject_text(expr);
    if text.is_empty() {
        return Root::Unknown;
    }
    let span = expr.span();
    Root::Php {
        text,
        start: span.start.offset,
        end: span.end.offset,
    }
}

/// What a query builder method's arguments hold, by position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arg {
    /// A fragment of DQL, or an array of them.
    Fragment,
    /// An entity class, declared with the variable the next argument names.
    Entity,
    /// An association path or an entity, joined with the variable the next
    /// argument names.
    Join,
    /// The variable the argument before declares.
    Alias,
    /// The variable `createQueryBuilder()` declares for the repository's
    /// entity.
    RepositoryAlias,
    /// Something else.
    Skip,
}

/// The arguments of a query builder method, the last repeating for a
/// variadic one.  `None` for a method that takes no DQL.
fn builder_args(method: &str) -> Option<(&'static [Arg], bool)> {
    use Arg::*;
    const VARIADIC: &[&str] = &[
        "select",
        "addSelect",
        "where",
        "andWhere",
        "orWhere",
        "having",
        "andHaving",
        "orHaving",
        "groupBy",
        "addGroupBy",
    ];
    if VARIADIC.iter().any(|m| m.eq_ignore_ascii_case(method)) {
        return Some((&[Fragment], true));
    }
    let args: &'static [Arg] = match method.to_ascii_lowercase().as_str() {
        "orderby" | "addorderby" => &[Fragment],
        "from" => &[Entity, Alias, Fragment],
        "join" | "innerjoin" | "leftjoin" => &[Join, Alias, Skip, Fragment, Fragment],
        "update" | "delete" => &[Entity, Alias],
        "set" => &[Fragment, Fragment],
        "indexby" => &[Skip, Fragment],
        "createquerybuilder" => &[RepositoryAlias, Fragment],
        _ => return None,
    };
    Some((args, false))
}

/// The parameter names of a query builder method's arguments, for a call
/// that names them.
fn builder_parameter(method: &str, name: &str) -> Option<usize> {
    let names: &[&str] = match method.to_ascii_lowercase().as_str() {
        "from" => &["from", "alias", "indexBy"],
        "join" | "innerjoin" | "leftjoin" => {
            &["join", "alias", "conditionType", "condition", "indexBy"]
        }
        "update" => &["update", "alias"],
        "delete" => &["delete", "alias"],
        "set" => &["key", "value"],
        "indexby" => &["alias", "indexBy"],
        "orderby" | "addorderby" => &["sort", "order"],
        "createquerybuilder" => &["alias", "indexBy"],
        _ => return None,
    };
    names.iter().position(|n| *n == name)
}

/// Whether `object` is an expression builder, `….expr()`, whose methods
/// take DQL fragments.
fn is_expression_builder(object: &Expression<'_>) -> bool {
    matches!(object, Expression::Call(Call::Method(call))
        if matches!(&call.method, ClassLikeMemberSelector::Identifier(ident)
            if ident.value.eq_ignore_ascii_case(b"expr"))
            && call.argument_list.arguments.is_empty())
}

/// Read the DQL the instance call `object->method(arguments)` takes, if
/// it takes any.
pub(super) fn record_dql_call(
    object: &Expression<'_>,
    method: &str,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
    scope_start: u32,
) {
    if !ctx.doctrine {
        return;
    }
    if method.eq_ignore_ascii_case("createQuery") || method.eq_ignore_ascii_case("setDQL") {
        let Some(argument) = argument_at(arguments, 0, "dql") else {
            return;
        };
        let span = argument.span();
        let table = DqlTable::Query(span.start.offset);
        let mut source = DqlSource::new();
        source.push_expr(argument, ctx.content);
        if source.literal {
            ctx.dql.queries.push((span.start.offset, span.end.offset));
            read_source(&source, table, ctx);
        }
        return;
    }

    let table = DqlTable::Scope(scope_start);
    if is_expression_builder(object) {
        // `literal()` quotes a value rather than reading DQL.
        if method.eq_ignore_ascii_case("literal") {
            return;
        }
        for argument in arguments.arguments.iter() {
            read_fragment(argument.value(), table, ctx);
        }
        return;
    }
    let Some((roles, variadic)) = builder_args(method) else {
        return;
    };
    let role_at = |index: usize| match roles.get(index) {
        Some(role) => *role,
        None if variadic => roles[roles.len() - 1],
        None => Arg::Skip,
    };
    let mut positional: Vec<Option<&Expression<'_>>> = Vec::new();
    for (index, argument) in arguments.arguments.iter().enumerate() {
        let slot = match argument {
            Argument::Positional(positional) if positional.ellipsis.is_none() => index,
            Argument::Named(named) => {
                match builder_parameter(method, crate::atom::bytes_to_str(named.name.value)) {
                    Some(slot) => slot,
                    None => continue,
                }
            }
            _ => break,
        };
        if positional.len() <= slot {
            positional.resize(slot + 1, None);
        }
        positional[slot] = Some(argument.value());
    }

    for (index, argument) in positional.iter().enumerate() {
        let Some(argument) = argument else {
            continue;
        };
        match role_at(index) {
            Arg::Fragment => match crate::parser::array_literal_elements(argument) {
                Some(elements) => {
                    for element in elements.iter() {
                        if let Some(value) = crate::parser::array_element_value(element) {
                            read_fragment(value, table, ctx);
                        }
                    }
                }
                None => read_fragment(argument, table, ctx),
            },
            role @ (Arg::Entity | Arg::Join) => {
                // Without the variable it declares the call is no query
                // builder's: `$cache->delete('key')`.
                let Some(alias) = positional
                    .get(index + 1)
                    .copied()
                    .flatten()
                    .filter(|alias| string_literal_text(alias).is_some_and(is_identifier))
                else {
                    continue;
                };
                let mut source = DqlSource::new();
                source.push_synthetic(if role == Arg::Entity {
                    "FROM "
                } else {
                    "JOIN "
                });
                source.push_expr(argument, ctx.content);
                source.push_synthetic(" ");
                source.push_expr(alias, ctx.content);
                read_source(&source, table, ctx);
            }
            Arg::RepositoryAlias => {
                let Some(alias) = string_literal_text(argument) else {
                    continue;
                };
                ctx.dql.decls.push(PendingDecl {
                    table,
                    alias: alias.to_string(),
                    root: php_root(object),
                });
            }
            Arg::Alias | Arg::Skip => {}
        }
    }
}

/// The argument at `index`, or the one named `name`.
fn argument_at<'a>(
    arguments: &'a ArgumentList<'a>,
    index: usize,
    name: &str,
) -> Option<&'a Expression<'a>> {
    for (i, argument) in arguments.arguments.iter().enumerate() {
        match argument {
            Argument::Positional(positional) if positional.ellipsis.is_none() && i == index => {
                return Some(positional.value);
            }
            Argument::Named(named) if named.name.value == name.as_bytes() => {
                return Some(named.value);
            }
            _ => {}
        }
    }
    None
}

fn string_literal_text<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Literal(literal::Literal::String(string)) => {
            string.value.and_then(crate::atom::literal_bytes_to_str)
        }
        _ => None,
    }
}

fn read_fragment(expr: &Expression<'_>, table: DqlTable, ctx: &mut ExtractionCtx<'_>) {
    let mut source = DqlSource::new();
    source.push_expr(expr, ctx.content);
    if source.literal {
        read_source(&source, table, ctx);
    }
}

/// Record what the DQL `source` declares and names.
fn read_source(source: &DqlSource, table: DqlTable, ctx: &mut ExtractionCtx<'_>) {
    let scan = dql::scan(&source.text);
    for decl in scan.decls {
        let root = match &decl.root {
            DeclRoot::Entity(name) => {
                emit_class(source, name, ctx);
                source.root(name)
            }
            DeclRoot::Association | DeclRoot::Unknown => Root::Unknown,
        };
        ctx.dql.decls.push(PendingDecl {
            table,
            alias: source.text[decl.alias].to_string(),
            root,
        });
    }
    for reference in scan.refs {
        match reference {
            DqlRef::Class(name) => emit_class(source, &name, ctx),
            DqlRef::Field { alias, field } => {
                let Some((start, end)) = source.source_range(&field) else {
                    continue;
                };
                // Only a field written plainly is named where it is
                // written.
                if end - start != field.len() as u32 {
                    continue;
                }
                ctx.dql.fields.push(PendingField {
                    table,
                    alias: source.text[alias].to_string(),
                    start,
                    end,
                });
            }
        }
    }
}

/// Emit a class reference for the class name the text writes at `range`.
fn emit_class(source: &DqlSource, range: &Range<usize>, ctx: &mut ExtractionCtx<'_>) {
    if !matches!(source.root(range), Root::Fqn(_)) {
        return;
    }
    let Some((start, end)) = source.source_range(range) else {
        return;
    };
    let name = source.text[range.clone()].trim_start_matches('\\');
    ctx.spans.push(class_ref_span_ctx(
        start,
        end,
        &format!("\\{name}"),
        ClassRefContext::Dql,
    ));
}

/// Settle the variables of every query and function body read, emitting a
/// span for each field whose variable stands for something, and recording
/// the variables for completion.
pub(super) fn finish_dql(ctx: &mut ExtractionCtx<'_>) {
    let pending = std::mem::take(&mut ctx.dql);
    if pending.decls.is_empty() {
        return;
    }
    let resolve = |table: DqlTable, alias: &str| -> Option<&Root> {
        let mut found: Option<&Root> = None;
        for decl in pending
            .decls
            .iter()
            .filter(|d| d.table == table && d.alias == alias)
        {
            match found {
                None => found = Some(&decl.root),
                Some(known) if known.same(&decl.root) => {}
                Some(_) => return None,
            }
        }
        found.filter(|root| !matches!(root, Root::Unknown))
    };

    for field in &pending.fields {
        let Some(root) = resolve(field.table, &field.alias) else {
            continue;
        };
        let subject_text = match root {
            Root::Php { text, start, end } => {
                SubjectText::new(text.clone(), *start, *end, ctx.content)
            }
            Root::Fqn(name) => SubjectText::owned(format!("\\{name}")),
            Root::Unknown => continue,
        };
        ctx.spans.push(SymbolSpan {
            start: field.start,
            end: field.end,
            kind: SymbolKind::MemberAccess {
                subject_text,
                member_name: crate::atom::atom(
                    &ctx.content[field.start as usize..field.end as usize],
                ),
                is_static: false,
                is_method_call: false,
                docblock_ref: DocblockMemberRef::EntityField,
                is_array_callable: false,
                is_nullsafe: false,
            },
        });
    }

    let content_len = ctx.content.len() as u32;
    let mut sites: Vec<crate::symbol_map::DqlAliasSite> = Vec::new();
    for decl in &pending.decls {
        let Some(root) = resolve(decl.table, &decl.alias) else {
            continue;
        };
        let subject = match root {
            Root::Php { text, .. } => text.clone(),
            Root::Fqn(name) => format!("\\{name}"),
            Root::Unknown => continue,
        };
        let (start, end) = match decl.table {
            DqlTable::Query(start) => match pending.queries.iter().find(|(s, _)| *s == start) {
                Some(range) => *range,
                None => continue,
            },
            DqlTable::Scope(0) => (0, content_len),
            DqlTable::Scope(start) => match ctx.scopes.iter().find(|(s, _)| *s == start) {
                Some(range) => *range,
                None => continue,
            },
        };
        let alias = crate::atom::atom(&decl.alias);
        if sites
            .iter()
            .any(|s| s.alias == alias && s.start == start && s.end == end)
        {
            continue;
        }
        sites.push(crate::symbol_map::DqlAliasSite {
            start,
            end,
            alias,
            subject: subject.into_boxed_str(),
        });
    }
    ctx.dql_aliases = sites;
}
