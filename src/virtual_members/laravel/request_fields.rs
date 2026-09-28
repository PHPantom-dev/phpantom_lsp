//! Request input field names, recovered from validation rules.
//!
//! Inside a controller action or a `FormRequest`, the set of input keys a
//! request may carry is a static fact: it is the key set of the rules array
//! that validates it.  This module answers "which field names are in scope
//! at this cursor?" for the string arguments that name one —
//! `$request->input('…')`, `->string('…')`, `->has('…')`,
//! `->validated('…')`, `->safe()->only(['…'])`, `$request['…']`, and
//! friends.
//!
//! Completion and go-to-definition both go through
//! [`request_fields_at_position`]; the rules parsing itself lives in
//! [`super::validation_rules`].

use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::source::code_context::{CodeContext, code_context_at};
use crate::text_position::position_to_offset;
use crate::types::{ClassInfo, FileContext};

use super::validation_rules::{
    ResolvedRules, RuleField, RulesSource, is_request_like, is_validated_input, rule_fields,
    rules_in_scope, safe_source_variable,
};

/// Request accessors whose *first* argument names a single input field.
const FIELD_METHODS: &[&str] = &[
    "input",
    "query",
    "post",
    "get",
    "old",
    "string",
    "str",
    "integer",
    "float",
    "boolean",
    "date",
    "enum",
    "enums",
    "array",
    "collect",
    "file",
    "hasfile",
    "whenhas",
    "whenfilled",
    "whenmissing",
    "validated",
];

/// Request accessors that take any number of field names, either variadically
/// or as a single array argument.
const MULTI_FIELD_METHODS: &[&str] = &[
    "has",
    "hasany",
    "filled",
    "isnotfilled",
    "anyfilled",
    "missing",
    "only",
    "except",
];

/// A cursor sitting inside a string that names a request input field.
pub(crate) struct RequestFieldContext {
    /// Text of the receiver expression, e.g. `"$request"` or `"$this"`.
    pub receiver: String,
    /// Text typed so far inside the string.
    pub prefix: String,
    /// Byte offset of the string content (just after the opening quote).
    pub content_start: usize,
    /// The quote character that opened the string.
    pub quote_char: char,
}

impl RequestFieldContext {
    /// The complete literal value, when the string is closed on this line.
    ///
    /// Completion only needs [`Self::prefix`], but go-to-definition has to
    /// match the whole key.
    pub fn full_value<'c>(&self, content: &'c str) -> Option<&'c str> {
        let rest = content.get(self.content_start..)?;
        let end = rest.find([self.quote_char, '\n'])?;
        if rest.as_bytes()[end] == b'\n' {
            return None;
        }
        Some(&rest[..end])
    }
}

// ─── Detection ──────────────────────────────────────────────────────────────

/// Detect a cursor inside a request input-field string.
pub(crate) fn detect_request_field_context(
    content: &str,
    cursor_offset: usize,
    code: &CodeContext<'_>,
) -> Option<RequestFieldContext> {
    let (quote_pos, quote_char) = code.open_string?;
    let prefix = content.get(quote_pos + 1..cursor_offset)?.to_string();

    // ── Array access: `$request['|']` ───────────────────────────────
    // Checked before the call form because the backwards scan for a call's
    // opening paren would otherwise wander into unrelated code.
    if let Some(before_bracket) = code.code_before.strip_suffix('[')
        && let Some(receiver) = trailing_variable(before_bracket)
    {
        return Some(RequestFieldContext {
            receiver,
            prefix,
            content_start: quote_pos + 1,
            quote_char,
        });
    }

    // ── Method argument: `$request->input('|')` ─────────────────────
    let call = detect_string_call_context(content, cursor_offset, code)?;
    if call.is_static {
        return None;
    }
    let method = call.method_name.to_ascii_lowercase();
    let accepted = if MULTI_FIELD_METHODS.contains(&method.as_str()) {
        true
    } else {
        FIELD_METHODS.contains(&method.as_str()) && call.arg_index == 0
    };
    if !accepted {
        return None;
    }

    // The scan records the `->` / `?->` / `::` before the callee, and the hop
    // through any call that closed immediately before it, so a comment
    // anywhere in `$request /* … */ ->safe() /* … */ ->only('|')` cannot
    // hide the receiver the way a raw backwards text walk would.
    let op = call.callee_operator?;
    let receiver_end = match op.hop {
        Some(hop)
            if content
                .get(hop.name_start..hop.name_end)?
                .eq_ignore_ascii_case("safe") =>
        {
            hop.code_before
        }
        _ => op.code_before,
    };
    let receiver = trailing_variable(content.get(..receiver_end)?)?;

    Some(RequestFieldContext {
        receiver,
        prefix,
        content_start: call.string_content_start,
        quote_char: call.quote_char,
    })
}

/// Strip a trailing PHP identifier, returning the text before it.
fn strip_trailing_ident(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1].is_ascii_alphanumeric() || bytes[end - 1] == b'_') {
        end -= 1;
    }
    &text[..end]
}

/// Extract a trailing `$variable` token.
fn trailing_variable(text: &str) -> Option<String> {
    let trimmed = text.trim_end();
    let before = strip_trailing_ident(trimmed);
    let name = &trimmed[before.len()..];
    if name.is_empty() || !before.ends_with('$') {
        return None;
    }
    Some(format!("${name}"))
}

// ─── Field resolution ───────────────────────────────────────────────────────

/// The input field names in scope at `position`, or `None` when the cursor is
/// not in a request input-field string or no rules describe it.
///
/// Returns the detected context alongside the fields so callers can filter by
/// the typed prefix and place the replacement edit.
pub(crate) fn request_fields_at_position(
    backend: &Backend,
    uri: &str,
    content: &str,
    position: Position,
    ctx: &FileContext,
    code: &CodeContext<'_>,
) -> Option<(RequestFieldContext, ResolvedRules, Vec<RuleField>)> {
    let cursor_offset = position_to_offset(content, position);
    let field_ctx = detect_request_field_context(content, cursor_offset as usize, code)?;

    let class_loader = backend.class_loader_with(
        &ctx.classes,
        ctx.use_map_at(cursor_offset),
        ctx.namespace_at(cursor_offset),
    );
    let current_class = find_class_at_offset(&ctx.classes, cursor_offset);

    let loaded: Option<Arc<ClassInfo>>;
    let receiver_class: &ClassInfo = if field_ctx.receiver == "$this" {
        current_class?
    } else {
        let mut resolved = resolve_variable_class(
            &field_ctx.receiver,
            current_class,
            ctx,
            content,
            cursor_offset,
            &class_loader,
            backend,
        )?;
        // `$safe = $request->safe()` narrows the request's own rules array,
        // so follow the assignment back to the request: `ValidatedInput`
        // itself has no `rules()` to read.
        if is_validated_input(&resolved)
            && let Some(source) =
                safe_source_variable(content, cursor_offset as usize, &field_ctx.receiver)
            && let Some(request) = resolve_variable_class(
                &source,
                current_class,
                ctx,
                content,
                cursor_offset,
                &class_loader,
                backend,
            )
        {
            resolved = request;
        }
        loaded = Some(resolved);
        loaded.as_deref()?
    };

    if !is_request_like(receiver_class, &class_loader) {
        return None;
    }

    let rules = rules_in_scope(
        backend,
        receiver_class,
        uri,
        content,
        cursor_offset as usize,
    )?;

    let fields = rule_fields(&rules.rules.entries);
    if fields.is_empty() {
        return None;
    }
    Some((field_ctx, rules, fields))
}

/// Resolve a `$variable` receiver to the class it holds.
#[allow(clippy::too_many_arguments)]
fn resolve_variable_class(
    variable: &str,
    current_class: Option<&ClassInfo>,
    ctx: &FileContext,
    content: &str,
    cursor_offset: u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: &Backend,
) -> Option<Arc<ClassInfo>> {
    let fallback;
    let effective_class = match current_class {
        Some(cc) => cc,
        None => {
            fallback = crate::class_lookup::class_context_placeholder(content, cursor_offset);
            &fallback
        }
    };
    let types = crate::type_engine::variable::resolution::resolve_variable_types(
        variable,
        effective_class,
        &ctx.classes,
        content,
        cursor_offset,
        class_loader,
        Some(backend),
        crate::type_engine::resolver::Loaders::default(),
    );
    types
        .iter()
        .find_map(|resolved| resolved.type_string.base_name().and_then(class_loader))
}

// ─── Go to definition ───────────────────────────────────────────────────────

/// Resolve go-to-definition on a request input-field string to the rule that
/// declares it.
pub(crate) fn resolve_request_field_definition(
    backend: &Backend,
    uri: &str,
    content: &str,
    position: Position,
) -> Option<Location> {
    let ctx = backend.file_context(uri);
    let cursor_offset = position_to_offset(content, position) as usize;
    let code = code_context_at(content, cursor_offset)?;
    let (field_ctx, rules, fields) =
        request_fields_at_position(backend, uri, content, position, &ctx, &code)?;
    let value = field_ctx.full_value(content)?;

    // An exact rule key wins over the root segment it also contributes, so
    // `input('address.city')` lands on that key rather than on `address`.
    let (key_start, origin) = rules
        .rules
        .entries
        .iter()
        .find(|rule| rule.key == value)
        .map(|rule| (rule.key_start, rule.origin.as_ref()))
        .or_else(|| {
            fields
                .iter()
                .find(|field| field.name == value)
                .map(|field| (field.key_start, field.origin.as_ref()))
        })?;

    // A key merged in from `parent::rules()` lives in the ancestor's file, not
    // in the file the rules array itself was read from.
    let (target_uri, target_content) = match (origin, &rules.source) {
        (Some(origin), _) => (origin.uri.as_str(), origin.content.as_str()),
        (None, RulesSource::CurrentFile) => (uri, content),
        (None, RulesSource::OtherFile(file)) => (file.uri.as_str(), file.content.as_str()),
    };

    let position = crate::text_position::offset_to_position(target_content, key_start);
    Some(crate::definition::point_location(
        Url::parse(target_uri).ok()?,
        position,
    ))
}

#[cfg(test)]
#[path = "request_fields_tests.rs"]
mod tests;
