//! Dispatch of `textDocument/codeAction` and `codeAction/resolve`: the
//! collector table, the deferred-edit resolution, and the post-processing
//! every offered edit goes through (template translation, line endings).

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::blade::template_edits::TemplateText;

use super::{CodeActionData, visit_file_text_edits};

/// When the request range starts in whitespace, return a copy of `params`
/// whose start is the first non-whitespace byte inside the range (or, for a
/// collapsed range, on the same line), so member collectors that match the
/// start offset against a member span still find the member.
fn skip_leading_whitespace(content: &str, params: &CodeActionParams) -> Option<CodeActionParams> {
    use crate::text_position::{offset_to_position, position_to_byte_offset};

    let start = position_to_byte_offset(content, params.range.start);
    let end = position_to_byte_offset(content, params.range.end);
    let collapsed = end <= start;
    let rest = content.get(start..)?;
    let limit = if collapsed {
        rest.find('\n').unwrap_or(rest.len())
    } else {
        (end - start).min(rest.len())
    };
    let window = rest.get(..limit).unwrap_or(rest);
    let skipped = window.len() - window.trim_start().len();
    if skipped == 0 || skipped == window.len() {
        return None;
    }

    let new_start = offset_to_position(content, start + skipped);
    let mut adjusted = params.clone();
    adjusted.range.start = new_start;
    if collapsed {
        adjusted.range.end = new_start;
    }
    Some(adjusted)
}

/// A code-action collector: appends the actions it offers for the
/// request to the list.
type CollectFn = fn(&Backend, &str, &str, &CodeActionParams, &mut Vec<CodeActionResponse>);

/// Whether a collector's actions are offered for a Blade template, whose
/// edits are planned against the PHP it lowers to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TemplatePolicy {
    /// Offer the actions, translated back into the template.
    Offer,
    /// Offer the actions, but hand the collector the request range in the
    /// template's own coordinates: it matches the range against
    /// diagnostics, which a template's are reported in, and its edits are
    /// planned against the template's own text.
    TemplateRange,
    /// Never offer the actions in a template: they are planned against the
    /// raw file (the external tools' fixes), are about class files (fix
    /// namespace, fix class name), or place code in the wrapper the
    /// preprocessor generates (extract function).
    Skip,
}

struct Collector {
    collect: CollectFn,
    template: TemplatePolicy,
    /// Whether the actions are offered in a resource document (a Twig
    /// template, a YAML or XML file).  Only the ones about a string key
    /// it names are; the rest are about PHP code it does not hold.
    in_resource: bool,
}

const fn offer(collect: CollectFn) -> Collector {
    Collector {
        collect,
        template: TemplatePolicy::Offer,
        in_resource: false,
    }
}

const fn template_range(collect: CollectFn) -> Collector {
    Collector {
        collect,
        template: TemplatePolicy::TemplateRange,
        in_resource: false,
    }
}

const fn skip_in_template(collect: CollectFn) -> Collector {
    Collector {
        collect,
        template: TemplatePolicy::Skip,
        in_resource: false,
    }
}

const fn offer_in_resource(collect: CollectFn) -> Collector {
    Collector {
        collect,
        template: TemplatePolicy::Offer,
        in_resource: true,
    }
}

/// Every code-action collector, in the order their actions are listed.
static COLLECTORS: &[Collector] = &[
    offer(Backend::collect_import_class_actions),
    offer(Backend::collect_replace_fqcn_actions),
    offer(Backend::collect_import_all_classes_action),
    template_range(Backend::collect_remove_unused_import_actions),
    offer(Backend::collect_sort_use_statements_action),
    offer(Backend::collect_implement_methods_actions),
    offer(Backend::collect_replace_deprecated_actions),
    skip_in_template(Backend::collect_phpstan_actions),
    skip_in_template(Backend::collect_mago_fix_actions),
    offer(Backend::collect_change_visibility_actions),
    offer(Backend::collect_update_docblock_actions),
    offer(Backend::collect_promote_constructor_param_actions),
    offer(Backend::collect_generate_constructor_actions),
    offer(Backend::collect_generate_getter_setter_actions),
    offer(Backend::collect_generate_property_hook_actions),
    offer(Backend::collect_extract_constant_actions),
    offer(Backend::collect_extract_variable_actions),
    skip_in_template(Backend::collect_extract_function_actions),
    offer(Backend::collect_inline_variable_actions),
    offer(Backend::collect_convert_to_instance_variable_actions),
    offer(Backend::collect_simplify_null_actions),
    offer(Backend::collect_convert_to_arrow_function_actions),
    offer(Backend::collect_convert_to_closure_actions),
    offer(Backend::collect_convert_switch_to_match_actions),
    offer(Backend::collect_convert_to_interpolation_actions),
    skip_in_template(Backend::collect_fix_namespace_actions),
    skip_in_template(Backend::collect_fix_class_name_actions),
    offer(Backend::collect_fix_class_case_actions),
    offer(Backend::collect_extract_interface_actions),
    offer_in_resource(Backend::collect_create_missing_view_actions),
    offer_in_resource(Backend::collect_insert_translation_key_actions),
];

impl Backend {
    /// Handle a `textDocument/codeAction` request.
    ///
    /// Returns a list of code actions applicable at the given range.
    /// Expensive actions return a lightweight stub with a [`CodeActionData`]
    /// `data` field and no `edit`; the edit is computed lazily in
    /// [`resolve_code_action`](Self::resolve_code_action).
    pub fn handle_code_action(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
    ) -> Vec<CodeActionResponse> {
        let mut actions = Vec::new();

        // A template is planned against the virtual PHP it lowers to, so
        // the request range moves into it.  The diagnostics in the request
        // context stay in the template's coordinates, which is where the
        // collectors report a template's diagnostics, and what the editor
        // matches an action's diagnostics against.
        let is_template = self.is_blade_file(uri);
        let template = if is_template {
            // Nothing to offer for a template edited since it was lowered.
            let Some(template) = self.template_text(uri) else {
                return actions;
            };
            Some(template)
        } else {
            None
        };
        let template_params = params;
        let translated;
        let params = if is_template {
            let mut moved = params.clone();
            moved.range = self.translate_blade_range_to_php(uri, params.range);
            translated = moved;
            &translated
        } else {
            params
        };

        let adjusted = skip_leading_whitespace(content, params);
        let params = adjusted.as_ref().unwrap_or(params);

        // Inside a request this re-enters the scope `with_file_content`
        // opened; called on its own it still parses the file once for every
        // collector below instead of once each.
        let _scope = self.enter_request_scope_borrowed(content);

        let is_resource = self.is_resource_uri(uri);
        for collector in COLLECTORS {
            if is_template && collector.template == TemplatePolicy::Skip
                || is_resource && !collector.in_resource
            {
                continue;
            }
            let params = match collector.template {
                TemplatePolicy::TemplateRange if is_template => template_params,
                _ => params,
            };
            (collector.collect)(self, uri, content, params, &mut actions);
        }

        // Every collector plans its edits against the PHP a template lowers
        // to; the editor applies them to the template itself, and an action
        // that cannot be moved there whole is not offered.
        let eol = Self::request_line_ending(template.as_ref(), content);
        actions.retain_mut(|action| match action {
            CodeActionResponse::CodeAction(CodeAction {
                edit: Some(edit), ..
            }) => self.finish_action_edit(edit, uri, eol, template.as_ref(), false),
            _ => true,
        });

        // A deferred action can resolve to nothing in a template (an
        // extraction whose statement starts in the PHP an echo lowers to),
        // so a template's are resolved before the list goes out.
        if is_template || !self.caps().code_action_resolve {
            actions.retain_mut(|action| match action {
                CodeActionResponse::CodeAction(action) => {
                    self.inline_deferred_edit(action, content, template.as_ref())
                }
                CodeActionResponse::Command(_) => true,
            });
        }

        actions
    }

    /// The line terminator of the file a request was made in: the
    /// template's own for a template, whose virtual PHP is always `\n`.
    fn request_line_ending(template: Option<&TemplateText>, content: &str) -> &'static str {
        crate::text_position::line_ending(template.map_or(content, TemplateText::template))
    }

    /// Move an action's edit into the coordinates of the files it edits
    /// and give its text their line terminators.  Returns `false` when the
    /// edit cannot be moved into a template whole, and the action has
    /// nothing to offer.
    fn finish_action_edit(
        &self,
        edit: &mut WorkspaceEdit,
        uri: &str,
        eol: &'static str,
        template: Option<&TemplateText>,
        planned_in_template: bool,
    ) -> bool {
        if !self.translate_action_edit(edit, template, planned_in_template) {
            return false;
        }
        self.match_workspace_edit_line_endings(edit, uri, eol);
        true
    }

    /// Fill in the edit of a deferred action for a client that cannot call
    /// `codeAction/resolve`, and drop the `data` it would never use.
    /// Returns `false` when the edit cannot be computed, since the action
    /// would do nothing.
    fn inline_deferred_edit(
        &self,
        action: &mut CodeAction,
        content: &str,
        template: Option<&TemplateText>,
    ) -> bool {
        if action.edit.is_some() {
            return true;
        }
        let Some(data) = Self::code_action_data(action) else {
            return true;
        };
        action.data = None;
        let Some(edit) = self.compute_deferred_edit(action, &data, content, template) else {
            return false;
        };
        action.edit = Some(edit);
        true
    }

    /// Handle a `codeAction/resolve` request.
    ///
    /// The editor sends back a `CodeAction` that was previously returned
    /// by [`handle_code_action`](Self::handle_code_action) with a `data`
    /// field but no `edit`.  This method deserializes the data, computes
    /// the full workspace edit, and returns the completed action.
    ///
    /// For PHPStan quickfixes the matched diagnostic is also eagerly
    /// removed from the cache and updated diagnostics are returned via
    /// the `diagnostics_to_republish` output parameter.
    pub fn resolve_code_action(&self, mut action: CodeAction) -> (CodeAction, Option<String>) {
        let Some(data) = Self::code_action_data(&action) else {
            return (action, None);
        };

        // A template's action was planned against its virtual PHP, so the
        // resolve reads the same text, and has nothing to resolve to once
        // the template no longer matches it.
        let template = self.template_text(&data.uri);
        if template.is_none() && self.is_blade_file(&data.uri) {
            return (action, None);
        }
        let Some(content) = self.analysable_content(&data.uri) else {
            return (action, None);
        };

        let Some(edit) =
            self.compute_deferred_edit(&mut action, &data, &content, template.as_ref())
        else {
            return (action, None);
        };
        action.edit = Some(edit);

        // Only clear diagnostics and republish when the resolve
        // actually produced an edit.  If the file changed between
        // Phase 1 and Phase 2 the resolve may return None, and we
        // must not remove a diagnostic that wasn't actually fixed.
        //
        // This applies to all quickfix actions that attach diagnostics
        // (PHPStan and unused-import alike).  The eager clear+republish
        // removes the squiggly line before the text edit is applied,
        // so the editor doesn't have to guess where to move it.
        let republish_uri = if let Some(ref diags) = action.diagnostics
            && !diags.is_empty()
            && action.edit.is_some()
        {
            if data.action_kind.starts_with("phpstan.")
                || data.action_kind == "refactor.changeVisibility"
            {
                // PHPStan diagnostics live in a separate cache.
                self.clear_phpstan_diagnostics_after_resolve(&data.uri, diags);
            }

            // Push all resolved diagnostics to the suppression list
            // so that `publish_diagnostics_for_file` filters them out.
            // This handles both PHPStan (cached) and native (recomputed)
            // diagnostics uniformly.
            {
                let mut suppressed = self.diag.suppressed.lock();
                suppressed.extend(diags.iter().cloned());
            }

            Some(data.uri.clone())
        } else {
            None
        };

        (action, republish_uri)
    }

    fn code_action_data(action: &CodeAction) -> Option<CodeActionData> {
        serde_json::from_value(action.data.clone()?).ok()
    }

    /// Compute the workspace edit of a deferred action, already translated
    /// for templates.  Returns `None` when the file changed since the
    /// action was offered, the kind is unknown, or the edit cannot be moved
    /// into the template whole.  `template` is the template the action was
    /// offered on, when it was.
    fn compute_deferred_edit(
        &self,
        action: &mut CodeAction,
        data: &CodeActionData,
        content: &str,
        template: Option<&TemplateText>,
    ) -> Option<WorkspaceEdit> {
        // Resolving an action can need types (an extracted function's
        // return type, a docblock's inferred `@return`) and walks the AST
        // several times.  This handler fetches its own file content, so it
        // enters the request scope itself rather than through
        // `with_file_content`.
        let _scope = self.enter_request_scope_borrowed(content);

        let result = match data.action_kind.as_str() {
            // ── PHPStan quickfixes ──────────────────────────────────
            "phpstan.addThrows" => {
                let edit = self.resolve_add_throws(data, content);

                // Adding a @throws tag for an exception resolves the
                // diagnostic for *every* throw of that exception in
                // the same function/method body.  Expand the action's
                // diagnostic list so they all get cleared at once.
                if edit.is_some() {
                    self.expand_sibling_checked_exception_diags(data, content, action);
                }

                edit
            }
            "phpstan.removeThrows" => self.resolve_remove_throws(data, content),
            "phpstan.addOverride" | "phpstan.addReturnTypeWillChange" => {
                self.resolve_add_attribute(data, content)
            }
            "phpstan.addIgnore" => self.resolve_add_ignore(data, content),
            "phpstan.removeIgnore" => self.resolve_remove_ignore(data, content),
            "phpstan.removeOverride" => self.resolve_remove_override(data, content),
            "phpstan.fixPhpDocType.update" | "phpstan.fixPhpDocType.remove" => {
                self.resolve_fix_phpdoc_type(data, content)
            }
            "phpstan.newStatic.addTag"
            | "phpstan.newStatic.finalClass"
            | "phpstan.newStatic.finalConstructor" => self.resolve_new_static(data, content),
            // ── Fix prefixed class name ─────────────────────────────
            "phpstan.fixPrefixedClass" => self.resolve_fix_prefixed_class(data, content),
            // ── Remove always-true assert() ─────────────────────────
            "phpstan.removeAssert" => self.resolve_remove_assert(data, content),
            // ── Fix return type ─────────────────────────────────────
            "phpstan.fixReturnType.stripExpr"
            | "phpstan.fixReturnType.changeTypeToActual"
            | "phpstan.fixReturnType.changeType"
            | "phpstan.fixReturnType.addType"
            | "phpstan.fixReturnType.updateReturnType" => {
                self.resolve_fix_return_type(data, content)
            }
            // ── Remove unused return type ────────────────────────────
            "phpstan.removeUnusedReturnType" => {
                self.resolve_remove_unused_return_type(data, content)
            }
            // ── Add iterable return type ────────────────────────────
            "phpstan.addIterableType" => self.resolve_add_iterable_type(data, content),
            // ── Remove unreachable statement ────────────────────────
            "phpstan.removeUnreachable" => self.resolve_remove_unreachable(data, content),
            // ── Change visibility (parent-aware) ────────────────────
            "refactor.changeVisibility" => self.resolve_change_visibility(data, content),
            // ── Unused import quickfixes ─────────────────────────────
            "quickfix.removeUnusedImport" | "quickfix.removeAllUnusedImports" => {
                self.resolve_remove_unused_import(data, content, action.diagnostics.as_deref())
            }
            // ── Refactoring actions ─────────────────────────────────
            "refactor.extractConstant" | "refactor.extractConstantAll" => {
                self.resolve_extract_constant(data, content)
            }
            "refactor.extractVariable" | "refactor.extractVariableAll" => {
                self.resolve_extract_variable(data, content)
            }
            // ── Import all missing classes ───────────────────────────────
            "source.importAllClasses" => self.resolve_import_all_classes(data, content),
            // ── Sort use statements ──────────────────────────────────────
            "source.sortUseStatements" => self.resolve_sort_use_statements(data, content),
            "refactor.extractFunction" => self.resolve_extract_function(data, content),
            "refactor.extractInterface" => self.resolve_extract_interface(data, content),
            "refactor.inlineVariable" => self.resolve_inline_variable(data, content),
            "refactor.extractInstanceVariable" => {
                self.resolve_convert_to_instance_variable(data, content)
            }
            _ => None,
        };

        let mut edit = result?;
        // A template's unused imports are removed as the directives it
        // writes them as, planned against its own text.
        let planned_in_template = matches!(
            data.action_kind.as_str(),
            "quickfix.removeUnusedImport" | "quickfix.removeAllUnusedImports"
        );
        let eol = Self::request_line_ending(template, content);
        self.finish_action_edit(&mut edit, &data.uri, eol, template, planned_in_template)
            .then_some(edit)
    }

    /// Give the text of every edit in `edit` the line terminator of the
    /// file it goes into.  `eol` is the terminator of `uri`, the file the
    /// action was requested in, and also stands for a file with no content
    /// yet (one the edit creates).
    fn match_workspace_edit_line_endings(
        &self,
        edit: &mut WorkspaceEdit,
        uri: &str,
        eol: &'static str,
    ) {
        use crate::text_position::{line_ending, match_line_ending};

        let eol_of = |target: &Uri| {
            if target.as_str() == uri {
                return eol;
            }
            self.get_file_content_arc(target.as_str())
                .map_or(eol, |content| line_ending(&content))
        };
        visit_file_text_edits(edit, |target, mut edits| {
            let eol = eol_of(target);
            edits.for_each_mut(|edit| match_line_ending(&mut edit.new_text, eol));
        });
    }
}
