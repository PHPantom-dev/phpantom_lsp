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
/// `resolve_target_classes` and `resolve_call_return_types_expr_with_hint`
/// can use exhaustive `match` instead of fragile if-else chains.
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
    /// An index computed from at least one variable, e.g. `[$i]` or
    /// `[$count - 2]`.
    ///
    /// Carries the index in its spaceless written form.  Which entry it
    /// addresses is unknown, so it yields the same element type
    /// [`ElementAccess`](BracketSegment::ElementAccess) does; the text is
    /// kept because two reads written the same way are the same subject,
    /// which is what lets a guard on `$types[$i]` narrow a later read of
    /// it.
    ComputedIndex(String),
    /// An otherwise non-literal index access, e.g. `[strlen($s)]` or `[]`.
    ElementAccess,
}

/// Classify the text inside a `[…]` bracket into a [`BracketSegment`].
///
/// Quoted strings become [`BracketSegment::StringKey`]; bare integer
/// literals become [`BracketSegment::IntKey`]; an arithmetic offset built
/// from variables becomes [`BracketSegment::ComputedIndex`]; everything
/// else (empty `[]`, a call, a nested literal key) becomes
/// [`BracketSegment::ElementAccess`].
fn classify_bracket_inner(inner: &str) -> BracketSegment {
    if let Some(key) = crate::text_scan::unquote_php_string(inner) {
        BracketSegment::StringKey(key.to_string())
    } else if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
        BracketSegment::IntKey(inner.to_string())
    } else if let Some(text) = computed_index_text(inner) {
        BracketSegment::ComputedIndex(text)
    } else {
        BracketSegment::ElementAccess
    }
}

/// The spaceless form of an index that reads a variable (`$i`,
/// `$count - 2`), or `None` when the text is anything else.
///
/// Written text reaches this from the source (`$a[$count - 2]`) and from a
/// stored subject that was already normalised, so the spaces come out and
/// both spellings land on the key the AST side builds. The accepted
/// alphabet is deliberately narrow: an index that calls, concatenates, or
/// reads a nested literal key is left as a plain element access rather
/// than risk two different reads normalising to one string.
fn computed_index_text(inner: &str) -> Option<String> {
    if !inner.contains('$') {
        return None;
    }
    if !inner.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'_' | b'$' | b'+' | b'-' | b'*' | b'/' | b'%' | b'(' | b')'
            )
            || b.is_ascii_whitespace()
            || b >= 0x80
    }) {
        return None;
    }
    let text: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
    (!text.is_empty()).then_some(text)
}

impl SubjectExpr {
    /// Parse a raw subject string into a structured `SubjectExpr`.
    ///
    /// This is the bridge between the text-based world (symbol map
    /// `subject_text`, text scanner output) and the structured enum.
    /// The parser handles the same patterns that `resolve_target_classes`
    /// and `resolve_call_return_types_expr_with_hint` previously checked
    /// with `starts_with`, `contains`, `rfind`, etc.
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
        // chain like `ClassName::make()->prop`), and no bracket access
        // after it either: `self::$map['k']` is an element *of* the
        // static property, so it belongs to the array-access branch
        // below, which parses `self::$map` as its base.  Keeping it here
        // would look for a static member literally named `$map['k']`,
        // find nothing, and answer with the class itself.
        if !subject.starts_with('$')
            && subject.contains("::")
            && !subject.ends_with(')')
            && let Some((class_part, member)) = subject.split_once("::")
            && !member.contains("->")
            && !(member.contains('[') && subject.ends_with(']'))
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
    ///
    /// A method or property chain nests one variant per link, so this walks
    /// an explicit work stack rather than recursing: recursion would spend a
    /// stack frame per link (and re-format the whole prefix at every one),
    /// and a generated fluent chain has no length bound.
    pub fn to_subject_text(&self) -> String {
        /// A pending piece of output.  The stack is popped LIFO, so each
        /// node pushes its parts in reverse of the order they are written.
        enum Step<'a> {
            Node(&'a SubjectExpr),
            Text(&'a str),
            Brackets(&'a [BracketSegment]),
        }

        let mut out = String::new();
        let mut stack = vec![Step::Node(self)];
        while let Some(step) = stack.pop() {
            let node = match step {
                Step::Text(text) => {
                    out.push_str(text);
                    continue;
                }
                Step::Brackets(segments) => {
                    for segment in segments {
                        match segment {
                            BracketSegment::StringKey(key) => {
                                out.push('[');
                                out.push('\'');
                                out.push_str(key);
                                out.push('\'');
                                out.push(']');
                            }
                            BracketSegment::IntKey(n) => {
                                out.push('[');
                                out.push_str(n);
                                out.push(']');
                            }
                            BracketSegment::ComputedIndex(index) => {
                                out.push('[');
                                out.push_str(index);
                                out.push(']');
                            }
                            BracketSegment::ElementAccess => out.push_str("[]"),
                        }
                    }
                    continue;
                }
                Step::Node(node) => node,
            };
            match node {
                SubjectExpr::This => out.push_str("$this"),
                SubjectExpr::SelfKw => out.push_str("self"),
                SubjectExpr::StaticKw => out.push_str("static"),
                SubjectExpr::Parent => out.push_str("parent"),
                SubjectExpr::Variable(v) => out.push_str(v),
                SubjectExpr::PropertyChain { base, property } => {
                    stack.push(Step::Text(property));
                    stack.push(Step::Text("->"));
                    stack.push(Step::Node(base));
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
                    stack.push(Step::Text(")"));
                    stack.push(Step::Text(args_text));
                    stack.push(Step::Text("("));
                    if needs_parens {
                        // The opening paren precedes the callee, so it is
                        // written now rather than queued.
                        out.push('(');
                        stack.push(Step::Text(")"));
                    }
                    stack.push(Step::Node(callee));
                }
                SubjectExpr::MethodCall { base, method } => {
                    stack.push(Step::Text(method));
                    stack.push(Step::Text("->"));
                    stack.push(Step::Node(base));
                }
                SubjectExpr::StaticMethodCall { class, method } => {
                    out.push_str(class);
                    out.push_str("::");
                    out.push_str(method);
                }
                SubjectExpr::StaticAccess { class, member } => {
                    out.push_str(class);
                    out.push_str("::");
                    out.push_str(member);
                }
                SubjectExpr::NewExpr { class_name } => {
                    out.push_str("new ");
                    out.push_str(class_name);
                }
                SubjectExpr::ClassName(name) => out.push_str(name),
                SubjectExpr::FunctionCall(name) => out.push_str(name),
                SubjectExpr::ArrayAccess { base, segments } => {
                    stack.push(Step::Brackets(segments));
                    stack.push(Step::Node(base));
                }
                SubjectExpr::InlineArray {
                    elements,
                    index_segments,
                } => {
                    out.push('[');
                    out.push_str(&elements.join(", "));
                    out.push(']');
                    stack.push(Step::Brackets(index_segments));
                }
            }
        }
        out
    }

    /// The link below this one when the expression is rendered as a
    /// forward-walker scope key, or `None` when this node is the base of
    /// the key.
    ///
    /// Property and array-access links are always descended.  So is an
    /// *argument-less* method call: the AST side keys `$h->get()->name()`
    /// under its own written form, so everything below it has to render
    /// in the same format or the two never meet.  A call that carries
    /// arguments stops the walk, because its key spells the arguments in
    /// a canonical form that only the AST side can produce.
    pub fn scope_key_base(&self) -> Option<&SubjectExpr> {
        match self {
            SubjectExpr::PropertyChain { base, .. } | SubjectExpr::ArrayAccess { base, .. } => {
                Some(base.as_ref())
            }
            SubjectExpr::CallExpr { callee, args_text } if args_text.trim().is_empty() => {
                match callee.as_ref() {
                    SubjectExpr::MethodCall { base, .. } => Some(base.as_ref()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether the base of a scope key path is a variable, `$this`, or one
    /// of the class keywords — the roots the forward walker tracks.
    ///
    /// Walks the same links [`Self::scope_key_base`] does, so a receiver
    /// reached through calls and element accesses (`$e->getExpr()`,
    /// `$rows[0]->getExpr()`) counts as rooted just like a direct one.
    pub fn scope_key_roots_in_variable(&self) -> bool {
        let mut node = self;
        loop {
            if matches!(
                node,
                SubjectExpr::This
                    | SubjectExpr::SelfKw
                    | SubjectExpr::StaticKw
                    | SubjectExpr::Parent
                    | SubjectExpr::Variable(_)
            ) {
                return true;
            }
            match node.scope_key_base() {
                Some(base) => node = base,
                None => return false,
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

    /// Collects the names of genuine local variables (`$var`, but not
    /// `$this`) referenced anywhere in this expression — both as receivers
    /// (`$stmt->foo()`) and as call arguments (`$this->parse($stmt)`).
    /// Names are appended to `out` (with their leading `$`), possibly with
    /// duplicates.
    ///
    /// A local variable's type comes from assignments visible at the cursor,
    /// so the same expression text can resolve to different types at
    /// different call sites depending on what those variables hold.  Any
    /// cache keyed by the subject text alone must therefore mix in a
    /// discriminator built from these variables' resolved types.  `$this`,
    /// `self`, `static`, and `parent` are class-relative rather than local,
    /// so they are not collected.
    /// Walks the spine iteratively: a chain nests one variant per link, and
    /// there is no bound on how long a generated chain gets.
    pub fn collect_local_variables(&self, out: &mut Vec<String>) {
        // Argument texts are set aside on the way down the spine and drained
        // innermost-first afterwards, so `out` keeps the base-first order a
        // recursive walk produces.
        let mut pending_args: Vec<&str> = Vec::new();
        let mut node = self;
        loop {
            node = match node {
                SubjectExpr::Variable(name) => {
                    out.push(name.clone());
                    break;
                }
                SubjectExpr::PropertyChain { base, .. }
                | SubjectExpr::MethodCall { base, .. }
                | SubjectExpr::ArrayAccess { base, .. } => base,
                SubjectExpr::CallExpr { callee, args_text } => {
                    pending_args.push(args_text);
                    callee
                }
                SubjectExpr::InlineArray { elements, .. } => {
                    for elem in elements {
                        collect_text_local_variables(elem, out);
                    }
                    break;
                }
                SubjectExpr::This
                | SubjectExpr::SelfKw
                | SubjectExpr::StaticKw
                | SubjectExpr::Parent
                | SubjectExpr::StaticMethodCall { .. }
                | SubjectExpr::StaticAccess { .. }
                | SubjectExpr::NewExpr { .. }
                | SubjectExpr::ClassName(_)
                | SubjectExpr::FunctionCall(_) => break,
            };
        }
        for args_text in pending_args.into_iter().rev() {
            collect_text_local_variables(args_text, out);
        }
    }

    /// Returns `true` if this expression references any genuine local
    /// variable (see [`collect_local_variables`](Self::collect_local_variables)).
    pub fn references_local_variable(&self) -> bool {
        let mut vars = Vec::new();
        self.collect_local_variables(&mut vars);
        !vars.is_empty()
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

/// A chain nests one `Box` per link, so the derived drop glue recurses
/// through the spine — and a generated fluent chain has no length bound, so
/// that recursion can exhaust the stack while merely *freeing* an
/// expression.  Dismantle the spine iteratively instead.
impl Drop for SubjectExpr {
    fn drop(&mut self) {
        let mut pending: Vec<SubjectExpr> = Vec::new();
        detach_child(self, &mut pending);
        while let Some(mut node) = pending.pop() {
            detach_child(&mut node, &mut pending);
            // `node` is freed at the end of this iteration.  Its own child
            // is a leaf by now, so its drop cannot recurse back into the
            // spine.
        }
    }
}

/// Move `node`'s boxed child (if it has one) into `pending`, leaving a leaf
/// in its place so that dropping `node` cannot recurse.
fn detach_child(node: &mut SubjectExpr, pending: &mut Vec<SubjectExpr>) {
    let child = match node {
        SubjectExpr::PropertyChain { base, .. }
        | SubjectExpr::MethodCall { base, .. }
        | SubjectExpr::ArrayAccess { base, .. } => base,
        SubjectExpr::CallExpr { callee, .. } => callee,
        SubjectExpr::This
        | SubjectExpr::SelfKw
        | SubjectExpr::StaticKw
        | SubjectExpr::Parent
        | SubjectExpr::Variable(_)
        | SubjectExpr::StaticMethodCall { .. }
        | SubjectExpr::StaticAccess { .. }
        | SubjectExpr::NewExpr { .. }
        | SubjectExpr::ClassName(_)
        | SubjectExpr::FunctionCall(_)
        | SubjectExpr::InlineArray { .. } => return,
    };
    pending.push(std::mem::replace(child.as_mut(), SubjectExpr::This));
}

// ─── SubjectExpr parsing helpers ────────────────────────────────────────────

/// Appends the names of genuine local variables (`$name`, but not `$this`)
/// found in a raw argument/element text to `out`, each with its leading
/// `$`.  Used to gather the variables an expression's resolution depends
/// on so a cache key can be made scope-aware.
fn collect_text_local_variables(text: &str, out: &mut Vec<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start && !text[start..end].eq_ignore_ascii_case("this") {
                out.push(text[i..end].to_string());
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
}

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
pub(crate) fn split_call_subject_raw(subject: &str) -> Option<(&str, &str)> {
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
        if let Some(close) = crate::text_scan::find_matching_forward(after_class, 0, b'(', b')') {
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
        let arrow_len = if rest.starts_with("?->") {
            3
        } else if rest.starts_with("->") {
            2
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

        // PropertyChain doesn't distinguish `->` from `?->` — the operator
        // is already encoded in `to_subject_text` via the base's text, and
        // the resolver handles both equally.
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
