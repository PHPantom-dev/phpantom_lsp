//! Strategies for bare-identifier positions: class names, global
//! constants, global functions (including `new`/`throw new`/`use`
//! imports), and native-type-or-class completion in type-hint
//! positions.

use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::completion::class_completion::{
    ClassCompletionParams, ClassNameContext, detect_class_name_context, is_class_declaration_name,
};
use crate::text_position::position_to_offset;
use crate::types::{ClassInfo, FileContext};

/// Filter out completion items for classes defined in the current file.
///
/// When writing a `use` statement it makes no sense to import a class
/// from the file you are already in.  The `detail` field of each item
/// carries the FQN, which is matched against the FQNs of classes in the
/// file's `ctx.classes` (from the uri_classes_index).
fn filter_current_file_classes(
    items: Vec<CompletionItem>,
    ctx: &FileContext,
) -> Vec<CompletionItem> {
    if ctx.classes.is_empty() {
        return items;
    }
    let current_fqns: HashSet<String> = ctx
        .classes
        .iter()
        .map(|cls| {
            if let Some(ref ns) = ctx.namespace {
                format!("{}\\{}", ns, cls.name)
            } else {
                cls.name.to_string()
            }
        })
        .collect();
    items
        .into_iter()
        .filter(|item| {
            item.detail
                .as_ref()
                .is_none_or(|d| !current_fqns.contains(d))
        })
        .collect()
}

/// Filter out completion items for functions defined in the current file.
///
/// Collects the map keys (FQNs) of functions whose URI matches the
/// current file and removes any completion item whose `insert_text`
/// matches one of those FQNs.  This works for both use-import items
/// (where `insert_text` is the FQN) and inline items (where
/// `insert_text` is a snippet starting with the short name, which
/// equals the FQN for global functions).
fn filter_current_file_functions(
    items: Vec<CompletionItem>,
    current_uri: &str,
    backend: &Backend,
) -> Vec<CompletionItem> {
    let current_funcs: HashSet<String> = {
        let fmap = backend.global_functions().read();
        fmap.iter()
            .filter(|(_, (uri, _))| uri == current_uri)
            .map(|(key, _)| key.to_owned())
            .collect()
    };
    if current_funcs.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|item| {
            item.insert_text
                .as_ref()
                .is_none_or(|it| !current_funcs.contains(it))
        })
        .collect()
}

/// Filter out completion items for constants defined in the current file.
fn filter_current_file_constants(
    items: Vec<CompletionItem>,
    current_uri: &str,
    backend: &Backend,
) -> Vec<CompletionItem> {
    let current_consts: HashSet<String> = {
        let dmap = backend.global_defines().read();
        dmap.iter()
            .filter(|(_, info)| info.file_uri.as_str() == current_uri)
            .map(|(name, _)| name.clone())
            .collect()
    };
    if current_consts.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|item| {
            item.filter_text
                .as_ref()
                .is_none_or(|ft| !current_consts.contains(ft))
        })
        .collect()
}

/// Append a semicolon to the `insert_text` of each completion item.
///
/// Used for `use`, `use function`, and `use const` completions so that
/// accepting a suggestion produces a complete statement (e.g. `use Foo\Bar;`).
fn append_semicolon_to_insert_text(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items
        .into_iter()
        .map(|mut item| {
            // Namespace segment items (MODULE kind) represent
            // intermediate namespace paths the user can drill into.
            // They should not receive a trailing semicolon because
            // the user will continue typing after selecting one
            // (e.g. `use App\Models\` → pick a class next).
            if item.kind == Some(CompletionItemKind::MODULE) {
                return item;
            }
            if let Some(ref mut text) = item.insert_text
                && !text.ends_with(';')
            {
                text.push(';');
            }
            if let Some(CompletionTextEdit::Edit(ref mut edit)) = item.text_edit
                && !edit.new_text.ends_with(';')
            {
                edit.new_text.push(';');
            }
            item
        })
        .collect()
}

impl Backend {
    // ─── Strategy: type hint completion ──────────────────────────────────

    /// Build completions at a type-hint position inside a function/method
    /// parameter list, return type, or property declaration.
    ///
    /// Offers PHP native scalar types alongside class-name completions (but
    /// NOT constants or standalone functions, which are invalid in type
    /// positions).
    ///
    /// This check MUST run before named-argument detection so that typing
    /// inside a function *definition* like `function foo(Us|)` offers type
    /// completions rather than named-argument suggestions.
    pub(super) fn complete_type_hint(
        &self,
        content: &str,
        th_ctx: &crate::completion::type_hint_completion::TypeHintContext,
        ctx: &FileContext,
        position: Position,
        uri: &str,
    ) -> Option<CompletionResponse> {
        let partial_lower = th_ctx.partial.to_lowercase();
        let space_prefix = if th_ctx.needs_space_prefix { " " } else { "" };
        let mut items: Vec<CompletionItem> =
            crate::completion::type_hint_completion::PHP_NATIVE_TYPES
                .iter()
                .filter(|t| t.to_lowercase().starts_with(&partial_lower))
                .enumerate()
                .map(|(idx, t)| CompletionItem {
                    label: t.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("PHP built-in type".to_string()),
                    insert_text: Some(format!("{}{}", space_prefix, t)),
                    filter_text: Some(t.to_string()),
                    sort_text: Some(format!("0_{:03}", idx)),
                    ..CompletionItem::default()
                })
                .collect();

        let (class_items, class_incomplete) =
            self.build_class_name_completions(ClassCompletionParams {
                file_use_map: &ctx.use_map,
                file_namespace: &ctx.namespace,
                prefix: &th_ctx.partial,
                content,
                context: ClassNameContext::TypeHint,
                position,
                affinity_table_override: None,
                uri,
            });

        // When a leading space is needed (return type after `:` with no
        // space), prefix the insert text of each class-name item so that
        // the result is `: ClassName` rather than `:ClassName`.
        if th_ctx.needs_space_prefix {
            for mut item in class_items {
                if let Some(ref txt) = item.insert_text {
                    item.insert_text = Some(format!(" {}", txt));
                }
                if let Some(CompletionTextEdit::Edit(ref te)) = item.text_edit {
                    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                        range: te.range,
                        new_text: format!(" {}", te.new_text),
                    }));
                }
                items.push(item);
            }
        } else {
            items.extend(class_items);
        }

        if items.is_empty() {
            // Even when empty, the caller returns early so we don't fall
            // through to named-arg or class+constant+function completion.
            None
        } else {
            Some(CompletionResponse::List(CompletionList {
                is_incomplete: class_incomplete,
                items,
            }))
        }
    }

    // ─── Strategy: class / constant / function completion ────────────────

    /// Build completion item for class keywords (`self`, `static`, `parent`)
    /// in `new` expression contexts.
    ///
    /// When the cursor is inside a class and typing `new s`, these keywords
    /// should be offered alongside regular class names. If the current class
    /// has a constructor, the completion includes parameter snippets.
    fn build_class_keyword_completions(
        &self,
        prefix: &str,
        current_class: Option<&ClassInfo>,
    ) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        let Some(current_class) = current_class else {
            return items;
        };

        let prefix_lower = prefix.to_lowercase();

        for keyword in ["self", "static"] {
            if !keyword.starts_with(&prefix_lower) {
                continue;
            }

            let mut item = CompletionItem {
                label: keyword.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Instantiate current class".to_string()),
                filter_text: Some(keyword.to_string()),
                sort_text: Some(format!("0_{keyword}")),
                ..CompletionItem::default()
            };

            // Add constructor snippet if available
            if let Some(ctor) = current_class.get_method("__construct") {
                let snippet =
                    crate::completion::builder::build_callable_snippet(keyword, &ctor.parameters);
                item.insert_text = Some(snippet);
                item.insert_text_format = Some(InsertTextFormat::SNIPPET);
            } else {
                item.insert_text = Some(format!("{}()$0", keyword));
                item.insert_text_format = Some(InsertTextFormat::SNIPPET);
            }

            items.push(item);
        }

        // `parent` - reference the parent class
        if "parent".starts_with(&prefix_lower)
            && let Some(parent_name) = &current_class.parent_class
        {
            let mut item = CompletionItem {
                label: "parent".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(format!("Instantiate parent class ({})", parent_name)),
                filter_text: Some("parent".to_string()),
                sort_text: Some("0_parent".to_string()),
                ..CompletionItem::default()
            };

            // Try to load parent class and get its constructor
            if let Some(parent_cls) = self.find_or_load_class(parent_name) {
                if let Some(ctor) = parent_cls.get_method("__construct") {
                    let snippet = crate::completion::builder::build_callable_snippet(
                        "parent",
                        &ctor.parameters,
                    );
                    item.insert_text = Some(snippet);
                    item.insert_text_format = Some(InsertTextFormat::SNIPPET);
                } else {
                    item.insert_text = Some("parent()$0".to_string());
                    item.insert_text_format = Some(InsertTextFormat::SNIPPET);
                }
            } else {
                item.insert_text = Some("parent()$0".to_string());
                item.insert_text_format = Some(InsertTextFormat::SNIPPET);
            }

            items.push(item);
        }

        items
    }

    /// Suggest the filename (without `.php` extension) as the class name
    /// when the cursor is inside a class/interface/trait/enum declaration.
    ///
    /// Returns a single completion item so the user can quickly name the
    /// class to match the file, following PSR-4 conventions.
    pub(super) fn try_class_declaration_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        if !is_class_declaration_name(content, position) {
            return None;
        }

        let name = Self::filename_class_name(uri)?;

        let item = CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("Match filename".to_string()),
            insert_text: Some(name),
            ..CompletionItem::default()
        };

        Some(CompletionResponse::Array(vec![item]))
    }

    /// Extract the filename without extension from a `file://` URI.
    ///
    /// For example, `file:///home/user/Test.php` returns `Some("Test")`.
    fn filename_class_name(uri: &str) -> Option<String> {
        let url = Url::parse(uri).ok()?;
        let file_path = url.to_file_path().ok()?;
        let stem = file_path.file_stem()?;
        let name = stem.to_string_lossy();
        if name.is_empty() {
            return None;
        }
        Some(name.into_owned())
    }

    /// Try to offer class name, constant, and function completions.
    ///
    /// When there is no `->` or `::` operator, check whether the user is
    /// typing a class name, constant, or function name and offer
    /// completions from all known sources (use-imports, same namespace,
    /// stubs, fqn_uri_index, global_defines, stub_constant_index,
    /// global_functions, stub_function_index).
    ///
    /// Returns `None` when the cursor is not at an identifier position or
    /// when no completions could be produced.
    pub(super) fn try_class_constant_function_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        current_uri: &str,
    ) -> Option<CompletionResponse> {
        if let Some(partial) =
            crate::completion::keyword_completion::enum_backing_type_partial(content, position)
        {
            let items =
                crate::completion::keyword_completion::build_backed_enum_type_completions(&partial);
            if items.is_empty() {
                return None;
            }
            return Some(CompletionResponse::Array(items));
        }

        // Method/function/const *names* are not type/class positions.
        // Without this, `protected function getC` suggests classes like
        // Cache/Carbon that match the partial (issue #249 / #126).
        if crate::completion::type_hint_completion::is_function_or_const_name_position(
            content, position,
        ) {
            return None;
        }

        let class_ctx = detect_class_name_context(content, position);
        let keyword_ctx = {
            let cursor_offset = position_to_offset(content, position);
            let maps = self.symbol_maps.read();
            let map = maps.get(current_uri);
            crate::completion::keyword_completion::build_keyword_context(
                content,
                position,
                cursor_offset,
                map.map(|m| m.as_ref()),
                &ctx.classes,
            )
        };
        let partial = match Self::extract_partial_class_name(content, position) {
            Some(p) => p,
            None => {
                // Allow attribute and namespace-declaration completion on
                // empty prefix (e.g. `#[` or `namespace ` with nothing
                // typed yet).
                if matches!(
                    class_ctx,
                    ClassNameContext::Attribute(_) | ClassNameContext::NamespaceDeclaration
                ) {
                    String::new()
                }
                // Allow keyword completion on empty prefix inside class-like
                // bodies (e.g. after typing `public `).
                else if keyword_ctx.after_member_modifier_chain {
                    let items = crate::completion::keyword_completion::build_keyword_completions(
                        "",
                        class_ctx,
                        keyword_ctx,
                    );
                    if items.is_empty() {
                        return None;
                    }
                    return Some(CompletionResponse::Array(items));
                } else {
                    return None;
                }
            }
        };

        // ── `use function` → only functions ─────────────────────────
        if matches!(class_ctx, ClassNameContext::UseFunction) {
            let (function_items, func_incomplete) = self.build_function_completions(
                &partial,
                true,
                Some(content),
                &ctx.namespace,
                current_uri,
            );
            // Filter out functions defined in the current file.
            let function_items = filter_current_file_functions(function_items, current_uri, self);
            let items = append_semicolon_to_insert_text(function_items);
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: func_incomplete,
                items,
            }));
        }

        // ── `use const` → only constants ────────────────────────────
        if matches!(class_ctx, ClassNameContext::UseConst) {
            let (constant_items, const_incomplete) =
                self.build_constant_completions(&partial, current_uri, position);
            // Filter out constants defined in the current file.
            let constant_items = filter_current_file_constants(constant_items, current_uri, self);
            let items = append_semicolon_to_insert_text(constant_items);
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: const_incomplete,
                items,
            }));
        }

        // ── `namespace` declaration → only namespace names ──────────
        if matches!(class_ctx, ClassNameContext::NamespaceDeclaration) {
            let (ns_items, ns_incomplete) =
                self.build_namespace_completions(&partial, position, current_uri);
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: ns_incomplete,
                items: ns_items,
            }));
        }

        // For `use` imports, pass an empty use_map: the file's own
        // use_map contains the half-typed line (e.g. `use c` → "c")
        // which would appear as a bogus completion item.  Existing
        // imports are irrelevant when writing a new use statement.
        let (use_map_for_completion, affinity_override) =
            if matches!(class_ctx, ClassNameContext::UseImport) {
                // Pass an empty use_map so the half-typed `use` line
                // doesn't appear as a bogus completion item, but build
                // the affinity table from the *real* use-map so that
                // tier-2 candidates are still ranked by namespace affinity.
                let table = crate::completion::class_completion::build_affinity_table(
                    &ctx.use_map,
                    &ctx.namespace,
                );
                (&HashMap::new() as &HashMap<String, String>, Some(table))
            } else {
                (&ctx.use_map, None)
            };

        let (class_items, class_incomplete) =
            self.build_class_name_completions(ClassCompletionParams {
                file_use_map: use_map_for_completion,
                file_namespace: &ctx.namespace,
                prefix: &partial,
                content,
                context: class_ctx,
                position,
                affinity_table_override: affinity_override,
                uri: current_uri,
            });

        // ── `use` (class import) → classes + keyword hints ──────────
        if matches!(class_ctx, ClassNameContext::UseImport) {
            // Filter out classes defined in the current file.
            let class_items = filter_current_file_classes(class_items, ctx);
            // Filter out classes that are already imported via `use`.
            let already_imported: HashSet<&str> =
                ctx.use_map.values().map(|v| v.as_str()).collect();
            let class_items: Vec<CompletionItem> = class_items
                .into_iter()
                .filter(|item| {
                    item.detail
                        .as_deref()
                        .is_none_or(|fqn| !already_imported.contains(fqn))
                })
                .collect();
            let mut items = append_semicolon_to_insert_text(class_items);
            // Inject `function` / `const` keyword suggestions when the
            // partial is a case-sensitive prefix of the keyword.  This
            // lets the user type `use f` → select "function" → continue
            // with a function name.
            if "function".starts_with(&partial) {
                items.insert(
                    0,
                    CompletionItem {
                        label: "function".to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("use function import".to_string()),
                        insert_text: Some("function ".to_string()),
                        filter_text: Some("function".to_string()),
                        sort_text: Some("0_!function".to_string()),
                        ..CompletionItem::default()
                    },
                );
            }
            if "const".starts_with(&partial) {
                items.insert(
                    0,
                    CompletionItem {
                        label: "const".to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("use const import".to_string()),
                        insert_text: Some("const ".to_string()),
                        filter_text: Some("const".to_string()),
                        sort_text: Some("0_!const".to_string()),
                        ..CompletionItem::default()
                    },
                );
            }
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: class_incomplete,
                items,
            }));
        }

        // In restricted contexts (new, extends, implements, use,
        // instanceof), only class names are valid — skip constants
        // and functions.
        if class_ctx.is_class_only() {
            let mut items = if super::paren_follows_cursor(content, position) {
                super::strip_snippet_parens(class_items)
            } else {
                class_items
            };

            // For `new` expressions, also offer `self`, `static`, and `parent`
            // keywords when inside a class.
            if class_ctx.is_new() {
                let cursor_offset = position_to_offset(content, position);
                let current_class = find_class_at_offset(&ctx.classes, cursor_offset);
                let keyword_items = self.build_class_keyword_completions(&partial, current_class);
                items.extend(keyword_items);
            }

            if items.is_empty() {
                return None;
            }

            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: class_incomplete,
                items,
            }));
        }

        let keyword_items = crate::completion::keyword_completion::build_keyword_completions(
            &partial,
            class_ctx,
            keyword_ctx,
        );
        let (constant_items, const_incomplete) =
            self.build_constant_completions(&partial, current_uri, position);
        let (function_items, func_incomplete) = self.build_function_completions(
            &partial,
            false,
            Some(content),
            &ctx.namespace,
            current_uri,
        );

        if class_items.is_empty()
            && keyword_items.is_empty()
            && constant_items.is_empty()
            && function_items.is_empty()
        {
            return None;
        }

        let mut items = keyword_items;
        items.extend(class_items);
        items.extend(constant_items);
        items.extend(function_items);

        // Strip snippet parentheses when `(` already follows the cursor
        // (e.g. `array_map|()` or `new ClassName|()`).
        let items = if super::paren_follows_cursor(content, position) {
            super::strip_snippet_parens(items)
        } else {
            items
        };

        Some(CompletionResponse::List(CompletionList {
            is_incomplete: class_incomplete || const_incomplete || func_incomplete,
            items,
        }))
    }
}
