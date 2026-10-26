use super::*;

#[test]
fn route_skeletons_infer_conventional_names_and_leave_ambiguous_parts_editable() {
    for (name, path, controller, method) in [
        ("users.index", "/users", "UserController", "index"),
        ("users.store", "/users", "UserController", "store"),
        ("users.show", "/users/{user}", "UserController", "show"),
        ("s.show", "/s/{s}", "SController", "show"),
        ("users.update", "/users/{user}", "UserController", "update"),
        (
            "users.destroy",
            "/users/{user}",
            "UserController",
            "destroy",
        ),
        ("users.edit", "/users/{user}/edit", "UserController", "edit"),
        ("users.create", "/users/create", "UserController", "create"),
        (
            "admin.blog-posts.publish-now",
            "/admin/blog-posts/publish-now",
            "Admin\\BlogPostController",
            "publishNow",
        ),
        (
            "blog_posts.edit",
            "/blog_posts/{blog_post}/edit",
            "BlogPostController",
            "edit",
        ),
        ("home", "/home", "PlaceholderController", "method"),
        ("99.show", "/path", "PlaceholderController", "method"),
        ("a..show", "/path", "PlaceholderController", "method"),
        ("", "/path", "PlaceholderController", "method"),
    ] {
        assert_eq!(
            route_stub(name),
            format!(
                "\\Illuminate\\Support\\Facades\\Route::get('{path}', [\\App\\Http\\Controllers\\{controller}::class, '{method}'])->name('{name}');"
            )
        );
    }
    let stub = route_stub("user's\\export");
    assert!(stub.ends_with("->name('user\\'s\\\\export');"));
    assert!(stub.contains("PlaceholderController"));
    assert!(
        route_stub("line\r\nbreak").ends_with("->name('line' . \"\\r\" . '' . \"\\n\" . 'break');")
    );
}

fn inserted(content: &str) -> String {
    let (edit, position) = route_insertion(content, "registration();").unwrap();
    let offset = crate::text_position::position_to_byte_offset(content, edit.range.start);
    let mut result = content.to_string();
    result.insert_str(offset, &edit.new_text);
    let route = crate::text_position::position_to_byte_offset(&result, position);
    assert_eq!(
        &result[route..route + "registration();".len()],
        "registration();"
    );
    crate::parser::with_parsed_program(&result, "new_route", |program, _| {
        assert!(program.errors.is_empty(), "{result}: {:?}", program.errors);
    });
    result
}

#[test]
fn insertion_keeps_php_open_and_preserves_target_text() {
    assert_eq!(inserted(""), "<?php\n\nregistration();\n");
    assert_eq!(inserted("<?php\n\n"), "<?php\n\nregistration();\n");
    assert_eq!(inserted("<?php\n"), "<?php\n\nregistration();\n");
    assert_eq!(
        inserted("<?php // trailing comment"),
        "<?php // trailing comment\n\nregistration();\n"
    );
    assert_eq!(
        inserted("<?php\n?>html"),
        "<?php\n\nregistration();\n?>html"
    );
    assert_eq!(
        inserted("<?php existing() ?>html"),
        "<?php existing() ;\n\nregistration();\n?>html"
    );
    assert_eq!(
        inserted("<?php namespace App;\nexisting();\n"),
        "<?php namespace App;\nexisting();\n\nregistration();\n"
    );
    assert_eq!(
        inserted("<?php namespace App {\n    existing();\n}\n"),
        "<?php namespace App {\n    existing();\n\n    registration();\n}\n"
    );
    assert!(inserted("<?php namespace App {}\n").contains("    registration();"));
    assert_eq!(
        inserted("<?php namespace App {\n    existing();\n} ?>"),
        "<?php namespace App {\n    existing();\n\n    registration();\n} ?>"
    );
    assert!(
        inserted("<?php namespace App ?>html")
            .contains("namespace App ;\n\nregistration();\n?>html")
    );
}

#[test]
fn insertion_declines_invalid_or_nonexecuting_targets() {
    for content in [
        "hello",
        "<?php function (",
        "<?php return [];",
        "<?php return []; ?>html",
        "<?php return []; unreachable();",
        "<?php __halt_compiler(); data",
    ] {
        assert!(
            route_insertion(content, "registration();").is_none(),
            "{content}"
        );
    }
}

fn names(content: &str) -> OwnedResolvedNames {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(b"api_context.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolver = mago_names::resolver::NameResolver::new(&arena);
    OwnedResolvedNames::from_resolved(&resolver.resolve(program))
}

#[test]
fn api_context_follows_only_the_enclosing_resolved_resource_chain() {
    let root_alias_available = || true;
    for (content, expected) in [
        (
            "<?php Route::apiResource('users', UserController::class)->missing(fn () => route('missing'));",
            true,
        ),
        (
            "<?php use Illuminate\\Support\\Facades\\Route as Routes; (Routes::apiResources([]))->missing(fn () => route('missing'));",
            true,
        ),
        (
            "<?php namespace App; Route::apiResource('users', UserController::class)->missing(fn () => route('missing'));",
            false,
        ),
        (
            "<?php Route::resource('users', UserController::class)->missing(fn () => route('missing'));",
            false,
        ),
        (
            "<?php Route::apiResource('users', UserController::class); route('missing');",
            false,
        ),
        ("<?php Route::$method(fn () => route('missing'));", false),
        (
            "<?php $router::apiResource(fn () => route('missing'));",
            false,
        ),
    ] {
        assert_eq!(
            api_resource_context(
                content,
                content.find("'missing'").unwrap() as u32,
                Some(&names(content)),
                root_alias_available
            ),
            expected,
            "{content}"
        );
    }
    assert!(!api_resource_context(
        "<?php route('missing');",
        15,
        None,
        root_alias_available
    ));
    let unrelated = "<?php class Route {} Route::apiResource('users', UserController::class)->missing(fn () => route('missing'));";
    assert!(!api_resource_context(
        unrelated,
        unrelated.find("'missing'").unwrap() as u32,
        Some(&names(unrelated)),
        || false
    ));
}
