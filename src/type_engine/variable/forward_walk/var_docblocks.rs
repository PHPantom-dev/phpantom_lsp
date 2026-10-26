//! `@var` docblock handling for the forward walker.
//!
//! Covers both the docblock that precedes an assignment and the
//! standalone one that annotates a variable without assigning to it.

use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::types::ResolvedType;

use crate::parser::{CommentKind, with_comments};

/// Visit the docblocks stacked directly above `offset`, nearest first,
/// until `visit` returns `false`.
///
/// The docblocks of a stack are separated only by whitespace and other
/// comments; any code ends it, since a `@var` above code that has run since
/// may no longer hold.  A preprocessed Blade template puts a plain comment
/// between a template's `@var` block and the code it annotates for every
/// `{{-- --}}` comment and every component tag.
pub(crate) fn for_each_docblock_above<'c>(
    content: &'c str,
    offset: usize,
    mut visit: impl FnMut(&'c str) -> bool,
) {
    if !may_end_with_comment(content, offset) {
        return;
    }
    with_comments(content, |comments| {
        let bytes = content.as_bytes();
        let mut covered_from = offset.min(content.len());
        let above = comments.partition_point(|c| c.end as usize <= covered_from);
        for comment in comments[..above].iter().rev() {
            let gap = bytes.get(comment.end as usize..covered_from).unwrap_or(&[]);
            if !gap.iter().all(u8::is_ascii_whitespace) {
                return;
            }
            if comment.kind == CommentKind::Doc {
                let Some(doc) = content.get(comment.start as usize..comment.end as usize) else {
                    return;
                };
                if !visit(doc) {
                    return;
                }
            }
            covered_from = comment.start as usize;
        }
    });
}

/// Whether the text before `offset` can end with a comment, judged from
/// the text alone so that the many statements with none above them skip
/// the comment lookup.
fn may_end_with_comment(content: &str, offset: usize) -> bool {
    let Some(before) = content.get(..offset) else {
        return false;
    };
    let before = before.trim_end();
    if before.ends_with("*/") {
        return true;
    }
    // Otherwise only a line comment can, which the last line would hold.
    let window = before.len().saturating_sub(MAX_LINE_LOOKBACK);
    let tail = &before.as_bytes()[window..];
    match tail.iter().rposition(|&b| b == b'\n') {
        Some(newline) => {
            let line = &tail[newline + 1..];
            line.contains(&b'#') || line.windows(2).any(|pair| pair == b"//")
        }
        // A line too long to look through: let the comment index decide.
        None => window > 0 || tail.contains(&b'#') || tail.windows(2).any(|pair| pair == b"//"),
    }
}

/// How far back [`may_end_with_comment`] looks for the start of the last
/// line before giving the question to the comment index.
const MAX_LINE_LOOKBACK: usize = 512;

/// The docblocks stacked directly above `offset` that can carry a `@var`
/// tag, nearest first (see [`for_each_docblock_above`]).  Every spelling
/// of the tag ends in `var`, so a docblock without it (an empty one a
/// template leaves behind, a `@return` above a closure) is passed over.
fn docblocks_above(content: &str, offset: usize) -> Vec<&str> {
    let mut docs = Vec::new();
    for_each_docblock_above(content, offset, |doc| {
        if doc.contains("var") {
            docs.push(doc);
        }
        true
    });
    docs
}

/// The docblock directly above `offset` (see [`for_each_docblock_above`]).
pub(crate) fn docblock_above(content: &str, offset: usize) -> Option<&str> {
    let mut found = None;
    for_each_docblock_above(content, offset, |doc| {
        found = Some(doc);
        false
    });
    found
}

/// The docblock whose closing `*/` ends exactly at byte `end`.
pub(crate) fn docblock_ending_at(content: &str, end: usize) -> Option<&str> {
    let (start, end) = with_comments(content, |comments| {
        let at = comments.partition_point(|c| (c.end as usize) < end);
        comments
            .get(at)
            .filter(|c| c.end as usize == end && c.kind == CommentKind::Doc)
            .map(|c| (c.start as usize, c.end as usize))
    })?;
    content.get(start..end)
}

/// Try to process an inline `/** @var Type $x */` docblock override.
pub(crate) fn try_process_inline_var_override<'b>(
    expr: &'b Expression<'b>,
    expr_offset: u32,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> VarOverrideResult {
    // Parse the inline @var docblock at this expression's position.
    let offset = expr_offset as usize;
    if offset == 0 {
        return VarOverrideResult::None;
    }

    // Look for `/** @var Type $varName */` before this expression.
    let docs = docblocks_above(ctx.file.content, offset);
    let Some((&doc_text, further)) = docs.split_first() else {
        return VarOverrideResult::None;
    };

    // Try multi-@var first: a single docblock may declare several
    // variables (e.g. `/** @var App $app  @var array{…} $params */`).
    let multi = parse_var_docblock_pairs(doc_text);
    if !multi.is_empty() {
        // A name this docblock declares carries the type written closest
        // to the expression, so the blocks further back skip it.
        let mut declared: Vec<String> = multi.iter().map(|(name, _)| name.clone()).collect();
        apply_var_docblock_stack(further, scope, ctx, |_| true, &mut declared);
        // When the cursor is inside the RHS of an assignment, skip
        // overriding the LHS variable so that hover/completion on the
        // RHS sees the pre-override type.  E.g.:
        //   /** @var array<string, mixed> $response */
        //   $response = $response->json();
        // Hovering on the RHS `$response` should show `ApiResponse`,
        // not `array<string, mixed>`.
        let skip_var: Option<String> = if let Expression::Assignment(assignment) = expr {
            let rhs_span = assignment.rhs.span();
            let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                && ctx.cursor_offset <= rhs_span.end.offset;
            if cursor_in_rhs {
                if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    Some(bytes_to_str(dv.name).to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        for (var_name, php_type) in &multi {
            if skip_var.as_deref() == Some(var_name.as_str()) {
                continue;
            }
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
        }

        // A destructuring still hands every variable no tag names its
        // element of the value; the ones a tag names keep the tag's type.
        if let Expression::Assignment(assignment) = expr
            && matches!(assignment.lhs, Expression::Array(_) | Expression::List(_))
        {
            let annotated: Vec<(&str, Vec<ResolvedType>)> = declared
                .iter()
                .map(|name| (name.as_str(), scope.get(name).to_vec()))
                .collect();
            process_destructuring_assignment(assignment, scope, ctx);
            for (name, types) in annotated {
                scope.set(name, types);
            }
        }

        // When the @var variable names all differ from the assignment
        // LHS, return None so the caller continues processing the
        // assignment.  E.g.:
        //   /** @var Foo[] $items */
        //   $item = array_shift($items);
        // The @var sets `$items` in scope (done above), and the caller
        // must also process `$item = array_shift($items)`.
        //
        // When any @var name in the stack of docblocks matches the LHS,
        // return NamedVar so the caller skips the assignment (the @var
        // type is authoritative).
        if let Expression::Assignment(assignment) = expr
            && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
        {
            let lhs_name = bytes_to_str(dv.name);
            if !declared.iter().any(|n| n == lhs_name) {
                return VarOverrideResult::None;
            }
        }
        return VarOverrideResult::NamedVar;
    }

    // Also check for `/** @var Type */` without variable name — this
    // applies to the immediately following expression if it's a simple
    // variable or assignment.
    if let Some(php_type) = parse_inline_var_docblock_no_var(doc_text) {
        let resolved = resolve_type_to_resolved_types(&php_type, ctx);
        if let Expression::Assignment(assignment) = expr {
            if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                // When the cursor is inside the RHS, skip the override
                // so that the variable retains its pre-assignment type.
                // E.g. `/** @var array<string, mixed> */ $data = $data->toArray()`
                // — the cursor on `$data->` in the RHS should see Data, not array.
                let rhs_span = assignment.rhs.span();
                let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                    && ctx.cursor_offset <= rhs_span.end.offset;
                if cursor_in_rhs {
                    return VarOverrideResult::None;
                }

                // Scalar-blocking: when the RHS resolves to a concrete
                // scalar type (string, int, bool, etc.), reject a class
                // `@var` override.  E.g. `/** @var Session */ $s =
                // $this->getName()` where `getName()` returns `string`
                // should NOT override `$s` to `Session`.
                let native_type = resolve_rhs_native_type(assignment.rhs, scope, ctx);
                if let Some(ref native) = native_type
                    && !crate::docblock::should_override_type_typed(&php_type, native)
                {
                    // The override was rejected (scalar blocking).
                    return VarOverrideResult::None;
                }

                let var_name = bytes_to_str(dv.name).to_string();
                // Scan for preceding docblocks first, so this one wins.
                apply_preceding_var_docblocks(further, scope, ctx);
                scope.set(&var_name, resolved);
                return VarOverrideResult::NoVar;
            } else if let Expression::ArrayAccess(array_access) = assignment.lhs {
                // Same rules as the plain-variable case above, applied to
                // an array-element target:
                // `/** @var string */ $GLOBALS['sql_query'] = rand(0, 1) ? 'asd' : null;`
                let rhs_span = assignment.rhs.span();
                let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                    && ctx.cursor_offset <= rhs_span.end.offset;
                if cursor_in_rhs {
                    return VarOverrideResult::None;
                }

                let native_type = resolve_rhs_native_type(assignment.rhs, scope, ctx);
                if let Some(ref native) = native_type
                    && !crate::docblock::should_override_type_typed(&php_type, native)
                {
                    return VarOverrideResult::None;
                }

                apply_preceding_var_docblocks(further, scope, ctx);
                process_array_key_write(array_access, resolved, scope, ctx);
                return VarOverrideResult::NoVar;
            }
        } else if let Expression::Variable(Variable::Direct(dv)) = expr {
            let var_name = bytes_to_str(dv.name).to_string();
            apply_preceding_var_docblocks(further, scope, ctx);
            scope.set(&var_name, resolved);
            return VarOverrideResult::NoVar;
        }
    }

    VarOverrideResult::None
}

/// Apply the `@var` tags of `docs`, a stack of docblocks nearest first
/// (see [`for_each_docblock_above`]), to `scope`.
///
/// Returns whether any `@var` annotation was applied, so callers that
/// recorded a diagnostic scope snapshot before this call know to
/// re-record it afterward.
pub(crate) fn apply_preceding_var_docblocks(
    docs: &[&str],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    apply_var_docblock_stack(docs, scope, ctx, |_| true, &mut Vec::new())
}

/// [`apply_preceding_var_docblocks`], restricted to the variables `applies`
/// accepts.
///
/// Every name applied is pushed onto `declared`.  The blocks are visited
/// nearest first, so a name already in `declared` (from a nearer block, or
/// seeded by the caller) is not overwritten by a further one that repeats
/// it.
fn apply_var_docblock_stack(
    docs: &[&str],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    applies: impl Fn(&str) -> bool,
    declared: &mut Vec<String>,
) -> bool {
    let mut applied = false;
    for doc_text in docs {
        let vars = parse_var_docblock_pairs(doc_text);
        for (var_name, php_type) in &vars {
            if !applies(var_name) || declared.iter().any(|seen| seen == var_name) {
                continue;
            }
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
            declared.push(var_name.clone());
            applied = true;
        }
    }
    applied
}

/// Apply `/** @var Type $var */` docblocks that precede a statement
/// without being attached to an assignment.
///
/// [`try_process_inline_var_override`] covers the assignment case
/// (`/** @var Foo $x */ $x = …`); this covers annotations that stand on
/// their own, such as the `@var` block a Blade template opens with or a
/// docblock written above an `if`.  Without it the variable never enters
/// the walker's scope and every use of it falls back to a backward text
/// scan, which cannot tell a preceding sibling block from a preceding
/// sibling function body.
///
/// Returns whether any `@var` annotation was applied.
pub(crate) fn apply_standalone_var_docblocks(
    stmt_offset: u32,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let offset = (stmt_offset as usize).min(ctx.file.content.len());
    if offset == 0 {
        return false;
    }
    apply_preceding_var_docblocks(&docblocks_above(ctx.file.content, offset), scope, ctx)
}

/// Apply the `@var` docblocks above the code at the cursor when the parser
/// dropped that code's statement, as it does with one still being typed
/// (`$x->` at the end of a file).  The walk never reaches such a statement,
/// so the docblocks written above it would otherwise go unread.
///
/// A statement the parser drops can take the control structures around it
/// down too (`foreach ($xs as $x) { $x-> }`), so the docblocks above each of
/// those apply as well, outermost first.
///
/// Returns whether any `@var` annotation was applied.
pub(crate) fn apply_var_docblocks_above_unparsed_cursor(
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let cursor = ctx.cursor_offset as usize;
    let starts = unparsed_statement_starts(ctx.file.content, cursor);
    let mut applied = false;
    for &start in starts.iter().rev().filter(|&&start| start <= cursor) {
        applied |=
            apply_preceding_var_docblocks(&docblocks_above(ctx.file.content, start), scope, ctx);
    }
    applied
}

/// Where the statement holding `cursor` starts, judged from the text, and
/// then where each control structure around it starts, innermost first.
///
/// A statement starts just past the nearest `;`, `}` or unclosed `{` before
/// it that no bracket, string or comment encloses.  The climb continues
/// through an unclosed `{` only while it opens the body of a control
/// structure; the body of a function or class is where the walk itself
/// started.
fn unparsed_statement_starts(content: &str, cursor: usize) -> Vec<usize> {
    let bytes = content.as_bytes();
    with_comments(content, |comments| {
        let mut starts = Vec::new();
        let mut before = comments.partition_point(|c| c.end as usize <= cursor.min(bytes.len()));
        let mut depth: u32 = 0;
        let mut i = cursor.min(bytes.len());
        while i > 0 {
            if before > 0 && comments[before - 1].end as usize == i {
                before -= 1;
                i = comments[before].start as usize;
                continue;
            }
            i -= 1;
            match bytes[i] {
                quote @ (b'\'' | b'"') => {
                    while i > 0 {
                        i -= 1;
                        if bytes[i] == quote && !crate::text_scan::is_escaped(bytes, i) {
                            break;
                        }
                    }
                }
                b')' | b']' => depth += 1,
                b'(' | b'[' => depth = depth.saturating_sub(1),
                b'}' if depth > 0 => depth += 1,
                b'{' if depth > 0 => depth -= 1,
                b'{' => {
                    starts.push(crate::text_scan::skip_php_trivia(bytes, i + 1));
                    if !opens_control_structure_body(&content[..i]) {
                        return starts;
                    }
                }
                b';' | b'}' if depth == 0 => {
                    starts.push(crate::text_scan::skip_php_trivia(bytes, i + 1));
                    return starts;
                }
                _ => {}
            }
        }
        starts
    })
}

/// Whether a `{` written after `before` opens the body of a control
/// structure: it follows `else`, `try`, `do` or `finally`, or the
/// parenthesised head of `if`, `elseif`, `foreach`, `for`, `while`,
/// `switch` or `catch`.
fn opens_control_structure_body(before: &str) -> bool {
    use crate::text_scan::keyword_ends_at;
    let before_text = before.trim_end();
    let before = before_text.as_bytes();
    let end = before.len();
    if ["else", "try", "do", "finally"]
        .iter()
        .any(|kw| keyword_ends_at(before, end, kw.as_bytes()))
    {
        return true;
    }
    if end == 0 || before[end - 1] != b')' {
        return false;
    }
    let Some(open) = crate::text_scan::find_matching_backward(before_text, end - 1, b'(', b')')
    else {
        return false;
    };
    let head = before[..open].trim_ascii_end();
    ["if", "elseif", "foreach", "for", "while", "switch", "catch"]
        .iter()
        .any(|kw| keyword_ends_at(head, head.len(), kw.as_bytes()))
}

/// Apply the `@var` docblocks above a `global` statement to the variables
/// it imports.
///
/// A named `@var` types only a variable the statement imports; one naming
/// anything else is not about this statement.  A nameless `@var` is
/// unambiguous only when a single variable is imported.
///
/// Returns whether any `@var` annotation was applied.
pub(crate) fn apply_global_var_docblocks(
    stmt_offset: u32,
    imported: &[&str],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let offset = (stmt_offset as usize).min(ctx.file.content.len());
    if offset == 0 {
        return false;
    }
    let docs = docblocks_above(ctx.file.content, offset);
    if let [only] = imported
        && let Some(php_type) = docs
            .first()
            .and_then(|doc| parse_inline_var_docblock_no_var(doc))
    {
        let resolved = resolve_type_to_resolved_types(&php_type, ctx);
        scope.set(only, resolved);
        return true;
    }
    apply_var_docblock_stack(
        &docs,
        scope,
        ctx,
        |name| imported.contains(&name),
        &mut Vec::new(),
    )
}

/// Look up a standalone `/** @var Type */` docblock (no variable name)
/// immediately preceding the statement starting at `stmt_start`.
///
/// This casts the type of that statement's expression outright — used for
/// a `return` statement, where PHPStan treats the annotation as
/// authoritative rather than as a refinement.  That is stricter than the
/// same annotation above an assignment ([`try_process_inline_var_override`]'s
/// `NoVar` case), which only overrides when the override is a genuine
/// refinement of the RHS's native type.
pub(crate) fn find_preceding_nameless_var_cast(
    content: &str,
    stmt_start: usize,
) -> Option<PhpType> {
    let offset = stmt_start.min(content.len());
    if offset == 0 {
        return None;
    }
    parse_inline_var_docblock_no_var(docblock_above(content, offset)?)
}

/// The type the `@var` docblock above the statement at `stmt_start` gives
/// the value an assignment reads: a nameless tag, or one naming `value_var`.
///
/// A tag naming any other variable describes that variable, not the value,
/// so `@var Scope[] $map` above `[$map, $hashes] = $cached` says nothing
/// about `$cached`.
pub(crate) fn preceding_var_type_of_value(
    content: &str,
    stmt_start: usize,
    value_var: Option<&str>,
) -> Option<PhpType> {
    let doc_text = docblock_above(content, stmt_start)?;
    if let Some(value_var) = value_var
        && let Some((_, php_type)) = parse_var_docblock_pairs(doc_text)
            .into_iter()
            .find(|(name, _)| name == value_var)
    {
        return Some(php_type);
    }
    parse_inline_var_docblock_no_var(doc_text)
}

/// Parse ALL `@var Type $varName` pairs from a docblock.  Returns an
/// empty vec when none are found.  Handles multi-line docblocks with one
/// annotation per line as well as a single annotation whose type spans
/// several lines:
/// ```text
/// /**
///  * @var App                      $app
///  * @var array{indexName: string} $params
///  */
/// ```
/// ```text
/// /**
///  * @var array{
///  *     Label,
///  *     Stmt,
///  * } $pair
///  */
/// ```
///
/// The vendor spellings `@phpstan-var` and `@psalm-var` are read too, and
/// win over a plain `@var` naming the same variable.
pub(crate) fn parse_var_docblock_pairs(doc_text: &str) -> Vec<(String, PhpType)> {
    // Every spelling of the tag ends in `var`; skip the docblock parse for
    // the many docblocks that have none.
    if !doc_text.contains("var") {
        return vec![];
    }
    let Some(info) = crate::docblock::parse_docblock_for_tags(doc_text) else {
        return vec![];
    };
    let mut results: Vec<(String, PhpType)> = Vec::new();
    for tag in info.tags_by_kind_vendor_first(crate::docblock::TagKind::Var) {
        let (Some(var_name), Some(type_text)) = (tag.variable(), tag.type_text()) else {
            continue;
        };
        if results.iter().any(|(seen, _)| *seen == var_name) {
            continue;
        }
        if let Some(php_type) = crate::docblock::sanitise_and_parse_docblock_type(&type_text) {
            results.push((var_name.into_owned(), php_type));
        }
    }
    results
}

/// Parse `/** @var Type */` (without variable name) and return the PhpType.
///
/// Reads the highest-precedence spelling of the tag (`@phpstan-var`, then
/// `@psalm-var`, then `@var`); a docblock whose tag names a variable is
/// not the no-var form.
pub(crate) fn parse_inline_var_docblock_no_var(doc_text: &str) -> Option<PhpType> {
    if !doc_text.contains("var") {
        return None;
    }
    let (php_type, var_name) = crate::docblock::extract_var_type_with_name(doc_text)?;
    if var_name.is_some() {
        return None;
    }
    Some(php_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn above_end(content: &str) -> Vec<&str> {
        docblocks_above(content, content.len())
    }

    #[test]
    fn line_comments_are_stepped_over_to_reach_a_docblock() {
        assert_eq!(
            above_end("<?php\n/** @var Foo $x */\n// short\n"),
            ["/** @var Foo $x */"]
        );
        assert_eq!(
            above_end("<?php\n/** @var Foo $x */\n\n# short\n\n"),
            ["/** @var Foo $x */"]
        );
        assert_eq!(
            above_end("<?php\n/** @var Foo $x */ // trailing\n"),
            ["/** @var Foo $x */"]
        );
        // Code in between detaches the docblock, even when a comment
        // follows the code on its line.
        assert!(above_end("<?php\n/** @var Foo $x */\n$url = 'https://x'; // c\n").is_empty());
        // `#[` opens an attribute, not a comment.
        assert!(above_end("<?php\n/** @var Foo $x */\n#[Attr]\n").is_empty());
    }

    #[test]
    fn only_a_docblock_continues_the_stack() {
        assert_eq!(
            above_end("<?php\n/** @var A $a */\n/** @var B $b */\n"),
            ["/** @var B $b */", "/** @var A $a */"]
        );
        // Plain comments are stepped over; code is where the search stops.
        assert_eq!(
            above_end("<?php\n/** @var Foo $x */\n/* x-component */\n"),
            ["/** @var Foo $x */"]
        );
        assert!(above_end("<?php\n/** @var Foo $x */ $x = 1; /* note */\n").is_empty());
        assert!(above_end("<?php\n$a = 1;").is_empty());
        // A `/*` inside a string is not a comment.
        assert_eq!(
            above_end("<?php\n$f = glob('src/*.php');\n/** @var Foo $x */\n"),
            ["/** @var Foo $x */"]
        );
    }

    #[test]
    fn var_docblock_with_closure_signature_binds_the_trailing_variable() {
        // A closure type's own `$`-prefixed parameter name must not be
        // mistaken for the variable the `@var` tag annotates.
        let pairs = parse_var_docblock_pairs(
            "/** @var \\Closure(\\App\\Models\\User $user): string $callback */",
        );
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "$callback");
        assert_eq!(
            pairs[0].1.to_string(),
            "\\Closure(\\App\\Models\\User): string"
        );
    }

    #[test]
    fn vendor_var_tags_are_read_and_win_over_plain_var() {
        let pairs = parse_var_docblock_pairs(
            "/**\n * @var array $res\n * @phpstan-var array<Foo> $res\n * @psalm-var int $n\n */",
        );
        let types: Vec<(String, String)> = pairs
            .into_iter()
            .map(|(name, ty)| (name, ty.to_string()))
            .collect();
        assert_eq!(
            types,
            vec![
                ("$res".to_string(), "array<Foo>".to_string()),
                ("$n".to_string(), "int".to_string()),
            ]
        );

        assert_eq!(
            parse_inline_var_docblock_no_var("/**\n * @var array\n * @psalm-var list<int>\n */")
                .map(|ty| ty.to_string()),
            Some("list<int>".to_string())
        );
        assert_eq!(
            parse_inline_var_docblock_no_var("/** @phpstan-var Foo $x */"),
            None
        );
    }
}
