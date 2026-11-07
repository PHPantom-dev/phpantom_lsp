//! A check of Doctrine Query Language grammar.
//!
//! Doctrine parses DQL when a query runs and throws on text that does not
//! follow its grammar.  This follows the grammar of Doctrine ORM's own
//! `Query\Lexer` and `Query\Parser`, making the same decisions from the same
//! tokens, so that what it rejects Doctrine rejects too.  Where Doctrine
//! decides by something the text does not hold, it accepts what Doctrine
//! might:
//!
//! - A function is any name followed by parentheses.  A project registers
//!   functions of its own, which parse their arguments as they please and
//!   can take over the name of a built-in one, so a call's arguments are
//!   only checked to close.  One that is not a built-in may go on past its
//!   closing parenthesis with another call (`MATCH (…) AGAINST (…)`).
//! - A bare name where Doctrine accepts it only when it is a result
//!   variable the query declares is accepted.
//!
//! A query builder hands its fragments to the parser joined into one query.
//! Each fragment is checked on its own, as the part of the grammar its
//! place in the query gives it ([`Grammar`]).

use std::ops::Range;

/// The part of the grammar a piece of DQL is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grammar {
    /// A whole query: `SELECT …`, `UPDATE …`, or `DELETE …`.
    Statement,
    /// What a `SELECT` lists: `select()`, `addSelect()`.
    SelectList,
    /// A condition: `where()`, `having()`, a join's condition, `andX()`.
    Condition,
    /// What a `GROUP BY` lists.
    GroupBy,
    /// What an `ORDER BY` lists.
    OrderBy,
    /// A path, `e.field`: `indexBy()`, the field `set()` assigns.
    Path,
    /// The value `set()` assigns.
    NewValue,
    /// An operand of a comparison or a function: `eq()`, `lower()`.
    Operand,
    /// What an aggregate function takes: `count()`, `max()`.
    Aggregate,
    /// A subquery: `exists()`, `all()`.
    Subselect,
    /// What `IN (…)` takes: a subquery or a list of values.
    InList,
}

/// Where a piece of DQL stops following the grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxError {
    /// The token the grammar does not allow, or `None` when the text ends
    /// too early.
    pub(crate) range: Option<Range<usize>>,
    pub(crate) message: String,
}

/// Check the DQL `text` against the part of the grammar `grammar`.
pub(crate) fn check(text: &str, grammar: Grammar) -> Option<SyntaxError> {
    let tokens = lex(text);
    let mut parser = Parser {
        text,
        tokens: &tokens,
        pos: 0,
        nesting: 0,
    };
    let result = parser.entry(grammar);
    let Err(Fail::Syntax { at, expected }) = result else {
        return None;
    };
    let token = tokens.get(at);
    let got = match token {
        Some(token) => {
            let written = &text[token.start..token.end];
            if token.kind == Kind::Str {
                written.to_string()
            } else {
                format!("'{written}'")
            }
        }
        None => "end of string".to_string(),
    };
    let message = match expected {
        Some(expected) => format!("DQL expects {expected}, got {got}"),
        None => format!("Unexpected {got} in DQL"),
    };
    Some(SyntaxError {
        range: token.map(|token| token.start..token.end),
        message,
    })
}

// ─── Lexer ──────────────────────────────────────────────────────────────────

macro_rules! keywords {
    ($($name:ident => $text:literal,)*) => {
        /// A word DQL reserves.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Kw { $($name,)* }

        const KEYWORDS: &[(&str, Kw)] = &[$(($text, Kw::$name),)*];

        impl Kw {
            fn text(self) -> &'static str {
                match self { $(Kw::$name => $text,)* }
            }
        }
    };
}

keywords! {
    All => "ALL", And => "AND", Any => "ANY", As => "AS", Asc => "ASC", Avg => "AVG",
    Between => "BETWEEN", Both => "BOTH", By => "BY", Case => "CASE", Coalesce => "COALESCE",
    Count => "COUNT", Delete => "DELETE", Desc => "DESC", Distinct => "DISTINCT", Else => "ELSE",
    Empty => "EMPTY", End => "END", Escape => "ESCAPE", Exists => "EXISTS", False => "FALSE",
    From => "FROM", Group => "GROUP", Having => "HAVING", Hidden => "HIDDEN", In => "IN",
    Index => "INDEX", Inner => "INNER", Instance => "INSTANCE", Is => "IS", Join => "JOIN",
    Leading => "LEADING", Left => "LEFT", Like => "LIKE", Max => "MAX", Member => "MEMBER",
    Min => "MIN", New => "NEW", Not => "NOT", Null => "NULL", Nullif => "NULLIF", Of => "OF",
    Or => "OR", Order => "ORDER", Outer => "OUTER", Partial => "PARTIAL", Select => "SELECT",
    Set => "SET", Some => "SOME", Sum => "SUM", Then => "THEN", Trailing => "TRAILING",
    True => "TRUE", Update => "UPDATE", When => "WHEN", Where => "WHERE", With => "WITH",
    Named => "NAMED", On => "ON",
}

/// What a token is, as Doctrine's lexer types it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Int,
    Float,
    Str,
    Param,
    /// A name with no backslash.
    Ident,
    /// A name with a backslash.
    Fqn,
    Kw(Kw),
    /// One of `.,()=<>+-*/!{}`.
    Sym(u8),
    /// Anything else.
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Split `text` into tokens the way Doctrine's `Query\Lexer` does.
fn lex(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let word_end = |mut i: usize| {
        while i < len && is_word_byte(bytes[i]) {
            i += 1;
        }
        i
    };
    // `(?:\\[a-z_][a-z0-9_]*)*`
    let segments_end = |mut i: usize| {
        while i + 1 < len && bytes[i] == b'\\' && is_word_start(bytes[i + 1]) {
            i = word_end(i + 2);
        }
        i
    };
    let digits_end = |mut i: usize| {
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
        i
    };

    let mut tokens = Vec::new();
    let mut i = 0;
    while i < len {
        let c = bytes[i];
        let start = i;
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            i += 1;
            continue;
        }
        if c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            // A comment, to the end of the line.
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let kind = if is_word_start(c) || c == b'\\' {
            let word = word_end(i + 1);
            if is_word_start(c)
                && word + 1 < len
                && bytes[word] == b':'
                && is_word_start(bytes[word + 1])
            {
                // An aliased name, `App:User`.
                i = segments_end(word_end(word + 2));
                Kind::Ident
            } else {
                i = segments_end(word);
                let name = &text[start..i];
                match KEYWORDS.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                    Some((_, kw)) => Kind::Kw(*kw),
                    None if name.contains('\\') => Kind::Fqn,
                    None => Kind::Ident,
                }
            }
        } else if c.is_ascii_digit() {
            i = digits_end(i);
            let mut dots = 0;
            while i + 1 < len && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                i = digits_end(i + 1);
                dots += 1;
            }
            let mut exponent = false;
            if i < len && bytes[i].eq_ignore_ascii_case(&b'e') {
                let mut j = i + 1;
                if j < len && matches!(bytes[j], b'+' | b'-') {
                    j += 1;
                }
                if j < len && bytes[j].is_ascii_digit() {
                    i = digits_end(j);
                    exponent = true;
                }
            }
            match (dots, exponent) {
                (0, false) => Kind::Int,
                (0 | 1, _) => Kind::Float,
                _ => Kind::Other,
            }
        } else if c == b'\'' {
            let mut j = i + 1;
            loop {
                if j >= len {
                    // Unterminated: the quote is a character of its own.
                    i += 1;
                    break Kind::Other;
                }
                if bytes[j] == b'\'' {
                    if bytes.get(j + 1) == Some(&b'\'') {
                        j += 2;
                        continue;
                    }
                    i = j + 1;
                    break Kind::Str;
                }
                j += 1;
            }
        } else if c == b'?' {
            i = digits_end(i + 1);
            Kind::Param
        } else if c == b':' && i + 1 < len && is_word_start(bytes[i + 1]) {
            i = word_end(i + 1);
            Kind::Param
        } else {
            i += 1;
            while i < len && !text.is_char_boundary(i) {
                i += 1;
            }
            match c {
                b'.' | b',' | b'(' | b')' | b'=' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/'
                | b'!' | b'{' | b'}' => Kind::Sym(c),
                _ => Kind::Other,
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: i,
        });
    }
    tokens
}

// ─── Parser ─────────────────────────────────────────────────────────────────

enum Fail {
    /// The token at `at` (the end, when past the last one) is not what the
    /// grammar allows there.
    Syntax {
        at: usize,
        expected: Option<&'static str>,
    },
    /// The text nests deeper than is worth recursing into; no verdict.
    TooDeep,
}

type Parsed = Result<(), Fail>;

/// How deep parentheses, `CASE`s, and subqueries may nest before the
/// check gives up.  Each level recurses, and a query is short, so a
/// string nested this deep is not a query anyone runs.
const MAX_NESTING: u32 = 200;

/// The names of Doctrine's built-in functions, which stop at their closing
/// parenthesis.
const BUILT_IN_FUNCTIONS: &[&str] = &[
    "concat",
    "substring",
    "trim",
    "lower",
    "upper",
    "identity",
    "length",
    "locate",
    "abs",
    "sqrt",
    "mod",
    "size",
    "date_diff",
    "bit_and",
    "bit_or",
    "min",
    "max",
    "avg",
    "sum",
    "count",
    "current_date",
    "current_time",
    "current_timestamp",
    "date_add",
    "date_sub",
];

const COMPARISON: &str = "=, <, <=, <>, >, >= or !=";

struct Parser<'t> {
    text: &'t str,
    tokens: &'t [Token],
    pos: usize,
    nesting: u32,
}

impl Parser<'_> {
    fn kind(&self, i: usize) -> Option<Kind> {
        self.tokens.get(i).map(|t| t.kind)
    }

    fn la(&self) -> Option<Kind> {
        self.kind(self.pos)
    }

    fn glimpse(&self) -> Option<Kind> {
        self.kind(self.pos + 1)
    }

    fn is_kw(&self, i: usize, kw: Kw) -> bool {
        self.kind(i) == Some(Kind::Kw(kw))
    }

    fn is_sym(&self, i: usize, c: u8) -> bool {
        self.kind(i) == Some(Kind::Sym(c))
    }

    fn next_is(&self, kw: Kw) -> bool {
        self.is_kw(self.pos, kw)
    }

    fn next_is_sym(&self, c: u8) -> bool {
        self.is_sym(self.pos, c)
    }

    fn fail(&self, expected: Option<&'static str>) -> Parsed {
        Err(Fail::Syntax {
            at: self.pos,
            expected,
        })
    }

    fn eat(&mut self, kw: Kw) -> bool {
        let found = self.next_is(kw);
        if found {
            self.pos += 1;
        }
        found
    }

    fn eat_sym(&mut self, c: u8) -> bool {
        let found = self.next_is_sym(c);
        if found {
            self.pos += 1;
        }
        found
    }

    fn expect(&mut self, kw: Kw) -> Parsed {
        if self.eat(kw) {
            Ok(())
        } else {
            self.fail(Some(kw.text()))
        }
    }

    fn expect_sym(&mut self, c: u8, name: &'static str) -> Parsed {
        if self.eat_sym(c) {
            Ok(())
        } else {
            self.fail(Some(name))
        }
    }

    /// Match an identifier, which a reserved word also is when Doctrine's
    /// parser asks for one.
    fn expect_name(&mut self, what: &'static str) -> Parsed {
        match self.la() {
            Some(Kind::Ident | Kind::Kw(_)) => {
                self.pos += 1;
                Ok(())
            }
            _ => self.fail(Some(what)),
        }
    }

    fn end(&self) -> Parsed {
        if self.pos < self.tokens.len() {
            self.fail(Some("end of string"))
        } else {
            Ok(())
        }
    }

    /// Run `f` one level deeper.
    fn nested(&mut self, f: impl FnOnce(&mut Self) -> Parsed) -> Parsed {
        if self.nesting >= MAX_NESTING {
            return Err(Fail::TooDeep);
        }
        self.nesting += 1;
        let result = f(self);
        self.nesting -= 1;
        result
    }

    fn is_math(kind: Option<Kind>) -> bool {
        matches!(kind, Some(Kind::Sym(b'+' | b'-' | b'*' | b'/')))
    }

    /// Whether the token at `i` starts a function call.
    fn is_function_at(&self, i: usize) -> bool {
        matches!(self.kind(i), Some(Kind::Ident | Kind::Kw(_))) && self.is_sym(i + 1, b'(')
    }

    /// The index past the parenthesis that closes one opened before `from`,
    /// or `None` when none does.
    fn closing(&self, from: usize) -> Option<usize> {
        let mut unmatched = 1;
        let mut i = from;
        while unmatched > 0 && i < self.tokens.len() {
            match self.kind(i) {
                Some(Kind::Sym(b'(')) => unmatched += 1,
                Some(Kind::Sym(b')')) => unmatched -= 1,
                _ => {}
            }
            i += 1;
        }
        (unmatched == 0).then_some(i)
    }

    /// [`Self::closing`] as Doctrine's `peekBeyondClosingParenthesis()`
    /// finds it: past the end when nothing closes.
    fn beyond_closing(&self, from: usize) -> usize {
        self.closing(from).unwrap_or(self.tokens.len())
    }

    fn entry(&mut self, grammar: Grammar) -> Parsed {
        match grammar {
            Grammar::Statement => self.statement()?,
            Grammar::SelectList => {
                self.eat(Kw::Distinct);
                self.select_expression()?;
                while self.eat_sym(b',') {
                    self.select_expression()?;
                }
            }
            Grammar::Condition => self.conditional_expression()?,
            Grammar::GroupBy => {
                self.path()?;
                while self.eat_sym(b',') {
                    self.path()?;
                }
            }
            Grammar::OrderBy => {
                self.order_by_item()?;
                while self.eat_sym(b',') {
                    self.order_by_item()?;
                }
            }
            Grammar::Path => self.path()?,
            Grammar::NewValue => self.new_value()?,
            Grammar::Operand => {
                if matches!(self.la(), Some(Kind::Kw(Kw::All | Kw::Any | Kw::Some))) {
                    self.quantified()?;
                } else {
                    self.arithmetic_expression()?;
                }
            }
            Grammar::Aggregate => {
                self.eat(Kw::Distinct);
                self.simple_arithmetic()?;
            }
            Grammar::Subselect => self.subselect()?,
            Grammar::InList => self.in_list()?,
        }
        self.end()
    }

    // ── Statements and clauses ──────────────────────────────────────────

    fn statement(&mut self) -> Parsed {
        match self.la() {
            Some(Kind::Kw(Kw::Select)) => {
                self.select_clause()?;
                self.range_declarations()?;
                self.tail_clauses()
            }
            Some(Kind::Kw(Kw::Update)) => {
                self.pos += 1;
                self.abstract_schema_name()?;
                self.eat(Kw::As);
                self.expect_name("an identification variable")?;
                self.expect(Kw::Set)?;
                self.update_item()?;
                while self.eat_sym(b',') {
                    self.update_item()?;
                }
                self.where_clause()
            }
            Some(Kind::Kw(Kw::Delete)) => {
                self.pos += 1;
                self.eat(Kw::From);
                self.abstract_schema_name()?;
                self.eat(Kw::As);
                if self.la() == Some(Kind::Ident) {
                    self.pos += 1;
                }
                self.where_clause()
            }
            _ => self.fail(Some("SELECT, UPDATE or DELETE")),
        }
    }

    /// `[WHERE …] [GROUP BY …] [HAVING …] [ORDER BY …]`
    fn tail_clauses(&mut self) -> Parsed {
        self.where_clause()?;
        if self.eat(Kw::Group) {
            self.expect(Kw::By)?;
            self.path()?;
            while self.eat_sym(b',') {
                self.path()?;
            }
        }
        if self.eat(Kw::Having) {
            self.conditional_expression()?;
        }
        if self.eat(Kw::Order) {
            self.expect(Kw::By)?;
            self.order_by_item()?;
            while self.eat_sym(b',') {
                self.order_by_item()?;
            }
        }
        Ok(())
    }

    fn where_clause(&mut self) -> Parsed {
        if self.eat(Kw::Where) {
            self.conditional_expression()?;
        }
        Ok(())
    }

    fn select_clause(&mut self) -> Parsed {
        self.expect(Kw::Select)?;
        self.eat(Kw::Distinct);
        self.select_expression()?;
        while self.eat_sym(b',') {
            self.select_expression()?;
        }
        Ok(())
    }

    fn range_declarations(&mut self) -> Parsed {
        self.expect(Kw::From)?;
        self.identification_variable_declaration()?;
        while self.eat_sym(b',') {
            self.identification_variable_declaration()?;
        }
        Ok(())
    }

    fn identification_variable_declaration(&mut self) -> Parsed {
        self.range_variable_declaration()?;
        if self.next_is(Kw::Index) {
            self.index_by()?;
        }
        while matches!(self.la(), Some(Kind::Kw(Kw::Left | Kw::Inner | Kw::Join))) {
            self.join()?;
        }
        Ok(())
    }

    fn range_variable_declaration(&mut self) -> Parsed {
        if self.next_is_sym(b'(') && self.is_kw(self.pos + 1, Kw::Select) {
            return self.fail(Some("an entity class (a subquery is not supported here)"));
        }
        self.abstract_schema_name()?;
        self.eat(Kw::As);
        self.expect_name("an identification variable")
    }

    fn abstract_schema_name(&mut self) -> Parsed {
        if self.la() == Some(Kind::Fqn) {
            self.pos += 1;
            return Ok(());
        }
        self.expect_name("an entity class")
    }

    fn join(&mut self) -> Parsed {
        if self.eat(Kw::Left) {
            self.eat(Kw::Outer);
        } else {
            self.eat(Kw::Inner);
        }
        self.expect(Kw::Join)?;
        if self.glimpse() == Some(Kind::Sym(b'.')) {
            self.expect_name("an identification variable")?;
            self.expect_sym(b'.', "'.'")?;
            self.expect_name("an association")?;
            self.eat(Kw::As);
            self.expect_name("an identification variable")?;
            if self.next_is(Kw::Index) {
                self.index_by()?;
            }
            if self.eat(Kw::With) {
                self.conditional_expression()?;
            }
        } else {
            self.range_variable_declaration()?;
            if self.eat(Kw::On) || self.eat(Kw::With) {
                self.conditional_expression()?;
            }
        }
        Ok(())
    }

    fn index_by(&mut self) -> Parsed {
        self.expect(Kw::Index)?;
        self.expect(Kw::By)?;
        self.path()
    }

    /// `IdentificationVariable {"." identifier}*`, which every kind of path
    /// expression parses as.
    fn path(&mut self) -> Parsed {
        self.expect_name("an identification variable")?;
        while self.eat_sym(b'.') {
            self.expect_name("a field")?;
        }
        Ok(())
    }

    fn order_by_item(&mut self) -> Parsed {
        let p = self.pos;
        let glimpse = self.kind(p + 1);
        if Self::is_math(self.kind(p + 3)) || Self::is_math(glimpse) {
            self.simple_arithmetic()?;
        } else if glimpse == Some(Kind::Sym(b'.')) {
            self.path()?;
        } else if p + 1 < self.tokens.len() && Self::is_math(self.kind(self.beyond_closing(p + 2)))
        {
            self.scalar_expression()?;
        } else if self.next_is(Kw::Case) {
            self.case_expression()?;
        } else if self.is_function_at(p) {
            self.function()?;
        } else {
            self.expect_name("a result variable")?;
        }
        if !self.eat(Kw::Desc) {
            self.eat(Kw::Asc);
        }
        Ok(())
    }

    fn update_item(&mut self) -> Parsed {
        self.path()?;
        self.expect_sym(b'=', "'='")?;
        self.new_value()
    }

    fn new_value(&mut self) -> Parsed {
        if self.eat(Kw::Null) {
            return Ok(());
        }
        if self.la() == Some(Kind::Param) {
            self.pos += 1;
            return Ok(());
        }
        self.arithmetic_expression()
    }

    fn subselect(&mut self) -> Parsed {
        self.nested(|p| {
            p.expect(Kw::Select)?;
            p.eat(Kw::Distinct);
            p.simple_select_expression()?;
            p.range_declarations()?;
            p.tail_clauses()
        })
    }

    // ── Select expressions ──────────────────────────────────────────────

    fn select_expression(&mut self) -> Parsed {
        let p = self.pos;
        let glimpse = self.glimpse();
        match self.la() {
            Some(Kind::Ident) if glimpse == Some(Kind::Sym(b'.')) => self.scalar_expression()?,
            Some(Kind::Ident) if glimpse != Some(Kind::Sym(b'(')) => self.pos += 1,
            Some(Kind::Kw(Kw::Null)) => self.scalar_expression()?,
            Some(Kind::Kw(Kw::Case | Kw::Coalesce | Kw::Nullif)) => self.case_expression()?,
            _ if self.is_function_at(p) => {
                if Self::is_math(self.kind(self.beyond_closing(p + 2))) {
                    self.scalar_expression()?;
                } else {
                    self.function()?;
                }
            }
            Some(Kind::Kw(Kw::Partial)) => self.partial_object()?,
            Some(Kind::Sym(b'(')) if glimpse == Some(Kind::Kw(Kw::Select)) => {
                self.pos += 1;
                self.subselect()?;
                self.expect_sym(b')', "')'")?;
            }
            Some(Kind::Sym(b'(' | b'-' | b'+') | Kind::Int | Kind::Str | Kind::Float) => {
                self.simple_arithmetic()?
            }
            Some(Kind::Kw(Kw::New)) => self.new_object()?,
            _ => return self.fail(Some("a select expression")),
        }
        let must_alias = self.eat(Kw::As);
        self.eat(Kw::Hidden);
        if must_alias {
            self.expect_name("a result variable")?;
        } else if self.la() == Some(Kind::Ident) {
            self.pos += 1;
        }
        Ok(())
    }

    fn simple_select_expression(&mut self) -> Parsed {
        let p = self.pos;
        let glimpse = self.glimpse();
        match self.la() {
            Some(Kind::Ident) if glimpse == Some(Kind::Sym(b'.')) => return self.path(),
            Some(Kind::Ident) if glimpse != Some(Kind::Sym(b'(')) => {
                self.pos += 1;
                return Ok(());
            }
            Some(Kind::Ident) => {
                return if Self::is_math(self.kind(self.beyond_closing(p + 2))) {
                    self.scalar_expression()
                } else {
                    self.function()
                };
            }
            Some(Kind::Sym(b'(')) if glimpse != Some(Kind::Kw(Kw::Select)) => {
                return self.simple_arithmetic();
            }
            Some(Kind::Sym(b'(')) => {
                self.pos += 1;
                self.subselect()?;
                return self.expect_sym(b')', "')'");
            }
            _ => {}
        }
        self.scalar_expression()?;
        self.eat(Kw::As);
        if self.la() == Some(Kind::Ident) {
            self.pos += 1;
        }
        Ok(())
    }

    fn scalar_expression(&mut self) -> Parsed {
        let p = self.pos;
        match self.la() {
            Some(Kind::Int | Kind::Float | Kind::Sym(b'-' | b'+' | b'(')) => {
                self.simple_arithmetic()
            }
            Some(Kind::Str) => self.string_primary(),
            Some(Kind::Kw(Kw::True | Kw::False | Kw::Null)) => {
                self.pos += 1;
                Ok(())
            }
            Some(Kind::Param) => {
                if Self::is_math(self.glimpse()) {
                    self.simple_arithmetic()
                } else {
                    self.pos += 1;
                    Ok(())
                }
            }
            Some(Kind::Kw(Kw::Case | Kw::Coalesce | Kw::Nullif)) => self.case_expression(),
            _ if self.is_function_at(p) => {
                if Self::is_math(self.kind(self.beyond_closing(p + 2))) {
                    self.simple_arithmetic()
                } else {
                    self.function()
                }
            }
            Some(Kind::Ident) => {
                if Self::is_math(self.kind(p + 3)) {
                    self.simple_arithmetic()
                } else {
                    self.path()
                }
            }
            _ => self.fail(None),
        }
    }

    fn case_expression(&mut self) -> Parsed {
        self.nested(|p| match p.la() {
            Some(Kind::Kw(Kw::Nullif)) => {
                p.pos += 1;
                p.expect_sym(b'(', "'('")?;
                p.scalar_expression()?;
                p.expect_sym(b',', "','")?;
                p.scalar_expression()?;
                p.expect_sym(b')', "')'")
            }
            Some(Kind::Kw(Kw::Coalesce)) => {
                p.pos += 1;
                p.expect_sym(b'(', "'('")?;
                p.scalar_expression()?;
                while p.eat_sym(b',') {
                    p.scalar_expression()?;
                }
                p.expect_sym(b')', "')'")
            }
            Some(Kind::Kw(Kw::Case)) => {
                p.pos += 1;
                let general = p.next_is(Kw::When);
                if !general {
                    p.path()?;
                }
                loop {
                    p.expect(Kw::When)?;
                    if general {
                        p.conditional_expression()?;
                    } else {
                        p.scalar_expression()?;
                    }
                    p.expect(Kw::Then)?;
                    p.scalar_expression()?;
                    if !p.next_is(Kw::When) {
                        break;
                    }
                }
                p.expect(Kw::Else)?;
                p.scalar_expression()?;
                p.expect(Kw::End)
            }
            _ => p.fail(None),
        })
    }

    fn partial_object(&mut self) -> Parsed {
        self.expect(Kw::Partial)?;
        self.expect_name("an identification variable")?;
        self.expect_sym(b'.', "'.'")?;
        self.expect_sym(b'{', "'{'")?;
        loop {
            self.expect_name("a field")?;
            while self.eat_sym(b'.') {
                self.expect_name("a field")?;
            }
            if !self.eat_sym(b',') {
                break;
            }
        }
        self.expect_sym(b'}', "'}'")
    }

    fn new_object(&mut self) -> Parsed {
        self.nested(|p| {
            p.expect(Kw::New)?;
            p.eat(Kw::Named);
            p.abstract_schema_name()?;
            p.expect_sym(b'(', "'('")?;
            p.new_object_arg()?;
            while p.eat_sym(b',') {
                p.new_object_arg()?;
            }
            p.expect_sym(b')', "')'")
        })
    }

    fn new_object_arg(&mut self) -> Parsed {
        let glimpse = self.glimpse();
        match self.la() {
            Some(Kind::Sym(b'(')) if glimpse == Some(Kind::Kw(Kw::Select)) => {
                self.pos += 1;
                self.subselect()?;
                self.expect_sym(b')', "')'")?;
            }
            Some(Kind::Kw(Kw::New)) => self.new_object()?,
            Some(Kind::Ident) if !matches!(glimpse, Some(Kind::Sym(b'.' | b'('))) => {
                self.pos += 1;
            }
            _ => self.scalar_expression()?,
        }
        if self.eat(Kw::As) {
            self.expect_name("a field")?;
        }
        Ok(())
    }

    // ── Conditions ──────────────────────────────────────────────────────

    fn conditional_expression(&mut self) -> Parsed {
        self.nested(|p| {
            p.conditional_term()?;
            while p.eat(Kw::Or) {
                p.conditional_term()?;
            }
            Ok(())
        })
    }

    fn conditional_term(&mut self) -> Parsed {
        self.conditional_factor()?;
        while self.eat(Kw::And) {
            self.conditional_factor()?;
        }
        Ok(())
    }

    fn conditional_factor(&mut self) -> Parsed {
        self.eat(Kw::Not);
        if !self.next_is_sym(b'(') {
            return self.simple_conditional();
        }
        let beyond = self.kind(self.beyond_closing(self.pos + 1));
        let simple = matches!(beyond, Some(Kind::Sym(b'=' | b'<' | b'>')))
            || matches!(
                beyond,
                Some(Kind::Kw(
                    Kw::Not | Kw::Between | Kw::Like | Kw::In | Kw::Is | Kw::Exists
                ))
            )
            || Self::is_math(beyond);
        if simple {
            return self.simple_conditional();
        }
        self.pos += 1;
        self.conditional_expression()?;
        self.expect_sym(b')', "')'")
    }

    /// Doctrine's `SimpleConditionalExpression()`, which picks the kind of
    /// condition by peeking past its left-hand side.
    fn simple_conditional(&mut self) -> Parsed {
        if self.next_is(Kw::Exists) {
            return self.exists();
        }
        let p = self.pos;
        let mut token = p;
        let mut lookahead = p;
        if self.is_kw(p, Kw::Not) {
            token = p + 1;
        }
        if matches!(self.kind(token), Some(Kind::Ident | Kind::Param)) || self.is_function_at(p) {
            // The index of the token peeked last.
            let mut peeked = p + 1;
            if self.is_sym(p + 1, b'(') {
                peeked = self.beyond_closing(p + 2);
                token = peeked;
                if self.is_kw(peeked, Kw::Not) {
                    peeked += 1;
                    token = peeked;
                }
                if self.is_kw(token, Kw::Is) {
                    peeked += 1;
                    lookahead = peeked;
                }
            } else {
                token = peeked;
                while self.is_sym(token, b'.') {
                    peeked += 2;
                    token = peeked;
                }
                if self.is_kw(token, Kw::Not) {
                    peeked += 1;
                    token = peeked;
                }
                peeked += 1;
                lookahead = peeked;
            }
            if self.is_kw(lookahead, Kw::Not) {
                peeked += 1;
                lookahead = peeked;
            }
        }
        match self.kind(token) {
            Some(Kind::Kw(Kw::Between)) => self.between(),
            Some(Kind::Kw(Kw::Like)) => self.like(),
            Some(Kind::Kw(Kw::In)) => self.in_expression(),
            Some(Kind::Kw(Kw::Instance)) => self.instance_of(),
            Some(Kind::Kw(Kw::Member)) => self.collection_member(),
            Some(Kind::Kw(Kw::Is)) if self.is_kw(lookahead, Kw::Null) => self.null_comparison(),
            Some(Kind::Kw(Kw::Is)) if self.is_kw(lookahead, Kw::Empty) => {
                self.path()?;
                self.expect(Kw::Is)?;
                self.eat(Kw::Not);
                self.expect(Kw::Empty)
            }
            _ => self.comparison(),
        }
    }

    fn exists(&mut self) -> Parsed {
        self.eat(Kw::Not);
        self.expect(Kw::Exists)?;
        self.expect_sym(b'(', "'('")?;
        self.subselect()?;
        self.expect_sym(b')', "')'")
    }

    fn between(&mut self) -> Parsed {
        self.arithmetic_expression()?;
        self.eat(Kw::Not);
        self.expect(Kw::Between)?;
        self.arithmetic_expression()?;
        self.expect(Kw::And)?;
        self.arithmetic_expression()
    }

    fn comparison(&mut self) -> Parsed {
        self.arithmetic_expression()?;
        match self.la() {
            Some(Kind::Sym(b'=')) => self.pos += 1,
            Some(Kind::Sym(b'<')) => {
                self.pos += 1;
                if !self.eat_sym(b'=') {
                    self.eat_sym(b'>');
                }
            }
            Some(Kind::Sym(b'>')) => {
                self.pos += 1;
                self.eat_sym(b'=');
            }
            Some(Kind::Sym(b'!')) => {
                self.pos += 1;
                self.expect_sym(b'=', "'='")?;
            }
            _ => return self.fail(Some(COMPARISON)),
        }
        if matches!(self.la(), Some(Kind::Kw(Kw::All | Kw::Any | Kw::Some))) {
            self.quantified()
        } else {
            self.arithmetic_expression()
        }
    }

    fn quantified(&mut self) -> Parsed {
        if !matches!(self.la(), Some(Kind::Kw(Kw::All | Kw::Any | Kw::Some))) {
            return self.fail(Some("ALL, ANY or SOME"));
        }
        self.pos += 1;
        self.expect_sym(b'(', "'('")?;
        self.subselect()?;
        self.expect_sym(b')', "')'")
    }

    fn in_expression(&mut self) -> Parsed {
        self.arithmetic_expression()?;
        self.eat(Kw::Not);
        self.expect(Kw::In)?;
        self.expect_sym(b'(', "'('")?;
        self.in_list()?;
        self.expect_sym(b')', "')'")
    }

    fn in_list(&mut self) -> Parsed {
        if self.next_is(Kw::Select) {
            return self.subselect();
        }
        self.in_parameter()?;
        while self.eat_sym(b',') {
            self.in_parameter()?;
        }
        Ok(())
    }

    fn in_parameter(&mut self) -> Parsed {
        if self.la() == Some(Kind::Param) {
            self.pos += 1;
            return Ok(());
        }
        self.arithmetic_expression()
    }

    fn instance_of(&mut self) -> Parsed {
        self.expect_name("an identification variable")?;
        self.eat(Kw::Not);
        self.expect(Kw::Instance)?;
        self.expect(Kw::Of)?;
        if !self.eat_sym(b'(') {
            return self.instance_of_parameter();
        }
        self.instance_of_parameter()?;
        while self.eat_sym(b',') {
            self.instance_of_parameter()?;
        }
        self.expect_sym(b')', "')'")
    }

    fn instance_of_parameter(&mut self) -> Parsed {
        if self.la() == Some(Kind::Param) {
            self.pos += 1;
            return Ok(());
        }
        self.abstract_schema_name()
    }

    fn like(&mut self) -> Parsed {
        // StringExpression
        let glimpse = self.glimpse();
        match self.la() {
            Some(Kind::Sym(b'(')) if glimpse == Some(Kind::Kw(Kw::Select)) => {
                self.pos += 1;
                self.subselect()?;
                self.expect_sym(b')', "')'")?;
            }
            // A result variable, which only the declarations can tell.
            Some(Kind::Ident) if !matches!(glimpse, Some(Kind::Sym(b'.' | b'('))) => {
                self.pos += 1;
            }
            _ => self.string_primary()?,
        }
        self.eat(Kw::Not);
        self.expect(Kw::Like)?;
        if self.la() == Some(Kind::Param) {
            self.pos += 1;
        } else {
            self.string_primary()?;
        }
        if self.eat(Kw::Escape) {
            if self.la() != Some(Kind::Str) {
                return self.fail(Some("a string"));
            }
            self.pos += 1;
        }
        Ok(())
    }

    fn string_primary(&mut self) -> Parsed {
        let glimpse = self.glimpse();
        match self.la() {
            Some(Kind::Ident) => match glimpse {
                Some(Kind::Sym(b'.')) => self.path(),
                Some(Kind::Sym(b'(')) => self.function(),
                _ => self.fail(Some("'.' or '('")),
            },
            Some(Kind::Str | Kind::Param) => {
                self.pos += 1;
                Ok(())
            }
            Some(Kind::Kw(Kw::Case | Kw::Coalesce | Kw::Nullif)) => self.case_expression(),
            Some(Kind::Kw(Kw::Avg | Kw::Max | Kw::Min | Kw::Sum | Kw::Count)) => self.aggregate(),
            _ => self.fail(Some("a field, string, parameter or function")),
        }
    }

    fn null_comparison(&mut self) -> Parsed {
        match self.la() {
            Some(Kind::Param) => self.pos += 1,
            Some(Kind::Kw(Kw::Nullif | Kw::Coalesce)) => self.case_expression()?,
            _ if self.is_function_at(self.pos) => self.function()?,
            _ => self.path()?,
        }
        self.expect(Kw::Is)?;
        self.eat(Kw::Not);
        self.expect(Kw::Null)
    }

    fn collection_member(&mut self) -> Parsed {
        if self.la() == Some(Kind::Param) {
            self.pos += 1;
        } else {
            self.path()?;
        }
        self.eat(Kw::Not);
        self.expect(Kw::Member)?;
        self.eat(Kw::Of);
        self.path()
    }

    // ── Arithmetic ──────────────────────────────────────────────────────

    fn arithmetic_expression(&mut self) -> Parsed {
        if self.next_is_sym(b'(') && self.is_kw(self.pos + 1, Kw::Select) {
            self.pos += 1;
            self.subselect()?;
            return self.expect_sym(b')', "')'");
        }
        self.simple_arithmetic()
    }

    fn simple_arithmetic(&mut self) -> Parsed {
        self.nested(|p| {
            p.arithmetic_term()?;
            while matches!(p.la(), Some(Kind::Sym(b'+' | b'-'))) {
                p.pos += 1;
                p.arithmetic_term()?;
            }
            Ok(())
        })
    }

    fn arithmetic_term(&mut self) -> Parsed {
        self.arithmetic_factor()?;
        while matches!(self.la(), Some(Kind::Sym(b'*' | b'/'))) {
            self.pos += 1;
            self.arithmetic_factor()?;
        }
        Ok(())
    }

    fn arithmetic_factor(&mut self) -> Parsed {
        if matches!(self.la(), Some(Kind::Sym(b'+' | b'-'))) {
            self.pos += 1;
        }
        self.arithmetic_primary()
    }

    fn arithmetic_primary(&mut self) -> Parsed {
        if self.eat_sym(b'(') {
            self.simple_arithmetic()?;
            return self.expect_sym(b')', "')'");
        }
        let glimpse = self.glimpse();
        match self.la() {
            None => self.fail(Some("an expression")),
            Some(Kind::Kw(Kw::Case | Kw::Coalesce | Kw::Nullif)) => self.case_expression(),
            Some(Kind::Ident) if glimpse == Some(Kind::Sym(b'(')) => self.function(),
            Some(Kind::Ident) => self.path(),
            Some(Kind::Param) => {
                self.pos += 1;
                Ok(())
            }
            _ if glimpse == Some(Kind::Sym(b'(')) => self.function(),
            Some(Kind::Str | Kind::Int | Kind::Float | Kind::Kw(Kw::True | Kw::False)) => {
                self.pos += 1;
                Ok(())
            }
            _ => self.fail(Some("a literal")),
        }
    }

    fn aggregate(&mut self) -> Parsed {
        self.pos += 1;
        self.expect_sym(b'(', "'('")?;
        self.eat(Kw::Distinct);
        self.simple_arithmetic()?;
        self.expect_sym(b')', "')'")
    }

    /// A function call, whose arguments are only checked to close.
    fn function(&mut self) -> Parsed {
        let name = self.pos;
        let built_in = self.tokens.get(name).is_some_and(|token| {
            let word = &self.text[token.start..token.end];
            BUILT_IN_FUNCTIONS
                .iter()
                .any(|f| f.eq_ignore_ascii_case(word))
        });
        self.call()?;
        if !built_in {
            while self.la() == Some(Kind::Ident) && self.is_sym(self.pos + 1, b'(') {
                self.call()?;
            }
        }
        Ok(())
    }

    /// A name and its parenthesized arguments.
    fn call(&mut self) -> Parsed {
        self.expect_name("a function")?;
        self.expect_sym(b'(', "'('")?;
        match self.closing(self.pos) {
            Some(close) => {
                self.pos = close;
                Ok(())
            }
            None => {
                self.pos = self.tokens.len();
                self.fail(Some("')'"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(text: &str, grammar: Grammar) -> Option<String> {
        check(text, grammar).map(|e| {
            let at = match e.range {
                Some(range) => text[range].to_string(),
                None => "<end>".to_string(),
            };
            format!("{at}: {}", e.message)
        })
    }

    #[track_caller]
    fn accepts(text: &str, grammar: Grammar) {
        assert_eq!(error(text, grammar), None, "{text}");
    }

    #[track_caller]
    fn rejects(text: &str, grammar: Grammar, expected: &str) {
        assert_eq!(error(text, grammar).as_deref(), Some(expected), "{text}");
    }

    #[test]
    fn queries_doctrine_accepts() {
        for query in [
            "SELECT u FROM App\\Entity\\User u",
            "SELECT u, a FROM App\\Entity\\User u LEFT JOIN u.address AS a WITH a.city = 'x' WHERE u.email = :e ORDER BY u.name DESC",
            "SELECT DISTINCT u.id, COUNT(p.id) AS HIDDEN posts FROM User u INDEX BY u.id JOIN u.posts p GROUP BY u.id HAVING COUNT(p.id) > 1",
            "SELECT u FROM User u, Group g WHERE u MEMBER OF g.users AND u INSTANCE OF (Admin, Editor)",
            "SELECT u FROM User u WHERE u.id IN (SELECT u2.id FROM User u2 WHERE u2.name LIKE :n ESCAPE '!')",
            "SELECT u FROM User u WHERE NOT EXISTS (SELECT p FROM Post p WHERE p.author = u) AND u.deletedAt IS NULL",
            "SELECT u FROM User u WHERE u.age BETWEEN 18 AND 65 AND u.posts IS NOT EMPTY AND u.status NOT IN ('a', 'b')",
            "SELECT u FROM User u WHERE (u.a = 1 OR u.b = 2) AND (u.c + 1) * 2 >= u.d AND u.x <> ALL (SELECT p.id FROM Post p)",
            "SELECT PARTIAL u.{id, name}, NEW App\\Dto(u.id, u.name AS label) FROM User u",
            "SELECT CASE WHEN u.a = 1 THEN 'x' ELSE 'y' END AS kind, COALESCE(u.n, 0), NULLIF(u.a, u.b) FROM User u",
            "SELECT u FROM User u WHERE LOWER(u.name) LIKE LOWER(:q) AND SIZE(u.posts) > 0 AND u.created < CURRENT_TIMESTAMP()",
            "SELECT u FROM User u WHERE MATCH(u.name, u.bio) AGAINST(:q BOOLEAN) > 0",
            "SELECT u FROM User u WHERE DATE_FORMAT(u.created, '%Y') = :year ORDER BY RAND()",
            "SELECT u FROM User u JOIN App\\Post p ON p.author = u WHERE u.flag = true AND u.id != ?1",
            "SELECT u FROM User u -- a comment\n WHERE u.id = 1",
            "UPDATE User u SET u.name = :n, u.age = u.age + 1, u.deletedAt = NULL WHERE u.id = 1",
            "DELETE FROM User u WHERE u.id = 1",
            "DELETE User u",
            "SELECT COUNT(u) total FROM User u GROUP BY u.team ORDER BY total DESC",
            "SELECT u FROM User u WHERE :tag MEMBER OF u.tags AND u.order = 1",
        ] {
            accepts(query, Grammar::Statement);
        }
    }

    #[test]
    fn queries_doctrine_rejects() {
        rejects(
            "SELECT e FROM App\\Entity\\User",
            Grammar::Statement,
            "<end>: DQL expects an identification variable, got end of string",
        );
        rejects(
            "SELECT e FROM User e WHERE e.id = 1)",
            Grammar::Statement,
            "): DQL expects end of string, got ')'",
        );
        rejects(
            "SELECT e FROM User e WHERE foo",
            Grammar::Statement,
            "<end>: DQL expects =, <, <=, <>, >, >= or !=, got end of string",
        );
        rejects(
            "SELECT e FROM User e WHERE e.id = NULL",
            Grammar::Statement,
            "NULL: DQL expects a literal, got 'NULL'",
        );
        rejects(
            "SELECT e FROM User e ORDER BY e.id WHERE e.id = 1",
            Grammar::Statement,
            "WHERE: DQL expects end of string, got 'WHERE'",
        );
        rejects(
            "SELEC e FROM User e",
            Grammar::Statement,
            "SELEC: DQL expects SELECT, UPDATE or DELETE, got 'SELEC'",
        );
        rejects(
            "SELECT e FROM User e WHERE e.name = 'open",
            Grammar::Statement,
            "': DQL expects a literal, got '''",
        );
        rejects(
            "SELECT LOWER(e.name FROM User e",
            Grammar::Statement,
            "<end>: DQL expects ')', got end of string",
        );
        rejects(
            "SELECT e FROM User e WHERE e.id IN :ids",
            Grammar::Statement,
            ":ids: DQL expects '(', got ':ids'",
        );
    }

    #[test]
    fn fragments() {
        accepts("e.id = 1", Grammar::Condition);
        accepts("e.id IN (:ids) OR e.parent IS NULL", Grammar::Condition);
        rejects(
            "e.id = 1)",
            Grammar::Condition,
            "): DQL expects end of string, got ')'",
        );
        rejects(
            "e.id = 1 OR e.nickname) IS NULL",
            Grammar::Condition,
            "): DQL expects =, <, <=, <>, >, >= or !=, got ')'",
        );
        accepts("e", Grammar::SelectList);
        accepts("e.id, e.title AS t", Grammar::SelectList);
        accepts("DISTINCT e", Grammar::SelectList);
        accepts("COUNT(e.id) AS HIDDEN n", Grammar::SelectList);
        accepts("e.name", Grammar::OrderBy);
        accepts("e.name DESC, total", Grammar::OrderBy);
        accepts("e.id, t", Grammar::GroupBy);
        accepts("e.id", Grammar::Path);
        accepts(":name", Grammar::NewValue);
        accepts("e.nickname", Grammar::Operand);
        accepts("'%x%'", Grammar::Operand);
        rejects(
            "e.nickname)",
            Grammar::Operand,
            "): DQL expects end of string, got ')'",
        );
        accepts("DISTINCT e.id", Grammar::Aggregate);
        accepts("SELECT p.id FROM Post p", Grammar::Subselect);
        accepts("1, 2, :three", Grammar::InList);
        rejects(
            "",
            Grammar::Condition,
            "<end>: DQL expects an expression, got end of string",
        );
    }

    #[test]
    fn deep_nesting_gives_no_verdict() {
        let text = format!("{}1{}", "(".repeat(5000), ")".repeat(4999));
        assert_eq!(check(&text, Grammar::Operand), None);
    }
}
