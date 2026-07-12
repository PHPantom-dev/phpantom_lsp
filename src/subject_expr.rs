//! Structured subject expression parsing.
//!
//! This module defines [`SubjectExpr`], a typed enum that represents the
//! structured form of a completion subject string.  It replaces ad-hoc
//! string-shape dispatch (checking `starts_with('$')`, `contains("->")`,
//! `ends_with(')')`, etc.) with exhaustive `match` in the resolver.
//!
//! The parser ([`SubjectExpr::parse`]) accepts the raw subject strings
//! produced by the symbol map or text scanner and returns the
//! corresponding variant.

// ─── Structured Subject Expression ──────────────────────────────────────────

/// Structured representation of a completion subject expression.
///
/// Replaces the string-shape dispatch (checking `starts_with('$')`,
/// `contains("->")`, `ends_with(')')`, etc.) with a typed enum so that
/// `resolve_target_classes` and `resolve_call_return_types_expr` can use
/// exhaustive `match` instead of fragile if-else chains.
///
/// Constructed via [`SubjectExpr::parse`] from the raw subject string
/// that the symbol map or text scanner produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectExpr {
    /// `$this` keyword.
    This,
    /// `self` keyword (may appear before `::` or as a subject).
    SelfKw,
    /// `static` keyword.
    StaticKw,
    /// `parent` keyword.
    Parent,
    /// A bare `$variable` (no chain, no brackets).
    Variable(String),
    /// A property chain: `base->property` or `base?->property`.
    ///
    /// The `base` is itself a `SubjectExpr` (e.g. `$this`, `$var`,
    /// or another `PropertyChain`), and `property` is the trailing
    /// identifier after the last `->`.
    PropertyChain {
        /// The expression to the left of the last `->`.
        base: Box<SubjectExpr>,
        /// The property name to the right of the last `->`.
        property: String,
    },
    /// A method/function call expression: `base(args)`.
    ///
    /// `callee` is the structured expression for the call target
    /// (which may be an instance method chain, a static method, or a
    /// bare function name) and `args_text` is the raw text between
    /// the parentheses (preserved for conditional return type
    /// resolution and template substitution).
    CallExpr {
        /// The structured callee expression (e.g. `MethodCall`,
        /// `StaticMethodCall`, `FunctionCall`, or a nested `CallExpr`).
        callee: Box<SubjectExpr>,
        /// Raw text of the arguments between `(` and `)`.
        args_text: String,
    },
    /// Instance method call target: `base->method`.
    ///
    /// This variant represents the *callee* of a call expression
    /// (i.e. what appears to the left of `(…)`), not the full call.
    /// The full call is wrapped in [`CallExpr`](SubjectExpr::CallExpr).
    MethodCall {
        /// The expression to the left of `->`.
        base: Box<SubjectExpr>,
        /// The method name to the right of `->`.
        method: String,
    },
    /// Static method call target: `ClassName::method`.
    ///
    /// Like `MethodCall`, this is the callee portion; the full call
    /// with arguments is wrapped in `CallExpr`.
    StaticMethodCall {
        /// The class name (or keyword) to the left of `::`.
        class: String,
        /// The method name to the right of `::`.
        method: String,
    },
    /// Static member access (enum case or constant): `ClassName::MEMBER`.
    ///
    /// Used when the RHS of `::` is a non-call identifier (e.g.
    /// `Status::Active`, `MyClass::SOME_CONST`).
    StaticAccess {
        /// The class name to the left of `::`.
        class: String,
        /// The member name to the right of `::`.
        member: String,
    },
    /// Constructor call target: `new ClassName`.
    ///
    /// The wrapping `CallExpr` (if any) carries the constructor
    /// arguments.
    NewExpr {
        /// The class name being instantiated.
        class_name: String,
    },
    /// A bare class name used as a subject (e.g. after `new` or before `::`).
    ClassName(String),
    /// A bare function name used as a call target.
    FunctionCall(String),
    /// Array index access: `base['key']` or `base[]`.
    ArrayAccess {
        /// The base expression being indexed.
        base: Box<SubjectExpr>,
        /// The bracket segments in left-to-right order.
        segments: Vec<BracketSegment>,
    },
    /// Inline array literal with index access: `[expr1, expr2][0]`.
    InlineArray {
        /// The raw element expressions inside the `[…]` literal.
        elements: Vec<String>,
        /// The bracket segments after the literal.
        index_segments: Vec<BracketSegment>,
    },
}

/// A single bracket segment in an array access chain.
///
/// Used by [`SubjectExpr::ArrayAccess`] and [`SubjectExpr::InlineArray`]
/// to represent each `[…]` dereference in a chain like `$var['a'][0][]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BracketSegment {
    /// A string-key access, e.g. `['items']`.
    StringKey(String),
    /// An integer-literal index access, e.g. `[0]` or `[2]`. Carries the
    /// decimal string form so it can address positional shape entries
    /// (`array{Foo, Bar}`) as well as explicit numeric keys.
    IntKey(String),
    /// A variable or otherwise non-literal index access, e.g. `[$i]` or `[]`.
    ElementAccess,
}

/// Classify the text inside a `[…]` bracket into a [`BracketSegment`].
///
/// Quoted strings become [`BracketSegment::StringKey`]; bare integer
/// literals become [`BracketSegment::IntKey`]; everything else (variables,
/// expressions, empty `[]`) becomes [`BracketSegment::ElementAccess`].
fn classify_bracket_inner(inner: &str) -> BracketSegment {
    if let Some(key) = crate::util::unquote_php_string(inner) {
        BracketSegment::StringKey(key.to_string())
    } else if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
        BracketSegment::IntKey(inner.to_string())
    } else {
        BracketSegment::ElementAccess
    }
}

impl SubjectExpr {
    /// Parse a raw subject string into a structured `SubjectExpr`.
    ///
    /// This is the bridge between the text-based world (symbol map
    /// `subject_text`, text scanner output) and the structured enum.
    /// The parser handles the same patterns that `resolve_target_classes`
    /// and `resolve_call_return_types_expr` previously checked with
    /// `starts_with`, `contains`, `rfind`, etc.
    pub fn parse(subject: &str) -> Self {
        let subject = subject.trim();
        if subject.is_empty() {
            return SubjectExpr::ClassName(String::new());
        }

        // ── Keywords ────────────────────────────────────────────────
        match subject {
            "$this" => return SubjectExpr::This,
            "self" => return SubjectExpr::SelfKw,
            "static" => return SubjectExpr::StaticKw,
            "parent" => return SubjectExpr::Parent,
            _ => {}
        }

        // ── `new ClassName(…)` or `(new ClassName(…))` ──────────────
        if let Some(class_name) = parse_new_expression_class(subject) {
            return SubjectExpr::NewExpr { class_name };
        }

        // ── Inline array literal with index: `[expr][0]` ───────────
        if subject.starts_with('[')
            && subject.contains("][")
            && let Some(result) = parse_inline_array(subject)
        {
            return result;
        }

        // ── Call expression: ends with `)` ──────────────────────────
        // Must be checked before property chains so that
        // `$this->getFactory()` is parsed as a call, not a property.
        if subject.ends_with(')')
            && let Some((call_body, args_text)) = split_call_subject_raw(subject)
        {
            let callee = parse_callee(call_body);
            return SubjectExpr::CallExpr {
                callee: Box::new(callee),
                args_text: args_text.to_string(),
            };
        }

        // ── Call expression with array access: `$c->items()[]` ──────
        // When the subject ends with `]` and the base before the first
        // `[` that follows a `)` is a call expression, parse as
        // `ArrayAccess` with a `CallExpr` base.  This handles patterns
        // like `$c->items()[0]->`, `Collection::all()[0]->`, and
        // `getItems()[0]->`.
        if subject.ends_with(']')
            && let Some(result) = parse_call_array_access(subject)
        {
            return result;
        }

        // ── `$var::member` — class-string variable static access ────
        // When a variable is followed by `::`, it holds a class-string
        // (e.g. `$cls = Pen::class; $cls::make()`).  Parse as
        // `StaticMethodCall` so that callable resolution can route
        // through `resolve_target_classes` with `DoubleColon` access.
        if subject.starts_with('$')
            && subject.contains("::")
            && !subject.ends_with(')')
            && let Some((var_part, member)) = subject.split_once("::")
            && !member.contains("->")
        {
            return SubjectExpr::StaticMethodCall {
                class: var_part.to_string(),
                method: member.to_string(),
            };
        }

        // ── Enum case / static access: `ClassName::Member` ─────────
        // Only match when there is no `->` after `::` (that would be a
        // chain like `ClassName::make()->prop`).
        if !subject.starts_with('$')
            && subject.contains("::")
            && !subject.ends_with(')')
            && let Some((class_part, member)) = subject.split_once("::")
            && !member.contains("->")
        {
            return SubjectExpr::StaticAccess {
                class: class_part.to_string(),
                member: member.to_string(),
            };
        }

        // ── Variable/property with bracket access: `$var['key']`,
        //    `$this->cache[]`, `$obj->items['k']` ───────────────────
        // Must be checked before the property chain so that
        // `$this->cache[]` is parsed as `ArrayAccess { PropertyChain
        // { This, "cache" }, [ElementAccess] }` instead of
        // `PropertyChain { This, "cache[]" }`.
        if subject.contains('[')
            && subject.ends_with(']')
            && let Some(result) = parse_variable_array_access(subject)
        {
            return result;
        }

        // ── Property chain (split at last depth-0 arrow) ───────────
        if subject.contains("->")
            && let Some((base_str, prop)) = split_last_arrow_raw(subject)
        {
            let base = SubjectExpr::parse(base_str);
            return SubjectExpr::PropertyChain {
                base: Box::new(base),
                property: prop.to_string(),
            };
        }

        // ── Bare variable: `$var` ──────────────────────────────────
        if subject.starts_with('$') {
            return SubjectExpr::Variable(subject.to_string());
        }

        // ── Bare class name ────────────────────────────────────────
        SubjectExpr::ClassName(subject.to_string())
    }

    /// Return the raw text representation of this expression.
    ///
    /// This is used as a bridge while callers are migrated: they can
    /// parse a string into `SubjectExpr`, match on it, and still pass
    /// the original text to functions that haven't been converted yet.
    pub fn to_subject_text(&self) -> String {
        match self {
            SubjectExpr::This => "$this".to_string(),
            SubjectExpr::SelfKw => "self".to_string(),
            SubjectExpr::StaticKw => "static".to_string(),
            SubjectExpr::Parent => "parent".to_string(),
            SubjectExpr::Variable(v) => v.clone(),
            SubjectExpr::PropertyChain { base, property } => {
                format!("{}->{}", base.to_subject_text(), property)
            }
            SubjectExpr::CallExpr { callee, args_text } => {
                // Wrap the callee in parentheses when it is an
                // expression form that is not naturally callable by
                // name.  Without this, `PropertyChain { $this, "prop" }`
                // serialises as `$this->prop(args)` (a method call)
                // instead of the correct `($this->prop)(args)` (invoke
                // property as callable via __invoke).
                let needs_parens = matches!(
                    callee.as_ref(),
                    SubjectExpr::PropertyChain { .. }
                        | SubjectExpr::This
                        | SubjectExpr::SelfKw
                        | SubjectExpr::StaticKw
                        | SubjectExpr::Parent
                        | SubjectExpr::ArrayAccess { .. }
                        | SubjectExpr::InlineArray { .. }
                        | SubjectExpr::CallExpr { .. }
                );
                if needs_parens {
                    format!("({})({})", callee.to_subject_text(), args_text)
                } else {
                    format!("{}({})", callee.to_subject_text(), args_text)
                }
            }
            SubjectExpr::MethodCall { base, method } => {
                format!("{}->{}", base.to_subject_text(), method)
            }
            SubjectExpr::StaticMethodCall { class, method } => {
                format!("{}::{}", class, method)
            }
            SubjectExpr::StaticAccess { class, member } => {
                format!("{}::{}", class, member)
            }
            SubjectExpr::NewExpr { class_name } => {
                format!("new {}", class_name)
            }
            SubjectExpr::ClassName(name) => name.clone(),
            SubjectExpr::FunctionCall(name) => name.clone(),
            SubjectExpr::ArrayAccess { base, segments } => {
                let mut s = base.to_subject_text();
                for seg in segments {
                    match seg {
                        BracketSegment::StringKey(k) => {
                            s.push_str(&format!("['{}']", k));
                        }
                        BracketSegment::IntKey(n) => {
                            s.push_str(&format!("[{}]", n));
                        }
                        BracketSegment::ElementAccess => {
                            s.push_str("[]");
                        }
                    }
                }
                s
            }
            SubjectExpr::InlineArray {
                elements,
                index_segments,
            } => {
                let mut s = format!("[{}]", elements.join(", "));
                for seg in index_segments {
                    match seg {
                        BracketSegment::StringKey(k) => {
                            s.push_str(&format!("['{}']", k));
                        }
                        BracketSegment::IntKey(n) => {
                            s.push_str(&format!("[{}]", n));
                        }
                        BracketSegment::ElementAccess => {
                            s.push_str("[]");
                        }
                    }
                }
                s
            }
        }
    }

    /// Returns `true` if this expression is one of the "current class"
    /// keywords (`$this`, `self`, `static`).
    pub fn is_self_like(&self) -> bool {
        matches!(
            self,
            SubjectExpr::This | SubjectExpr::SelfKw | SubjectExpr::StaticKw
        )
    }

    /// Parse the callee portion of a call expression (everything before
    /// the opening `(`).
    ///
    /// This distinguishes instance method calls (`base->method`), static
    /// method calls (`Class::method`), constructor calls (`new Class`),
    /// and bare function names.
    pub fn parse_callee(call_body: &str) -> SubjectExpr {
        parse_callee(call_body)
    }
}

// ─── SubjectExpr parsing helpers ────────────────────────────────────────────

/// Parse the callee portion of a call expression (everything before the
/// opening `(`).
///
/// This distinguishes instance method calls (`base->method`), static
/// method calls (`Class::method`), constructor calls (`new Class`),
/// and bare function names.
fn parse_callee(call_body: &str) -> SubjectExpr {
    let call_body = call_body.trim();

    // ── First-class callable invocation: `Foo::method(...)()` ───
    // When a first-class callable like `method(...)` is immediately
    // invoked, the return type equals the original method's return
    // type.  Strip the trailing `(...)` so the callee resolves as a
    // normal method/function call.
    let call_body = call_body.strip_suffix("(...)").unwrap_or(call_body);

    // ── Parenthesized expression: `($this->prop)`, `($var)` ─────
    // Strip balanced outer parens so the inner expression is parsed
    // normally.  This handles `($this->formatter)()` etc.
    // Only strip when the opening `(` at position 0 matches the
    // closing `)` at the end (i.e. the entire string is one
    // parenthesized group, not something like `(foo)(bar)`).
    if call_body.starts_with('(') && call_body.ends_with(')') {
        let mut depth = 0i32;
        let bytes = call_body.as_bytes();
        let mut closes_at_end = false;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        closes_at_end = i == bytes.len() - 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if closes_at_end {
            let inner = &call_body[1..call_body.len() - 1];
            return SubjectExpr::parse(inner);
        }
    }

    // ── `new ClassName` ─────────────────────────────────────────
    // Only match when there is no `->` chain after the constructor
    // args (e.g. `new Decimal($x)->toFixed(2)` should be parsed as
    // a method call, not a bare `new` expression).
    if call_body.starts_with("new ")
        && !call_body.contains("->")
        && let Some(class_name) = call_body
            .strip_prefix("new ")
            .map(|s| s.trim().trim_start_matches('\\'))
            .filter(|s| !s.is_empty())
    {
        // Strip trailing parens content if any (e.g. from `(new Foo(…))`)
        let clean = class_name
            .find(|c: char| c == '(' || c.is_whitespace())
            .map_or(class_name, |pos| &class_name[..pos]);
        return SubjectExpr::NewExpr {
            class_name: clean.to_string(),
        };
    }

    // ── Instance method: `base->method` ─────────────────────────
    // Use rfind to find the last `->` at depth 0 (outside parens).
    if let Some((base_str, method)) = split_last_arrow_raw(call_body) {
        let base = SubjectExpr::parse(base_str);
        return SubjectExpr::MethodCall {
            base: Box::new(base),
            method: method.to_string(),
        };
    }

    // ── Static method: `Class::method` ──────────────────────────
    if let Some(pos) = call_body.rfind("::") {
        let class_part = &call_body[..pos];
        let method_name = &call_body[pos + 2..];
        return SubjectExpr::StaticMethodCall {
            class: class_part.to_string(),
            method: method_name.to_string(),
        };
    }

    // ── Bare variable: `$fn` ────────────────────────────────────
    if call_body.starts_with('$') {
        return SubjectExpr::Variable(call_body.to_string());
    }

    // ── Bare function name ──────────────────────────────────────
    SubjectExpr::FunctionCall(call_body.to_string())
}

/// Split a subject at the **last** `->` or `?->` at depth 0.
///
/// Returns `(base, property)` or `None` if no arrow is found.
/// Arrows inside balanced parentheses are ignored.
fn split_last_arrow_raw(subject: &str) -> Option<(&str, &str)> {
    let bytes = subject.as_bytes();
    let mut depth = 0i32;
    let mut last_arrow: Option<(usize, usize)> = None;

    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'-' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                let arrow_start = if i > 0 && bytes[i - 1] == b'?' {
                    i - 1
                } else {
                    i
                };
                let prop_start = i + 2;
                last_arrow = Some((arrow_start, prop_start));
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }

    let (arrow_start, prop_start) = last_arrow?;
    if prop_start >= subject.len() {
        return None;
    }
    let base = &subject[..arrow_start];
    let prop = &subject[prop_start..];
    if base.is_empty() || prop.is_empty() {
        return None;
    }
    Some((base, prop))
}

/// Split a call expression at the matching `(` for the trailing `)`.
///
/// Returns `(call_body, args_text)` where `call_body` is the expression
/// before `(` and `args_text` is the trimmed content between `(` and `)`.
fn split_call_subject_raw(subject: &str) -> Option<(&str, &str)> {
    let inner = subject.strip_suffix(')')?;
    let bytes = inner.as_bytes();
    let mut depth: u32 = 0;
    let mut open = None;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                if depth == 0 {
                    open = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let open = open?;
    let call_body = &inner[..open];
    let args_text = inner[open + 1..].trim();
    if call_body.is_empty() {
        return None;
    }
    Some((call_body, args_text))
}

/// Parse a `new ClassName` or `(new ClassName(…))` expression and extract
/// the class name.
pub(crate) fn parse_new_expression_class(s: &str) -> Option<String> {
    // Strip balanced outer parentheses.
    let inner = if s.starts_with('(') && s.ends_with(')') {
        &s[1..s.len() - 1]
    } else {
        s
    };
    let rest = inner.trim().strip_prefix("new ")?;
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(rest.len());

    // If there is a `->` chain after the constructor call (e.g.
    // `new Decimal($x)->toFixed(2)`), bail out so that the call
    // expression / property chain parsers handle the full expression.
    // Also bail out when the constructor has non-empty arguments so
    // that the `CallExpr` parser preserves them for template inference
    // (e.g. `new C("foo")` should become `CallExpr { callee: NewExpr, args_text }`).
    if let Some(paren_start) = rest[end..].find('(') {
        let after_class = &rest[end + paren_start..];
        if let Some(close) = find_matching_paren(after_class) {
            let remainder = after_class[close + 1..].trim_start();
            if remainder.starts_with("->") {
                return None;
            }
            // Non-empty constructor args: bail out so CallExpr
            // parser wraps NewExpr and preserves the arguments.
            let args_inner = &after_class[1..close];
            if !args_inner.trim().is_empty() {
                return None;
            }
        }
    } else {
        // No opening paren found — check for `->` after the class name.
        let after_name = rest[end..].trim_start();
        if after_name.starts_with("->") {
            return None;
        }
    }

    let class_name = rest[..end].trim_start_matches('\\');
    if class_name.is_empty()
        || class_name == "class"
        || !class_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
    {
        return None;
    }
    Some(class_name.to_string())
}

/// Find the index of the closing `)` that matches the opening `(` at the
/// start of `s`.  Returns `None` if `s` doesn't start with `(` or the
/// parens are unbalanced.
fn find_matching_paren(s: &str) -> Option<usize> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0u32;
    let mut in_single = false;
    let mut in_double = false;
    let mut prev_backslash = false;
    for (i, ch) in s.char_indices() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        match ch {
            '\\' if in_single || in_double => prev_backslash = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse a variable with bracket access like `$var['key'][0]`.
fn parse_variable_array_access(subject: &str) -> Option<SubjectExpr> {
    let first_bracket = subject.find('[')?;
    let base_var = &subject[..first_bracket];
    if base_var.len() < 2 {
        return None;
    }

    let mut segments = Vec::new();
    let mut rest = &subject[first_bracket..];

    while rest.starts_with('[') {
        let close = rest.find(']')?;
        let inner = rest[1..close].trim();

        segments.push(classify_bracket_inner(inner));

        rest = &rest[close + 1..];
    }

    if segments.is_empty() {
        return None;
    }

    let mut result = SubjectExpr::ArrayAccess {
        base: Box::new(SubjectExpr::parse(base_var)),
        segments,
    };

    // Handle interleaved property-arrow and bracket access patterns.
    // After consuming the first set of bracket segments, there may be
    // a continuation like `->activities[]` (or `?->prop['key']`).
    // Build up the result by alternating PropertyChain and ArrayAccess
    // nodes until the remaining text is consumed.
    while !rest.is_empty() {
        let (arrow_len, is_nullsafe) = if rest.starts_with("?->") {
            (3, true)
        } else if rest.starts_with("->") {
            (2, false)
        } else {
            // Unexpected continuation — bail out with what we have.
            break;
        };

        let after_arrow = &rest[arrow_len..];
        if after_arrow.is_empty() {
            break;
        }

        // Find where the property name ends (at the next `[` or end).
        let prop_end = after_arrow.find('[').unwrap_or(after_arrow.len());
        let prop_name = &after_arrow[..prop_end];
        if prop_name.is_empty() {
            break;
        }

        // Build PropertyChain (or NullSafe — but SubjectExpr doesn't
        // distinguish at the PropertyChain level; the `?->` is already
        // encoded in `to_subject_text` via the base's text, and the
        // resolver handles both equally).
        let _ = is_nullsafe; // reserved for future use
        result = SubjectExpr::PropertyChain {
            base: Box::new(result),
            property: prop_name.to_string(),
        };

        rest = &after_arrow[prop_end..];

        // If the property is followed by bracket segments, consume them.
        if rest.starts_with('[') {
            let mut new_segments = Vec::new();
            while rest.starts_with('[') {
                let close = match rest.find(']') {
                    Some(c) => c,
                    None => break,
                };
                let inner = rest[1..close].trim();
                new_segments.push(classify_bracket_inner(inner));
                rest = &rest[close + 1..];
            }
            if !new_segments.is_empty() {
                result = SubjectExpr::ArrayAccess {
                    base: Box::new(result),
                    segments: new_segments,
                };
            }
        }
    }

    Some(result)
}

/// Parse a call expression followed by bracket access: `$c->items()[]`,
/// `Collection::all()[]`, `getItems()[]`.
///
/// Finds the `)` that ends the call expression, splits off the bracket
/// segments after it, then recursively parses the call portion as the
/// base of an `ArrayAccess`.
fn parse_call_array_access(subject: &str) -> Option<SubjectExpr> {
    // Scan for `)` followed immediately by `[` — that is the boundary
    // between the call expression and the bracket segments.
    // We need to find the *last* `)` that is followed by `[`, walking
    // balanced parens.  A simpler approach: find the position of `)[`
    // at paren-depth 0, scanning left-to-right.
    let bytes = subject.as_bytes();
    let mut depth = 0i32;
    let mut split = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                // Check if the next char is `[` — that marks the boundary.
                if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                    split = Some(i + 1); // position right after `)`
                }
            }
            _ => {}
        }
    }
    let split = split?;

    let call_part = &subject[..split];
    let bracket_part = &subject[split..];

    // The call part must end with `)` and be a valid call expression.
    if !call_part.ends_with(')') {
        return None;
    }

    // Parse bracket segments.
    let mut segments = Vec::new();
    let mut rest = bracket_part;
    while rest.starts_with('[') {
        let close = rest.find(']')?;
        let inner = rest[1..close].trim();
        segments.push(classify_bracket_inner(inner));
        rest = &rest[close + 1..];
    }

    if segments.is_empty() {
        return None;
    }

    // Recursively parse the call portion as the base expression.
    let base = SubjectExpr::parse(call_part);

    // Only accept if the base actually parsed as a CallExpr.
    if !matches!(base, SubjectExpr::CallExpr { .. }) {
        return None;
    }

    Some(SubjectExpr::ArrayAccess {
        base: Box::new(base),
        segments,
    })
}

/// Parse an inline array literal with index access: `[expr1, expr2][0]`.
fn parse_inline_array(subject: &str) -> Option<SubjectExpr> {
    let split_pos = subject.find("][")?;
    let literal_text = &subject[..split_pos + 1];
    if !literal_text.starts_with('[') || !literal_text.ends_with(']') {
        return None;
    }
    let inner = literal_text[1..literal_text.len() - 1].trim();
    let elements: Vec<String> = inner.split(',').map(|e| e.trim().to_string()).collect();

    // Parse the bracket segments after the literal.
    let index_part = &subject[split_pos + 1..];
    let mut index_segments = Vec::new();
    let mut rest = index_part;
    while rest.starts_with('[') {
        let close = rest.find(']')?;
        let idx_inner = rest[1..close].trim();
        index_segments.push(classify_bracket_inner(idx_inner));
        rest = &rest[close + 1..];
    }

    Some(SubjectExpr::InlineArray {
        elements,
        index_segments,
    })
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "subject_expr_tests.rs"]
mod tests;
