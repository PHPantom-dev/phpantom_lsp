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
use crate::completion::source::code_context::{CodeContext, Operator};
use crate::php_type::{PhpType, TypeKind};
use crate::text_position::position_to_offset;
use crate::text_scan::collapse_continuation_lines;
use crate::type_engine::resolver::{CtxLoaders, resolve_target_classes};
use crate::type_engine::subject_extraction::detect_access_operator;
use crate::types::{AccessKind, ClassInfo, FileContext};
use crate::virtual_members::laravel::{
    classify_relationship_typed, extends_eloquent_model, resolve_relation_chain,
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

/// Generic context for a cursor inside a string argument of a
/// function or method call.  Shared by Eloquent string completion
/// and `model-property<Model>` completion.
#[derive(Debug)]
pub(crate) struct StringCallContext {
    pub partial: String,
    pub quote_char: char,
    pub method_name: String,
    pub subject: Option<String>,
    pub is_static: bool,
    pub arg_index: usize,
    pub string_content_start: usize,
    /// The `->` / `?->` / `::` immediately before the callee, if any — the
    /// same value `subject` and `is_static` were derived from, kept around
    /// so a caller can look through a hop (e.g. `$request->safe()->only(…)`)
    /// via [`Operator::hop`].
    pub(crate) callee_operator: Option<Operator>,
}

/// Detect a string-inside-call context at the cursor position.
///
/// Returns `Some(StringCallContext)` when the cursor is inside a
/// string literal that is an argument to a function or method call.
/// Works for both `$obj->method('|')` and standalone `func('|')`.
///
/// `code` is the cursor's lexical position, which the caller resolves once
/// and shares with the other string-argument strategies: the forward scan
/// behind it is a pass over the file, so running it per strategy would cost
/// as many passes as there are strategies on every keystroke.
pub(crate) fn detect_string_call_context(
    content: &str,
    cursor_offset: usize,
    code: &CodeContext<'_>,
) -> Option<StringCallContext> {
    let (quote_pos, quote_char) = code.open_string?;
    let string_content_start = quote_pos + 1;
    let partial = content[string_content_start..cursor_offset].to_string();

    // The literal has to open an argument, or an element of an argument's
    // array, rather than continue an expression.  The last byte of code skips
    // any comment before it, so `only([/* note */ '|'])` still reads as an
    // element of the array.
    let last_char = code.last_code_byte()?;
    if last_char != b'(' && last_char != b',' && last_char != b'[' {
        return None;
    }

    // The call is the innermost paren the scan left open, so a bracket or a
    // comma inside a comment cannot move it or shift the argument index.
    let call = code.enclosing_paren()?;
    let before_paren = content.get(..call.code_before)?;
    let (method_name, _) = extract_identifier_backwards(before_paren)?;

    // The scan records the `->` / `?->` / `::` before the callee, if any,
    // rather than this recovering it from the text after the fact, so a
    // comment between the receiver and the operator cannot hide it.
    let (is_static, subject) = match &call.callee_operator {
        Some(op) => (op.is_static, extract_receiver_subject(content, op)),
        None => (false, None),
    };

    Some(StringCallContext {
        partial,
        quote_char,
        method_name,
        subject,
        is_static,
        arg_index: call.commas,
        string_content_start,
        callee_operator: call.callee_operator,
    })
}

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
    cursor_offset: usize,
    code: &CodeContext<'_>,
) -> Option<EloquentStringContext> {
    let ctx = detect_string_call_context(content, cursor_offset, code)?;

    let subject = ctx.subject?;

    let kind = if RELATION_METHODS.contains(&ctx.method_name.as_str()) {
        EloquentStringKind::Relation
    } else if COLUMN_METHODS.contains(&ctx.method_name.as_str()) {
        EloquentStringKind::Column
    } else {
        return None;
    };

    Some(EloquentStringContext {
        kind,
        partial: ctx.partial,
        quote_char: ctx.quote_char,
        subject,
        is_static: ctx.is_static,
        string_content_start: ctx.string_content_start,
    })
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

/// The receiver expression in front of the callee's operator, e.g.
/// `$user->posts()` in `$user->posts()->where('|')`.
///
/// The text runs up to where the scan says the receiver ends, so a comment
/// between the receiver and the operator is already cut off, and the
/// operator is put back right after it.  That hands the member-completion
/// subject extractor the same shape it sees after a typed `->`, chains and
/// multi-line continuations included.
fn extract_receiver_subject(content: &str, op: &Operator) -> Option<String> {
    let mut lines: Vec<&str> = content.get(..op.code_before)?.lines().collect();
    let receiver_line = format!("{}{}", lines.pop()?, if op.is_static { "::" } else { "->" });
    lines.push(&receiver_line);
    let (line, col) =
        collapse_continuation_lines(&lines, lines.len() - 1, receiver_line.chars().count());
    let chars: Vec<char> = line.chars().collect();
    detect_access_operator(&chars, col).map(|(subject, _)| subject)
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
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor_offset = position_to_offset(content, position) as usize;
        let es_ctx = detect_eloquent_string_context(content, cursor_offset, code)?;

        let class_loader = self.class_loader(ctx);
        let model_class = self.resolve_eloquent_model_from_subject(
            &es_ctx,
            content,
            cursor_offset as u32,
            ctx,
            &class_loader,
        )?;

        let items = match es_ctx.kind {
            EloquentStringKind::Relation => {
                self.build_relation_completions(&model_class, &es_ctx, &class_loader)
            }
            EloquentStringKind::Column => {
                // Base resolution folds in the `$fillable`/`$casts` a
                // parent model declares.
                let model_class =
                    crate::virtual_members::resolve_class_base_cached(&model_class, &class_loader);
                self.build_column_completions(&model_class, &es_ctx)
            }
        };

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }

    /// Resolve the model the method call's receiver queries or holds.
    ///
    /// The receiver goes through the shared subject resolver, so a chain
    /// like `$user->posts()` or `$user->posts` resolves the same way member
    /// completion after it would.  The model is then the receiver itself, or
    /// the first model among its generic arguments: `Builder<Post>`,
    /// `HasMany<Post, User>` (the related model comes first) and
    /// `Collection<int, Post>` all name it there.
    fn resolve_eloquent_model_from_subject(
        &self,
        es_ctx: &EloquentStringContext,
        content: &str,
        cursor_offset: u32,
        ctx: &FileContext,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<Arc<ClassInfo>> {
        let current_class = crate::class_lookup::find_class_at_offset(&ctx.classes, cursor_offset);
        let function_loader = self.function_loader(ctx);
        let laravel_macro_this_resolver = self.laravel_macro_this_resolver(class_loader);
        let rctx = self.resolution_ctx_at(
            current_class,
            &ctx.classes,
            content,
            cursor_offset,
            CtxLoaders::new(class_loader, &function_loader, &laravel_macro_this_resolver),
        );
        let access_kind = if es_ctx.is_static {
            AccessKind::DoubleColon
        } else {
            AccessKind::Arrow
        };

        let is_model = |cls: &ClassInfo| extends_eloquent_model(cls, class_loader);
        for rt in resolve_target_classes(&es_ctx.subject, access_kind, &rctx) {
            if let Some(cls) = &rt.class_info
                && is_model(cls)
            {
                return class_loader(&cls.fqn());
            }
            if let TypeKind::Generic(g) = rt.type_string.kind() {
                let model = g
                    .args
                    .iter()
                    .find_map(|arg| class_loader(arg.base_name()?).filter(|cls| is_model(cls)));
                if model.is_some() {
                    return model;
                }
            }
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
                label: format!("{}{}", prefix, method_name),
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
        let columns = crate::virtual_members::laravel::where_property::collect_column_names(model);

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

    /// Try completion for `model-property<Model>` typed parameters.
    ///
    /// When the cursor is inside a string argument whose corresponding
    /// parameter is typed as `model-property<Model>`, suggests the
    /// model's known property names.  Uses the shared
    /// [`detect_string_call_context`] to locate the enclosing call,
    /// then resolves the callable target to inspect the parameter type.
    pub(crate) fn try_model_property_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor_offset = position_to_offset(content, position) as usize;
        let sc = detect_string_call_context(content, cursor_offset, code)?;

        let call_expr = match &sc.subject {
            Some(subj) if sc.is_static => format!("{}::{}", subj, sc.method_name),
            Some(subj) => format!("{}->{}", subj, sc.method_name),
            None => sc.method_name.clone(),
        };

        let resolved = self.resolve_callable_target(&call_expr, content, position, ctx)?;
        let param = resolved.parameters.get(sc.arg_index)?;
        let param_type = param.type_hint.as_ref()?;

        let model_name_owned: String;
        let model_name: &str = if let TypeKind::Generic(g) = param_type.kind()
            && g.name.eq_ignore_ascii_case("model-property")
            && g.args.len() == 1
        {
            g.args[0].base_name()?
        } else {
            let name = extract_model_property_from_generic_args(param_type)?;
            model_name_owned = name;
            &model_name_owned
        };

        let class_loader = self.class_loader(ctx);
        let model_class = class_loader(model_name)?;
        let resolved = crate::virtual_members::resolve_class_fully_cached(
            &model_class,
            &class_loader,
            &self.resolved_class_cache,
        );
        let columns: Vec<String> = resolved
            .properties
            .iter()
            .map(|p| p.name.to_string())
            .collect();

        let mut items = Vec::new();
        for col in &columns {
            if !sc.partial.is_empty() && !col.to_lowercase().starts_with(&sc.partial.to_lowercase())
            {
                continue;
            }
            items.push(CompletionItem {
                label: col.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("model property".to_string()),
                insert_text: Some(col.clone()),
                ..Default::default()
            });
        }

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }
}

/// Extract the model name from a `model-property<Model>` type nested
/// inside an array or list generic argument.
fn extract_model_property_from_generic_args(ty: &PhpType) -> Option<String> {
    let TypeKind::Generic(g) = ty.kind() else {
        return None;
    };
    if !crate::php_type::is_array_like_name(&g.name) && !g.name.eq_ignore_ascii_case("list") {
        return None;
    }
    for arg in &g.args {
        if let TypeKind::Generic(inner) = arg.kind()
            && inner.name.eq_ignore_ascii_case("model-property")
            && inner.args.len() == 1
        {
            return inner.args[0].base_name().map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
#[path = "eloquent_string_tests.rs"]
mod tests;
