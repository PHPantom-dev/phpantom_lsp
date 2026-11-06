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
//! and one declared differently stands for nothing, as does one a dynamic
//! part of the text declares.  A variable joined through an association
//! (`JOIN u.team t`) stands for the association's target, which its subject
//! text names in DQL's own path notation, `\App\Entity\User.team`, for the
//! type engine to look up in the mapping.
//!
//! A variable a builder uses is reported when nothing declares it, or when
//! some path through the function body to its query never does.  A builder
//! another method creates and returns, or that is passed to another method,
//! counts the variables that method's body declares on it, which the
//! diagnostic pass looks up once the type engine finds the method; each
//! method's are recorded for its callers.  A builder a function body
//! returns is left to its callers, which may declare more.
//!
//! Each query, and each fragment whose place in the query is known, is also
//! checked against DQL's grammar.  Where one stops following it is recorded
//! with the call it was read from, for the diagnostic pass to report once
//! the type engine confirms the call is Doctrine's.
//!
//! Only a Doctrine project's files are read: builder methods such as
//! `where()` and `from()` are common names elsewhere.

use std::ops::Range;

use crate::doctrine::dql::{self, DYNAMIC, DeclRoot, DqlRef};
use crate::doctrine::dql_syntax::{self, Grammar};
use crate::symbol_map::{DqlProvider, DqlSlot};

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
    /// The target of the association `association` of what the variable
    /// `alias` of the same query or function body stands for.
    Join { alias: String, association: String },
    /// Something no text names.
    Unknown,
}

impl Root {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Php { text: a, .. }, Self::Php { text: b, .. }) => a == b,
            (Self::Fqn(a), Self::Fqn(b)) => a.eq_ignore_ascii_case(b),
            (
                Self::Join {
                    alias: a,
                    association: x,
                },
                Self::Join {
                    alias: b,
                    association: y,
                },
            ) => a == b && x == y,
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
    /// Where the method name of the call that declares it starts.
    at: u32,
}

#[derive(Debug)]
struct PendingField {
    table: DqlTable,
    alias: String,
    start: u32,
    end: u32,
}

/// A variable a path starts from, `u` in `u.email`, and the call whose DQL
/// writes it.
#[derive(Debug)]
struct PendingUse {
    table: DqlTable,
    alias: String,
    start: u32,
    end: u32,
    call: DqlCall,
}

/// Where the query builder a call is made on comes from, as far as the
/// call's own function body shows.
#[derive(Debug, Clone, PartialEq)]
enum Builder {
    /// A `createQueryBuilder()` call of the chain the call is made in.
    Created,
    /// A variable.
    Variable(String),
    /// What the call of another method whose name starts at the offset
    /// returns.
    Method(u32),
    /// Anything else: a parameter's, a property's.
    Elsewhere,
}

/// What an assignment gives a variable that holds a query builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// A new builder `createQueryBuilder()` creates.
    New,
    /// The builder the variable already holds (`$qb = $qb->andWhere(…)`).
    Same,
    /// The builder the call of another method whose name starts at the
    /// offset returns.
    Method(u32),
}

/// A variable passed as an argument to a method call.
#[derive(Debug)]
struct Pass {
    /// Where the variable is written.
    at: u32,
    /// Where the method name of the call starts.
    call: u32,
    index: u32,
}

/// A method's body and what its parameters are named, `None` for one that
/// is variadic or promoted.
#[derive(Debug)]
struct MethodNote<'a> {
    scope_start: u32,
    name_offset: u32,
    parameters: Vec<Option<&'a str>>,
}

/// The variables and fields read so far, settled once the whole file is.
#[derive(Debug, Default)]
pub(super) struct DqlPending<'a> {
    decls: Vec<PendingDecl>,
    fields: Vec<PendingField>,
    uses: Vec<PendingUse>,
    /// The source range each query covers.
    queries: Vec<(u32, u32)>,
    /// The tables a declaration whose variable the text does not spell is
    /// read into, which may declare any variable.
    opaque: Vec<DqlTable>,
    /// The builders the DQL of each function body is given to.
    builders: Vec<(DqlTable, Builder)>,
    /// The offset of each variable a method is called on.
    receivers: Vec<u32>,
    /// The offset of each variable assigned a builder, and where the
    /// builder comes from.
    created: Vec<(u32, Origin)>,
    /// The variables passed to method calls.
    passes: Vec<Pass>,
    /// The statements of each function body, by where its table starts,
    /// for following its query builders' calls along each path.
    bodies: Vec<(u32, &'a [Statement<'a>])>,
    /// The methods, for what they declare on the builders they hand on.
    methods: Vec<MethodNote<'a>>,
}

/// Record the statements of the function body whose DQL table starts at
/// `scope_start`.
pub(super) fn note_dql_body<'a>(
    scope_start: u32,
    statements: &'a [Statement<'a>],
    ctx: &mut ExtractionCtx<'a>,
) {
    if ctx.doctrine {
        ctx.dql.bodies.push((scope_start, statements));
    }
}

/// Record the method `method` whose body's DQL table starts at
/// `scope_start`.
pub(super) fn note_dql_method<'a>(
    scope_start: u32,
    method: &'a Method<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    if !ctx.doctrine {
        return;
    }
    let parameters = method
        .parameter_list
        .parameters
        .iter()
        .map(|parameter| {
            (parameter.ellipsis.is_none() && parameter.modifiers.is_empty())
                .then(|| crate::atom::bytes_to_str(parameter.variable.name))
        })
        .collect();
    ctx.dql.methods.push(MethodNote {
        scope_start,
        name_offset: method.name.span.start.offset,
        parameters,
    });
}

/// Where the builder `expr` is comes from: the `createQueryBuilder()` call
/// its chain starts with, the variable it starts with, the call of another
/// method on something other than a variable that it starts with, or
/// something else.  A call made on an expression builder is made on the
/// builder whose `expr()` it is.
fn builder_of(mut expr: &Expression<'_>) -> Builder {
    loop {
        expr = match expr {
            Expression::Parenthesized(inner) => inner.expression,
            Expression::Call(Call::Method(call)) => match &call.method {
                ClassLikeMemberSelector::Identifier(ident)
                    if ident.value.eq_ignore_ascii_case(b"createQueryBuilder") =>
                {
                    return Builder::Created;
                }
                ClassLikeMemberSelector::Identifier(ident)
                    if !matches!(
                        call.object,
                        Expression::Call(_)
                            | Expression::Parenthesized(_)
                            | Expression::Variable(Variable::Direct(_))
                    ) || is_this(call.object) =>
                {
                    let name = crate::atom::bytes_to_str(ident.value);
                    if name.eq_ignore_ascii_case("expr")
                        || crate::doctrine::query::is_replayed(name)
                    {
                        return Builder::Elsewhere;
                    }
                    return Builder::Method(ident.span.start.offset);
                }
                _ => call.object,
            },
            Expression::Call(Call::NullSafeMethod(call)) => call.object,
            Expression::Variable(Variable::Direct(variable)) if variable.name != b"$this" => {
                return Builder::Variable(crate::atom::bytes_to_str(variable.name).to_string());
            }
            _ => return Builder::Elsewhere,
        };
    }
}

fn is_this(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Variable(Variable::Direct(variable)) if variable.name == b"$this")
}

/// Record an assignment to a variable of a builder `createQueryBuilder()`
/// creates or another method returns, or of the builder the variable holds
/// back to it.
pub(super) fn note_builder_assignment(assign: &Assignment<'_>, ctx: &mut ExtractionCtx<'_>) {
    if !ctx.doctrine || !assign.operator.is_assign() {
        return;
    }
    let Expression::Variable(Variable::Direct(variable)) = assign.lhs else {
        return;
    };
    let origin = match builder_of(assign.rhs) {
        Builder::Created => Origin::New,
        Builder::Method(call) => Origin::Method(call),
        Builder::Variable(name) if name.as_bytes() == variable.name => Origin::Same,
        _ => return,
    };
    ctx.dql.created.push((variable.span.start.offset, origin));
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
    /// A fragment of DQL, or an array of them, of the part of the grammar
    /// its place in the query gives it.
    Fragment(Grammar),
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
    let args: &'static [Arg] = match method.to_ascii_lowercase().as_str() {
        "select" | "addselect" => return Some((&[Fragment(Grammar::SelectList)], true)),
        "where" | "andwhere" | "orwhere" | "having" | "andhaving" | "orhaving" => {
            return Some((&[Fragment(Grammar::Condition)], true));
        }
        "groupby" | "addgroupby" => return Some((&[Fragment(Grammar::GroupBy)], true)),
        "orderby" | "addorderby" => &[Fragment(Grammar::OrderBy)],
        "from" => &[Entity, Alias, Fragment(Grammar::Path)],
        "join" | "innerjoin" | "leftjoin" => &[
            Join,
            Alias,
            Skip,
            Fragment(Grammar::Condition),
            Fragment(Grammar::Path),
        ],
        "update" | "delete" => &[Entity, Alias],
        "set" => &[Fragment(Grammar::Path), Fragment(Grammar::NewValue)],
        "indexby" => &[Skip, Fragment(Grammar::Path)],
        "createquerybuilder" => &[RepositoryAlias, Fragment(Grammar::Path)],
        _ => return None,
    };
    Some((args, false))
}

/// The part of the grammar the argument at `index` of the expression
/// builder method `method` is, for the methods whose arguments are known.
fn expr_grammar(method: &str, index: usize) -> Option<Grammar> {
    Some(match method.to_ascii_lowercase().as_str() {
        "andx" | "orx" | "not" => Grammar::Condition,
        "count" | "countdistinct" | "avg" | "max" | "min" => Grammar::Aggregate,
        "exists" | "all" | "some" | "any" => Grammar::Subselect,
        "in" | "notin" if index == 1 => Grammar::InList,
        "isinstanceof" if index == 1 => return None,
        "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "in" | "notin" | "isnull" | "isnotnull"
        | "between" | "like" | "notlike" | "ismemberof" | "isinstanceof" | "sum" | "diff"
        | "prod" | "quot" | "abs" | "sqrt" | "mod" | "lower" | "upper" | "length" | "trim"
        | "concat" | "substring" => Grammar::Operand,
        "asc" | "desc" => Grammar::OrderBy,
        _ => return None,
    })
}

/// The classes whose methods take DQL, which the receiver of a call that
/// does must be for its text to be judged.
const QUERY_BUILDER: &str = "Doctrine\\ORM\\QueryBuilder";
const EXPRESSION_BUILDER: &str = "Doctrine\\ORM\\Query\\Expr";
const ENTITY_MANAGER: &str = "Doctrine\\ORM\\EntityManagerInterface";
const QUERY: &str = "Doctrine\\ORM\\Query";
const REPOSITORY: &str = "Doctrine\\ORM\\EntityRepository";

/// The call DQL is read from: where its method name starts, and the class
/// its receiver must be.
#[derive(Debug, Clone, Copy)]
struct DqlCall {
    method_start: u32,
    receiver: &'static str,
}

impl DqlCall {
    fn with(self, receiver: &'static str) -> Self {
        Self { receiver, ..self }
    }
}

/// Record where the DQL `source`, read from `expr`, stops following the
/// part of the grammar `grammar`, if it does.  Text a dynamic part runs
/// through is not judged.
fn check_syntax(
    source: &DqlSource,
    expr: &Expression<'_>,
    grammar: Grammar,
    call: DqlCall,
    ctx: &mut ExtractionCtx<'_>,
) {
    if source.text.contains(DYNAMIC) {
        return;
    }
    let Some(error) = dql_syntax::check(&source.text, grammar) else {
        return;
    };
    let span = expr.span();
    let (start, end) = error
        .range
        .as_ref()
        .and_then(|range| source.source_range(range))
        .unwrap_or((span.start.offset, span.end.offset));
    ctx.dql_syntax_errors
        .push(crate::symbol_map::DqlSyntaxSite {
            start,
            end,
            message: error.message.into_boxed_str(),
            code: crate::diagnostics::dql_syntax::DQL_SYNTAX_ERROR_CODE,
            call: call.method_start,
            receiver: call.receiver,
            requires: Box::default(),
        });
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
    method_start: u32,
    arguments: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
    scope_start: u32,
) {
    if !ctx.doctrine {
        return;
    }
    if let Expression::Variable(Variable::Direct(variable)) = object {
        ctx.dql.receivers.push(variable.span.start.offset);
    }
    for (index, argument) in arguments.arguments.iter().enumerate() {
        let Argument::Positional(positional) = argument else {
            break;
        };
        if positional.ellipsis.is_some() {
            break;
        }
        if let Expression::Variable(Variable::Direct(variable)) = positional.value {
            ctx.dql.passes.push(Pass {
                at: variable.span.start.offset,
                call: method_start,
                index: index as u32,
            });
        }
    }
    let call = DqlCall {
        method_start,
        receiver: QUERY_BUILDER,
    };
    let create_query = method.eq_ignore_ascii_case("createQuery");
    if create_query || method.eq_ignore_ascii_case("setDQL") {
        let Some(argument) = argument_at(arguments, 0, "dql") else {
            return;
        };
        let span = argument.span();
        let table = DqlTable::Query(span.start.offset);
        let mut source = DqlSource::new();
        source.push_expr(argument, ctx.content);
        if source.literal {
            ctx.dql.queries.push((span.start.offset, span.end.offset));
            let receiver = if create_query { ENTITY_MANAGER } else { QUERY };
            read_source(&source, table, call.with(receiver), ctx);
            check_syntax(
                &source,
                argument,
                Grammar::Statement,
                call.with(receiver),
                ctx,
            );
        }
        return;
    }

    let table = DqlTable::Scope(scope_start);
    if is_expression_builder(object) {
        // `literal()` quotes a value rather than reading DQL.
        if method.eq_ignore_ascii_case("literal") {
            return;
        }
        ctx.dql.builders.push((table, builder_of(object)));
        for (index, argument) in arguments.arguments.iter().enumerate() {
            let grammar = match argument {
                Argument::Positional(positional) if positional.ellipsis.is_none() => {
                    expr_grammar(method, index)
                }
                _ => None,
            };
            read_fragment(
                argument.value(),
                table,
                call.with(EXPRESSION_BUILDER),
                grammar,
                ctx,
            );
        }
        return;
    }
    let call = if method.eq_ignore_ascii_case("createQueryBuilder") {
        call.with(REPOSITORY)
    } else {
        call
    };
    let Some((roles, variadic)) = builder_args(method) else {
        return;
    };
    // The builder `createQueryBuilder()` returns is a new one, whatever it
    // is called on.
    let builder = if call.receiver == REPOSITORY {
        Builder::Created
    } else {
        builder_of(object)
    };
    ctx.dql.builders.push((table, builder));
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
            Arg::Fragment(grammar) => match crate::parser::array_literal_elements(argument) {
                Some(elements) => {
                    for element in elements.iter() {
                        if let Some(value) = crate::parser::array_element_value(element) {
                            read_fragment(value, table, call, Some(grammar), ctx);
                        }
                    }
                }
                None => read_fragment(argument, table, call, Some(grammar), ctx),
            },
            role @ (Arg::Entity | Arg::Join) => {
                // Without the variable it declares the call is no query
                // builder's: `$cache->delete('key')`.
                let Some(alias) = positional.get(index + 1).copied().flatten() else {
                    continue;
                };
                if !string_literal_text(alias).is_some_and(is_identifier) {
                    ctx.dql.opaque.push(table);
                    continue;
                }
                let mut source = DqlSource::new();
                source.push_synthetic(if role == Arg::Entity {
                    "FROM "
                } else {
                    "JOIN "
                });
                source.push_expr(argument, ctx.content);
                source.push_synthetic(" ");
                source.push_expr(alias, ctx.content);
                read_source(&source, table, call, ctx);
            }
            Arg::RepositoryAlias => {
                let Some(alias) = string_literal_text(argument) else {
                    ctx.dql.opaque.push(table);
                    continue;
                };
                ctx.dql.decls.push(PendingDecl {
                    table,
                    alias: alias.to_string(),
                    root: php_root(object),
                    at: method_start,
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

/// Read the DQL fragment `expr` that `call` takes, and check it against
/// `grammar`'s part of the grammar when one is given.
fn read_fragment(
    expr: &Expression<'_>,
    table: DqlTable,
    call: DqlCall,
    grammar: Option<Grammar>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let mut source = DqlSource::new();
    source.push_expr(expr, ctx.content);
    if source.literal {
        read_source(&source, table, call, ctx);
        if let Some(grammar) = grammar {
            check_syntax(&source, expr, grammar, call, ctx);
        }
    }
}

/// Record what the DQL `source`, which `call` takes, declares and names.
fn read_source(source: &DqlSource, table: DqlTable, call: DqlCall, ctx: &mut ExtractionCtx<'_>) {
    let scan = dql::scan(&source.text);
    if scan.opaque {
        ctx.dql.opaque.push(table);
    }
    for decl in scan.decls {
        let root = match &decl.root {
            DeclRoot::Entity(name) => {
                emit_class(source, name, ctx);
                source.root(name)
            }
            DeclRoot::Association { alias, field } => Root::Join {
                alias: source.text[alias.clone()].to_string(),
                association: source.text[field.clone()].to_string(),
            },
            DeclRoot::Unknown => Root::Unknown,
        };
        ctx.dql.decls.push(PendingDecl {
            table,
            alias: source.text[decl.alias].to_string(),
            root,
            at: call.method_start,
        });
    }
    for reference in scan.refs {
        match reference {
            DqlRef::Class(name) => emit_class(source, &name, ctx),
            DqlRef::Field { alias, field } => {
                if let Some((start, end)) = source.source_range(&alias) {
                    ctx.dql.uses.push(PendingUse {
                        table,
                        alias: source.text[alias.clone()].to_string(),
                        start,
                        end,
                        call,
                    });
                }
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

/// What a variable stands for, as a subject text.
struct Subject<'a> {
    /// The subject text of the root the variable's joins start from, and
    /// the source span it was read from when PHP code names it.
    root: std::borrow::Cow<'a, str>,
    span: Option<(u32, u32)>,
    /// The associations joined from the root, outermost last.
    path: Vec<&'a str>,
}

impl Subject<'_> {
    fn text(&self) -> String {
        let mut text = self.root.to_string();
        for association in &self.path {
            text.push('.');
            text.push_str(association);
        }
        text
    }

    fn subject_text(&self, content: &str) -> SubjectText {
        match self.span {
            Some((start, end)) if self.path.is_empty() => {
                SubjectText::new(self.root.to_string(), start, end, content)
            }
            _ => SubjectText::owned(self.text()),
        }
    }
}

/// Settle the variables of every query and function body read, emitting a
/// span for each field whose variable stands for something, and recording
/// the variables for completion.
pub(super) fn finish_dql(ctx: &mut ExtractionCtx<'_>) {
    let pending = std::mem::take(&mut ctx.dql);
    summarize_methods(&pending, ctx);
    if pending.decls.is_empty() {
        report_undeclared(&pending, ctx);
        return;
    }
    let declared = |table: DqlTable, alias: &str| -> Option<&Root> {
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
        found
    };
    // A join's variable stands for what the variable it joins from stands
    // for, followed through each join back to a root.  A variable joined
    // from itself, directly or not, stands for nothing.
    let resolve = |table: DqlTable, alias: &str| -> Option<Subject<'_>> {
        let mut path: Vec<&str> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        let mut current: &str = alias;
        loop {
            if seen.contains(&current) {
                return None;
            }
            seen.push(current);
            let (root, span) = match declared(table, current)? {
                Root::Php { text, start, end } => (text.into(), Some((*start, *end))),
                Root::Fqn(name) => (format!("\\{name}").into(), None),
                Root::Join { alias, association } => {
                    path.push(association);
                    current = alias;
                    continue;
                }
                Root::Unknown => return None,
            };
            path.reverse();
            return Some(Subject { root, span, path });
        }
    };

    for field in &pending.fields {
        let Some(subject) = resolve(field.table, &field.alias) else {
            continue;
        };
        let subject_text = subject.subject_text(ctx.content);
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

    report_undeclared(&pending, ctx);

    let content_len = ctx.content.len() as u32;
    let mut sites: Vec<crate::symbol_map::DqlAliasSite> = Vec::new();
    for decl in &pending.decls {
        let Some(subject) = resolve(decl.table, &decl.alias) else {
            continue;
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
            subject: subject.text().into_boxed_str(),
        });
    }
    ctx.dql_aliases = sites;
}

/// Record each variable a path starts from that nothing declares, or that
/// some path through its function body never declares, at its first use
/// left undeclared in the query or function body, where
/// everything that could declare it is in sight: a whole query that spells
/// all its declarations out, or a function body whose query builders it
/// creates itself and keeps to itself.  A builder from anywhere else may
/// have had any variable declared before it got here.
///
/// A builder another method returns, or that is passed to one, is in sight
/// when that method's own body is: the variable is reported only once the
/// diagnostic pass finds each such method declares no variable but the
/// ones its body is known to (see [`summarize_methods`]), and none of them
/// is this one.
fn report_undeclared(pending: &DqlPending, ctx: &mut ExtractionCtx<'_>) {
    let mut tables: Vec<(DqlTable, Option<Vec<DqlProvider>>)> = Vec::new();
    let mut reported: Vec<(DqlTable, &str)> = Vec::new();
    let mut uses: Vec<&PendingUse> = pending.uses.iter().collect();
    uses.sort_by_key(|site| site.start);
    for site in uses {
        if reported.contains(&(site.table, site.alias.as_str()))
            || pending
                .decls
                .iter()
                .any(|d| d.table == site.table && d.alias == site.alias)
        {
            continue;
        }
        let index = match tables.iter().position(|(table, _)| *table == site.table) {
            Some(index) => index,
            None => {
                tables.push((site.table, is_closed(site.table, pending, ctx)));
                tables.len() - 1
            }
        };
        let Some(providers) = &tables[index].1 else {
            continue;
        };
        reported.push((site.table, &site.alias));
        push_undeclared(site, providers, ctx);
    }

    // A variable declared somewhere in a function body may still be
    // undeclared on some path through it to its query.
    for &(scope_start, statements) in &pending.bodies {
        let table = DqlTable::Scope(scope_start);
        let providers = match tables.iter().find(|(t, _)| *t == table) {
            Some((_, providers)) => providers.clone(),
            None => is_closed(table, pending, ctx),
        };
        let Some(providers) = providers else {
            continue;
        };
        let sites: Vec<&PendingUse> = pending
            .uses
            .iter()
            .filter(|site| {
                site.table == table
                    && !reported.contains(&(table, site.alias.as_str()))
                    && pending
                        .decls
                        .iter()
                        .any(|d| d.table == table && d.alias == site.alias)
            })
            .collect();
        if sites.is_empty() {
            continue;
        }
        let mut events: Vec<(u32, Event<'_>)> = sites
            .iter()
            .enumerate()
            .map(|(index, site)| (site.start, Event::Use(index)))
            .chain(
                pending
                    .decls
                    .iter()
                    .filter(|d| d.table == table)
                    .map(|d| (d.at, Event::Declare(d.alias.as_str()))),
            )
            .collect();
        events.sort_by_key(|(at, _)| *at);
        let paths = Paths {
            events: &events,
            sites: &sites,
        };
        let flow = paths.statements(statements, vec![NOTHING_YET; sites.len()]);
        let mut exits = flow.next;
        merge(&mut exits, &flow.exits);
        let mut bad: Vec<&PendingUse> = sites
            .iter()
            .zip(&exits)
            .filter(|(_, state)| *state & UNDECLARED != 0)
            .map(|(site, _)| *site)
            .collect();
        bad.sort_by_key(|site| site.start);
        for site in bad {
            if reported.contains(&(table, site.alias.as_str())) {
                continue;
            }
            reported.push((table, &site.alias));
            push_undeclared(site, &providers, ctx);
        }
    }
}

fn push_undeclared(site: &PendingUse, providers: &[DqlProvider], ctx: &mut ExtractionCtx<'_>) {
    ctx.dql_syntax_errors
        .push(crate::symbol_map::DqlSyntaxSite {
            start: site.start,
            end: site.end,
            message: format!("Undefined DQL variable '{}'", site.alias).into_boxed_str(),
            code: crate::diagnostics::dql_syntax::UNKNOWN_DQL_VARIABLE_CODE,
            call: site.call.method_start,
            receiver: site.call.receiver,
            requires: providers.into(),
        });
}

/// A query builder call along a path: one declaring a variable, or one
/// using the variable of the tracked use at the index.
#[derive(Debug, Clone, Copy)]
enum Event<'a> {
    Declare(&'a str),
    Use(usize),
}

/// For each use a function body's paths are followed for, the states the
/// paths reaching a point are in, one bit each: whether the use's variable
/// has been declared, and whether the use has been passed.  The variables
/// a builder declares count however late in its path they are declared,
/// so a use is only wrong on a path that passes it and never declares its
/// variable, and paths that agree on both can be merged.  An empty set
/// means no path reaches the point.
type States = Vec<u8>;

const NOTHING_YET: u8 = 0b0001;
const DECLARED: u8 = 0b0010;
const UNDECLARED: u8 = 0b0100;
const USED_DECLARED: u8 = 0b1000;

fn merge(into: &mut States, from: &States) {
    for (a, b) in into.iter_mut().zip(from) {
        *a |= b;
    }
}

fn unreachable(states: &States) -> bool {
    states.iter().all(|s| *s == 0)
}

/// Where the paths through a statement go: on to the next one, out of the
/// loop or `switch` around it, to the loop's next iteration, or out of
/// the function.  A path that throws goes nowhere a query is run.
struct Flow {
    next: States,
    breaks: States,
    continues: States,
    exits: States,
}

impl Flow {
    fn to_next(next: States) -> Self {
        let len = next.len();
        Self {
            next,
            breaks: vec![0; len],
            continues: vec![0; len],
            exits: vec![0; len],
        }
    }

    fn nowhere(len: usize) -> Self {
        Self::to_next(vec![0; len])
    }

    /// Take in the paths of another branch alongside this one's.
    fn join(&mut self, other: &Flow) {
        merge(&mut self.next, &other.next);
        self.leave_with(other);
    }

    /// Take in the paths that leave `other` other than by going on.
    fn leave_with(&mut self, other: &Flow) {
        merge(&mut self.breaks, &other.breaks);
        merge(&mut self.continues, &other.continues);
        merge(&mut self.exits, &other.exits);
    }
}

/// Follows the tracked uses of a function body along each path through
/// its statements.  Control flow inside an expression (`?:`, `&&`,
/// `match`) is read as though every part of it runs, which can only make
/// a path declare more.
struct Paths<'p, 'a> {
    events: &'p [(u32, Event<'a>)],
    sites: &'p [&'p PendingUse],
}

impl Paths<'_, '_> {
    /// Apply the calls made in the source range `start..end`.
    fn apply(&self, states: &mut States, start: u32, end: u32) {
        let from = self.events.partition_point(|(at, _)| *at < start);
        for (at, event) in &self.events[from..] {
            if *at >= end {
                break;
            }
            match event {
                Event::Use(index) => {
                    let s = &mut states[*index];
                    *s = ((*s & (NOTHING_YET | DECLARED)) << 2)
                        | (*s & (UNDECLARED | USED_DECLARED));
                }
                Event::Declare(alias) => {
                    for (site, s) in self.sites.iter().zip(states.iter_mut()) {
                        if site.alias != *alias {
                            continue;
                        }
                        let mut next = 0;
                        if *s & (NOTHING_YET | DECLARED) != 0 {
                            next |= DECLARED;
                        }
                        if *s & (UNDECLARED | USED_DECLARED) != 0 {
                            next |= USED_DECLARED;
                        }
                        *s = next;
                    }
                }
            }
        }
    }

    fn apply_span(&self, states: &mut States, node: &impl HasSpan) {
        let span = node.span();
        self.apply(states, span.start.offset, span.end.offset);
    }

    fn statements(&self, statements: &[Statement<'_>], states: States) -> Flow {
        let mut flow = Flow::to_next(states);
        for statement in statements {
            if unreachable(&flow.next) {
                break;
            }
            let next = std::mem::take(&mut flow.next);
            let inner = self.statement(statement, next);
            flow.next = inner.next.clone();
            flow.leave_with(&inner);
        }
        flow
    }

    fn statement(&self, statement: &Statement<'_>, mut states: States) -> Flow {
        let len = states.len();
        match statement {
            Statement::Block(block) => self.statements(block.statements.as_slice(), states),
            Statement::Namespace(namespace) => {
                self.statements(namespace.statements().as_slice(), states)
            }
            Statement::If(if_stmt) => {
                self.apply_span(&mut states, if_stmt.condition);
                let mut flow = self.statements(if_stmt.body.statements(), states.clone());
                for (condition, statements) in if_stmt.body.else_if_clauses() {
                    self.apply_span(&mut states, condition);
                    flow.join(&self.statements(statements, states.clone()));
                }
                match if_stmt.body.else_statements() {
                    Some(statements) => flow.join(&self.statements(statements, states)),
                    None => merge(&mut flow.next, &states),
                }
                flow
            }
            Statement::While(while_stmt) => {
                self.apply_span(&mut states, while_stmt.condition);
                let body = self.statements(while_stmt.body.statements(), states.clone());
                self.after_loop(states, body, |_| {})
            }
            Statement::Foreach(foreach) => {
                self.apply(
                    &mut states,
                    foreach.span().start.offset,
                    foreach.body.span().start.offset,
                );
                let body = self.statements(foreach.body.statements(), states.clone());
                self.after_loop(states, body, |_| {})
            }
            Statement::For(for_stmt) => {
                for expr in for_stmt
                    .initializations
                    .iter()
                    .chain(for_stmt.conditions.iter())
                {
                    self.apply_span(&mut states, *expr);
                }
                let body = self.statements(for_stmt.body.statements(), states.clone());
                self.after_loop(states, body, |next| {
                    for expr in for_stmt.increments.iter().chain(for_stmt.conditions.iter()) {
                        self.apply_span(next, *expr);
                    }
                })
            }
            Statement::DoWhile(do_while) => {
                let body = self.statements(std::slice::from_ref(do_while.statement), states);
                let mut next = body.next.clone();
                merge(&mut next, &body.continues);
                self.apply_span(&mut next, do_while.condition);
                merge(&mut next, &body.breaks);
                let mut flow = Flow::to_next(next);
                merge(&mut flow.exits, &body.exits);
                flow
            }
            Statement::Switch(switch) => {
                self.apply_span(&mut states, switch.expression);
                let mut flow = Flow::nowhere(len);
                let mut unmatched = states;
                let mut falling = vec![0; len];
                let mut has_default = false;
                for case in switch.body.cases().iter() {
                    match case {
                        SwitchCase::Expression(case) => {
                            self.apply_span(&mut unmatched, case.expression);
                        }
                        SwitchCase::Default(_) => has_default = true,
                    }
                    merge(&mut falling, &unmatched);
                    let body = self.statements(case.statements(), falling);
                    falling = body.next;
                    // A `continue` aimed at a `switch` acts as a `break`.
                    merge(&mut flow.next, &body.breaks);
                    merge(&mut flow.next, &body.continues);
                    merge(&mut flow.exits, &body.exits);
                }
                merge(&mut flow.next, &falling);
                if !has_default {
                    merge(&mut flow.next, &unmatched);
                }
                flow
            }
            Statement::Try(try_stmt) => {
                let body = self.statements(try_stmt.block.statements.as_slice(), states.clone());
                // A catch is entered from anywhere in the block; from its
                // start or its end, a path has made fewer calls or more.
                let mut caught = states;
                merge(&mut caught, &body.next);
                let mut flow = Flow::to_next(body.next.clone());
                flow.leave_with(&body);
                for catch in try_stmt.catch_clauses.iter() {
                    let handler =
                        self.statements(catch.block.statements.as_slice(), caught.clone());
                    flow.join(&handler);
                }
                let Some(finally) = &try_stmt.finally_clause else {
                    return flow;
                };
                let block = finally.block.statements.as_slice();
                for leaving in [&mut flow.breaks, &mut flow.continues, &mut flow.exits] {
                    self.apply_span(leaving, &finally.block);
                }
                let after = self.statements(block, std::mem::take(&mut flow.next));
                flow.next = after.next.clone();
                flow.leave_with(&after);
                flow
            }
            Statement::Return(ret) => {
                self.apply_span(&mut states, ret);
                let mut flow = Flow::nowhere(len);
                flow.exits = states;
                flow
            }
            Statement::Break(brk) => {
                let mut flow = Flow::nowhere(len);
                if loop_level(brk.level) == 1 {
                    flow.breaks = states;
                }
                flow
            }
            Statement::Continue(cont) => {
                let mut flow = Flow::nowhere(len);
                if loop_level(cont.level) == 1 {
                    flow.continues = states;
                }
                flow
            }
            Statement::Expression(expr_stmt) if ends_script(expr_stmt.expression) => {
                Flow::nowhere(len)
            }
            _ => {
                self.apply_span(&mut states, statement);
                Flow::to_next(states)
            }
        }
    }

    /// The paths out of a loop entered with `entry`, whose body is `body`
    /// and whose next iteration starts with `repeat`.  One pass through
    /// the body is enough: a path that goes round more than once only
    /// adds calls to one that goes round once.
    fn after_loop(&self, entry: States, body: Flow, repeat: impl FnOnce(&mut States)) -> Flow {
        let mut again = body.next;
        merge(&mut again, &body.continues);
        repeat(&mut again);
        let mut next = entry;
        merge(&mut next, &again);
        merge(&mut next, &body.breaks);
        let mut flow = Flow::to_next(next);
        merge(&mut flow.exits, &body.exits);
        flow
    }
}

/// How many loops a `break` or `continue` leaves.  Paths that leave more
/// than one are not followed.
fn loop_level(level: Option<&Expression<'_>>) -> u64 {
    match level {
        None => 1,
        Some(Expression::Literal(literal::Literal::Integer(integer))) => integer.value.unwrap_or(0),
        Some(_) => 0,
    }
}

/// Whether the expression statement `expr` throws or exits.
fn ends_script(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::Throw(_) | Expression::Construct(Construct::Exit(_) | Construct::Die(_))
    )
}

/// Whether every declaration the DQL of `table` can see is read, and if so
/// the calls of other methods whose bodies hold some of them.  A function
/// body that returns a builder is not closed: its caller may go on to
/// declare anything on it.
fn is_closed(
    table: DqlTable,
    pending: &DqlPending,
    ctx: &ExtractionCtx<'_>,
) -> Option<Vec<DqlProvider>> {
    if pending.opaque.contains(&table) {
        return None;
    }
    let DqlTable::Scope(scope_start) = table else {
        return Some(Vec::new());
    };
    let mut providers: Vec<DqlProvider> = Vec::new();
    let mut any = false;
    for (_, builder) in pending.builders.iter().filter(|(t, _)| *t == table) {
        any = true;
        match builder {
            Builder::Created => {}
            Builder::Method(call) => providers.push(DqlProvider {
                call: *call,
                slot: DqlSlot::Returned,
            }),
            Builder::Variable(name) => {
                if !keeps_builder(
                    name,
                    scope_start,
                    pending,
                    ctx,
                    Keep::Report,
                    &mut providers,
                ) {
                    return None;
                }
            }
            Builder::Elsewhere => return None,
        }
    }
    if !any || returns_builder(scope_start, pending) {
        return None;
    }
    providers.sort_by_key(|p| p.call);
    providers.dedup();
    Some(providers)
}

/// What a variable that holds a query builder is checked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keep {
    /// The function body's own queries: the builder may come from another
    /// method, or be passed to one, whose body is read in turn.
    Report,
    /// What a method returns: a builder it creates, which it may return.
    Returned,
    /// What a method is passed: a parameter, only ever used to call its
    /// methods.
    Parameter,
}

/// Whether the variable `name` of the function body starting at
/// `scope_start` only ever holds a builder of the kind `keep` allows, and
/// is only ever used to call its methods: passed to anything else, the
/// builder may have variables declared on it out of sight.  The calls of
/// other methods the builder comes from or is passed to are added to
/// `providers`.
fn keeps_builder(
    name: &str,
    scope_start: u32,
    pending: &DqlPending,
    ctx: &ExtractionCtx<'_>,
    keep: Keep,
    providers: &mut Vec<DqlProvider>,
) -> bool {
    let bare = name.strip_prefix('$').unwrap_or(name);
    let defs = ctx
        .var_defs
        .iter()
        .filter(|def| def.scope_start == scope_start && def.name == bare);
    let mut assigned = false;
    let mut def_offsets: Vec<u32> = Vec::new();
    for def in defs {
        def_offsets.push(def.offset);
        match def.kind {
            VarDefKind::Assignment if keep != Keep::Parameter => {
                match pending.created.iter().find(|(o, _)| *o == def.offset) {
                    Some((_, Origin::New)) => assigned = true,
                    Some((_, Origin::Same)) => {}
                    Some(&(_, Origin::Method(call))) if keep == Keep::Report => {
                        assigned = true;
                        providers.push(DqlProvider {
                            call,
                            slot: DqlSlot::Returned,
                        });
                    }
                    _ => return false,
                }
            }
            VarDefKind::Parameter if keep == Keep::Parameter => assigned = true,
            VarDefKind::DocblockVar | VarDefKind::DocblockParam | VarDefKind::Unset => {}
            _ => return false,
        }
    }
    if !assigned {
        return false;
    }
    let Some((start, end)) = scope_range(scope_start, ctx) else {
        return false;
    };
    let returned: Vec<u32> = match keep {
        Keep::Returned => returned_values(scope_start, pending)
            .filter_map(|value| match value {
                Some(Expression::Variable(Variable::Direct(variable))) => {
                    Some(variable.span.start.offset)
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    for span in &ctx.spans {
        let SymbolKind::Variable { name } = &span.kind else {
            continue;
        };
        if span.start < start
            || span.end > end
            || name.as_str() != bare
            || pending.receivers.contains(&span.start)
            || def_offsets.contains(&span.start)
            || returned.contains(&span.start)
        {
            continue;
        }
        match pending.passes.iter().find(|pass| pass.at == span.start) {
            Some(pass) if keep == Keep::Report => providers.push(DqlProvider {
                call: pass.call,
                slot: DqlSlot::Parameter(pass.index),
            }),
            _ => return false,
        }
    }
    true
}

/// The source range of the function body starting at `scope_start`.
fn scope_range(scope_start: u32, ctx: &ExtractionCtx<'_>) -> Option<(u32, u32)> {
    match scope_start {
        0 => Some((0, ctx.content.len() as u32)),
        start => ctx.scopes.iter().find(|(s, _)| *s == start).copied(),
    }
}

/// The values the function body starting at `scope_start` returns, `None`
/// for a `return;`.
fn returned_values<'p, 'a>(
    scope_start: u32,
    pending: &'p DqlPending<'a>,
) -> impl Iterator<Item = Option<&'a Expression<'a>>> + 'p {
    pending
        .bodies
        .iter()
        .filter(move |(start, _)| *start == scope_start)
        .flat_map(|&(_, statements)| {
            let mut values = Vec::new();
            collect_returns(statements, &mut values);
            values
        })
}

/// Collect the values `statements` return, leaving out the functions they
/// declare.
fn collect_returns<'a>(statements: &'a [Statement<'a>], out: &mut Vec<Option<&'a Expression<'a>>>) {
    for statement in statements {
        match statement {
            Statement::Return(ret) => out.push(ret.value),
            Statement::Block(block) => collect_returns(block.statements.as_slice(), out),
            Statement::If(if_stmt) => {
                collect_returns(if_stmt.body.statements(), out);
                for (_, statements) in if_stmt.body.else_if_clauses() {
                    collect_returns(statements, out);
                }
                if let Some(statements) = if_stmt.body.else_statements() {
                    collect_returns(statements, out);
                }
            }
            Statement::While(while_stmt) => collect_returns(while_stmt.body.statements(), out),
            Statement::Foreach(foreach) => collect_returns(foreach.body.statements(), out),
            Statement::For(for_stmt) => collect_returns(for_stmt.body.statements(), out),
            Statement::DoWhile(do_while) => {
                collect_returns(std::slice::from_ref(do_while.statement), out);
            }
            Statement::Switch(switch) => {
                for case in switch.body.cases().iter() {
                    collect_returns(case.statements(), out);
                }
            }
            Statement::Try(try_stmt) => {
                collect_returns(try_stmt.block.statements.as_slice(), out);
                for catch in try_stmt.catch_clauses.iter() {
                    collect_returns(catch.block.statements.as_slice(), out);
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    collect_returns(finally.block.statements.as_slice(), out);
                }
            }
            _ => {}
        }
    }
}

/// The builder `expr` is when it is one: the chain of builder calls it is
/// starts with a `createQueryBuilder()` call, a variable, or the call of
/// another method.
fn returned_builder(mut expr: &Expression<'_>) -> Option<Builder> {
    loop {
        expr = match expr {
            Expression::Parenthesized(inner) => inner.expression,
            Expression::Call(Call::Method(call)) => {
                let ClassLikeMemberSelector::Identifier(ident) = &call.method else {
                    return None;
                };
                let name = crate::atom::bytes_to_str(ident.value);
                if name.eq_ignore_ascii_case("createQueryBuilder") {
                    return Some(Builder::Created);
                }
                if crate::doctrine::query::is_replayed(name) {
                    call.object
                } else if is_this(call.object)
                    || !matches!(
                        call.object,
                        Expression::Call(_)
                            | Expression::Parenthesized(_)
                            | Expression::Variable(Variable::Direct(_))
                    )
                {
                    return Some(Builder::Method(ident.span.start.offset));
                } else {
                    return None;
                }
            }
            Expression::Variable(Variable::Direct(variable)) if !is_this(expr) => {
                return Some(Builder::Variable(
                    crate::atom::bytes_to_str(variable.name).to_string(),
                ));
            }
            _ => return None,
        };
    }
}

/// Whether the function body starting at `scope_start` returns one of the
/// query builders its DQL is given to.
fn returns_builder(scope_start: u32, pending: &DqlPending) -> bool {
    let table = DqlTable::Scope(scope_start);
    returned_values(scope_start, pending).any(|value| {
        match value.and_then(|value| returned_builder(value)) {
            Some(Builder::Variable(name)) => pending
                .builders
                .iter()
                .any(|(t, builder)| *t == table && *builder == Builder::Variable(name.clone())),
            Some(_) => true,
            None => false,
        }
    })
}

/// Record the variables each method declares on a query builder it
/// creates and returns, or is passed and only calls the methods of, for
/// the callers that build a query on the builder.  A method that may
/// declare a variable its text does not spell declares nothing known.
fn summarize_methods(pending: &DqlPending, ctx: &mut ExtractionCtx<'_>) {
    let mut summaries: Vec<crate::symbol_map::DqlBuilderMethod> = Vec::new();
    for method in &pending.methods {
        let table = DqlTable::Scope(method.scope_start);
        if pending.opaque.contains(&table) {
            continue;
        }
        let mut aliases: Vec<crate::atom::Atom> = Vec::new();
        for decl in pending.decls.iter().filter(|d| d.table == table) {
            let alias = crate::atom::atom(&decl.alias);
            if !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
        let mut slots: Vec<DqlSlot> = Vec::new();
        if returns_own_builder(method.scope_start, pending, ctx) {
            slots.push(DqlSlot::Returned);
        }
        for (index, parameter) in method.parameters.iter().enumerate() {
            let Some(name) = parameter else {
                continue;
            };
            let used = pending
                .builders
                .iter()
                .any(|(t, b)| *t == table && matches!(b, Builder::Variable(v) if v == name));
            if used
                && keeps_builder(
                    name,
                    method.scope_start,
                    pending,
                    ctx,
                    Keep::Parameter,
                    &mut Vec::new(),
                )
            {
                slots.push(DqlSlot::Parameter(index as u32));
            }
        }
        for slot in slots {
            summaries.push(crate::symbol_map::DqlBuilderMethod {
                name_offset: method.name_offset,
                slot,
                aliases: aliases.clone().into_boxed_slice(),
            });
        }
    }
    ctx.dql_builder_methods = summaries;
}

/// Whether every value the function body starting at `scope_start` returns
/// is `null` or a builder the body creates and keeps to itself, and one is
/// a builder.
fn returns_own_builder(scope_start: u32, pending: &DqlPending, ctx: &ExtractionCtx<'_>) -> bool {
    let mut any = false;
    for value in returned_values(scope_start, pending) {
        let Some(value) = value else {
            continue;
        };
        if matches!(value, Expression::Literal(literal::Literal::Null(_))) {
            continue;
        }
        let kept = match returned_builder(value) {
            Some(Builder::Created) => true,
            // Only a variable the body's DQL is given to is worth following
            // through the body.
            Some(Builder::Variable(name))
                if pending.builders.iter().any(|(t, b)| {
                    *t == DqlTable::Scope(scope_start)
                        && matches!(b, Builder::Variable(v) if *v == name)
                }) =>
            {
                keeps_builder(
                    &name,
                    scope_start,
                    pending,
                    ctx,
                    Keep::Returned,
                    &mut Vec::new(),
                )
            }
            _ => false,
        };
        if !kept {
            return false;
        }
        any = true;
    }
    any
}
