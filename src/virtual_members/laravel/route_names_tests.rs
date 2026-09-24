use super::*;

/// The route file the helpers below pretend to be reading.
fn test_uri() -> Url {
    Url::parse("file:///app/routes/web.php").unwrap()
}

/// Resolve `target` against a single route file, as `resolve_route_definitions`
/// does per file (without the workspace walk).
fn definitions_of(
    content: &str,
    target: &str,
    uri: &Url,
    file_path: Option<&Path>,
    workspace_root: Option<&Path>,
    macros: &MacroScope,
) -> Vec<Location> {
    let mut sink = RouteDefinitions {
        target,
        out: Vec::new(),
    };
    walk_route_file(content, uri, file_path, workspace_root, macros, &mut sink);
    sink.out
}

/// Collect the routes of a single route file, as `enumerate_all_routes` does
/// per file (without the workspace walk).
fn routes_of(content: &str) -> Vec<RouteEntry> {
    routes_and_prefixes(content).0
}

fn routes_and_prefixes(content: &str) -> (Vec<RouteEntry>, Vec<String>) {
    let (routes, prefixes, _) = routes_and_open_names(content);
    (routes, prefixes)
}

fn routes_and_open_names(content: &str) -> (Vec<RouteEntry>, Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut open_prefixes = Vec::new();
    let mut open_suffixes = Vec::new();
    walk_route_file(
        content,
        &test_uri(),
        None,
        None,
        &MacroScope::default(),
        &mut RouteCollector {
            out: &mut out,
            open_prefixes: &mut open_prefixes,
            open_suffixes: &mut open_suffixes,
        },
    );
    (out, open_prefixes, open_suffixes)
}

/// Collect the routes of a route file that can call the router macros
/// `macro_source` registers.
///
/// Each name in `names` is located the way the macro index locates it: at the
/// closure passed to `Route::macro('<name>', …)`.  The source is written to a
/// real file because a macro body is read from the file it was written in.
fn routes_with_macros(content: &str, macro_source: &str, names: &[&str]) -> Vec<RouteEntry> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider.php");
    std::fs::write(&path, macro_source).unwrap();
    let uri = Url::from_file_path(&path).unwrap().to_string();

    let scope = MacroScope {
        bodies: names
            .iter()
            .map(|name| {
                let call = format!("macro('{name}', ");
                let offset = macro_source
                    .find(&call)
                    .unwrap_or_else(|| panic!("no registration of macro {name}"))
                    + call.len();
                (name.to_ascii_lowercase(), (uri.clone(), offset as u32))
            })
            .collect(),
        ..MacroScope::default()
    };

    let mut out = Vec::new();
    let mut open_prefixes = Vec::new();
    let mut open_suffixes = Vec::new();
    walk_route_file(
        content,
        &test_uri(),
        None,
        None,
        &scope,
        &mut RouteCollector {
            out: &mut out,
            open_prefixes: &mut open_prefixes,
            open_suffixes: &mut open_suffixes,
        },
    );
    out
}

fn uri_of(content: &str, name: &str) -> String {
    routes_of(content)
        .into_iter()
        .find(|route| route.name == name)
        .unwrap_or_else(|| panic!("route {name} not collected"))
        .uri
}

#[test]
fn records_uri_of_simple_registration() {
    let content = "<?php\nRoute::get('/users/{user}', 'show')->name('users.show');\n";
    assert_eq!(uri_of(content, "users.show"), "users/{user}");
}

#[test]
fn root_uri_is_reported_as_slash() {
    let content = "<?php\nRoute::get('/', fn() => 'home')->name('home');\n";
    assert_eq!(uri_of(content, "home"), "/");
}

#[test]
fn records_uri_through_intermediate_chain_links() {
    let content = "<?php\nRoute::middleware('auth')->get('/orders/{order}', 'show')\n    ->whereNumber('order')->name('orders.show');\n";
    assert_eq!(uri_of(content, "orders.show"), "orders/{order}");
}

#[test]
fn records_uri_of_match_registration() {
    let content =
        "<?php\nRoute::match(['get', 'post'], '/search/{term?}', 'run')->name('search');\n";
    assert_eq!(uri_of(content, "search"), "search/{term?}");
}

#[test]
fn applies_fluent_group_uri_prefix() {
    let content = "<?php\nRoute::prefix('admin')->name('admin.')->group(function () {\n    Route::get('users/{user}', 'show')->name('users.show');\n});\n";
    assert_eq!(uri_of(content, "admin.users.show"), "admin/users/{user}");
}

#[test]
fn applies_nested_group_uri_prefixes() {
    let content = "<?php\nRoute::prefix('api')->group(function () {\n    Route::prefix('v1/{tenant}')->group(function () {\n        Route::get('/teams/{team}', 'show')->name('teams.show');\n    });\n});\n";
    assert_eq!(
        uri_of(content, "teams.show"),
        "api/v1/{tenant}/teams/{team}"
    );
}

#[test]
fn applies_chain_uri_prefix_without_a_group() {
    let content =
        "<?php\nRoute::prefix('{tenant}')->get('/users/{user}', 'show')->name('users.show');\n";
    assert_eq!(uri_of(content, "users.show"), "{tenant}/users/{user}");
}

#[test]
fn applies_array_group_uri_prefix() {
    let content = "<?php\nRoute::group(['prefix' => 'admin', 'as' => 'admin.'], function () {\n    Route::patch('/posts/{post}', 'update')->name('posts.update');\n});\n";
    assert_eq!(uri_of(content, "admin.posts.update"), "admin/posts/{post}");
}

#[test]
fn unrecoverable_uri_is_left_empty() {
    // A variable URI cannot be read from the source text.
    let content = "<?php\nRoute::get($uri, 'show')->name('dynamic');\n";
    assert_eq!(uri_of(content, "dynamic"), "");
}

#[test]
fn derives_resource_route_uris() {
    let content = "<?php\nRoute::resource('photos', PhotoController::class);\n";
    assert_eq!(uri_of(content, "photos.index"), "photos");
    assert_eq!(uri_of(content, "photos.create"), "photos/create");
    assert_eq!(uri_of(content, "photos.store"), "photos");
    assert_eq!(uri_of(content, "photos.show"), "photos/{photo}");
    assert_eq!(uri_of(content, "photos.edit"), "photos/{photo}/edit");
    assert_eq!(uri_of(content, "photos.update"), "photos/{photo}");
    assert_eq!(uri_of(content, "photos.destroy"), "photos/{photo}");
}

#[test]
fn api_resource_omits_create_and_edit() {
    let content = "<?php\nRoute::apiResource('photos', PhotoController::class);\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert!(!names.contains(&"photos.create".to_string()), "{names:?}");
    assert!(!names.contains(&"photos.edit".to_string()), "{names:?}");
    assert_eq!(uri_of(content, "photos.show"), "photos/{photo}");
}

#[test]
fn nested_resource_singularizes_each_parent_segment() {
    let content = "<?php\nRoute::resource('photos.comments', CommentController::class);\n";
    assert_eq!(
        uri_of(content, "photos.comments.index"),
        "photos/{photo}/comments"
    );
    assert_eq!(
        uri_of(content, "photos.comments.show"),
        "photos/{photo}/comments/{comment}"
    );
    assert_eq!(
        uri_of(content, "photos.comments.edit"),
        "photos/{photo}/comments/{comment}/edit"
    );
}

#[test]
fn shallow_nested_resource_drops_parent_segments() {
    // The routes that identify the child by its own id lose the parent
    // segments from their name as well as their URI.
    let content =
        "<?php\nRoute::resource('photos.comments', CommentController::class)->shallow();\n";
    assert_eq!(
        uri_of(content, "photos.comments.create"),
        "photos/{photo}/comments/create"
    );
    assert_eq!(uri_of(content, "comments.show"), "comments/{comment}");
    assert_eq!(uri_of(content, "comments.edit"), "comments/{comment}/edit");
}

#[test]
fn parameters_override_the_derived_wildcard() {
    let content = "<?php\nRoute::resource('photos', PhotoController::class)\n    ->parameters(['photos' => 'grid']);\n";
    assert_eq!(uri_of(content, "photos.show"), "photos/{grid}");
}

#[test]
fn hyphenated_resource_wildcard_uses_underscores() {
    let content = "<?php\nRoute::resource('blog-posts', PostController::class);\n";
    assert_eq!(uri_of(content, "blog-posts.show"), "blog-posts/{blog_post}");
}

/// The enclosing group's prefix reaches the resource, but a `->prefix()` on
/// the registration's own chain does not: `ResourceRegistrar` builds its
/// action from `as`/`uses`/`middleware`/`where`/`missing` and never copies
/// `prefix` across, so Laravel silently drops it.
#[test]
fn resource_uri_inherits_the_group_prefix_but_not_the_chain_prefix() {
    let content = "<?php\nRoute::prefix('admin')->name('admin.')->group(function () {\n    Route::prefix('v2')->resource('photos', PhotoController::class);\n});\n";
    assert_eq!(uri_of(content, "admin.photos.show"), "admin/photos/{photo}");
}

/// `->as()` and `->name()` ahead of the registration prefix every generated
/// route name, and the last one on the chain wins.
#[test]
fn a_chain_as_prefix_reaches_the_generated_route_names() {
    let content = "<?php\nRoute::as('admin')->resource('photos', PhotoController::class);\n";
    assert_eq!(uri_of(content, "admin.photos.show"), "photos/{photo}");

    let replaced = "<?php\nRoute::as('a')->as('b')->resource('photos', PhotoController::class);\n";
    assert_eq!(uri_of(replaced, "b.photos.show"), "photos/{photo}");
}

/// The registrar appends its own separator, so the trailing dot users write
/// out of habit produces a doubled one rather than being absorbed.
#[test]
fn a_chain_name_prefix_keeps_a_trailing_dot_the_user_wrote() {
    let content = "<?php\nRoute::name('admin.')->resource('photos', PhotoController::class);\n";
    assert_eq!(uri_of(content, "admin..photos.show"), "photos/{photo}");
}

#[test]
fn slash_in_resource_name_becomes_a_uri_prefix() {
    // Laravel registers `photos/comments` as the resource `comments` under
    // the URI prefix `photos`, so the names are not prefixed with `photos.`.
    let content = "<?php\nRoute::resource('photos/comments', CommentController::class);\n";
    assert_eq!(
        uri_of(content, "comments.show"),
        "photos/comments/{comment}"
    );
}

#[test]
fn only_and_except_still_filter_the_generated_routes() {
    let only =
        "<?php\nRoute::resource('photos', PhotoController::class)->only(['index', 'show']);\n";
    let names: Vec<String> = routes_of(only).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["photos.index", "photos.show"]);
    assert_eq!(uri_of(only, "photos.show"), "photos/{photo}");

    // The filter is found even behind another chain link.
    let except = "<?php\nRoute::resource('photos', PhotoController::class)\n    ->middleware('auth')->except(['create', 'edit', 'update', 'destroy']);\n";
    let names: Vec<String> = routes_of(except).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["photos.index", "photos.store", "photos.show"]);
}

#[test]
fn individual_string_args_filter_the_generated_routes() {
    // `->only()` also accepts its suffixes as separate arguments.
    let content =
        "<?php\nRoute::resource('photos', PhotoController::class)->only('index', 'show');\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["photos.index", "photos.show"]);
}

#[test]
fn nested_parameters_override_a_parent_wildcard() {
    let content = "<?php\nRoute::resource('photos.comments', CommentController::class)\n    ->parameters(['photos' => 'grid']);\n";
    assert_eq!(
        uri_of(content, "photos.comments.show"),
        "photos/{grid}/comments/{comment}"
    );
}

#[test]
fn chains_that_register_no_resource_contribute_nothing() {
    // A method chain on something that is not a registration must not be
    // mistaken for one, however deep the receiver goes.
    assert!(routes_of("<?php\n$router->middleware('auth')->boot();\n").is_empty());
}

#[test]
fn an_unrecognized_modifier_leaves_the_registration_intact() {
    // A dynamic method name cannot be matched against the known modifiers, so
    // it is skipped rather than discarding the registration.
    let content = "<?php\nRoute::resource('photos', C::class)->{$modifier}();\n";
    assert_eq!(uri_of(content, "photos.show"), "photos/{photo}");
}

#[test]
fn legacy_array_parameters_override_the_wildcard() {
    let content = "<?php\nRoute::resource('photos', PhotoController::class)\n    ->parameters(array('photos' => 'grid'));\n";
    assert_eq!(uri_of(content, "photos.show"), "photos/{grid}");
}

#[test]
fn a_non_array_parameters_argument_leaves_the_wildcard_derived() {
    // Laravel's `->parameters('singular')` string form asks for exactly the
    // singularization that is already the default here.
    let singular =
        "<?php\nRoute::resource('photos', PhotoController::class)->parameters('singular');\n";
    assert_eq!(uri_of(singular, "photos.show"), "photos/{photo}");

    let dynamic = "<?php\nRoute::resource('photos', PhotoController::class)->parameters($map);\n";
    assert_eq!(uri_of(dynamic, "photos.show"), "photos/{photo}");
}

#[test]
fn computed_parameters_entries_leave_the_wildcard_derived() {
    // An entry whose key or value is not a literal contributes nothing rather
    // than a wrong wildcard, and an element without a key is not an override.
    let dynamic_value =
        "<?php\nRoute::resource('photos', C::class)->parameters(['photos' => $name]);\n";
    assert_eq!(uri_of(dynamic_value, "photos.show"), "photos/{photo}");

    let dynamic_key = "<?php\nRoute::resource('photos', C::class)->parameters([$key => 'grid']);\n";
    assert_eq!(uri_of(dynamic_key, "photos.show"), "photos/{photo}");

    let keyless = "<?php\nRoute::resource('photos', C::class)->parameters(['grid']);\n";
    assert_eq!(uri_of(keyless, "photos.show"), "photos/{photo}");
}

#[test]
fn single_parameter_call_overrides_the_wildcard() {
    let content = "<?php\nRoute::resource('photos', PhotoController::class)\n    ->parameter('photos', 'grid');\n";
    assert_eq!(uri_of(content, "photos.show"), "photos/{grid}");

    // Both arguments are required for an override; a partial call is ignored.
    let partial = "<?php\nRoute::resource('photos', C::class)->parameter('photos');\n";
    assert_eq!(uri_of(partial, "photos.show"), "photos/{photo}");
}

#[test]
fn array_form_group_prefixes_apply_to_a_resource() {
    let content = "<?php\nRoute::group(['prefix' => 'admin', 'as' => 'admin.'], function () {\n    Route::resource('photos', PhotoController::class);\n});\n";
    assert_eq!(uri_of(content, "admin.photos.show"), "admin/photos/{photo}");
}

#[test]
fn api_resource_member_routes_share_one_uri() {
    let content = "<?php\nRoute::apiResource('photos', PhotoController::class);\n";
    assert_eq!(uri_of(content, "photos.index"), "photos");
    assert_eq!(uri_of(content, "photos.store"), "photos");
    assert_eq!(uri_of(content, "photos.update"), "photos/{photo}");
    assert_eq!(uri_of(content, "photos.destroy"), "photos/{photo}");
}

#[test]
fn unrecoverable_resource_name_generates_no_routes() {
    // A variable or empty name yields nothing rather than a bogus route.
    assert!(routes_of("<?php\nRoute::resource($name, PhotoController::class);\n").is_empty());
    assert!(routes_of("<?php\nRoute::resource('', PhotoController::class);\n").is_empty());
    assert!(routes_of("<?php\nRoute::resource('.', PhotoController::class);\n").is_empty());
}

/// `->name()` on a resource registration is Laravel's per-method name
/// override (`name($method, $name)`), not a route name of its own.  The
/// override is the *whole* name, so it replaces `photos.index` rather than
/// being appended to it.
#[test]
fn a_resource_chain_name_overrides_one_methods_route_name() {
    let content =
        "<?php\nRoute::resource('photos', PhotoController::class)->name('index', 'listing');\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert!(
        names.contains(&"listing".to_string()),
        "the override should name the index route, got {names:?}"
    );
    assert!(
        !names.contains(&"photos.index".to_string()),
        "the override should replace the derived name, got {names:?}"
    );
    assert!(
        !names.contains(&"index".to_string()),
        "the first argument is a method name, not a route name, got {names:?}"
    );
    // The other six keep their derived names.
    assert!(names.contains(&"photos.show".to_string()), "{names:?}");
}

/// `->names('images')` renames the resource every route is derived from,
/// while a per-method entry bypasses the `->as()` prefix entirely.
#[test]
fn names_rewrites_the_resource_the_routes_are_derived_from() {
    let content =
        "<?php\nRoute::as('admin')->resource('photos', PhotoController::class)->names('images');\n";
    assert_eq!(uri_of(content, "admin.images.show"), "photos/{photo}");

    let per_method = "<?php\nRoute::as('admin')->resource('photos', PhotoController::class)->name('index', 'x');\n";
    let names: Vec<String> = routes_of(per_method).into_iter().map(|r| r.name).collect();
    assert!(names.contains(&"x".to_string()), "{names:?}");
}

/// `getResourceMethods()` intersects with `only` and *then* subtracts
/// `except`; neither cancels the other out.
#[test]
fn only_and_except_are_both_applied() {
    let content = "<?php\nRoute::resource('photos', PhotoController::class)->only(['index', 'create'])->except(['create']);\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["photos.index".to_string()]);
}

/// An empty `->only([])` restricts to nothing, which is not the same as
/// never having called it.
#[test]
fn an_empty_only_registers_no_routes() {
    let content = "<?php\nRoute::resource('photos', PhotoController::class)->only([]);\n";
    assert!(routes_of(content).is_empty());
}

/// `apiResource()` is an implicit `only` of the five API methods, so an
/// explicit `->only()` replaces it and can bring `create` back.
#[test]
fn an_explicit_only_replaces_the_api_resource_restriction() {
    let content =
        "<?php\nRoute::apiResource('photos', PhotoController::class)->only(['create']);\n";
    assert_eq!(uri_of(content, "photos.create"), "photos/create");

    // `->except()` narrows the API set instead of replacing it.
    let except = "<?php\nRoute::apiResource('photos', PhotoController::class)->except(['show']);\n";
    let names: Vec<String> = routes_of(except).into_iter().map(|r| r.name).collect();
    assert!(!names.contains(&"photos.create".to_string()), "{names:?}");
    assert!(!names.contains(&"photos.show".to_string()), "{names:?}");
    assert!(names.contains(&"photos.index".to_string()), "{names:?}");
}

/// `->shallow(false)` turns shallow routing back off.
#[test]
fn shallow_reads_its_argument() {
    let off =
        "<?php\nRoute::resource('photos.comments', CommentController::class)->shallow(false);\n";
    assert_eq!(
        uri_of(off, "photos.comments.show"),
        "photos/{photo}/comments/{comment}"
    );

    let on = "<?php\nRoute::resource('photos.comments', CommentController::class)->shallow();\n";
    assert_eq!(uri_of(on, "comments.show"), "comments/{comment}");
}

/// `->parameters()` replaces the whole map and `->parameter()` appends to
/// it, so whichever came last on the chain is the one that applies.
#[test]
fn the_last_parameter_override_for_a_segment_wins() {
    let content = "<?php\nRoute::resource('photos', PhotoController::class)->parameters(['photos' => 'grid'])->parameter('photos', 'other');\n";
    assert_eq!(uri_of(content, "photos.show"), "photos/{other}");
}

/// Laravel deletes the last segment's wildcard from the nested URI without
/// anchoring the deletion, so segments that singularize alike collapse.
#[test]
fn a_repeated_wildcard_collapses_the_nested_uri() {
    let content = "<?php\nRoute::resource('company.companies', CompanyController::class);\n";
    assert_eq!(
        uri_of(content, "company.companies.show"),
        "company/companies/{company}"
    );
}

#[test]
fn resource_registered_on_a_router_variable_is_collected() {
    // `Route::group([], function ($router) { … })` hands the router to the
    // closure, and older code registers resources on it directly.
    let content = "<?php\nRoute::group([], function ($router) {\n    $router->resource('photos', PhotoController::class);\n});\n";
    assert_eq!(uri_of(content, "photos.show"), "photos/{photo}");
}

#[test]
fn extracts_parameter_names() {
    assert_eq!(
        route_uri_parameters("users/{user}/posts/{post}"),
        vec!["user", "post"]
    );
}

#[test]
fn strips_optional_marker_and_binding_field() {
    assert_eq!(
        route_uri_parameters("posts/{post:slug}/comments/{comment?}"),
        vec!["post", "comment"]
    );
}

#[test]
fn parameterless_uri_yields_no_parameters() {
    assert!(route_uri_parameters("/").is_empty());
    assert!(route_uri_parameters("admin/users").is_empty());
    // Unterminated braces must not loop or panic.
    assert!(route_uri_parameters("users/{user").is_empty());
}

/// `RouteRegistrar::group()` returns the registrar, so a `->group()` and a
/// `->resource()` can share one chain.  The resource must not swallow the
/// group's body, or its routes vanish from completion and every `route()`
/// call naming one is reported as unknown.
#[test]
fn a_group_sharing_the_resource_chain_still_registers_its_routes() {
    let content = "<?php\nRoute::prefix('admin')->group(function () {\n    Route::get('/dashboard', 'index')->name('dashboard');\n})->resource('photos', PhotoController::class);\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert!(names.contains(&"dashboard".to_string()), "{names:?}");
    assert!(names.contains(&"photos.show".to_string()), "{names:?}");
    assert_eq!(uri_of(content, "dashboard"), "admin/dashboard");
}

/// Route files guard registrations behind configuration (`if ($domain)`) and
/// register variants from loops.  Skipping those bodies loses every route
/// they declare, so each `route()` call naming one reads as unknown.
#[test]
fn registrations_inside_control_flow_are_collected() {
    let content = "<?php\nif ($fqdn) {\n    Route::domain($fqdn)->group(function () {\n        Route::middleware('kiosk')->name('kiosk.')->group(function () {\n            Route::get('/register', 'register')->name('register');\n        });\n    });\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["kiosk.register".to_string()]);
}

#[test]
fn registrations_inside_a_foreach_body_are_collected() {
    let content = "<?php\nforeach ($locales as $locale) {\n    Route::get('/about', 'about')->name('about');\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["about".to_string()]);
}

/// A route file that registers one route per entry of a literal array names
/// each of them by interpolation.  The loop variables are statically known,
/// so the names are too, and nested loops compose.
#[test]
fn names_interpolated_from_a_literal_foreach_are_collected() {
    let content = "<?php
$events = ['black-friday' => ['perfume-her', 'k-beauty'], 'valentines' => ['perfume']];

foreach ($events as $event => $subcategories) {
    Route::get(\"/{$event}\", [EventsController::class, 'landing'])
        ->name(\"events.{$event}.landing\");

    foreach ($subcategories as $subcategory) {
        Route::get(\"/{$event}/{$subcategory}\", [EventsController::class, 'sub'])
            ->name(\"events.{$event}.{$subcategory}\");
    }
}
";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(
        names,
        vec![
            "events.black-friday.landing",
            "events.black-friday.perfume-her",
            "events.black-friday.k-beauty",
            "events.valentines.landing",
            "events.valentines.perfume",
        ]
    );
    assert_eq!(
        uri_of(content, "events.black-friday.k-beauty"),
        "black-friday/k-beauty"
    );
}

/// The loop value is a `list`, so the key the name is built from is the
/// element's index.
#[test]
fn names_interpolated_from_a_list_use_the_element_index() {
    let content = "<?php\nforeach (['first', 'second'] as $index => $step) {\n    Route::get('/step', 'run')->name('wizard.' . $index . '.' . $step);\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["wizard.0.first", "wizard.1.second"]);
}

/// Route data is often a list of rows rather than a flat array, so a name
/// built from one of a row's fields has to be read out of the row.
#[test]
fn names_read_a_field_of_the_looped_row() {
    let content = "<?php\nforeach ([['slug' => 'faq'], ['slug' => 'terms']] as $page) {\n    Route::get(\"/{$page['slug']}\", 'show')->name(\"pages.{$page['slug']}\");\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["pages.faq", "pages.terms"]);
    assert_eq!(uri_of(content, "pages.terms"), "terms");
}

/// A value the evaluator cannot fold contributes no name, while the entries
/// around it still do.  A partial name would be worse than none: it would
/// report a route that does not exist and hide one that does.
#[test]
fn an_unevaluable_element_contributes_no_name() {
    let content = "<?php\nforeach (['gifts', slug('/xmas/gift-sets')] as $section) {\n    Route::get(\"/{$section}\", 'show')->name(\"shop.{$section}\");\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["shop.gifts"]);
}

/// A loop that normalizes its element with a pure string function before
/// naming the route still yields a known name, rather than leaving every
/// name built from `$slug` unknown.
#[test]
fn preg_replace_normalizes_a_loop_variable_before_naming_the_route() {
    let content = "<?php
foreach (['/xmas/gift-sets', '/xmas/decorations'] as $subcategory) {
    $slug = preg_replace('#^/xmas/#', '', $subcategory);

    Route::get('/xmas/' . $slug, 'page')
        ->name('events.xmas.' . $slug);
}
";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(
        names,
        vec!["events.xmas.gift-sets", "events.xmas.decorations"]
    );
    assert_eq!(uri_of(content, "events.xmas.gift-sets"), "xmas/gift-sets");
}

/// A pattern using a bracket delimiter, or a replacement containing a
/// backreference, is not folded: the two are not worth the complexity of
/// nested delimiter matching and PHP-to-`regex`-crate backreference
/// translation, so the name they would produce stays unknown.
#[test]
fn an_unsupported_preg_replace_form_leaves_the_name_unknown() {
    let bracket_delimiter =
        "<?php\n$slug = preg_replace('(a)', 'b', 'a');\nRoute::get('/x', 'x')->name($slug);\n";
    assert!(routes_of(bracket_delimiter).is_empty());

    let backreference = r"<?php
$slug = preg_replace('/(a)/', '\1\1', 'a');
Route::get('/x', 'x')->name($slug);
";
    assert!(routes_of(backreference).is_empty());
}

/// `str_replace` folds with both scalar and array search/replace arguments,
/// pairing array entries positionally and padding a short `$replace` with
/// `""` exactly as PHP does.
#[test]
fn str_replace_folds_into_a_route_name() {
    let content = "<?php\n$slug = str_replace(['_', ' '], '-', 'blog_posts recent');\nRoute::get('/x', 'x')->name($slug);\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["blog-posts-recent"]);
}

/// `trim`/`ltrim`/`rtrim`, `strtolower`/`strtoupper`, and `ucfirst` all fold,
/// composing through concatenation the same way a plain string builder does.
#[test]
fn trim_and_case_functions_fold_into_a_route_name() {
    let content = "<?php
$name = ucfirst(strtolower(trim(' Blog ')));
Route::get('/x', 'x')->name($name . '.' . ltrim('//posts', '/') . '.' . rtrim('index//', '/'));
";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["Blog.posts.index"]);
}

/// `implode` folds a literal array of scalars, and `sprintf` folds its `%s`
/// and zero-padded `%d` specifiers.
#[test]
fn implode_and_sprintf_fold_into_a_route_name() {
    let content = "<?php
$name = implode('.', ['shop', 'section']) . '.' . sprintf('%s-%03d', 'item', 7);
Route::get('/x', 'x')->name($name);
";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["shop.section.item-007"]);
}

/// A call to a function outside the fixed list contributes nothing, rather
/// than being mistaken for one of the folded ones.
#[test]
fn a_call_to_an_unlisted_function_leaves_the_name_unknown() {
    let content = "<?php\n$slug = strrev('x');\nRoute::get('/x', 'x')->name($slug);\n";
    assert!(routes_of(content).is_empty());
}

/// A loop over something that is not a literal array still walks its body
/// once, and the interpolated name stays unknown rather than being guessed.
#[test]
fn a_non_literal_loop_subject_yields_no_interpolated_name() {
    let content = "<?php\nforeach (config('shop.sections') as $section) {\n    Route::get(\"/{$section}\", 'show')->name(\"shop.{$section}\");\n    Route::get('/all', 'all')->name('shop.all');\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["shop.all"]);
}

/// The loop variable goes out of scope with the loop, so a later
/// registration cannot pick up the last element it held.
#[test]
fn a_loop_variable_does_not_leak_past_its_loop() {
    let content = "<?php\nforeach (['a', 'b'] as $section) {\n    Route::get(\"/{$section}\", 'show')->name(\"shop.{$section}\");\n}\nRoute::get('/rest', 'rest')->name(\"shop.{$section}\");\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["shop.a", "shop.b"]);
}

/// A registration kept in a variable so later lines can extend it still
/// declares its route.
#[test]
fn a_registration_assigned_to_a_variable_is_collected() {
    let content = "<?php\n$dashboard = Route::get('/dashboard', 'index')->name('dashboard');\n";
    assert_eq!(uri_of(content, "dashboard"), "dashboard");
}

/// Go-to-definition on a route named in a loop lands on the `->name()`
/// argument that produced it.  An interpolated name has no single literal to
/// point inside, so the whole argument is the target.
#[test]
fn definition_resolves_a_name_built_in_a_loop() {
    let content = "<?php\nforeach (['faq', 'terms'] as $page) {\n    Route::get(\"/{$page}\", 'show')->name(\"pages.{$page}\");\n}\n";
    let uri = Url::parse("file:///app/routes/web.php").unwrap();

    let macros = MacroScope::default();
    let found = definitions_of(content, "pages.terms", &uri, None, None, &macros);
    let line = content.lines().nth(2).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].range.start.line, 2);
    assert_eq!(
        found[0].range.start.character,
        line.find("\"pages.").unwrap() as u32
    );

    assert!(definitions_of(content, "pages.missing", &uri, None, None, &macros).is_empty());
}

#[test]
fn registrations_inside_a_try_block_are_collected() {
    let content = "<?php\ntry {\n    Route::get('/health', 'health')->name('health');\n} catch (Throwable $e) {\n    Route::get('/down', 'down')->name('down');\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["health".to_string(), "down".to_string()]);
}

/// A `RouteServiceProvider` registers routes from a method body rather than
/// at the top level of a route file, so the provider is itself a route
/// source and its methods must be walked.
#[test]
fn registrations_inside_a_provider_method_are_collected() {
    let content = "<?php\nfinal class RouteServiceProvider extends ServiceProvider {\n    public function map(): void {\n        Route::middleware('web')->name('admin.')->group(function () {\n            Route::get('/dashboard', 'index')->name('dashboard');\n        });\n    }\n}\n";
    let names: Vec<String> = routes_of(content).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["admin.dashboard".to_string()]);
}

// ─── Included route files ────────────────────────────────────────────────────

/// Collect the routes of a route file that lives on disk, which is what the
/// files it pulls in have to be read relative to.
fn routes_of_file(path: &Path, workspace_root: &Path) -> Vec<RouteEntry> {
    let content = std::fs::read_to_string(path).unwrap();
    let mut out = Vec::new();
    let mut open_prefixes = Vec::new();
    let mut open_suffixes = Vec::new();
    walk_route_file(
        &content,
        &Url::from_file_path(path).unwrap(),
        Some(path),
        Some(workspace_root),
        &MacroScope::default(),
        &mut RouteCollector {
            out: &mut out,
            open_prefixes: &mut open_prefixes,
            open_suffixes: &mut open_suffixes,
        },
    );
    out
}

/// Write `files` (relative paths) into a fresh workspace and return it.
fn workspace_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (relative, content) in files {
        let path = dir.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }
    dir
}

/// A route file that keeps the sub-file's path in a variable pulls it in just
/// as one that writes the concatenation inline does.
#[test]
fn a_group_target_held_in_a_variable_is_followed() {
    let dir = workspace_with(&[
        (
            "routes/web.php",
            "<?php\n$api = __DIR__.'/api.php';\nRoute::group(['prefix' => 'v1'], $api);\n",
        ),
        (
            "routes/api.php",
            "<?php\nRoute::get('/ping', 'ping')->name('ping');\n",
        ),
    ]);

    let collected = routes_of_file(&dir.path().join("routes/web.php"), dir.path());
    assert_eq!(collected.len(), 1, "{collected:?}");
    assert_eq!(collected[0].name, "ping");
    assert_eq!(collected[0].uri, "v1/ping");
}

/// Go-to-definition follows the same indirection through a `require`, and
/// lands in the file that was pulled in.
#[test]
fn definition_follows_a_require_target_held_in_a_variable() {
    let dir = workspace_with(&[
        (
            "routes/web.php",
            "<?php\n$admin = __DIR__.'/admin.php';\nrequire $admin;\n",
        ),
        (
            "routes/admin.php",
            "<?php\nRoute::get('/users', 'index')->name('users.index');\n",
        ),
    ]);
    let web = dir.path().join("routes/web.php");
    let content = std::fs::read_to_string(&web).unwrap();

    let found = definitions_of(
        &content,
        "users.index",
        &Url::from_file_path(&web).unwrap(),
        Some(&web),
        Some(dir.path()),
        &MacroScope::default(),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].uri,
        Url::from_file_path(dir.path().join("routes/admin.php").canonicalize().unwrap()).unwrap()
    );
}

/// A function body has variables of its own, so an assignment written there
/// must not be read as the value a file-scope include target holds.
#[test]
fn a_variable_assigned_inside_a_function_is_not_read_at_file_scope() {
    let dir = workspace_with(&[
        (
            "routes/web.php",
            "<?php\nfunction boot() {\n    $api = __DIR__.'/api.php';\n}\nRoute::group([], $api);\n",
        ),
        (
            "routes/api.php",
            "<?php\nRoute::get('/ping', 'ping')->name('ping');\n",
        ),
    ]);

    let collected = routes_of_file(&dir.path().join("routes/web.php"), dir.path());
    assert!(collected.is_empty(), "{collected:?}");
}

// ─── Router macros ───────────────────────────────────────────────────────────

const ADMIN_PANEL_MACRO: &str = "\
<?php
Route::macro('adminPanel', function () {
    $this->get('dashboard', 'index')->name('dashboard');
    $this->post('dashboard', 'store');
});
";

#[test]
fn a_router_macro_contributes_the_routes_its_body_registers() {
    let content = "<?php\nRoute::adminPanel();\n";
    let routes = routes_with_macros(content, ADMIN_PANEL_MACRO, &["adminPanel"]);
    let names: Vec<String> = routes.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, vec!["dashboard".to_string()]);
    assert_eq!(routes[0].uri, "dashboard");
}

#[test]
fn a_group_enclosing_a_macro_call_prefixes_the_routes_it_registers() {
    let content = "<?php\nRoute::name('admin.')->prefix('admin')->group(function () {\n    Route::adminPanel();\n});\n";
    let routes = routes_with_macros(content, ADMIN_PANEL_MACRO, &["adminPanel"]);
    let names: Vec<String> = routes.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, vec!["admin.dashboard".to_string()]);
    assert_eq!(routes[0].uri, "admin/dashboard");
}

/// `Route::name('admin.')->adminPanel()` prefixes the macro's routes the way
/// the same chain would prefix a `->group()` body.
#[test]
fn a_chain_ahead_of_a_macro_call_prefixes_the_routes_it_registers() {
    let content = "<?php\nRoute::name('admin.')->prefix('admin')->adminPanel();\n";
    let routes = routes_with_macros(content, ADMIN_PANEL_MACRO, &["adminPanel"]);
    let names: Vec<String> = routes.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, vec!["admin.dashboard".to_string()]);
    assert_eq!(routes[0].uri, "admin/dashboard");
}

/// A macro body may call another macro (`laravel/ui`'s `auth()` delegates the
/// password routes to `resetPassword()`), so the expansion has to nest.
#[test]
fn a_macro_body_that_calls_another_macro_contributes_both_sets() {
    let macro_source = "\
<?php
Route::macro('auth', function () {
    $this->get('login', 'show')->name('login');
    $this->resetPassword();
});
Route::macro('resetPassword', function () {
    $this->post('password/reset', 'reset')->name('password.update');
});
";
    let content = "<?php\nRoute::auth();\n";
    let names: Vec<String> = routes_with_macros(content, macro_source, &["auth", "resetPassword"])
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(
        names,
        vec!["login".to_string(), "password.update".to_string()]
    );
}

#[test]
fn a_macro_that_calls_itself_back_terminates() {
    let macro_source = "\
<?php
Route::macro('recursive', function () {
    $this->get('a', 'index')->name('a');
    $this->recursive();
});
";
    let content = "<?php\nRoute::recursive();\n";
    let names: Vec<String> = routes_with_macros(content, macro_source, &["recursive"])
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, vec!["a".to_string()]);
}

/// Two calls to the same macro are both expanded — only re-entry while a
/// macro is still being walked is a cycle.
#[test]
fn sibling_calls_to_one_macro_are_each_expanded() {
    let content = "<?php\nRoute::adminPanel();\nRoute::name('admin.')->group(function () {\n    Route::adminPanel();\n});\n";
    let names: Vec<String> = routes_with_macros(content, ADMIN_PANEL_MACRO, &["adminPanel"])
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(
        names,
        vec!["dashboard".to_string(), "admin.dashboard".to_string()]
    );
}

/// An ordinary chain link that happens not to be a macro must still be walked
/// through, or the registration behind it is lost.
#[test]
fn an_unknown_chain_link_is_still_walked_through() {
    let content = "<?php\nRoute::domain('admin.test')->get('/dashboard', 'index')->withoutMiddleware('web')->name('dashboard');\n";
    let names: Vec<String> = routes_with_macros(content, ADMIN_PANEL_MACRO, &["adminPanel"])
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, vec!["dashboard".to_string()]);
}

// ─── Open prefixes and suffixes (dynamic group names) ───────────────────────

#[test]
fn dynamic_name_in_group_chain_records_an_open_prefix() {
    let content = "\
<?php
Route::name('filament.')
    ->group(function () {
        Route::name($panelId . '.')->group(function () {
            Route::get('/dashboard', fn() => 'hi')->name('pages.dashboard');
        });
    });
";
    let (routes, prefixes, suffixes) = routes_and_open_names(content);
    assert!(
        prefixes.contains(&"filament.".to_string()),
        "the known static prefix should be recorded as open, got: {prefixes:?}"
    );
    // The route inside the dynamic group is also recorded as an open suffix,
    // since the real name has an unrecoverable run of characters ahead of it.
    assert!(
        suffixes.contains(&"filament.pages.dashboard".to_string()),
        "the route inside the dynamic group should be recorded as an open suffix too, got: {suffixes:?}"
    );
    // The statically resolvable route inside the dynamic group is still found.
    let names: Vec<&str> = routes.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"filament.pages.dashboard"),
        "statically resolvable routes inside the group should still be collected, got: {names:?}"
    );
}

#[test]
fn top_level_dynamic_name_group_records_the_literal_head_of_its_name() {
    let content = "\
<?php
Route::name('filament.' . $panelId . '.')
    ->group(function () {
        Route::get('/dashboard', fn() => 'hi')->name('pages.dashboard');
    });
";
    let (_, prefixes, suffixes) = routes_and_open_names(content);
    assert!(
        prefixes.contains(&"filament.".to_string()),
        "a group with no enclosing literal group should still record what its own name spells out, got: {prefixes:?}"
    );
    assert!(
        suffixes.contains(&"pages.dashboard".to_string()),
        "the route registered inside the group should be recorded as an open suffix, got: {suffixes:?}"
    );
}

#[test]
fn interpolated_group_name_records_the_literal_head() {
    let content = "\
<?php
Route::name(\"filament.{$panelId}.\")->group(function () {
    Route::get('/dashboard', fn() => 'hi')->name('pages.dashboard');
});
";
    let (_, prefixes, suffixes) = routes_and_open_names(content);
    assert!(
        prefixes.contains(&"filament.".to_string()),
        "an interpolated name should record the text ahead of the first hole, got: {prefixes:?}"
    );
    assert!(
        suffixes.contains(&"pages.dashboard".to_string()),
        "the route registered inside the group should be recorded as an open suffix, got: {suffixes:?}"
    );
}

#[test]
fn legacy_array_group_with_a_dynamic_name_records_an_open_prefix() {
    let content = "\
<?php
Route::group(['as' => 'filament.' . $panelId . '.'], function () {
    Route::get('/dashboard', fn() => 'hi')->name('pages.dashboard');
});
";
    let (_, prefixes, suffixes) = routes_and_open_names(content);
    assert!(
        prefixes.contains(&"filament.".to_string()),
        "the array form of a dynamic group name should record an open prefix too, got: {prefixes:?}"
    );
    assert!(
        suffixes.contains(&"pages.dashboard".to_string()),
        "the route registered inside the group should be recorded as an open suffix, got: {suffixes:?}"
    );
}

/// A group whose name spells out nothing at all (no enclosing literal group,
/// and its own `->name()` argument is entirely a variable) records no open
/// prefix, since the empty prefix would match every route name in the
/// project.  It must still record the names it registers as open suffixes,
/// or every `route()` call naming one of them is wrongly flagged unknown.
#[test]
fn a_wholly_unknown_group_name_records_the_names_it_registers() {
    let content = "\
<?php
Route::name($panelId)->group(function () {
    Route::get('/dashboard', fn() => 'hi')->name('pages.dashboard');
});
";
    let (_, prefixes, suffixes) = routes_and_open_names(content);
    assert!(
        prefixes.is_empty(),
        "an entirely unknown group name should not open every route name, got: {prefixes:?}"
    );
    assert!(
        suffixes.contains(&"pages.dashboard".to_string()),
        "the route registered inside the group should be recorded as an open suffix, got: {suffixes:?}"
    );
}

/// `Route::resource()` generates a whole family of route names rather than
/// registering one at a time, and each of those also needs recording as an
/// open suffix when it sits under an unknowable group.
#[test]
fn a_resource_under_a_wholly_unknown_group_records_its_generated_names() {
    let content = "\
<?php
Route::name($panelId)->group(function () {
    Route::resource('photos', PhotoController::class);
});
";
    let (_, prefixes, suffixes) = routes_and_open_names(content);
    assert!(
        prefixes.is_empty(),
        "an entirely unknown group name should not open every route name, got: {prefixes:?}"
    );
    assert!(
        suffixes.contains(&"photos.index".to_string()),
        "the resource's generated names should be recorded as open suffixes, got: {suffixes:?}"
    );
}

#[test]
fn fully_static_group_does_not_record_an_open_prefix() {
    let content = "\
<?php
Route::name('admin.')->group(function () {
    Route::get('/dashboard', fn() => 'hi')->name('dashboard');
});
";
    let (_, prefixes, suffixes) = routes_and_open_names(content);
    assert!(
        prefixes.is_empty(),
        "a fully static group should not record an open prefix, got: {prefixes:?}"
    );
    assert!(
        suffixes.is_empty(),
        "a fully static group should not record an open suffix, got: {suffixes:?}"
    );
}

/// A name without a star is compared as itself, so the overwhelmingly common
/// call site pays nothing for the pattern support.
#[test]
fn a_plain_name_matches_only_itself() {
    assert!(route_name_matches("orders.show", "orders.show"));
    assert!(!route_name_matches("orders.show", "orders.showAll"));
    assert!(!route_name_matches("orders.show", "orders"));
}

/// `Route::is('admin.*')` matches whatever the project registers under the
/// prefix, the way Laravel's own `Str::is()` does.
#[test]
fn a_star_stands_for_any_run_of_characters() {
    assert!(route_name_matches("admin.*", "admin.users.index"));
    assert!(route_name_matches("admin.*", "admin."));
    assert!(!route_name_matches("admin.*", "administration.index"));
    assert!(route_name_matches("*.index", "admin.users.index"));
    assert!(!route_name_matches("*.index", "admin.users.show"));
    assert!(route_name_matches("*", "anything.at.all"));
}

/// A star between two literals has to leave room for both, and the segment
/// after the last star is anchored at the end rather than found anywhere.
#[test]
fn the_segments_around_a_star_are_anchored() {
    assert!(route_name_matches("orders.*.edit", "orders.items.edit"));
    assert!(!route_name_matches("orders.*.edit", "orders.items.editAll"));
    // The earliest match of an inner segment must not strand the rest: the
    // trailing `c` here is the second one in the name.
    assert!(route_name_matches("a*c", "acbc"));
    assert!(route_name_matches("a*b*c", "axbyc"));
    assert!(!route_name_matches("a*b*c", "axc"));
}
