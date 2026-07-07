//! Eloquent relation dot-notation and column name string completion.
//!
//! Detects when the cursor is inside a string argument to an Eloquent
//! method that accepts relationship names (with dot-notation for nested
//! eager loads) or column/attribute names, and offers appropriate
//! completions.
//!
//! # Relation string completion
//!
//! Methods like `with()`, `load()`, `has()`, `whereHas()` etc. accept
//! relationship method names as string arguments. Dot-notation chains
//! traverse nested relationships: `'mother.sister.son'`.
//!
//! # Column name string completion
//!
//! Methods like `where()`, `orderBy()`, `select()`, `pluck()` etc.
//! accept column/attribute names as string arguments.

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FileContext};
use crate::util::position_to_offset;
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, classify_relationship_typed, extends_eloquent_model,
    resolve_relation_chain,
};

/// Relationship-building method names on the Model base class.
/// These return relationship types but are not actual relationship
/// declarations — they are the factory methods used *inside*
/// relationship methods (e.g. `return $this->hasMany(...)`).
const RELATIONSHIP_BUILDER_METHODS: &[&str] = &[
    "hasOne",
    "hasMany",
    "belongsTo",
    "belongsToMany",
    "morphOne",
    "morphMany",
    "morphTo",
    "morphToMany",
    "morphedByMany",
    "hasManyThrough",
    "hasOneThrough",
];

/// Methods whose first string argument is a relation name (supports dot-notation).
const RELATION_METHODS: &[&str] = &[
    "with",
    "without",
    "load",
    "loadMissing",
    "loadCount",
    "loadMorph",
    "has",
    "orHas",
    "doesntHave",
    "orDoesntHave",
    "whereHas",
    "orWhereHas",
    "withWhereHas",
    "whereDoesntHave",
    "orWhereDoesntHave",
    "withCount",
    "withSum",
    "withAvg",
    "withMin",
    "withMax",
    "withExists",
];

/// Methods whose first string argument is a column/attribute name.
const COLUMN_METHODS: &[&str] = &[
    "where",
    "orWhere",
    "whereIn",
    "whereNotIn",
    "whereBetween",
    "whereNotBetween",
    "whereNull",
    "whereNotNull",
    "orderBy",
    "orderByDesc",
    "groupBy",
    "having",
    "select",
    "addSelect",
    "pluck",
    "value",
    "increment",
    "decrement",
    "latest",
    "oldest",
];

/// The kind of string argument the cursor is inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EloquentStringKind {
    /// A relation name (supports dot-notation).
    Relation,
    /// A column/attribute name.
    Column,
}

/// Context extracted when the cursor is inside an Eloquent string argument.
#[derive(Debug)]
pub(crate) struct EloquentStringContext {
    /// The kind of string completion needed.
    kind: EloquentStringKind,
    /// The text the user has typed so far inside the string (e.g. `"mother.si"`).
    pub partial: String,
    /// The quote character used.
    #[allow(dead_code)]
    pub quote_char: char,
    /// The subject text before the method call (e.g. `"User"`, `"$user"`, `"$query"`).
    pub subject: String,
    /// Whether this is a static call (`::`) vs instance call (`->`).
    pub is_static: bool,
    /// Byte offset where the string content starts (after the opening quote).
    #[allow(dead_code)]
    pub string_content_start: usize,
}

/// Try to detect an Eloquent string context at the given cursor position.
///
/// Returns `None` if the cursor is not inside a string argument to a
/// recognized Eloquent method.
pub(crate) fn detect_eloquent_string_context(
    content: &str,
    position: Position,
) -> Option<EloquentStringContext> {
    let cursor_offset = position_to_offset(content, position) as usize;
    let bytes = content.as_bytes();

    if cursor_offset == 0 || cursor_offset > bytes.len() {
        return None;
    }

    // Find the opening quote before the cursor.
    let mut quote_pos = None;
    let mut quote_char = '\'';
    let mut i = cursor_offset;
    while i > 0 {
        i -= 1;
        let ch = bytes[i];
        if ch == b'\'' || ch == b'"' {
            // Make sure this isn't an escaped quote.
            let mut backslashes = 0;
            let mut j = i;
            while j > 0 && bytes[j - 1] == b'\\' {
                backslashes += 1;
                j -= 1;
            }
            if backslashes % 2 == 0 {
                quote_pos = Some(i);
                quote_char = ch as char;
                break;
            }
        }
        // Stop at newlines — strings don't span lines in PHP (except heredoc).
        if ch == b'\n' {
            return None;
        }
    }

    let quote_pos = quote_pos?;
    let string_content_start = quote_pos + 1;

    // The partial text typed so far.
    let partial = content[string_content_start..cursor_offset].to_string();

    // Scan backwards from the quote to find the method call pattern.
    // We expect: `subject->method(` or `subject::method(` possibly with
    // additional arguments before us (e.g. inside an array `['posts', '`).
    let before_quote = &content[..quote_pos];
    let trimmed = before_quote.trim_end();

    // The character before the string could be `(`, `,`, or `[` (for array args).
    let last_char = trimmed.as_bytes().last().copied()?;
    if last_char != b'(' && last_char != b',' && last_char != b'[' {
        return None;
    }

    // Find the opening paren of the method call.
    let paren_pos = find_matching_open_paren(trimmed)?;
    let before_paren = content[..paren_pos].trim_end();

    // Extract the method name.
    let (method_name, before_method) = extract_identifier_backwards(before_paren)?;

    // Determine the kind based on method name.
    let kind = if RELATION_METHODS.contains(&method_name.as_str()) {
        EloquentStringKind::Relation
    } else if COLUMN_METHODS.contains(&method_name.as_str()) {
        EloquentStringKind::Column
    } else {
        return None;
    };

    // Extract the access operator and subject.
    let before_method_trimmed = before_method.trim_end();
    let (is_static, before_op) = if let Some(stripped) = before_method_trimmed.strip_suffix("::") {
        (true, stripped)
    } else if let Some(stripped) = before_method_trimmed.strip_suffix("?->") {
        (false, stripped)
    } else if let Some(stripped) = before_method_trimmed.strip_suffix("->") {
        (false, stripped)
    } else {
        return None;
    };

    // Extract subject (class name or variable).
    let subject = extract_subject_backwards(before_op.trim_end())?;

    Some(EloquentStringContext {
        kind,
        partial,
        quote_char,
        subject,
        is_static,
        string_content_start,
    })
}

/// Find the opening paren for the method call, scanning backwards.
/// Handles the case where we might be past a comma (second+ argument).
fn find_matching_open_paren(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' | b']' => depth += 1,
            b'(' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            b'[' => {
                if depth == 0 {
                    // We hit an array bracket — keep scanning for the paren.
                    continue;
                }
                depth -= 1;
            }
            b'\n' => {
                // Allow multi-line, but limit scan depth.
                // Count newlines; bail after 5 lines.
            }
            _ => {}
        }
    }
    None
}

/// Extract an identifier (method name) scanning backwards from the end of `text`.
/// Returns (identifier, text_before_identifier).
fn extract_identifier_backwards(text: &str) -> Option<(String, &str)> {
    let trimmed = text.trim_end();
    let bytes = trimmed.as_bytes();
    let mut end = bytes.len();
    // Walk backwards while we have valid identifier chars.
    while end > 0 && (bytes[end - 1].is_ascii_alphanumeric() || bytes[end - 1] == b'_') {
        end -= 1;
    }
    if end == bytes.len() {
        return None; // no identifier found
    }
    let ident = &trimmed[end..];
    if ident.is_empty() {
        return None;
    }
    Some((ident.to_string(), &trimmed[..end]))
}

/// Extract a subject (class name or $variable) scanning backwards.
fn extract_subject_backwards(text: &str) -> Option<String> {
    let trimmed = text.trim_end();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut end = bytes.len();
    // Walk backwards collecting identifier chars and backslashes (for FQNs).
    while end > 0
        && (bytes[end - 1].is_ascii_alphanumeric()
            || bytes[end - 1] == b'_'
            || bytes[end - 1] == b'\\'
            || bytes[end - 1] == b'$')
    {
        end -= 1;
    }

    let subject = &trimmed[end..];
    if subject.is_empty() {
        return None;
    }
    Some(subject.to_string())
}

impl Backend {
    /// Try Eloquent relation/column string completion.
    ///
    /// Returns `Some(CompletionResponse)` when the cursor is inside a string
    /// argument to a recognized Eloquent method and we can resolve the model.
    pub(crate) fn try_eloquent_string_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let es_ctx = detect_eloquent_string_context(content, position)?;

        // Resolve the model class.
        let class_loader = self.class_loader(ctx);
        let model_class = self.resolve_eloquent_model_from_subject(
            &es_ctx.subject,
            es_ctx.is_static,
            content,
            position,
            ctx,
            &class_loader,
        )?;

        // Verify it's actually an Eloquent model.
        if !extends_eloquent_model(&model_class, &class_loader) {
            return None;
        }

        let items = match es_ctx.kind {
            EloquentStringKind::Relation => {
                self.build_relation_completions(&model_class, &es_ctx, &class_loader)
            }
            EloquentStringKind::Column => self.build_column_completions(&model_class, &es_ctx),
        };

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }

    /// Resolve the model class from the subject of the method call.
    fn resolve_eloquent_model_from_subject(
        &self,
        subject: &str,
        is_static: bool,
        content: &str,
        position: Position,
        ctx: &FileContext,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<Arc<ClassInfo>> {
        if is_static {
            // Static call: `User::with(...)` — subject is the class name.
            let fqn = self.resolve_class_name_to_fqn(subject, ctx)?;
            class_loader(&fqn)
        } else if subject == "$this" || subject == "static" || subject == "self" {
            // Inside the model class itself.
            let cursor_offset = position_to_offset(content, position);
            let current_class = crate::util::find_class_at_offset(&ctx.classes, cursor_offset)?;
            Some(Arc::new(current_class.clone()))
        } else if subject.starts_with('$') {
            // Variable — resolve its type. Use a simplified approach:
            // look for the resolved type in the class hierarchy.
            // For now, try to resolve via the forward walker.
            let cursor_offset = position_to_offset(content, position);
            let default_class = ClassInfo::default();
            let current_class = crate::util::find_class_at_offset(&ctx.classes, cursor_offset)
                .unwrap_or(&default_class);
            let results = crate::completion::variable::resolution::resolve_variable_types(
                subject,
                current_class,
                &ctx.classes,
                content,
                cursor_offset,
                class_loader,
                crate::completion::resolver::Loaders::default(),
            );
            for rt in &results {
                if let Some(model_fqn) = extract_model_from_builder_type(&rt.type_string)
                    && let Some(cls) = class_loader(&model_fqn)
                {
                    return Some(cls);
                }
                if let Some(base) = rt.type_string.base_name()
                    && let Some(cls) = class_loader(base)
                    && extends_eloquent_model(&cls, class_loader)
                {
                    return Some(cls);
                }
            }
            None
        } else {
            let fqn = self.resolve_class_name_to_fqn(subject, ctx)?;
            class_loader(&fqn)
        }
    }

    /// Resolve a short/relative class name to FQN using use statements.
    fn resolve_class_name_to_fqn(&self, name: &str, ctx: &FileContext) -> Option<String> {
        let clean = name.trim_start_matches('\\');
        // Check use map.
        if let Some(fqn) = ctx.use_map.get(clean) {
            return Some(fqn.clone());
        }
        // If it looks like a FQN already.
        if clean.contains('\\') {
            return Some(clean.to_string());
        }
        // Try prepending the file namespace.
        if let Some(ref ns) = ctx.namespace {
            let fqn = format!("{}\\{}", ns, clean);
            if self.find_or_load_class(&fqn).is_some() {
                return Some(fqn);
            }
        }
        // Try bare name.
        if self.find_or_load_class(clean).is_some() {
            return Some(clean.to_string());
        }
        None
    }

    /// Build completion items for relation names on the given model.
    fn build_relation_completions(
        &self,
        model: &ClassInfo,
        es_ctx: &EloquentStringContext,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<CompletionItem> {
        let partial = &es_ctx.partial;

        // If there's a dot, resolve the chain up to the last dot.
        let (prefix, current_partial, current_model) = if let Some(dot_pos) = partial.rfind('.') {
            let chain_prefix = &partial[..dot_pos];
            let after_dot = &partial[dot_pos + 1..];
            // Resolve the chain to get the model at the end.
            let Some(resolved_fqn) =
                resolve_relation_chain(model, chain_prefix, class_loader, None)
            else {
                return Vec::new();
            };
            let Some(resolved_model) = class_loader(&resolved_fqn) else {
                return Vec::new();
            };
            // Resolve with inheritance for full method list.
            let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                &resolved_model,
                class_loader,
                None,
            );
            (
                format!("{}.", chain_prefix),
                after_dot.to_string(),
                resolved,
            )
        } else {
            // No dot — complete on the root model.
            let resolved =
                crate::virtual_members::resolve_class_fully_maybe_cached(model, class_loader, None);
            (String::new(), partial.clone(), resolved)
        };

        // Collect relationship methods from the current model.
        let mut items = Vec::new();
        for method in current_model.methods.iter() {
            // Only public methods.
            if method.visibility != crate::types::Visibility::Public {
                continue;
            }
            // Check if the return type is a relationship.
            let Some(ref return_type) = method.return_type else {
                continue;
            };
            if classify_relationship_typed(return_type).is_none() {
                continue;
            }
            let method_name = method.name.to_string();
            // Skip relationship-builder methods (hasOne, hasMany, etc.)
            // which are factory methods, not actual relationship declarations.
            if RELATIONSHIP_BUILDER_METHODS.contains(&method_name.as_str()) {
                continue;
            }
            // Filter by partial.
            if !current_partial.is_empty()
                && !method_name
                    .to_lowercase()
                    .starts_with(&current_partial.to_lowercase())
            {
                continue;
            }

            let insert_text = method_name.clone();
            let detail = return_type.to_string();

            items.push(CompletionItem {
                label: format!("{}{}", prefix, &method_name),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(detail),
                insert_text: Some(insert_text),
                filter_text: Some(method_name),
                ..Default::default()
            });
        }

        items
    }

    /// Build completion items for column/attribute names on the given model.
    fn build_column_completions(
        &self,
        model: &ClassInfo,
        es_ctx: &EloquentStringContext,
    ) -> Vec<CompletionItem> {
        let partial = &es_ctx.partial;
        let columns = collect_model_columns(model);

        let mut items = Vec::new();
        for col in &columns {
            if !partial.is_empty() && !col.to_lowercase().starts_with(&partial.to_lowercase()) {
                continue;
            }

            items.push(CompletionItem {
                label: col.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("column".to_string()),
                insert_text: Some(col.clone()),
                ..Default::default()
            });
        }

        items
    }
}

/// Extract the model FQN from a `Builder<Model>` type.
fn extract_model_from_builder_type(ty: &PhpType) -> Option<String> {
    if let PhpType::Generic(base, args) = ty
        && (base.ends_with("Builder") || base == ELOQUENT_BUILDER_FQN)
        && let Some(first) = args.first()
    {
        return first.base_name().map(|s| s.to_string());
    }
    None
}

/// Collect all column/attribute names from a model class.
///
/// Uses the same sources as `where_property::collect_column_names` but
/// we call it here to avoid coupling to internal module functions.
fn collect_model_columns(class: &ClassInfo) -> Vec<String> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut columns = Vec::new();

    let mut push = |name: &str| {
        if seen.insert(name.to_string()) {
            columns.push(name.to_string());
        }
    };

    if let Some(laravel) = class.laravel() {
        for (col, _) in &laravel.casts_definitions {
            push(col);
        }
        for col in &laravel.dates_definitions {
            push(col);
        }
        for (col, _) in &laravel.attributes_definitions {
            push(col);
        }
        for col in &laravel.column_names {
            push(col);
        }
        // Timestamps.
        let timestamps_enabled = laravel.timestamps.unwrap_or(true);
        if timestamps_enabled {
            let created_col = match &laravel.created_at_name {
                Some(Some(name)) => Some(name.as_str()),
                Some(None) => None,
                None => Some("created_at"),
            };
            let updated_col = match &laravel.updated_at_name {
                Some(Some(name)) => Some(name.as_str()),
                Some(None) => None,
                None => Some("updated_at"),
            };
            for col in [created_col, updated_col].into_iter().flatten() {
                push(col);
            }
        }
    }

    // Properties on the class (including virtual @property tags).
    for prop in class.properties.iter() {
        push(&prop.name);
    }

    // @property tags from docblock.
    if let Some(ref doc_text) = class.class_docblock {
        for (name, _type_str) in crate::docblock::extract_property_tags(doc_text) {
            push(&name);
        }
    }

    columns
}

#[cfg(test)]
#[path = "eloquent_string_tests.rs"]
mod tests;
