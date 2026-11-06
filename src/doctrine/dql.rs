//! A reader for the names Doctrine Query Language text holds.
//!
//! DQL names entity classes by their fully-qualified name (`FROM
//! App\Entity\User u`) and their fields through the identification variable
//! a class or a join declares (`u.email`).  This reads both out of a query,
//! or out of a fragment a query builder joins into one, without checking
//! the rest of the grammar: a query that does not parse names what it
//! names all the same.
//!
//! The text may hold bytes no source text spells, where a PHP expression
//! that is not a constant was concatenated or interpolated into it.  Those
//! read as [`DYNAMIC`], and any name they touch is left alone: `'u.' .
//! $field` names no field the reader can tell.

use std::ops::Range;

/// The byte a dynamic part of the text reads as.
pub(crate) const DYNAMIC: char = '\u{1}';

/// What an identification variable stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeclRoot {
    /// The entity class the range of text names (`FROM App\Entity\User u`).
    Entity(Range<usize>),
    /// The target of the association `field` of the entity another variable
    /// stands for (`JOIN u.address a`).
    Association,
    /// Something the text does not spell out.
    Unknown,
}

/// An identification variable a query declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DqlDecl {
    pub(crate) alias: Range<usize>,
    pub(crate) root: DeclRoot,
}

/// A name a query holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DqlRef {
    /// A class name: an entity a declaration does not cover (an arbitrary
    /// join's is covered by its [`DqlDecl`]), the class `NEW` constructs,
    /// or the one `INSTANCE OF` tests.
    Class(Range<usize>),
    /// The field `field` of the entity `alias` stands for.
    Field {
        alias: Range<usize>,
        field: Range<usize>,
    },
}

/// What a query or a fragment of one declares and names.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct DqlScan {
    pub(crate) decls: Vec<DqlDecl>,
    pub(crate) refs: Vec<DqlRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tok {
    Ident,
    Dynamic,
    Dot,
    Comma,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Other,
}

#[derive(Debug, Clone)]
pub(super) struct Token {
    pub(super) kind: Tok,
    pub(super) start: usize,
    pub(super) end: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '\\' || c == DYNAMIC
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '\\' || c == DYNAMIC
}

pub(super) fn tokenize(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let kind = if is_ident_start(c) {
            let mut dynamic = false;
            while i < bytes.len() && is_ident_char(bytes[i] as char) {
                dynamic |= bytes[i] as char == DYNAMIC;
                i += 1;
            }
            if dynamic { Tok::Dynamic } else { Tok::Ident }
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            Tok::Other
        } else if c == '\'' {
            // A DQL string literal, with `''` for a quote inside it.
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    if bytes.get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            Tok::Other
        } else if c == ':' || c == '?' {
            // A parameter, `:name` or `?1`.
            i += 1;
            while i < bytes.len() && is_ident_char(bytes[i] as char) {
                i += 1;
            }
            Tok::Other
        } else {
            i += c.len_utf8().max(1);
            // Step over the rest of a multi-byte character.
            while i < bytes.len() && !text.is_char_boundary(i) {
                i += 1;
            }
            match c {
                '.' => Tok::Dot,
                ',' => Tok::Comma,
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                '{' => Tok::LBrace,
                '}' => Tok::RBrace,
                _ => Tok::Other,
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

/// The words DQL reserves, none of which can be an identification
/// variable.
const KEYWORDS: &[&str] = &[
    "ALL", "AND", "ANY", "AS", "ASC", "AVG", "BETWEEN", "BOTH", "BY", "CASE", "COALESCE", "COUNT",
    "DELETE", "DESC", "DISTINCT", "ELSE", "EMPTY", "END", "ESCAPE", "EXISTS", "FALSE", "FROM",
    "GROUP", "HAVING", "HIDDEN", "IN", "INDEX", "INNER", "INSTANCE", "IS", "JOIN", "LEADING",
    "LEFT", "LIKE", "MAX", "MEMBER", "MIN", "NEW", "NOT", "NULL", "NULLIF", "OF", "OR", "ORDER",
    "OUTER", "PARTIAL", "SELECT", "SET", "SOME", "SUM", "THEN", "TRAILING", "TRUE", "UPDATE",
    "WHEN", "WHERE", "WITH",
];

pub(super) fn is_keyword(word: &str) -> bool {
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word))
}

struct Scanner<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    out: DqlScan,
}

impl Scanner<'_> {
    fn word(&self, i: usize) -> Option<&str> {
        let token = self.tokens.get(i)?;
        (token.kind == Tok::Ident).then(|| &self.text[token.start..token.end])
    }

    fn is_word(&self, i: usize, keyword: &str) -> bool {
        self.word(i)
            .is_some_and(|w| w.eq_ignore_ascii_case(keyword))
    }

    fn kind(&self, i: usize) -> Option<Tok> {
        self.tokens.get(i).map(|t| t.kind)
    }

    fn range(&self, i: usize) -> Range<usize> {
        self.tokens[i].start..self.tokens[i].end
    }

    /// Whether the tokens `i` and `i + 1` touch, as the parts of a path do.
    fn adjacent(&self, i: usize) -> bool {
        match (self.tokens.get(i), self.tokens.get(i + 1)) {
            (Some(a), Some(b)) => a.end == b.start,
            _ => false,
        }
    }

    /// Whether `i` starts a path, `alias.field`.
    fn is_path(&self, i: usize) -> bool {
        matches!(self.kind(i), Some(Tok::Ident | Tok::Dynamic))
            && self.kind(i + 1) == Some(Tok::Dot)
            && self.adjacent(i)
            && matches!(
                self.kind(i + 2),
                Some(Tok::Ident | Tok::Dynamic | Tok::LBrace)
            )
            && self.adjacent(i + 1)
    }

    /// Whether `i` is a name that can declare or be a variable.
    fn is_variable(&self, i: usize) -> bool {
        match self.kind(i) {
            Some(Tok::Ident) => self
                .word(i)
                .is_some_and(|w| !is_keyword(w) && !w.contains('\\')),
            Some(Tok::Dynamic) => true,
            _ => false,
        }
    }

    /// Read `[AS] alias` at `i`, declaring it as `root`.  Returns where the
    /// reading stopped.
    fn declare(&mut self, mut i: usize, root: DeclRoot) -> usize {
        if self.is_word(i, "AS") {
            i += 1;
        }
        if !self.is_variable(i) {
            return i;
        }
        if self.kind(i) == Some(Tok::Ident) {
            let alias = self.range(i);
            self.out.decls.push(DqlDecl { alias, root });
        }
        i + 1
    }

    /// Read an entity name and the variable it declares at `i`.
    fn entity_decl(&mut self, i: usize) -> usize {
        match self.kind(i) {
            Some(Tok::Ident) if !self.word(i).is_some_and(is_keyword) && !self.is_path(i) => {
                let name = self.range(i);
                if !self.is_variable(i + 1) && !self.is_word(i + 1, "AS") {
                    // No variable follows; the name is still a class.
                    self.out.refs.push(DqlRef::Class(name));
                    return i + 1;
                }
                self.declare(i + 1, DeclRoot::Entity(name))
            }
            Some(Tok::Dynamic) if !self.is_path(i) => self.declare(i + 1, DeclRoot::Unknown),
            _ => i,
        }
    }

    /// Read what `JOIN` joins at `i`: an association path or an entity.
    fn join_decl(&mut self, i: usize) -> usize {
        if self.is_path(i) {
            let end = self.path(i);
            return self.declare(end, DeclRoot::Association);
        }
        self.entity_decl(i)
    }

    /// Read the path at `i`, recording its first field.  Returns the index
    /// past it.
    fn path(&mut self, i: usize) -> usize {
        let mut end = i + 3;
        if self.kind(i) == Some(Tok::Ident) && self.kind(i + 2) == Some(Tok::Ident) {
            self.out.refs.push(DqlRef::Field {
                alias: self.range(i),
                field: self.range(i + 2),
            });
        }
        // An embedded object's field, `u.address.city`, belongs to the
        // embeddable rather than the entity.
        while self.kind(end) == Some(Tok::Dot)
            && self.adjacent(end - 1)
            && matches!(self.kind(end + 1), Some(Tok::Ident | Tok::Dynamic))
            && self.adjacent(end)
        {
            end += 2;
        }
        end
    }

    /// Read `alias.{field, …}` at `i`, after `PARTIAL`.
    fn partial(&mut self, i: usize) -> usize {
        if !(self.kind(i) == Some(Tok::Ident)
            && self.kind(i + 1) == Some(Tok::Dot)
            && self.kind(i + 2) == Some(Tok::LBrace))
        {
            return i;
        }
        let alias = self.range(i);
        let mut j = i + 3;
        while let Some(kind) = self.kind(j) {
            match kind {
                Tok::RBrace => return j + 1,
                Tok::Ident => self.out.refs.push(DqlRef::Field {
                    alias: alias.clone(),
                    field: self.range(j),
                }),
                Tok::Comma | Tok::Dynamic => {}
                _ => return j,
            }
            j += 1;
        }
        j
    }

    /// Read the class names `INSTANCE OF` tests at `i`.
    fn instance_of(&mut self, i: usize) -> usize {
        let class = |s: &Self, j: usize| {
            s.kind(j) == Some(Tok::Ident) && !s.word(j).is_some_and(is_keyword) && !s.is_path(j)
        };
        if class(self, i) {
            self.out.refs.push(DqlRef::Class(self.range(i)));
            return i + 1;
        }
        if self.kind(i) != Some(Tok::LParen) {
            return i;
        }
        let mut j = i + 1;
        while let Some(kind) = self.kind(j) {
            match kind {
                Tok::RParen => return j + 1,
                Tok::Ident if class(self, j) => self.out.refs.push(DqlRef::Class(self.range(j))),
                Tok::Comma | Tok::Dynamic | Tok::Other => {}
                _ => return j,
            }
            j += 1;
        }
        j
    }

    fn run(&mut self) {
        // Whether each level of parentheses is in a `FROM` clause, where a
        // comma starts another range; a subquery's clause ends with it.
        let mut in_from = vec![false];
        let mut i = 0;
        while i < self.tokens.len() {
            match self.tokens[i].kind {
                Tok::Comma if in_from.last() == Some(&true) => {
                    i = self.entity_decl(i + 1).max(i + 1);
                    continue;
                }
                Tok::LParen => in_from.push(false),
                Tok::RParen if in_from.len() > 1 => {
                    in_from.pop();
                }
                _ => {}
            }
            if self.is_path(i) {
                i = self.path(i);
                continue;
            }
            let Some(word) = self.word(i) else {
                i += 1;
                continue;
            };
            let next = i + 1;
            i = match word.to_ascii_uppercase().as_str() {
                "FROM" => {
                    if let Some(f) = in_from.last_mut() {
                        *f = true;
                    }
                    self.entity_decl(next)
                }
                "UPDATE" => self.entity_decl(next),
                "DELETE" if !self.is_word(next, "FROM") => self.entity_decl(next),
                "JOIN" => self.join_decl(next),
                "PARTIAL" => self.partial(next),
                "OF" if i > 0 && self.is_word(i - 1, "INSTANCE") => self.instance_of(next),
                "NEW"
                    if self.kind(next) == Some(Tok::Ident)
                        && self.kind(next + 1) == Some(Tok::LParen) =>
                {
                    self.out.refs.push(DqlRef::Class(self.range(next)));
                    next + 1
                }
                "SELECT" | "WHERE" | "GROUP" | "HAVING" | "ORDER" | "SET" => {
                    if let Some(f) = in_from.last_mut() {
                        *f = false;
                    }
                    next
                }
                _ => next,
            }
            .max(next);
        }
    }
}

/// Read the declarations and names of the DQL `text`, a whole query or a
/// fragment of one.
pub(crate) fn scan(text: &str) -> DqlScan {
    let mut scanner = Scanner {
        text,
        tokens: tokenize(text),
        out: DqlScan::default(),
    };
    scanner.run();
    scanner.out
}

/// Whether `name` reads as a class name in DQL: a name a `FROM` can give,
/// as opposed to a variable or a dynamic part.
pub(crate) fn is_class_name(name: &str) -> bool {
    let mut parts = name.split('\\');
    // A leading `\` is allowed and means the same.
    if name.starts_with('\\') {
        parts.next();
    }
    let mut any = false;
    for part in parts {
        let mut chars = part.chars();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return false;
        }
        any = true;
    }
    any
}

/// The variable written right before `offset` in `text` and the start of
/// the partial field name after its dot, when `offset` follows `alias.` and
/// the start of a field: what completion offers fields for.
pub(crate) fn path_before(text: &str, offset: usize) -> Option<(&str, usize)> {
    let before = text.get(..offset)?;
    let field_start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    if !before[..field_start].ends_with('.') {
        return None;
    }
    let alias_end = field_start - 1;
    let alias_start = before[..alias_end]
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let alias = &before[alias_start..alias_end];
    // `App\Entity.` or `1.5` is no path.
    if alias.is_empty()
        || alias.starts_with(|c: char| c.is_ascii_digit())
        || before[..alias_start].ends_with(['\\', '.'])
        || is_keyword(alias)
    {
        return None;
    }
    Some((alias, field_start))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> (Vec<(String, String)>, Vec<String>) {
        let scan = scan(text);
        let decls = scan
            .decls
            .iter()
            .map(|d| {
                let root = match &d.root {
                    DeclRoot::Entity(r) => text[r.clone()].to_string(),
                    DeclRoot::Association => "<assoc>".to_string(),
                    DeclRoot::Unknown => "<unknown>".to_string(),
                };
                (text[d.alias.clone()].to_string(), root)
            })
            .collect();
        let refs = scan
            .refs
            .iter()
            .map(|r| match r {
                DqlRef::Class(r) => format!("class {}", &text[r.clone()]),
                DqlRef::Field { alias, field } => {
                    format!("{}.{}", &text[alias.clone()], &text[field.clone()])
                }
            })
            .collect();
        (decls, refs)
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn select_with_where_and_join() {
        let (decls, refs) = names(
            "SELECT u, a FROM App\\Entity\\User u LEFT JOIN u.address AS a \
             WHERE u.email = :e AND a.city LIKE 'x.y' ORDER BY u.name DESC",
        );
        assert_eq!(
            decls,
            vec![pair("u", "App\\Entity\\User"), pair("a", "<assoc>")]
        );
        assert_eq!(refs, vec!["u.address", "u.email", "a.city", "u.name"]);
    }

    #[test]
    fn several_ranges_and_arbitrary_join() {
        let (decls, refs) = names(
            "SELECT u FROM App\\User u INDEX BY u.id, App\\Group g \
             JOIN App\\Post p WITH p.author = u WHERE g.id IN (SELECT g2.id FROM App\\Group g2)",
        );
        assert_eq!(
            decls,
            vec![
                pair("u", "App\\User"),
                pair("g", "App\\Group"),
                pair("p", "App\\Post"),
                pair("g2", "App\\Group"),
            ]
        );
        assert_eq!(refs, vec!["u.id", "p.author", "g.id", "g2.id"]);
    }

    #[test]
    fn update_delete_partial_new_and_instance_of() {
        let (decls, _) = names("UPDATE App\\User u SET u.name = 'x'");
        assert_eq!(decls, vec![pair("u", "App\\User")]);
        let (decls, _) = names("DELETE App\\User u WHERE u.id = 1");
        assert_eq!(decls, vec![pair("u", "App\\User")]);
        let (decls, _) = names("DELETE FROM App\\User u");
        assert_eq!(decls, vec![pair("u", "App\\User")]);
        let (_, refs) = names(
            "SELECT PARTIAL u.{id, name}, NEW App\\Dto(u.id) FROM App\\User u \
             WHERE u INSTANCE OF (App\\Admin, App\\Editor)",
        );
        assert_eq!(
            refs,
            vec![
                "u.id",
                "u.name",
                "class App\\Dto",
                "u.id",
                "class App\\Admin",
                "class App\\Editor",
            ]
        );
    }

    #[test]
    fn missing_variable_still_names_the_class() {
        let (decls, refs) = names("SELECT e FROM Foo");
        assert!(decls.is_empty());
        assert_eq!(refs, vec!["class Foo"]);
    }

    #[test]
    fn dynamic_parts_name_nothing() {
        let text = format!("SELECT u FROM {DYNAMIC} u WHERE u.{DYNAMIC} = 1 AND u.na{DYNAMIC}");
        let (decls, refs) = names(&text);
        assert_eq!(decls, vec![pair("u", "<unknown>")]);
        assert!(refs.is_empty());
    }

    #[test]
    fn embedded_fields_and_numbers() {
        let (_, refs) = names("SELECT u FROM App\\User u WHERE u.address.city = 1.5");
        assert_eq!(refs, vec!["u.address"]);
    }

    #[test]
    fn path_before_cursor() {
        assert_eq!(path_before("WHERE u.em", 10), Some(("u", 8)));
        assert_eq!(path_before("WHERE u.", 8), Some(("u", 8)));
        assert_eq!(path_before("FROM App\\User.", 14), None);
        assert_eq!(path_before("= 1.", 4), None);
        assert_eq!(path_before("WHERE u", 7), None);
    }
}
