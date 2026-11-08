//! Completion inside a string argument: array shape keys, the Laravel
//! string keys (routes, config, views, translations, morph aliases, paths,
//! route and command parameters, request input), Symfony service IDs and
//! container parameters, Yii views, asset bundle files, DI definition keys
//! and CSS classes, WordPress hook names, Eloquent relation and column
//! names, the strings a parameter's declared type asks for, and method
//! names in mock expectations, route-controller groups and array callables.

use tower_lsp_server::gen_lsp_types::CompletionResponse;

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
        if let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.tempest_path_completion(uri, content, position, ctx, code)
        {
            return Some(response);
        }
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
        let is_laravel = self.resolved_class_cache.read().laravel().is_enabled();
        if is_laravel
            && matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_validation_string_completion(uri, content, position, ctx, code)
        {
            return Some(response);
        }
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
                self.try_laravel_string_key_completion_in_file(content, position, code, ctx)
        {
            return Some(response);
        }

        // ── Drupal route parameters ─────────────────────────────
        // The keys of `Url::fromRoute('entity.node.canonical', ['|' => …])`
        // are the placeholders of the named route's path.  Asked first, so
        // that a key after a comma is not read as an argument of the call.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_drupal_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_drupal_route_parameter_completion(content, position, code, ctx)
        {
            return Some(response);
        }

        // ── Drupal permissions ──────────────────────────────────
        // The lists and requirement arrays that name permissions, which
        // the Symfony names below do not read.  An account's
        // `hasPermission('|')` is left to them, which confirm its receiver.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_drupal_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_drupal_permission_completion(uri, content, position, code, ctx)
        {
            return Some(response);
        }

        // ── Drupal configuration names and keys ─────────────────
        // The lists that name configuration objects, a form element's
        // `#config_target`, and the keys a config object's `set()` and
        // `clear()` write.  A `get()`'s keys come with the Symfony names
        // below, which resolve its receiver as a container lookup's.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_drupal_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self
                .try_drupal_config_key_completion(content, position, code, ctx)
                .or_else(|| {
                    self.try_drupal_config_name_completion(uri, content, position, code, ctx)
                })
        {
            return Some(response);
        }

        // ── Drupal render elements ──────────────────────────────
        // `'#type' => '|'` and `$form['name']['#type'] = '|'`.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_drupal_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self
                .try_drupal_render_element_completion(uri, content, position, code)
                .or_else(|| self.try_drupal_display_type_completion(content, position, code))
        {
            return Some(response);
        }

        // ── Drupal theme hooks ──────────────────────────────────
        // `'#theme' => '|'`, a `#theme` or `#theme_wrappers` list, and the
        // `#` keys of a render array whose `#theme` names a hook.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_drupal_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_drupal_theme_hook_completion(content, position, code)
        {
            return Some(response);
        }

        // ── Drupal libraries ────────────────────────────────────
        // `'#attached' => ['library' => ['|']]` and
        // `$build['#attached']['library'][] = '|'`.  Asked before the
        // Symfony names, which would read an item after a comma as an
        // argument of a call around the array.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_drupal_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_drupal_library_completion(uri, content, position, code)
        {
            return Some(response);
        }

        // ── Symfony form options ────────────────────────────────
        // `$builder->add('email', EmailType::class, ['|' => …])`, wherever
        // the form component is used.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_symfony_form_option_completion(content, position, code, ctx)
        {
            return Some(response);
        }

        // ── Yii asset bundle files ──────────────────────────────
        // `public array $css = ['|']` in an asset bundle.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_yii_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_yii_asset_completion(uri, content, position, code, ctx)
        {
            return Some(response);
        }

        // ── Yii aliases ─────────────────────────────────────────
        // `'@|'` in any string, before the names a call's string could
        // otherwise be read as.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_yii_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_yii_alias_completion(content, position, code)
        {
            return Some(response);
        }

        // ── Yii params keys ─────────────────────────────────────
        // `$params['application']['|']` in a configuration file.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_yii_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_yii_param_completion(uri, content, position, code)
        {
            return Some(response);
        }

        // ── Symfony and Yii names ───────────────────────────────
        // `$container->get('|')`, `service('|')`, `#[Autowire(param: '|')]`,
        // `$translator->trans('|')`, in any project that uses Twig,
        // `$twig->render('|')`, and in a Yii one `$urls->generate('|')` and
        // `#[RouteArgument('|')]`.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && (self.reads_service_container() || self.is_twig_project() || self.is_yii_project())
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_symfony_string_key_completion(uri, content, position, code, ctx)
        {
            return Some(response);
        }

        // ── WordPress hook names ─────────────────────────────────
        // `add_action('|')`, `do_action('|')`, and the rest of the hook
        // functions.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_wordpress_hook_completion(uri, content, position, code)
        {
            return Some(response);
        }

        // ── Yii views ───────────────────────────────────────────
        // `$this->viewRenderer->render('|')`, and a path built from
        // `__DIR__` in any render call: `render(__DIR__ . '/|')`.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_yii_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_yii_view_completion(uri, content, position, code, ctx)
        {
            return Some(response);
        }

        // ── CSS classes ─────────────────────────────────────────
        // `Html::div()->class('card |')`, a name at a time.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_yii_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_yii_css_class_completion(content, position, code, ctx)
        {
            return Some(response);
        }

        // ── Yii DI definitions ──────────────────────────────────
        // The keys of a definition, `MailerInterface::class => ['|' …`,
        // and of the arguments its method keys pass by name.
        if self.is_yii_project()
            && matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_yii_definition_completion(content, position, code, ctx)
        {
            return Some(response);
        }

        // ── Yii form fields and hydration maps ──────────────────
        // `Field::text($form, '|')` and the keys of a hydration map,
        // `populate($form, $data, ['|' …`, name properties of the object.
        if self.is_yii_project()
            && matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_yii_form_property_completion(content, position, ctx, code)
        {
            return Some(response);
        }

        // ── Yii route arguments ─────────────────────────────────
        // The keys of `$urls->generate('blog/post', ['|' => …])` are the
        // placeholders of the named route's path.
        if self.is_yii_project()
            && matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_yii_route_param_completion(content, position, code, ctx)
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
            && let Some(response) = self
                .try_eloquent_string_completion(content, position, ctx, code)
                .or_else(|| {
                    self.try_eloquent_model_attribute_completion(content, position, ctx, code)
                })
        {
            return Some(response);
        }

        // ── Completion driven by the parameter's declaration ───
        // `model-property<Model>` asks for one of a model's
        // columns, `view-string` for one of the project's
        // templates, and `#[ExpectedValues]` for one of the strings
        // it lists, none of which the call's own spelling says —
        // only the callee's signature does. Locating the
        // call and resolving its callee is the expensive half, so
        // it happens once here and both strategies read the
        // parameter type it produced.
        if matches!(
            string_ctx,
            StringContext::InStringLiteral | StringContext::NotInString
        ) && let Some(code) = code_ctx.as_ref()
            && let Some((call, param)) = self.typed_string_argument(content, position, ctx, code)
        {
            if let Some(param_type) = &param.type_hint {
                if let Some(response) = self.model_property_completion(&call, param_type, ctx) {
                    return Some(response);
                }
                if is_laravel
                    && let Some(response) =
                        self.view_string_completion(&call, param_type, content, position)
                {
                    return Some(response);
                }
            }
            if let Some(response) =
                self.expected_string_value_completion(&call, &param, content, position)
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

        // ── Mock expectation method names ────────────────────────
        // `$mock->method('|')`, `$mock->shouldReceive('|')`,
        // `Facade::shouldReceive('|')` and the rest name a method of the
        // mocked class.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_mock_method_completion(uri, content, position, ctx, code)
        {
            return Some(response);
        }

        // ── Yii Active Record attributes and relations ──────────
        // `$post->get('|')` names an attribute of the record, and
        // `$post->relation('|')` and `Post::query()->with('|')` a
        // relation.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && self.is_yii_project()
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) =
                self.try_yii_active_record_completion(content, position, ctx, code)
        {
            return Some(response);
        }

        // ── Doctrine repository field names ─────────────────────
        // `$repository->findBy(['|' => …])` and its siblings key their
        // criteria and order by the entity's mapped fields.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_entity_field_completion(content, position, ctx, code)
        {
            return Some(response);
        }

        // ── Doctrine DQL field names ────────────────────────────
        // `'… WHERE u.|'` in a query or a query builder fragment names a
        // field of the entity the variable stands for.
        if matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_dql_field_completion(uri, content, position, ctx, code)
        {
            return Some(response);
        }
        if matches!(string_ctx, StringContext::InStringLiteral)
            && let Some(code) = code_ctx.as_ref()
            && let Some(response) = self.try_dql_entity_completion(uri, content, position, code)
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
