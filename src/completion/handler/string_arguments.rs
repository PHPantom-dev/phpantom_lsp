//! Completion inside a string argument: array shape keys, the Laravel
//! string keys (routes, config, views, translations, morph aliases, paths,
//! route and command parameters, request input), Eloquent relation and
//! column names, the strings a parameter's declared type asks for, and
//! method names in route-controller groups and array callables.

use tower_lsp::lsp_types::CompletionResponse;

use crate::Backend;
use crate::completion::comment_position::StringContext;

use super::CompletionSite;

impl Backend {
    /// Try the strategies that complete the contents of a string argument
    /// (and, for array shape keys and a few Laravel arguments, the
    /// position where one is about to be typed).
    pub(super) fn string_argument_completion(
        &self,
        site: &CompletionSite,
        string_ctx: StringContext,
    ) -> Option<CompletionResponse> {
        let CompletionSite {
            uri,
            content,
            position,
            cursor_offset,
            ctx,
        } = site;
        let position = *position;

        // ── Cursor's lexical position ───────────────────────────
        // Resolving it is a forward pass over the file, and every
        // string-argument strategy below needs the same answer (the
        // literal being typed in, and the brackets around it), so the
        // scan runs once here and is shared rather than repeated per
        // strategy on every keystroke.
        let code_ctx = crate::completion::source::code_context::code_context_at(
            content,
            *cursor_offset as usize,
        );
        // ── Array shape key completion ───────────────────────────
        // Runs before `InStringLiteral` suppression because in
        // normal code `$arr['` puts the scanner inside a
        // single-quoted string, yet array shape completion is
        // designed to work there.  Skip only in simple
        // interpolation: `"$arr['key']"` does NOT perform array
        // access in PHP (only `"{$arr['key']}"` does).
        if !matches!(string_ctx, StringContext::SimpleInterpolation)
            && let Some(response) = self.try_array_shape_completion(content, position, ctx)
        {
            return Some(response);
        }

        // ── Laravel string key completion (route/config/view/trans) ──
        // Inside `route('|')`, `config('|')`, `view('|')`, `__('|')`,
        // etc., offer matching key names from the project.
        // NB: `is_laravel` is extracted to a `let` so the read lock
        // on `resolved_class_cache` is dropped before calling
        // `try_laravel_string_key_completion`, which may trigger
        // `ensure_workspace_indexed` → `update_ast` → write lock.
        let is_laravel = self.resolved_class_cache.read().is_laravel();
        if is_laravel
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_translation_argument_completion(content, position, code, ctx)
        {
            return Some(response);
        }
        if is_laravel
            && matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(response) = self.try_morph_column_completion(uri, content, position)
        {
            return Some(response);
        }
        if is_laravel
            && matches!(
                string_ctx,
                StringContext::InStringLiteral | StringContext::NotInString
            )
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_laravel_string_key_completion_in_file(content, position, ctx, code)
        {
            return Some(response);
        }

        // ── Path helper completion ──────────────────────────────
        // `base_path('|')` and friends complete a segment at a time
        // from the directory the argument has reached.
        if is_laravel
            && matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_path_helper_completion(content, position, code)
        {
            return Some(response);
        }

        // ── Route parameter completion ──────────────────────────
        // The keys of `route('users.show', ['|' => 1])` are the URI
        // parameters of the named route.
        if is_laravel
            && matches!(
                string_ctx,
                StringContext::InStringLiteral | StringContext::NotInString
            )
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_route_param_completion(content, position, code)
        {
            return Some(response);
        }

        // ── Artisan command parameter completion ────────────────
        // `$this->argument('|')` / `$this->option('|')` against the
        // enclosing command's own signature, and array keys of
        // `Artisan::call('cmd', ['|' => ...])`.
        if is_laravel
            && matches!(
                string_ctx,
                StringContext::InStringLiteral | StringContext::NotInString
            )
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_command_param_completion(content, position, code)
        {
            return Some(response);
        }

        // ── Eloquent relation/column string completion ──────────
        // Like array shape completion, this triggers inside string
        // literals where the cursor is in a method argument position
        // for an Eloquent method that accepts relation or column names.
        if matches!(
            string_ctx,
            StringContext::InStringLiteral | StringContext::NotInString
        ) && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_eloquent_string_completion(content, position, ctx, code)
        {
            return Some(response);
        }

        // ── Completion driven by the parameter's declared type ──
        // `model-property<Model>` asks for one of a model's
        // columns and `view-string` for one of the project's
        // templates, neither of which the call's own spelling
        // says — only the callee's signature does. Locating the
        // call and resolving its callee is the expensive half, so
        // it happens once here and both strategies read the
        // parameter type it produced.
        if matches!(
            string_ctx,
            StringContext::InStringLiteral | StringContext::NotInString
        ) && let Some(code) = code_ctx.as_ref()
            && let Some((call, param_type)) =
                self.typed_string_argument(content, position, ctx, code)
        {
            if let Some(response) = self.model_property_completion(&call, &param_type, ctx) {
                return Some(response);
            }
            if is_laravel
                && let Some(response) =
                    self.view_string_completion(&call, &param_type, content, position)
            {
                return Some(response);
            }
        }

        // ── Request input key completion ────────────────────────
        // `$request->input('|')`, `->has('|')`, `$request['|']`, …
        // offer the field names the validation rules in scope define.
        // Runs after the Eloquent strategies because `has()` is also a
        // relation method: a Builder subject resolves there first, and
        // only a request-typed subject falls through to here.
        if is_laravel
            && matches!(
                string_ctx,
                StringContext::InStringLiteral | StringContext::NotInString
            )
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_request_input_key_completion(uri, content, position, ctx, code)
        {
            return Some(response);
        }

        // ── Laravel route controller method completion ─────────
        // Inside `Route::controller(X::class)->group(fn(){…})`,
        // the 2nd argument string of Route::get/post/patch/… is a
        // controller method name.
        if matches!(
            string_ctx,
            StringContext::InStringLiteral | StringContext::NotInString
        ) && let Some(response) =
            self.try_laravel_route_controller_completion(uri, content, position, ctx)
        {
            return Some(response);
        }

        // ── Array callable method completion ────────────────────
        // Like array shape and Eloquent string completion, this
        // triggers inside string literals — specifically the
        // method-name string in `[Class::class, 'method']`.
        if matches!(
            string_ctx,
            StringContext::InStringLiteral | StringContext::NotInString
        ) && let Some(response) = self.try_array_callable_completion(uri, content, position, ctx)
        {
            return Some(response);
        }

        None
    }
}
