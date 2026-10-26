use super::*;

fn ctx_at(content: &str, needle: &str) -> Option<RouteParamContext> {
    // Place the cursor right after `needle` (which ends inside a quote).
    let idx = content.find(needle).expect("needle not found") + needle.len();
    let code = crate::completion::source::code_context::code_context_at(content, idx)?;
    detect_context(content, idx, &code)
}

#[test]
fn detects_first_key_of_route_parameters() {
    let content = "<?php\nroute('users.show', ['us']);\n";
    let ctx = ctx_at(content, "['us").expect("should detect");
    assert_eq!(ctx.route_name, "users.show");
    assert_eq!(ctx.prefix, "us");
}

#[test]
fn detects_subsequent_key_of_route_parameters() {
    let content = "<?php\nroute('users.posts', ['user' => 1, 'po']);\n";
    let ctx = ctx_at(content, "'po").expect("should detect");
    assert_eq!(ctx.route_name, "users.posts");
    assert_eq!(ctx.prefix, "po");
}

#[test]
fn detects_to_route_and_signed_route() {
    for source in [
        "<?php\nto_route('users.show', ['']);\n",
        "<?php\nURL::signedRoute('users.show', ['']);\n",
        "<?php\nredirect()->route('users.show', ['']);\n",
    ] {
        let ctx = ctx_at(source, "['").unwrap_or_else(|| panic!("should detect in {source}"));
        assert_eq!(ctx.route_name, "users.show");
    }
}

#[test]
fn detects_parameters_after_an_intervening_argument() {
    // `temporarySignedRoute()` takes the expiration between the name and the
    // parameters, so the array is the third argument.
    for source in [
        "<?php\nURL::temporarySignedRoute('users.show', now()->addMinutes(30), ['']);\n",
        "<?php\nredirect()->temporarySignedRoute('users.show', $expiration, ['']);\n",
    ] {
        let ctx = ctx_at(source, "['").unwrap_or_else(|| panic!("should detect in {source}"));
        assert_eq!(ctx.route_name, "users.show");
    }
}

#[test]
fn detects_key_in_a_multiline_parameters_array() {
    let content = "<?php\nroute('users.show', [\n    'us',\n]);\n";
    let ctx = ctx_at(content, "    'us").expect("should detect across lines");
    assert_eq!(ctx.route_name, "users.show");
    assert_eq!(ctx.prefix, "us");
}

/// A comment between the call and the key may hold any punctuation; it must
/// not be read as part of the expression.
#[test]
fn detects_a_key_below_a_comment_holding_brackets_and_quotes() {
    let content =
        "<?php\nroute('users.show', [   // TODO: check (parameters), don't\n    'us' => 1,\n]);\n";
    let ctx = ctx_at(content, "    'us").expect("should detect past the comment");
    assert_eq!(ctx.route_name, "users.show");
    assert_eq!(ctx.prefix, "us");
}

#[test]
fn ignores_the_route_name_argument_itself() {
    let content = "<?php\nroute('users.');\n";
    assert!(ctx_at(content, "'users.").is_none());
}

#[test]
fn ignores_unrelated_calls() {
    let content = "<?php\nconfig('app.name', ['de']);\n";
    assert!(ctx_at(content, "['de").is_none());
}

#[test]
fn ignores_an_array_of_its_own() {
    // The array belongs to no call, so the `route()` on the line above must
    // not be picked up as its name.
    let content = "<?php\nroute('users.show');\n$parameters = ['us'];\n";
    assert!(ctx_at(content, "['us").is_none());
}

#[test]
fn ignores_to_route_as_a_method() {
    // `to_route()` is a helper function; a same-named method is unrelated.
    let content = "<?php\n$builder->to_route('users.show', ['us']);\n";
    assert!(ctx_at(content, "['us").is_none());
}

/// Register a route file on `backend` so the route enumeration picks it up.
fn backend_with_routes(routes: &str) -> crate::Backend {
    let backend = crate::Backend::new_test();
    let uri = "file:///app/routes/web.php";
    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(routes.to_string()));
    backend.update_ast(uri, routes);
    backend
}

fn labels_at(backend: &crate::Backend, content: &str, needle: &str) -> Vec<String> {
    let idx = content.find(needle).expect("needle not found") + needle.len();
    let position = crate::text_position::offset_to_position(content, idx);
    let code = crate::completion::source::code_context::code_context_at(content, idx)
        .expect("the cursor is in a string literal");
    match backend.try_route_param_completion(content, position, &code) {
        Some(CompletionResponse::CompletionItemList(items)) => {
            items.into_iter().map(|i| i.label).collect()
        }
        Some(CompletionResponse::CompletionList(list)) => {
            list.items.into_iter().map(|i| i.label).collect()
        }
        None => Vec::new(),
    }
}

#[test]
fn completes_uri_parameters_end_to_end() {
    let backend = backend_with_routes(
        "<?php\nRoute::get('/users/{user}/posts/{post}', 'show')->name('users.posts.show');\n",
    );
    let content = "<?php\nroute('users.posts.show', ['']);\n";
    let labels = labels_at(&backend, content, "['");
    assert_eq!(labels, vec!["user".to_string(), "post".to_string()]);
}

#[test]
fn completion_filters_by_typed_prefix() {
    let backend = backend_with_routes(
        "<?php\nRoute::get('/users/{user}/posts/{post}', 'show')->name('users.posts.show');\n",
    );
    let content = "<?php\nroute('users.posts.show', ['po']);\n";
    assert_eq!(
        labels_at(&backend, content, "['po"),
        vec!["post".to_string()]
    );
}

#[test]
fn parameterless_route_offers_nothing() {
    let backend = backend_with_routes("<?php\nRoute::get('/home', 'index')->name('home');\n");
    let content = "<?php\nroute('home', ['']);\n";
    assert!(labels_at(&backend, content, "['").is_empty());
}

#[test]
fn unknown_route_offers_nothing() {
    let backend = backend_with_routes("<?php\nRoute::get('/home', 'index')->name('home');\n");
    let content = "<?php\nroute('nope.at.all', ['']);\n";
    assert!(labels_at(&backend, content, "['").is_empty());
}

#[test]
fn group_prefix_parameters_are_offered() {
    let backend = backend_with_routes(
        "<?php\nRoute::prefix('bakeries')->group(function () {\n    Route::patch('{bakery}/cancel', 'cancel')->name('bakeries.cancel');\n});\n",
    );
    let content = "<?php\nroute('bakeries.cancel', ['']);\n";
    assert_eq!(
        labels_at(&backend, content, "['"),
        vec!["bakery".to_string()]
    );
}
