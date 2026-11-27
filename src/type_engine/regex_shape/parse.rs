//! A PCRE pattern body parsed into the structure the shape analysis reads.
//!
//! Only what decides the `$matches` shape is kept: where the capture groups
//! are, what can make one of them not participate (alternation, a quantifier
//! that allows zero repetitions, a negative lookaround), and which pieces
//! match one fixed piece of text, so a group built only from those has a
//! literal value. Every other atom is a [`Node::Other`].
//!
//! Constructs that would make the group structure depend on the subject
//! (conditional groups, recursion and subroutine calls, `(*ACCEPT)`) are
//! refused, and the caller falls back to the keyless array.

/// One node of a parsed pattern.
#[derive(Debug)]
pub(super) enum Node {
    /// Matches the empty string: an empty branch, an anchor, a comment.
    Empty,
    /// One fixed character.
    Char(char),
    /// A character class whose members are listed one by one (`[abc]`), or
    /// `None` for one that a range, an escape like `\d` or negation makes too
    /// broad to list.
    Class(Option<Vec<char>>),
    /// Any other atom that consumes input (`.`, `\d`, `\p{L}`, a
    /// backreference, a letter under case-insensitive matching).
    Other,
    /// `\K`: the reported match starts here.
    ResetMatchStart,
    /// `(*MARK:name)` / `(*:name)`.
    Mark(String),
    Group(Box<Group>),
    Concat(Vec<Node>),
    Alternation(Vec<Node>),
    Repeat {
        node: Box<Node>,
        min: u32,
        /// `None` for an unbounded repeat.
        max: Option<u32>,
    },
}

#[derive(Debug)]
pub(super) struct Group {
    pub kind: GroupKind,
    pub body: Node,
}

#[derive(Debug)]
pub(super) enum GroupKind {
    Capture {
        name: Option<String>,
    },
    /// `(?:…)`, `(?>…)` and `(?i:…)`: transparent to group numbering and
    /// participation.
    NonCapture,
    /// `(?=…)` and `(?<=…)`: its groups participate, but it adds nothing to
    /// the text the enclosing group matched.
    Lookaround,
    /// `(?!…)` and `(?<!…)`: a successful match means the groups inside did
    /// not participate.
    NegativeLookaround,
    /// `(?|…)`: each branch numbers its groups from the same starting point.
    BranchReset,
}

/// The parsed pattern.
pub(super) struct Pattern {
    pub root: Node,
    /// Whether two groups may share a name (the `J` modifier or `(?J)`).
    pub duplicate_names: bool,
}

/// The options that change how the rest of a group parses.
#[derive(Clone, Copy, Default)]
struct Options {
    caseless: bool,
    extended: bool,
    no_auto_capture: bool,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    duplicate_names: bool,
}

/// Parse a pattern body under the modifier letters written after its closing
/// delimiter. `None` for anything the analysis cannot model.
pub(super) fn parse(body: &str, modifiers: &str) -> Option<Pattern> {
    let mut options = Options::default();
    let mut duplicate_names = false;
    for c in modifiers.chars() {
        match c {
            'i' => options.caseless = true,
            'x' => options.extended = true,
            'n' => options.no_auto_capture = true,
            'J' => duplicate_names = true,
            'm' | 's' | 'A' | 'D' | 'S' | 'U' | 'X' | 'u' => {}
            _ if c.is_whitespace() => {}
            _ => return None,
        }
    }
    let mut parser = Parser {
        chars: body.chars().collect(),
        pos: 0,
        duplicate_names,
    };
    let root = parser.alternation(&mut options)?;
    if parser.pos != parser.chars.len() {
        // A `)` with nothing open.
        return None;
    }
    Some(Pattern {
        root,
        duplicate_names: parser.duplicate_names,
    })
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn starts_with(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c))
    }

    /// Read up to (and past) `close`, returning what came before it.
    fn read_until(&mut self, close: char) -> Option<String> {
        let start = self.pos;
        while self.peek()? != close {
            self.pos += 1;
        }
        let text = self.chars[start..self.pos].iter().collect();
        self.pos += 1;
        Some(text)
    }

    /// Skip the whitespace and `#` comments extended mode ignores.
    fn skip_extended(&mut self, options: &Options) {
        if !options.extended {
            return;
        }
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else if c == '#' {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Branches separated by `|`, up to a `)` or the end. An option setting
    /// such as `(?i)` reaches every later branch of the same group, so the
    /// options are shared across them.
    fn alternation(&mut self, options: &mut Options) -> Option<Node> {
        let mut branches = vec![self.concat(options)?];
        while self.eat('|') {
            branches.push(self.concat(options)?);
        }
        Some(if branches.len() == 1 {
            branches.pop()?
        } else {
            Node::Alternation(branches)
        })
    }

    fn concat(&mut self, options: &mut Options) -> Option<Node> {
        let mut items = Vec::new();
        loop {
            self.skip_extended(options);
            match self.peek() {
                None | Some('|') | Some(')') => break,
                _ => {}
            }
            let atom = self.atom(options)?;
            self.skip_extended(options);
            let atom = match self.quantifier()? {
                Some((min, max)) => {
                    // A quantifier on an anchor or an option setting is legal
                    // but means nothing for the groups.
                    Node::Repeat {
                        node: Box::new(atom),
                        min,
                        max,
                    }
                }
                None => atom,
            };
            items.push(atom);
        }
        Some(match items.len() {
            0 => Node::Empty,
            1 => items.pop()?,
            _ => Node::Concat(items),
        })
    }

    /// The quantifier at the cursor, if any: its bounds, with the lazy or
    /// possessive marker after it consumed. `Some(None)` when there is none;
    /// `None` for a stacked quantifier PCRE rejects.
    fn quantifier(&mut self) -> Option<Option<(u32, Option<u32>)>> {
        let bounds = match self.peek() {
            Some('{') => match self.brace_quantifier() {
                Some(bounds) => bounds,
                None => return Some(None),
            },
            Some(c @ ('?' | '*' | '+')) => {
                self.pos += 1;
                match c {
                    '?' => (0, Some(1)),
                    '*' => (0, None),
                    _ => (1, None),
                }
            }
            _ => return Some(None),
        };
        if matches!(self.peek(), Some('?' | '+')) {
            self.pos += 1;
        }
        // A second quantifier straight after is an error PCRE reports.
        if matches!(self.peek(), Some('?' | '*' | '+')) {
            return None;
        }
        Some(Some(bounds))
    }

    /// Read `{n}`, `{n,}`, `{n,m}` or `{,m}` at the cursor, moving past it.
    /// `None`, with the cursor unmoved, when the braces are a literal.
    fn brace_quantifier(&mut self) -> Option<(u32, Option<u32>)> {
        let mut i = self.pos + 1;
        let digits = |i: &mut usize, chars: &[char]| {
            let start = *i;
            while chars.get(*i).is_some_and(char::is_ascii_digit) {
                *i += 1;
            }
            chars[start..*i].iter().collect::<String>()
        };
        let min = digits(&mut i, &self.chars);
        let max = if self.chars.get(i) == Some(&',') {
            i += 1;
            Some(digits(&mut i, &self.chars))
        } else {
            None
        };
        if self.chars.get(i) != Some(&'}')
            || (min.is_empty() && max.as_deref().is_none_or(str::is_empty))
        {
            return None;
        }
        let min_value = if min.is_empty() { 0 } else { min.parse().ok()? };
        let max_value = match max {
            None => Some(min_value),
            Some(max) if max.is_empty() => None,
            Some(max) => Some(max.parse().ok()?),
        };
        self.pos = i + 1;
        Some((min_value, max_value))
    }

    fn atom(&mut self, options: &mut Options) -> Option<Node> {
        let c = self.peek()?;
        self.pos += 1;
        Some(match c {
            '(' => self.group(options)?,
            '[' => self.class(options)?,
            '\\' => self.escape(options)?,
            '.' => Node::Other,
            '^' | '$' => Node::Empty,
            // A quantifier with nothing before it.
            '?' | '*' | '+' => return None,
            c => literal(c, options),
        })
    }

    fn escape(&mut self, options: &Options) -> Option<Node> {
        let c = self.peek()?;
        self.pos += 1;
        Some(match c {
            // `\Q…\E` quotes everything up to `\E`, or to the end.
            'Q' => {
                let mut items = Vec::new();
                while self.pos < self.chars.len() && !self.starts_with("\\E") {
                    items.push(literal(self.chars[self.pos], options));
                    self.pos += 1;
                }
                if self.starts_with("\\E") {
                    self.pos += 2;
                }
                Node::Concat(items)
            }
            'E' | 'b' | 'B' | 'A' | 'z' | 'Z' | 'G' => Node::Empty,
            'K' => Node::ResetMatchStart,
            'd' | 'D' | 'w' | 'W' | 's' | 'S' | 'h' | 'H' | 'v' | 'V' | 'R' | 'N' | 'X' | 'C' => {
                Node::Other
            }
            'p' | 'P' => {
                if self.eat('{') {
                    self.read_until('}')?;
                } else {
                    self.pos += 1;
                }
                Node::Other
            }
            // Backreferences match whatever the group matched.
            '1'..='9' => {
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
                Node::Other
            }
            'g' => {
                match self.peek() {
                    Some('{') => {
                        self.pos += 1;
                        self.read_until('}')?;
                    }
                    // `\g<name>` and `\g'name'` are subroutine calls.
                    Some('<' | '\'') => return None,
                    _ => {
                        self.eat('-');
                        self.eat('+');
                        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                            self.pos += 1;
                        }
                    }
                }
                Node::Other
            }
            'k' => {
                let close = match self.peek()? {
                    '<' => '>',
                    '{' => '}',
                    '\'' => '\'',
                    _ => return None,
                };
                self.pos += 1;
                self.read_until(close)?;
                Node::Other
            }
            'n' => Node::Char('\n'),
            'r' => Node::Char('\r'),
            't' => Node::Char('\t'),
            'f' => Node::Char('\u{c}'),
            'e' => Node::Char('\u{1b}'),
            'a' => Node::Char('\u{7}'),
            'x' => match self.hex_escape() {
                Some(c) => literal(c, options),
                None => Node::Other,
            },
            '0' | 'o' | 'c' => {
                // Octal and control escapes: skip the digits or the letter.
                if c == 'c' {
                    self.pos += 1;
                } else if c == 'o' && self.eat('{') {
                    self.read_until('}')?;
                } else {
                    while self.peek().is_some_and(|c| ('0'..='7').contains(&c)) {
                        self.pos += 1;
                    }
                }
                Node::Other
            }
            c if c.is_ascii_alphanumeric() => return None,
            c => literal(c, options),
        })
    }

    /// The character a `\x` escape spells, the cursor just past the `x`.
    fn hex_escape(&mut self) -> Option<char> {
        let digits = if self.eat('{') {
            self.read_until('}')?
        } else {
            let start = self.pos;
            while self.pos < start + 2 && self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            self.chars[start..self.pos].iter().collect()
        };
        if digits.is_empty() {
            return Some('\0');
        }
        char::from_u32(u32::from_str_radix(&digits, 16).ok()?)
    }

    /// A character class, the cursor just past its `[`.
    fn class(&mut self, options: &Options) -> Option<Node> {
        let negated = self.eat('^');
        let mut members: Option<Vec<char>> = Some(Vec::new());
        let mut first = true;
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                // A `]` in first position is a literal.
                ']' if !first => break,
                '[' if self.peek() == Some(':') => {
                    self.read_until(']')?;
                    members = None;
                }
                '\\' => {
                    let escaped = self.peek()?;
                    self.pos += 1;
                    if escaped.is_ascii_alphanumeric() {
                        if escaped == 'p' || escaped == 'P' {
                            if self.eat('{') {
                                self.read_until('}')?;
                            } else {
                                self.pos += 1;
                            }
                        }
                        members = None;
                    } else if let Some(list) = members.as_mut() {
                        list.push(escaped);
                    }
                }
                // A range, unless the `-` is first or last.
                '-' if !first && self.peek() != Some(']') => {
                    members = None;
                    // Skip the range's upper end, escaped or not.
                    self.eat('\\');
                    self.pos += 1;
                }
                c => {
                    if let Some(list) = members.as_mut() {
                        list.push(c);
                    }
                }
            }
            first = false;
        }
        let varies_by_case = |list: &Vec<char>| {
            options.caseless && list.iter().any(|c| c.to_lowercase().ne(c.to_uppercase()))
        };
        if negated || members.as_ref().is_some_and(varies_by_case) {
            return Some(Node::Class(None));
        }
        Some(Node::Class(members.map(|mut list| {
            list.sort_unstable();
            list.dedup();
            list
        })))
    }

    /// A group, the cursor just past its `(`.
    fn group(&mut self, options: &mut Options) -> Option<Node> {
        if self.eat('*') {
            return self.verb();
        }
        if !self.eat('?') {
            let kind = if options.no_auto_capture {
                GroupKind::NonCapture
            } else {
                GroupKind::Capture { name: None }
            };
            return self.group_body(kind, *options);
        }
        let c = self.peek()?;
        match c {
            ':' | '>' => {
                self.pos += 1;
                self.group_body(GroupKind::NonCapture, *options)
            }
            '=' => {
                self.pos += 1;
                self.group_body(GroupKind::Lookaround, *options)
            }
            '!' => {
                self.pos += 1;
                self.group_body(GroupKind::NegativeLookaround, *options)
            }
            '|' => {
                self.pos += 1;
                self.group_body(GroupKind::BranchReset, *options)
            }
            '#' => {
                self.pos += 1;
                self.read_until(')')?;
                Some(Node::Empty)
            }
            '<' => match self.peek_at(1)? {
                '=' => {
                    self.pos += 2;
                    self.group_body(GroupKind::Lookaround, *options)
                }
                '!' => {
                    self.pos += 2;
                    self.group_body(GroupKind::NegativeLookaround, *options)
                }
                _ => {
                    self.pos += 1;
                    let name = self.group_name('>')?;
                    self.group_body(GroupKind::Capture { name: Some(name) }, *options)
                }
            },
            '\'' => {
                self.pos += 1;
                let name = self.group_name('\'')?;
                self.group_body(GroupKind::Capture { name: Some(name) }, *options)
            }
            'P' => match self.peek_at(1)? {
                '<' => {
                    self.pos += 2;
                    let name = self.group_name('>')?;
                    self.group_body(GroupKind::Capture { name: Some(name) }, *options)
                }
                // `(?P=name)` is a backreference; `(?P>name)` a subroutine
                // call, which this does not model.
                '=' => {
                    self.pos += 2;
                    self.read_until(')')?;
                    Some(Node::Other)
                }
                _ => None,
            },
            _ => self.option_setting(options),
        }
    }

    /// `(?imsx-imsx)` or `(?imsx-imsx:…)`, the cursor on the first letter.
    fn option_setting(&mut self, options: &mut Options) -> Option<Node> {
        let mut changed = *options;
        let mut on = true;
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                '-' => on = false,
                '^' => {
                    changed.caseless = false;
                    changed.extended = false;
                    changed.no_auto_capture = false;
                }
                'i' => changed.caseless = on,
                'x' => changed.extended = on,
                'n' => changed.no_auto_capture = on,
                'J' => {
                    if on {
                        self.duplicate_names = true;
                    }
                }
                'm' | 's' | 'U' => {}
                ')' => {
                    *options = changed;
                    return Some(Node::Empty);
                }
                ':' => return self.group_body(GroupKind::NonCapture, changed),
                // Conditionals, recursion, subroutine calls and callouts.
                _ => return None,
            }
        }
    }

    fn group_body(&mut self, kind: GroupKind, mut options: Options) -> Option<Node> {
        let body = self.alternation(&mut options)?;
        if !self.eat(')') {
            return None;
        }
        Some(Node::Group(Box::new(Group { kind, body })))
    }

    fn group_name(&mut self, close: char) -> Option<String> {
        let name = self.read_until(close)?;
        (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .then_some(name)
    }

    /// A backtracking verb, the cursor just past `(*`.
    fn verb(&mut self) -> Option<Node> {
        let text = self.read_until(')')?;
        let (verb, argument) = match text.split_once(':') {
            Some((verb, argument)) => (verb, Some(argument)),
            None => (text.as_str(), None),
        };
        match (verb, argument) {
            ("MARK" | "", Some(name)) => Some(Node::Mark(name.to_string())),
            // `(*ACCEPT)` ends the match with groups still open; `PRUNE`,
            // `SKIP` and `THEN` with a name also set the mark.
            ("ACCEPT", _) | (_, Some(_)) => None,
            _ => Some(Node::Empty),
        }
    }
}

/// A literal character, which only matches itself when its case cannot vary.
fn literal(c: char, options: &Options) -> Node {
    if options.caseless && c.to_lowercase().ne(c.to_uppercase()) {
        Node::Other
    } else {
        Node::Char(c)
    }
}
