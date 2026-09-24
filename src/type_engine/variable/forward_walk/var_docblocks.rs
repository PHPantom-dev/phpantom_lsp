//! `@var` docblock handling for the forward walker.
//!
//! Covers both the docblock that precedes an assignment and the
//! standalone one that annotates a variable without assigning to it.

use super::*;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::docblock::type_strings::split_type_token;
use crate::php_type::PhpType;

pub(crate) fn preceding_docblock_text(content: &str, node_start: usize) -> Option<&str> {
    let before = content.get(..node_start)?;
    let doc_end = before.rfind("*/")? + 2;
    let between = &before[doc_end..];
    if between.contains(';') || between.contains('{') || between.contains('}') {
        return None;
    }
    let doc_start = before[..doc_end].rfind("/**")?;
    Some(&before[doc_start..doc_end])
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
    let before = &ctx.content[..offset.min(ctx.content.len())];
    let trimmed = trim_trailing_line_comments(before);

    let Some(doc_text) = trailing_docblock(trimmed) else {
        return VarOverrideResult::None;
    };
    let doc_start = trimmed.len() - doc_text.len();

    // Try multi-@var first: a single docblock may declare several
    // variables (e.g. `/** @var App $app  @var array{…} $params */`).
    let multi = parse_var_docblock_pairs(doc_text);
    if !multi.is_empty() {
        // The blocks further back run first, so a name this docblock also
        // declares ends up carrying the type written closest to the
        // expression.
        apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
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

        // When the @var variable names all differ from the assignment
        // LHS, return None so the caller continues processing the
        // assignment.  E.g.:
        //   /** @var Foo[] $items */
        //   $item = array_shift($items);
        // The @var sets `$items` in scope (done above), and the caller
        // must also process `$item = array_shift($items)`.
        //
        // When any @var name matches the LHS, return NamedVar so the
        // caller skips the assignment (the @var type is authoritative).
        if let Expression::Assignment(assignment) = expr
            && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
        {
            let lhs_name = bytes_to_str(dv.name).to_string();
            if !multi.iter().any(|(n, _)| *n == lhs_name) {
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
                apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
                scope.set(&var_name, resolved);
                return VarOverrideResult::NoVar;
            }
        } else if let Expression::Variable(Variable::Direct(dv)) = expr {
            let var_name = bytes_to_str(dv.name).to_string();
            apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
            scope.set(&var_name, resolved);
            return VarOverrideResult::NoVar;
        }
    }

    VarOverrideResult::None
}

/// Scan backwards through `before` (content before a docblock we already
/// processed) for additional standalone `/** @var Type $var */` blocks.
/// Each discovered block's `@var` tags are applied to `scope`.  Stops as
/// soon as the text no longer ends with `*/` (after trimming).
///
/// Returns whether any `@var` annotation was applied, so callers that
/// recorded a diagnostic scope snapshot before this call know to
/// re-record it afterward.
pub(crate) fn apply_preceding_var_docblocks(
    before: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let mut applied = false;
    let mut remaining = trim_trailing_line_comments(before);
    // The blocks are visited nearest first, so a name a nearer one already
    // declared is not overwritten by a further one that repeats it.
    let mut declared: Vec<String> = Vec::new();
    // Keep scanning as long as the preceding text ends with a docblock.
    while let Some(doc_text) = trailing_docblock(remaining) {
        let doc_start = remaining.len() - doc_text.len();
        let vars = parse_var_docblock_pairs(doc_text);
        if vars.is_empty() {
            // Not a @var docblock — stop scanning.
            break;
        }
        for (var_name, php_type) in &vars {
            if declared.iter().any(|seen| seen == var_name) {
                continue;
            }
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
            declared.push(var_name.clone());
        }
        applied = true;
        remaining = trim_trailing_line_comments(&remaining[..doc_start]);
    }
    applied
}

/// The `/** … */` docblock `text` ends with, or `None` when it ends with
/// something else.
///
/// The comment that ends the text opens at the first `/*` no earlier
/// comment closes.  An ordinary `/* … */` block is not a docblock, and
/// searching past it for the nearest `/**` would take everything in
/// between — including whole statements — for the docblock's body, and
/// read out of it a `@var` annotation that the code in between has
/// already superseded.  A preprocessed Blade template is full of such
/// blocks (every `{{-- --}}` comment and every component tag becomes
/// one), but so is any PHP file with a commented-out line between two
/// annotated assignments.
fn trailing_docblock(text: &str) -> Option<&str> {
    let body = text.strip_suffix("*/")?;
    let after_previous = body.rfind("*/").map_or(0, |pos| pos + 2);
    let start = after_previous + body[after_previous..].find("/*")?;
    text.get(start..).filter(|doc| doc.starts_with("/**"))
}

/// Trim trailing whitespace and whole-line `//` / `#` comments from
/// `before`, so that a `/** @var Type $var */` block separated from the
/// code it annotates by ordinary comment lines is still discovered.
///
/// Only comments that occupy a whole line are removed: a `//` following
/// code on the same line may well sit inside a string literal
/// (`$url = 'https://…';`), and `#[` opens an attribute rather than a
/// comment.
///
/// This runs once per statement the forward walker visits, so the search
/// for the start of the trailing line is capped at
/// [`MAX_COMMENT_LINE_LOOKBACK`] bytes.  Beyond that the line is far too
/// long to be a comment worth stepping over, and an unbounded scan would
/// be quadratic on a file written as one very long line.
pub(crate) fn trim_trailing_line_comments(before: &str) -> &str {
    let mut head = before.trim_end();
    loop {
        // A docblock ends the search: the caller wants exactly this.
        if head.ends_with("*/") {
            return head;
        }
        let line_start = match head
            .as_bytes()
            .iter()
            .rev()
            .take(MAX_COMMENT_LINE_LOOKBACK)
            .position(|&b| b == b'\n')
        {
            // `position` counts back from the end, so the byte after the
            // newline sits at `len - offset`.  That is a char boundary.
            Some(offset) => head.len() - offset,
            None if head.len() <= MAX_COMMENT_LINE_LOOKBACK => 0,
            None => return head,
        };
        let line = head[line_start..].trim_start();
        if line.starts_with("//") || (line.starts_with('#') && !line.starts_with("#[")) {
            head = head[..line_start].trim_end();
        } else {
            return head;
        }
    }
}

/// How far back [`trim_trailing_line_comments`] looks for the start of
/// the line it is inspecting.
const MAX_COMMENT_LINE_LOOKBACK: usize = 512;

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
    let offset = (stmt_offset as usize).min(ctx.content.len());
    if offset == 0 {
        return false;
    }
    apply_preceding_var_docblocks(&ctx.content[..offset], scope, ctx)
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
    let before = &content[..offset];
    let trimmed = trim_trailing_line_comments(before);
    let doc_text = trailing_docblock(trimmed)?;
    parse_inline_var_docblock_no_var(doc_text)
}

/// Strip the `/**`…`*/` wrapper from a docblock and collapse its
/// line-continuation markers into a single space-joined string.
///
/// This flattens type strings that span multiple lines (e.g. a
/// `array{...}` shape written across several ` * ` lines) so they can be
/// parsed as one token sequence instead of retaining the leading `*`
/// markers, which [`PhpType::parse`] cannot interpret.
pub(crate) fn flatten_docblock_inner(doc_text: &str) -> Option<String> {
    let inner = doc_text.strip_prefix("/**")?.strip_suffix("*/")?;
    Some(
        inner
            .lines()
            .map(|l| l.trim().trim_start_matches('*').trim())
            .collect::<Vec<_>>()
            .join(" "),
    )
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
pub(crate) fn parse_var_docblock_pairs(doc_text: &str) -> Vec<(String, PhpType)> {
    let inner = match flatten_docblock_inner(doc_text) {
        Some(s) => s,
        None => return vec![],
    };
    let inner = inner.as_str();

    let mut results = Vec::new();

    // Split on `@var` and process each occurrence.
    let mut search_from = 0;
    while let Some(pos) = inner[search_from..].find("@var") {
        let abs_pos = search_from + pos;
        let tag_start = abs_pos + 4;
        let after = inner[tag_start..].trim_start();
        let leading_ws = inner[tag_start..].len() - after.len();

        // Split off the type token, respecting `()`/`<>`/`{}` nesting, so a
        // closure signature's own `$`-prefixed parameter names (e.g.
        // `\Closure(\App\Models\User $user): string`) are not mistaken for
        // the variable this `@var` annotates.
        let (type_str, remainder) = split_type_token(after);
        if !type_str.is_empty()
            && let Some(var_name) = remainder.split_whitespace().next()
            && var_name.starts_with('$')
        {
            let php_type = PhpType::parse(type_str);
            results.push((var_name.to_string(), php_type));
        }

        search_from = tag_start + leading_ws + type_str.len();
    }

    results
}

/// Parse `/** @var Type */` (without variable name) and return the PhpType.
pub(crate) fn parse_inline_var_docblock_no_var(doc_text: &str) -> Option<PhpType> {
    // Flatten line-continuation markers so a `array{...}` shape spread
    // across several lines is parsed as one type string.
    let inner = flatten_docblock_inner(doc_text)?;
    let inner = inner.trim().strip_prefix("@var")?.trim();

    // Stop at the next docblock tag so trailing tags (e.g. `@psalm-suppress`)
    // do not corrupt the type string.
    let type_str = match inner.find(" @") {
        Some(pos) => inner[..pos].trim(),
        None => inner,
    };
    // Strip a trailing `*` that may remain from `* @var Type *` formatting.
    let type_str = type_str.trim_end_matches('*').trim();

    // If there's a `$` it has a variable name — not the no-var form.
    if type_str.contains('$') {
        return None;
    }

    if type_str.is_empty() {
        return None;
    }

    Some(PhpType::parse(type_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_line_comments_are_stepped_over_to_reach_a_docblock() {
        // Whole-line `//` and `#` comments separate the block from the
        // code but do not detach it.
        assert!(trim_trailing_line_comments("/** @var Foo $x */\n// short\n").ends_with("*/"));
        assert!(trim_trailing_line_comments("/** @var Foo $x */\n\n# short\n\n").ends_with("*/"));
        assert!(trim_trailing_line_comments("/** @var Foo $x */\n// a\n  // b\n").ends_with("*/"));

        // A `//` after code on the same line may be inside a string.
        assert_eq!(
            trim_trailing_line_comments("/** @var Foo $x */\n$url = 'https://example.test';"),
            "/** @var Foo $x */\n$url = 'https://example.test';"
        );

        // `#[` opens an attribute, not a comment.
        assert_eq!(
            trim_trailing_line_comments("/** @var Foo $x */\n#[Attr]"),
            "/** @var Foo $x */\n#[Attr]"
        );

        // A comment line longer than the look-back window is left alone
        // rather than triggering an unbounded backward scan.
        let long = format!("/** @var Foo $x */\n// {}\n", "x".repeat(600));
        assert!(!trim_trailing_line_comments(&long).ends_with("*/"));

        // Nothing but comments: the scan bottoms out at an empty string.
        assert_eq!(trim_trailing_line_comments("// only\n"), "");
        assert_eq!(trim_trailing_line_comments(""), "");
    }

    #[test]
    fn only_a_docblock_ends_the_backward_search_for_one() {
        assert_eq!(
            trailing_docblock("$a = 1; /** @var Foo $x */"),
            Some("/** @var Foo $x */")
        );
        // An ordinary block comment is where the search stops: reading
        // past it would take the statements in between for the body of
        // the docblock beyond them.
        assert_eq!(
            trailing_docblock("/** @var Foo $x */ $x = 1; /* note */"),
            None
        );
        assert_eq!(trailing_docblock("$a = 1;"), None);
        assert_eq!(trailing_docblock(""), None);
        // PHP block comments do not nest, so a `/*` written inside a
        // docblock is part of its body rather than a comment of its own.
        assert_eq!(
            trailing_docblock("/** @var Foo $x  see /* the note */"),
            Some("/** @var Foo $x  see /* the note */")
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
}
