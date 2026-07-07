//! Shared subject-extraction helpers.
//!
//! This module contains free functions for extracting the expression
//! ("subject") to the left of an access operator (`->`, `?->`, `::`) in
//! a line of PHP source code.  These are used by both the **completion**
//! and **definition** subsystems so that the logic is defined once.
//!
//! All functions operate on a `&[char]` slice representing a single
//! (already-collapsed) line and work backwards from a given position.
//! Multi-line chain collapsing lives in [`crate::util::collapse_continuation_lines`].
//!
//! The main entry point is [`detect_access_operator`], which locates
//! `->`, `?->`, or `::` near the cursor and extracts the subject to its
//! left.  Internally it delegates to [`extract_arrow_subject`] and
//! [`extract_double_colon_subject`] for the character-level backward walk.
//!
//! # Subjects
//!
//! A "subject" is the textual expression that precedes an operator.
//! Examples:
//!
//! | Source                        | Operator | Subject                 |
//! |------------------------------|----------|-------------------------|
//! | `$this->`                    | `->`     | `$this`                 |
//! | `$this->prop->`              | `->`     | `$this->prop`           |
//! | `app()->`                    | `->`     | `app()`                 |
//! | `app(A::class)->`            | `->`     | `app(A::class)`         |
//! | `$this->getService()->`      | `->`     | `$this->getService()`   |
//! | `ClassName::make()->`        | `->`     | `ClassName::make()`     |
//! | `new Foo()->`                | `->`     | `Foo`                   |
//! | `(new Foo())->`              | `->`     | `Foo`                   |
//! | `(clone $var)->`             | `->`     | `$var`                  |
//! | `Status::Active->`           | `->`     | `Status::Active`        |
//! | `self::`                     | `::`     | `self`                  |
//! | `ClassName::`                | `::`     | `ClassName`             |
//! | `$var?->`                    | `?->`    | `$var`                  |

use crate::types::AccessKind;
use crate::util::strip_fqn_prefix;

// ─── Character-level helpers ────────────────────────────────────────────────
//
// These were previously in `util.rs` but are only consumed by the
// subject-extraction logic in this module, so they live here now.

/// Skip backwards past a balanced parenthesised group `(…)` in a char slice.
///
/// `pos` must point one past the closing `)`.  Returns the index of the
/// opening `(`, or `None` if parens are unbalanced.
fn skip_balanced_parens_back(chars: &[char], pos: usize) -> Option<usize> {
    if pos == 0 || chars[pos - 1] != ')' {
        return None;
    }
    let mut depth: u32 = 0;
    let mut j = pos;
    while j > 0 {
        j -= 1;
        match chars[j] {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// Skip backwards past a balanced bracket group `[…]` in a char slice.
///
/// `pos` must point one past the closing `]`.  Returns the index of the
/// opening `[`, or `None` if brackets are unbalanced.
fn skip_balanced_brackets_back(chars: &[char], pos: usize) -> Option<usize> {
    if pos == 0 || chars[pos - 1] != ']' {
        return None;
    }
    let mut depth: u32 = 0;
    let mut j = pos;
    while j > 0 {
        j -= 1;
        match chars[j] {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check if the `new` keyword (followed by whitespace) appears immediately
/// before the identifier starting at position `ident_start`.
///
/// Returns the class name (possibly with namespace) if `new` is found.
fn check_new_keyword_before(
    chars: &[char],
    ident_start: usize,
    class_name: &str,
) -> Option<String> {
    let mut j = ident_start;
    // Skip whitespace between `new` and the class name.
    while j > 0 && chars[j - 1] == ' ' {
        j -= 1;
    }
    // Check for the `new` keyword.
    if j >= 3 && chars[j - 3] == 'n' && chars[j - 2] == 'e' && chars[j - 1] == 'w' {
        // Verify word boundary before `new` (start of line, whitespace, `(`, etc.).
        let before_ok = j == 3 || {
            let prev = chars[j - 4];
            !prev.is_alphanumeric() && prev != '_'
        };
        if before_ok {
            // Strip leading `\` from FQN if present.
            let name = strip_fqn_prefix(class_name);
            return Some(name.to_string());
        }
    }
    None
}

/// Try to extract a subject from a parenthesized `clone` expression:
/// `(clone $expr)`.
///
/// `clone` preserves the type of the operand, so the subject is just
/// the inner expression after stripping the `clone` keyword.
///
/// `open` is the position of the outer `(`, `close` is one past the
/// outer `)`.
fn extract_clone_expression_inside_parens(
    chars: &[char],
    open: usize,
    close: usize,
) -> Option<String> {
    let inner_start = open + 1;
    let inner_end = close - 1;
    if inner_start >= inner_end {
        return None;
    }

    // Skip whitespace inside the opening `(`.
    let mut k = inner_start;
    while k < inner_end && chars[k] == ' ' {
        k += 1;
    }

    // Check for `clone` keyword (5 chars).
    if k + 5 > inner_end {
        return None;
    }
    if chars[k] != 'c'
        || chars[k + 1] != 'l'
        || chars[k + 2] != 'o'
        || chars[k + 3] != 'n'
        || chars[k + 4] != 'e'
    {
        return None;
    }
    k += 5;

    // Must be followed by whitespace.
    if k >= inner_end || chars[k] != ' ' {
        return None;
    }
    while k < inner_end && chars[k] == ' ' {
        k += 1;
    }

    // The rest is the expression being cloned.  Since `clone` preserves
    // the type, return the inner expression as-is so the resolver sees
    // e.g. `$date` instead of `(clone $date)`.
    let rest: String = chars[k..inner_end].iter().collect();
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    Some(rest.to_string())
}

/// Try to extract a class name from a parenthesized `new` expression:
/// `(new ClassName(...))`.
///
/// `open` is the position of the outer `(`, `close` is one past the
/// outer `)`.  The function looks inside for the pattern
/// `new ClassName(...)`.
fn extract_new_expression_inside_parens(
    chars: &[char],
    open: usize,
    close: usize,
) -> Option<String> {
    // Content is chars[open+1 .. close-1].
    let inner_start = open + 1;
    let inner_end = close - 1;
    if inner_start >= inner_end {
        return None;
    }

    // Skip whitespace inside the opening `(`.
    let mut k = inner_start;
    while k < inner_end && chars[k] == ' ' {
        k += 1;
    }

    // Check for `new` keyword.
    if k + 3 >= inner_end {
        return None;
    }
    if chars[k] != 'n' || chars[k + 1] != 'e' || chars[k + 2] != 'w' {
        return None;
    }
    k += 3;

    // Must be followed by whitespace.
    if k >= inner_end || chars[k] != ' ' {
        return None;
    }
    while k < inner_end && chars[k] == ' ' {
        k += 1;
    }

    // Read the class name (may include `\` for namespaces).
    let name_start = k;
    while k < inner_end && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '\\') {
        k += 1;
    }
    if k == name_start {
        return None;
    }
    let class_name: String = chars[name_start..k].iter().collect();
    let name = strip_fqn_prefix(&class_name);

    // Skip whitespace after the class name.
    while k < inner_end && chars[k] == ' ' {
        k += 1;
    }

    // If constructor args follow (a `(` ... `)` group), include them.
    if k < inner_end && chars[k] == '(' {
        let mut depth: u32 = 0;
        let mut p = k;
        let mut found = None;
        while p < inner_end {
            match chars[p] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        found = Some(p);
                        break;
                    }
                }
                _ => {}
            }
            p += 1;
        }
        if let Some(arg_close) = found {
            let args: String = chars[k + 1..arg_close].iter().collect();
            return Some(format!("new {}({})", name, args));
        }
    }

    Some(format!("new {}", name))
}

// ─── Subject extraction ─────────────────────────────────────────────────────

/// Extract the subject expression before an arrow operator (`->`).
///
/// `chars` is the line as a char slice.  `arrow_pos` is the index of
/// the `-` character (i.e. `chars[arrow_pos] == '-'` and
/// `chars[arrow_pos + 1] == '>'`).
///
/// Handles:
///   - `$this->`, `$var->` (simple variable)
///   - `$this->prop->` (property chain)
///   - `$this?->prop->` (nullsafe property chain)
///   - `app()->` (function call)
///   - `$this->getService()->` (method call chain)
///   - `ClassName::make()->` (static method call)
///   - `new ClassName()->` (instantiation, PHP 8.4+)
///   - `(new ClassName())->` (parenthesized instantiation)
///   - `(clone $var)->` (clone preserves type of operand)
///   - `Status::Active->` (enum case access)
///   - `tryFrom($int)?->` (nullsafe after call)
fn extract_arrow_subject(chars: &[char], arrow_pos: usize) -> String {
    // Position just before the `->`
    let mut end = arrow_pos;

    // Skip whitespace
    let mut i = end;
    while i > 0 && chars[i - 1] == ' ' {
        i -= 1;
    }

    // Skip the `?` of the nullsafe `?->` operator so that the rest
    // of the extraction logic sees the expression before the `?`
    // (e.g. the `)` of a call expression like `tryFrom($int)?->`,
    // or a simple variable like `$var?->`).
    if i > 0 && chars[i - 1] == '?' {
        i -= 1;
    }

    // Update `end` so the fallback `extract_simple_variable` at the
    // bottom of this function also starts from the correct position
    // (past any `?` and whitespace).
    end = i;

    // ── Array access: detect `]` ──
    // e.g. `$admins[0]->`, `$admins[$key]->`, `$config['key']->`
    // Also handles chained access: `$response['items'][0]->`
    //
    // Walk backward through one or more balanced `[…]` pairs, collecting
    // each bracket segment.  The segments are stored innermost-first and
    // reversed at the end so the final subject reads left-to-right.
    if i > 0 && chars[i - 1] == ']' {
        let mut segments: Vec<String> = Vec::new();
        // Track the raw bracket ranges so we can reconstruct the
        // array literal base when there is no `$var` prefix.
        let mut bracket_ranges: Vec<(usize, usize)> = Vec::new();
        let mut pos = i;

        while pos > 0
            && chars[pos - 1] == ']'
            && let Some(bracket_open) = skip_balanced_brackets_back(chars, pos)
        {
            let inner: String = chars[bracket_open + 1..pos - 1].iter().collect();
            let inner_trimmed = inner.trim();
            // Quoted string key → preserve it so the resolver can look
            // up the specific key in an array shape type annotation.
            if (inner_trimmed.starts_with('\'') && inner_trimmed.ends_with('\''))
                || (inner_trimmed.starts_with('"') && inner_trimmed.ends_with('"'))
            {
                segments.push(format!("[{}]", inner_trimmed));
            } else {
                // Generic / numeric index → strip to `[]`.
                segments.push("[]".to_string());
            }
            bracket_ranges.push((bracket_open, pos));
            pos = bracket_open;
        }

        if !segments.is_empty() {
            let before = extract_simple_variable(chars, pos);
            if !before.is_empty() {
                // When the extracted identifier has no `$` prefix, it
                // may be a property name preceded by `->` or `?->`.
                // For example, `$this->cache[$key]->` yields `cache`
                // here.  Walk back through the arrow to capture the
                // full property chain so the subject becomes
                // `$this->cache[]` instead of just `cache[]`.
                let base = if !before.starts_with('$') {
                    let prop_start = pos - before.len();
                    if prop_start >= 2
                        && chars[prop_start - 2] == '-'
                        && chars[prop_start - 1] == '>'
                    {
                        let chain = extract_arrow_subject(chars, prop_start - 2);
                        if !chain.is_empty() {
                            format!("{}->{}", chain, before)
                        } else {
                            before
                        }
                    } else if prop_start >= 3
                        && chars[prop_start - 3] == '?'
                        && chars[prop_start - 2] == '-'
                        && chars[prop_start - 1] == '>'
                    {
                        let chain = extract_arrow_subject(chars, prop_start - 3);
                        if !chain.is_empty() {
                            format!("{}?->{}", chain, before)
                        } else {
                            before
                        }
                    } else {
                        before
                    }
                } else {
                    before
                };
                // Reverse so segments read left-to-right.
                segments.reverse();
                return format!("{}{}", base, segments.join(""));
            }

            // ── Call expression base: `$c->items()[0]->` ─────────
            // When the base before `[…]` is a call expression (ends
            // with `)`), extract it so that patterns like
            // `$c->items()[0]->` and `Collection::all()[0]->`
            // produce a subject that the resolver can handle.
            if pos > 0
                && chars[pos - 1] == ')'
                && let Some(call_base) = extract_call_subject(chars, pos)
            {
                segments.reverse();
                return format!("{}{}", call_base, segments.join(""));
            }

            // ── Inline array literal: `[expr][0]->` ──────────────
            // When there is no `$var` base and we consumed at least
            // two bracket pairs, the outermost (last-consumed) bracket
            // pair is the array literal itself.  Treat it as the base
            // and the remaining bracket pairs as index accesses.
            //
            // Example: `[Customer::first()][0]->`
            //   bracket_ranges (innermost-first): [(pos_of_[0], ...), (pos_of_[literal], ...)]
            //   segments  (innermost-first):       ["[]",              "[]"]
            //   → base = "[Customer::first()]", index segments = ["[]"]
            if segments.len() >= 2 {
                // The last entry in bracket_ranges is the outermost
                // (leftmost) bracket pair — the array literal.
                let (lit_open, lit_close) = bracket_ranges[bracket_ranges.len() - 1];
                let literal: String = chars[lit_open..lit_close].iter().collect();

                // The remaining segments (all but the last) are the
                // index accesses, in innermost-first order → reverse.
                let mut index_segs: Vec<String> = segments[..segments.len() - 1].to_vec();
                index_segs.reverse();
                return format!("{}{}", literal, index_segs.join(""));
            }
        }
    }

    // ── Function / method call or `new` expression: detect `)` ──
    // e.g. `app()->`, `$this->getService()->`, `Class::make()->`,
    //      `new Foo()->`, `(new Foo())->`
    if i > 0
        && chars[i - 1] == ')'
        && let Some(call_subject) = extract_call_subject(chars, i)
    {
        return call_subject;
    }

    // Try to read an identifier (property name if chained)
    let ident_end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }

    // Include `$` prefix for static property access (e.g. `self::$instance->`)
    // so the `::` check below sees `::$instance` instead of just `instance`.
    if i > 0 && chars[i - 1] == '$' {
        i -= 1;
    }

    let ident_start = i;

    // Check whether this identifier is preceded by another `->` (chained access)
    if i >= 2 && chars[i - 2] == '-' && chars[i - 1] == '>' {
        // We have something like  `expr->ident->` — recursively extract
        // the full chain so that `$this->a->b->` produces `$this->a->b`.
        let inner_arrow = i - 2;
        let inner_subject = extract_arrow_subject(chars, inner_arrow);
        if !inner_subject.is_empty() {
            let prop: String = chars[ident_start..ident_end].iter().collect();
            return format!("{}->{}", inner_subject, prop);
        }
    }

    // Check if preceded by `?->` (null-safe)
    if i >= 3 && chars[i - 3] == '?' && chars[i - 2] == '-' && chars[i - 1] == '>' {
        let inner_arrow = i - 3;
        let inner_subject = extract_arrow_subject(chars, inner_arrow);
        if !inner_subject.is_empty() {
            let prop: String = chars[ident_start..ident_end].iter().collect();
            return format!("{}?->{}", inner_subject, prop);
        }
    }

    // Check if preceded by `::` (enum case or static member access,
    // e.g. `Status::Active->`)
    if i >= 2 && chars[i - 2] == ':' && chars[i - 1] == ':' {
        let class_subject = extract_double_colon_subject(chars, i - 2);
        if !class_subject.is_empty() {
            let ident: String = chars[ident_start..ident_end].iter().collect();
            return format!("{}::{}", class_subject, ident);
        }
    }

    // Otherwise treat the whole thing as a simple variable like `$this` or `$var`
    extract_simple_variable(chars, end)
}

/// Extract the full call-expression subject when `)` appears before an
/// operator.
///
/// `paren_end` is the position one past the closing `)`.
///
/// Returns subjects such as:
///   - `"app()"` for a standalone function call without arguments
///   - `"app(A::class)"` for a function call with arguments (preserved)
///   - `"$this->getService()"` for an instance method call
///   - `"ClassName::make()"` for a static method call
///   - `"ClassName::make(Arg::class)"` for a static call with arguments
///   - `"ClassName"` for `new ClassName()` instantiation
fn extract_call_subject(chars: &[char], paren_end: usize) -> Option<String> {
    let open = skip_balanced_parens_back(chars, paren_end)?;

    // Capture the argument text between the parentheses for later use
    // in conditional return-type resolution (e.g. `app(A::class)`).
    let args_text: String = chars[open + 1..paren_end - 1].iter().collect();
    let args_text = args_text.trim();

    // Read the function / method name before `(`
    let mut i = open;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\') {
        i -= 1;
    }
    // Include the `$` prefix for variable function calls (`$fn()`,
    // `$callback()`, etc.) so that the resolver can distinguish them
    // from named function calls.
    if i > 0 && chars[i - 1] == '$' {
        i -= 1;
    }
    if i == open {
        // No identifier before `(` — check if the contents inside the
        // balanced parens form a `(new ClassName(...))` expression.
        if let Some(new_expr) = extract_new_expression_inside_parens(chars, open, paren_end) {
            return Some(new_expr);
        }

        // `(clone $expr)` — clone preserves the type of the operand,
        // so extract the inner expression as the subject.
        if let Some(clone_inner) = extract_clone_expression_inside_parens(chars, open, paren_end) {
            return Some(clone_inner);
        }

        // ── Parenthesized expression invocation: `(expr)()` ─────
        // When a balanced `(…)` group immediately precedes the call
        // parens, the inner expression is the callee (e.g.
        // `($this->formatter)()` invokes __invoke() on the property).
        if open > 0
            && chars[open - 1] == ')'
            && let Some(inner_open) = skip_balanced_parens_back(chars, open)
        {
            let inner: String = chars[inner_open + 1..open - 1].iter().collect();
            let inner = inner.trim();
            if !inner.is_empty() {
                return Some(format!("({})({})", inner, args_text));
            }
        }

        return None;
    }
    let func_name: String = chars[i..open].iter().collect();

    // ── `new ClassName()` instantiation ──
    // Check if the `new` keyword immediately precedes the class name.
    if let Some(class_name) = check_new_keyword_before(chars, i, &func_name) {
        // Preserve constructor args so that template inference can use them.
        if args_text.is_empty() {
            return Some(format!("new {}", class_name));
        } else {
            return Some(format!("new {}({})", class_name, args_text));
        }
    }

    // Build the right-hand side of the call expression, preserving
    // arguments for conditional return-type resolution.
    let rhs = if args_text.is_empty() {
        format!("{}()", func_name)
    } else {
        format!("{}({})", func_name, args_text)
    };

    // Check what precedes the function name to determine the kind of
    // call expression.

    // Null-safe method call: `$var?->method()`.  Checked before the plain
    // `->` arm because that arm's two-character match (`-` then `>`) also
    // matches `?->`; handling it here preserves the operator and recurses
    // through the full receiver instead of collapsing it to a bare name.
    if i >= 3 && chars[i - 3] == '?' && chars[i - 2] == '-' && chars[i - 1] == '>' {
        // The receiver may itself be a call expression ending with `)` —
        // e.g. `make()?->method(...)` — so recurse the same way the plain
        // `->` arm does.
        let mut j = i - 3;
        while j > 0 && chars[j - 1] == ' ' {
            j -= 1;
        }
        if j > 0
            && chars[j - 1] == ')'
            && let Some(inner_call) = extract_call_subject(chars, j)
        {
            return Some(format!("{}?->{}", inner_call, rhs));
        }
        // Use `extract_arrow_subject` (which skips the leading `?`) so that
        // property/call chains like `$a->b?->c()` are fully captured as
        // `$a->b?->c()` rather than the bare identifier `b?->c()`.
        let inner_subject = extract_arrow_subject(chars, i - 3);
        if !inner_subject.is_empty() {
            return Some(format!("{}?->{}", inner_subject, rhs));
        }
    }

    // Instance method call: `$this->method()` / `$var->method()` /
    // `app()->method()` (chained call expression)
    if i >= 2 && chars[i - 2] == '-' && chars[i - 1] == '>' {
        // First check if the LHS is itself a call expression ending
        // with `)` — e.g. `app()->make(...)` where we need to
        // recursively resolve `app()`.
        let arrow_pos = i - 2;
        let mut j = arrow_pos;
        while j > 0 && chars[j - 1] == ' ' {
            j -= 1;
        }
        if j > 0
            && chars[j - 1] == ')'
            && let Some(inner_call) = extract_call_subject(chars, j)
        {
            return Some(format!("{}->{}", inner_call, rhs));
        }
        // Use `extract_arrow_subject` instead of `extract_simple_variable`
        // so that property chains like `$this->users->first()` are fully
        // captured as `$this->users->first()` rather than `users->first()`.
        let inner_subject = extract_arrow_subject(chars, arrow_pos);
        if !inner_subject.is_empty() {
            return Some(format!("{}->{}", inner_subject, rhs));
        }
    }

    // Static method call: `ClassName::method()` / `self::method()`
    if i >= 2 && chars[i - 2] == ':' && chars[i - 1] == ':' {
        let class_subject = extract_double_colon_subject(chars, i - 2);
        if !class_subject.is_empty() {
            return Some(format!("{}::{}", class_subject, rhs));
        }
    }

    // Standalone function call: preserve arguments for conditional
    // return-type resolution (e.g. `app(A::class)` instead of `app()`).
    Some(rhs)
}

/// Extract a simple `$variable` or bare identifier ending at position
/// `end` (exclusive).
///
/// Skips trailing whitespace, then walks backwards through identifier
/// characters.  If a `$` prefix is found, includes it (producing e.g.
/// `"$this"`, `"$var"`).  Otherwise returns whatever identifier was
/// collected (e.g. `"self"`, `"parent"`), which may be empty.
fn extract_simple_variable(chars: &[char], end: usize) -> String {
    let mut i = end;
    // skip whitespace
    while i > 0 && chars[i - 1] == ' ' {
        i -= 1;
    }
    let var_end = i;
    // walk back through identifier chars
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }
    // expect `$` prefix
    if i > 0 && chars[i - 1] == '$' {
        i -= 1;
        chars[i..var_end].iter().collect()
    } else {
        // no `$` — return whatever we collected (may be empty)
        chars[i..var_end].iter().collect()
    }
}

/// Extract the identifier/keyword before `::`.
///
/// `colon_pos` is the index of the first `:` (i.e. `chars[colon_pos] == ':'`
/// and `chars[colon_pos + 1] == ':'`).
///
/// Handles `self::`, `static::`, `parent::`, `ClassName::`, `Foo\Bar::`,
/// and the edge case `$var::`.
fn extract_double_colon_subject(chars: &[char], colon_pos: usize) -> String {
    let mut i = colon_pos;
    // skip whitespace
    while i > 0 && chars[i - 1] == ' ' {
        i -= 1;
    }
    let end = i;
    // walk back through identifier chars (including `\` for namespaces)
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\') {
        i -= 1;
    }
    // Also accept `$` prefix for `$var::` edge case (variable class name)
    if i > 0 && chars[i - 1] == '$' {
        i -= 1;
    }

    // ── Mixed accessor: `$obj->prop::` or `$obj?->prop::` ─────────
    // When the identifier is preceded by `->` or `?->`, the subject is
    // an arrow-chain expression, not a plain class name.  Delegate to
    // `extract_arrow_subject` so the full chain is captured (e.g.
    // `$foobar->me` for `$foobar->me::`).
    if i >= 2 && chars[i - 2] == '-' && chars[i - 1] == '>' {
        let prop: String = chars[i..end].iter().collect();
        let inner = extract_arrow_subject(chars, i - 2);
        if !inner.is_empty() {
            return format!("{}->{}", inner, prop);
        }
    }
    if i >= 3 && chars[i - 3] == '?' && chars[i - 2] == '-' && chars[i - 1] == '>' {
        let prop: String = chars[i..end].iter().collect();
        let inner = extract_arrow_subject(chars, i - 3);
        if !inner.is_empty() {
            return format!("{}?->{}", inner, prop);
        }
    }

    chars[i..end].iter().collect()
}

/// Detect an access operator (`->`, `?->`, or `::`) before a cursor
/// position in an already-collapsed character slice, and extract the
/// subject expression to the operator's left.
///
/// The cursor is expected to sit *after* the operator, possibly with a
/// partial identifier already typed (used by completion).
///
/// # Parameters
///
/// * `chars` — the collapsed line as a char slice.
/// * `col` — the cursor's character offset within `chars`.
///
/// # Returns
///
/// `Some((subject, AccessKind))` when an operator is found, `None`
/// otherwise.
pub(crate) fn detect_access_operator(chars: &[char], col: usize) -> Option<(String, AccessKind)> {
    let col = col.min(chars.len());

    if chars.is_empty() {
        return None;
    }

    // Walk backwards past any partial identifier the user has typed,
    // then skip whitespace so that `->   ` (operator followed by
    // spaces but no identifier yet) is still detected.
    let operator_end = {
        let mut i = col;
        while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            i -= 1;
        }
        // Skip `$` prefix for partially typed static properties
        // (e.g. `Foo::$f` — the `$` is the property sigil, not part
        // of the operator).
        if i > 0 && chars[i - 1] == '$' {
            i -= 1;
        }
        while i > 0 && chars[i - 1] == ' ' {
            i -= 1;
        }
        i
    };

    // Try `::`.
    if operator_end >= 2 && chars[operator_end - 2] == ':' && chars[operator_end - 1] == ':' {
        let subject = extract_double_colon_subject(chars, operator_end - 2);
        if !subject.is_empty() {
            return Some((subject, AccessKind::DoubleColon));
        }
    }

    // Try `->`.
    if operator_end >= 2 && chars[operator_end - 2] == '-' && chars[operator_end - 1] == '>' {
        let subject = extract_arrow_subject(chars, operator_end - 2);
        if !subject.is_empty() {
            return Some((subject, AccessKind::Arrow));
        }
    }

    // Try `?->` (null-safe operator).
    if operator_end >= 3
        && chars[operator_end - 3] == '?'
        && chars[operator_end - 2] == '-'
        && chars[operator_end - 1] == '>'
    {
        let subject = extract_arrow_subject(chars, operator_end - 3);
        if !subject.is_empty() {
            return Some((subject, AccessKind::Arrow));
        }
    }

    None
}

#[cfg(test)]
#[path = "subject_extraction_tests.rs"]
mod tests;
