//! Route-name resolution across the shapes a Laravel route file registers
//! names in: leaf `->name()` calls, name groups in their fluent and array
//! spellings, groups that load another file, and `Route::resource()`.
//!
//! Most cases are judged through the unknown-route diagnostic, which reads
//! the same route table that completion, hover, and go-to-definition use: a
//! name the project registers must not be flagged, and a name Laravel would
//! not register must be.  The expectations follow Laravel's own
//! `RouteRegistrar`, `RouteGroup`, and `ResourceRegistrar`.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.

use crate::common::{
    LARAVEL_SRC_COMPOSER, complete_labels_at_opened, create_initialized_psr4_workspace,
    definition_locations, goto_definition_at, hover_text_at, messages_with_code, position_after,
};
use tower_lsp::lsp_types::{Location, Url};

const CONSUMER_PATH: &str = "src/Links.php";

/// A class that names each of `names` in a `route()` call, one per line.
fn consumer_calling(names: &[&str]) -> String {
    let calls: String = names
        .iter()
        .map(|name| format!("        route('{name}');\n"))
        .collect();
    format!(
        "<?php\nnamespace App;\nclass Links {{\n    public function demo(): void {{\n{calls}    }}\n}}\n"
    )
}

/// A Laravel workspace holding `files` plus a consumer that names each of
/// `names`, with the consumer opened.
async fn workspace_calling(
    files: &[(&str, &str)],
    names: &[&str],
) -> (phpantom_lsp::Backend, tempfile::TempDir, Url, String) {
    let consumer = consumer_calling(names);
    let mut all: Vec<(&str, &str)> = files.to_vec();
    all.push((CONSUMER_PATH, &consumer));
    let (backend, dir, uri) =
        create_initialized_psr4_workspace(LARAVEL_SRC_COMPOSER, &all, CONSUMER_PATH).await;
    (backend, dir, uri, consumer)
}

/// Assert that every name in `known` is recognised as a registered route and
/// every name in `unknown` is reported as one the project does not register.
async fn assert_route_names(files: &[(&str, &str)], known: &[&str], unknown: &[&str]) {
    let names: Vec<&str> = known.iter().chain(unknown).copied().collect();
    let (backend, _dir, uri, consumer) = workspace_calling(files, &names).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), &consumer, &mut diags);
    let messages = messages_with_code(&diags, "invalid_laravel_route");
    let is_flagged = |name: &str| messages.iter().any(|m| m.contains(&format!("'{name}'")));

    let wrongly_flagged: Vec<&str> = known.iter().copied().filter(|n| is_flagged(n)).collect();
    let missed: Vec<&str> = unknown.iter().copied().filter(|n| !is_flagged(n)).collect();
    assert!(
        wrongly_flagged.is_empty(),
        "registered routes were reported as unknown: {wrongly_flagged:?} (diagnostics: {messages:?})"
    );
    assert!(
        missed.is_empty(),
        "routes the project does not register were not reported: {missed:?} (diagnostics: {messages:?})"
    );
}

/// Go-to-definition on the single `route()` call naming `name`.
async fn definitions_of(files: &[(&str, &str)], name: &str) -> Vec<Location> {
    let (backend, _dir, uri, consumer) = workspace_calling(files, &[name]).await;
    let mut position = position_after(&consumer, "route('");
    position.character += 1;
    definition_locations(
        goto_definition_at(&backend, &uri, position.line, position.character).await,
    )
}

/// The hover text on the single `route()` call naming `name`.
async fn hover_of(files: &[(&str, &str)], name: &str) -> Option<String> {
    let (backend, _dir, uri, consumer) = workspace_calling(files, &[name]).await;
    let mut position = position_after(&consumer, "route('");
    position.character += 1;
    hover_text_at(&backend, &uri, position.line, position.character).await
}

// ─── Leaf names ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_double_quoted_route_name_is_registered() {
    assert_route_names(
        &[(
            "routes/web.php",
            "<?php\nRoute::get('/dashboard', 'index')->name(\"dashboard.index\");\n",
        )],
        &["dashboard.index"],
        &["dashboard"],
    )
    .await;
}

#[tokio::test]
async fn whitespace_inside_the_name_call_is_tolerated() {
    assert_route_names(
        &[(
            "routes/web.php",
            "<?php\nRoute::get('/x', 'x')->name ( 'spaced' );\n",
        )],
        &["spaced"],
        &["x"],
    )
    .await;
}

#[tokio::test]
async fn every_route_a_file_names_is_registered() {
    assert_route_names(
        &[(
            "routes/auth.php",
            "<?php\nRoute::get('/login', 'show')->name('login');\nRoute::post('/logout', 'out')->name('logout');\nRoute::get('/register', 'form')->name('register');\n",
        )],
        &["login", "logout", "register"],
        &["password.request"],
    )
    .await;
}

/// A registration without `->name()` has no name at all; its URI is not one.
#[tokio::test]
async fn an_unnamed_route_is_not_known_by_its_uri() {
    assert_route_names(
        &[(
            "routes/web.php",
            "<?php\nRoute::get('/home', 'home')->name('home');\nRoute::get('/anon', function () {});\nRoute::get('/no-name', [Controller::class, 'method']);\n",
        )],
        &["home"],
        &["anon", "no-name"],
    )
    .await;
}

/// `->name()` is named on every registration method the router offers, not
/// only the HTTP verbs.
#[tokio::test]
async fn names_on_every_registration_method_are_registered() {
    let routes = "\
<?php
Route::get('/a', 'a')->name('verb.get');
Route::post('/b', 'b')->name('verb.post');
Route::put('/c', 'c')->name('verb.put');
Route::patch('/d', 'd')->name('verb.patch');
Route::delete('/e', 'e')->name('verb.delete');
Route::options('/f', 'f')->name('verb.options');
Route::any('/g', 'g')->name('verb.any');
Route::match(['get', 'post'], '/h', 'h')->name('verb.match');
Route::view('/static', 'pages.static')->name('static');
Route::redirect('/from', '/to')->name('redir');
Route::permanentRedirect('/legacy', '/current')->name('legacy');
Route::fallback(fn () => 'missing')->name('fallback');
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &[
            "verb.get",
            "verb.post",
            "verb.put",
            "verb.patch",
            "verb.delete",
            "verb.options",
            "verb.any",
            "verb.match",
            "static",
            "redir",
            "legacy",
            "fallback",
        ],
        &["pages.static", "verb"],
    )
    .await;
}

#[tokio::test]
async fn a_registration_spread_over_several_lines_is_registered() {
    let routes = "\
<?php
Route::get(
    '/users/{user}',
    [UserController::class, 'show'],
)->name('users.show');
";
    assert_route_names(&[("routes/web.php", routes)], &["users.show"], &["users"]).await;
}

#[tokio::test]
async fn modifiers_between_the_registration_and_its_name_are_stepped_over() {
    let routes = "\
<?php
Route::get('/admin', [AdminController::class, 'index'])->middleware('auth')->name('admin');
Route::get('/orders/{order}', 'show')->whereNumber('order')->withoutMiddleware('web')->name('orders.show');
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["admin", "orders.show"],
        &["auth"],
    )
    .await;
}

/// `->name()` on a chain that never reaches the router is some other
/// builder's method, not a route registration.
#[tokio::test]
async fn a_name_call_on_an_unrelated_builder_is_not_a_route() {
    let routes = "\
<?php
Route::get('/home', 'home')->name('home');
$builder->name('not-a-route')->save();
$obj->getUser('/x', SomeController::class)->name('user');
$query->name('prefixed')->get('/y');
$this->get('/z', 'z')->name('this-outside-a-macro');
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["home"],
        &["not-a-route", "user", "prefixed", "this-outside-a-macro"],
    )
    .await;
}

/// A routes file is required with `$router` in scope, and `Router::group()`
/// hands the router to its closure under whatever name the closure gives it.
#[tokio::test]
async fn a_name_call_on_the_router_variable_is_a_route() {
    let routes = "\
<?php
$router->get('/a', 'a')->name('file.router');
\\Illuminate\\Support\\Facades\\Route::get('/b', 'b')->name('facade.fqn');
Route::group([], function ($r) {
    $r->get('/c', 'c')->name('closure.param');
    $router->get('/d', 'd')->name('outer.router');
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["file.router", "facade.fqn", "closure.param"],
        &["outer.router"],
    )
    .await;
}

#[tokio::test]
async fn a_static_name_call_on_another_class_is_not_a_route() {
    let routes = "\
<?php
Route::get('/home', 'home')->name('home');
SomethingElse::name('also-not')->go();
";
    assert_route_names(&[("routes/web.php", routes)], &["home"], &["also-not"]).await;
}

/// `Route::name('x')->get(...)` sets the `as` attribute on the registrar,
/// and `RouteRegistrar::registerRoute()` merges it into the route's action,
/// so the route is named even though no `->name()` follows the verb.
#[tokio::test]
async fn a_name_ahead_of_the_registration_names_the_route() {
    let routes = "\
<?php
Route::name('users.index')->get('/users', [UserController::class, 'index']);
Route::as('users.store')->post('/users', [UserController::class, 'store']);
Route::middleware('auth')->name('profile')->get('/profile', 'show');
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["users.index", "users.store", "profile"],
        &["users"],
    )
    .await;
}

/// `Route::name()` on the route object appends to the `as` attribute the
/// registrar already put there, so the two compose into one name.
#[tokio::test]
async fn a_name_ahead_of_the_registration_prefixes_the_name_after_it() {
    let routes = "\
<?php
Route::name('admin.')->get('/dashboard', 'index')->name('dashboard');
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["admin.dashboard"],
        &["dashboard"],
    )
    .await;
}

// ─── Name groups ────────────────────────────────────────────────────────────

/// The group's prefix reaches the routes inside its body and nothing else:
/// a route after the group is not prefixed, and a route inside it is not
/// also registered under its bare leaf name.
#[tokio::test]
async fn a_name_group_prefixes_only_the_routes_inside_it() {
    let routes = "\
<?php
Route::name('admin.')->group(function () {
    Route::get('/users', [UserController::class, 'index'])->name('users.index');
});
Route::get('/login', 'login')->name('login');
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["admin.users.index", "login"],
        &["users.index", "admin.login"],
    )
    .await;
}

#[tokio::test]
async fn a_middleware_after_the_group_name_does_not_hide_it() {
    let routes = "\
<?php
Route::name('admin.')->middleware(['auth', 'verified'])->group(function () {
    Route::get('/users', [UserController::class, 'index'])->name('users.index');
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["admin.users.index"],
        &["users.index"],
    )
    .await;
}

#[tokio::test]
async fn nested_name_groups_compose_through_many_levels() {
    let routes = "\
<?php
Route::name('cloud.')->group(function () {
    Route::name('lead-settings.')->group(function () {
        Route::name('management-systems.')->group(function () {
            Route::name('decisioner-settings.')->group(function () {
                Route::get('/edit', [Controller::class, 'edit'])->name('edit');
            });
        });
    });
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["cloud.lead-settings.management-systems.decisioner-settings.edit"],
        &[
            "edit",
            "decisioner-settings.edit",
            "cloud.edit",
            "cloud.decisioner-settings.edit",
        ],
    )
    .await;
}

#[tokio::test]
async fn an_array_group_inside_a_fluent_group_composes_with_it() {
    let routes = "\
<?php
Route::name('api.')->group(function () {
    Route::group(['as' => 'v1.'], function () {
        Route::get('/users', 'index')->name('users.index');
    });
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["api.v1.users.index"],
        &["v1.users.index", "api.users.index"],
    )
    .await;
}

#[tokio::test]
async fn a_fluent_group_inside_an_array_group_composes_with_it() {
    let routes = "\
<?php
Route::group(['as' => 'api.', 'prefix' => 'api'], function () {
    Route::name('v1.')->group(function () {
        Route::get('/users', 'index')->name('users.index');
    });
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["api.v1.users.index"],
        &["v1.users.index", "api.users.index"],
    )
    .await;
}

#[tokio::test]
async fn sibling_groups_do_not_share_their_prefixes() {
    let routes = "\
<?php
Route::name('admin.')->group(function () {
    Route::get('/x', 'x')->name('x');
});
Route::name('api.')->group(function () {
    Route::get('/y', 'y')->name('y');
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["admin.x", "api.y"],
        &["admin.y", "api.x", "x", "y"],
    )
    .await;
}

/// `as()` is `RouteRegistrar`'s own name for the attribute `name()` aliases,
/// so it prefixes a group's routes the same way.
#[tokio::test]
async fn an_as_group_prefixes_the_routes_inside_it() {
    let routes = "\
<?php
Route::as('api.')->group(function () {
    Route::get('/users', 'index')->name('users');
});
";
    assert_route_names(&[("routes/web.php", routes)], &["api.users"], &["users"]).await;
}

/// The modifiers ahead of `->group()` may come in any order, and an inner
/// name group builds on the outer one's prefix.
#[tokio::test]
async fn group_modifiers_are_read_in_any_order() {
    let routes = "\
<?php
Route::middleware('security')->prefix('/security')->name('security.')->group(function () {
    Route::name('questions.')->group(function () {
        Route::get('/questions', 'create')->name('create');
        Route::post('/questions', 'store')->name('store');
    });
});
Route::name('x.')->prefix('/x')->group(function () {
    Route::get('/show', 'show')->name('show');
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &[
            "security.questions.create",
            "security.questions.store",
            "x.show",
        ],
        &["questions.create", "security.create", "show"],
    )
    .await;
}

/// `RouteRegistrar::attribute()` overwrites an attribute rather than
/// appending to it, so a second `->name()` on the same group chain replaces
/// the first.
#[tokio::test]
async fn a_second_name_on_one_group_chain_replaces_the_first() {
    let routes = "\
<?php
Route::name('a.')->name('b.')->group(function () {
    Route::get('/x', 'x')->name('x');
});
";
    assert_route_names(&[("routes/web.php", routes)], &["b.x"], &["a.b.x", "a.x"]).await;
}

// ─── Groups that load another file ──────────────────────────────────────────

const BACKSTAGE_ROUTES: &str =
    "<?php\nRoute::get('/patients', fn () => 'ok')->name('patient.index');\n";

#[tokio::test]
async fn a_group_loading_a_file_by_base_path_prefixes_its_routes() {
    assert_route_names(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::name('admin.')->group(base_path('routes/web_backstage.php'));\n",
            ),
            ("routes/web_backstage.php", BACKSTAGE_ROUTES),
        ],
        &["admin.patient.index"],
        &["admin.patient"],
    )
    .await;
}

#[tokio::test]
async fn a_group_loading_a_dir_relative_file_prefixes_its_routes() {
    assert_route_names(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::name('admin.')->group(__DIR__ . '/web_backstage.php');\n",
            ),
            ("routes/web_backstage.php", BACKSTAGE_ROUTES),
        ],
        &["admin.patient.index"],
        &["admin.patient"],
    )
    .await;
}

#[tokio::test]
async fn an_as_group_loading_a_file_prefixes_its_routes() {
    assert_route_names(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::as('admin.')->group(base_path('routes/web_backstage.php'));\n",
            ),
            ("routes/web_backstage.php", BACKSTAGE_ROUTES),
        ],
        &["admin.patient.index"],
        &["admin.patient"],
    )
    .await;
}

/// In the array form the file path is the second argument, after the
/// attribute array.
#[tokio::test]
async fn an_array_group_loading_a_file_prefixes_its_routes() {
    assert_route_names(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::group(['as' => 'admin.'], base_path('routes/web_backstage.php'));\n",
            ),
            ("routes/web_backstage.php", BACKSTAGE_ROUTES),
        ],
        &["admin.patient.index"],
        &["admin.patient"],
    )
    .await;
}

#[tokio::test]
async fn a_closure_group_around_a_file_load_composes_both_prefixes() {
    let web = "\
<?php
Route::name('admin.')->group(function () {
    Route::name('v1.')->group(base_path('routes/web_backstage.php'));
});
";
    assert_route_names(
        &[
            ("routes/web.php", web),
            ("routes/web_backstage.php", BACKSTAGE_ROUTES),
        ],
        &["admin.v1.patient.index"],
        &["admin.patient.index", "v1.admin.patient.index"],
    )
    .await;
}

#[tokio::test]
async fn a_chain_of_file_loads_accumulates_prefixes() {
    assert_route_names(
        &[
            (
                "routes/a.php",
                "<?php\nRoute::name('admin.')->group(base_path('routes/b.php'));\n",
            ),
            (
                "routes/b.php",
                "<?php\nRoute::name('patient.')->group(base_path('routes/c.php'));\n",
            ),
            (
                "routes/c.php",
                "<?php\nRoute::get('/edit', fn () => 'ok')->name('edit');\n",
            ),
        ],
        &["admin.patient.edit"],
        &["patient.admin.edit"],
    )
    .await;
}

/// Two route files that load each other must not send the scan round in
/// circles; each still contributes its routes, one hop in each direction.
#[tokio::test]
async fn route_files_that_load_each_other_terminate() {
    assert_route_names(
        &[
            (
                "routes/a.php",
                "<?php\nRoute::name('x.')->group(base_path('routes/b.php'));\nRoute::get('/a', fn () => 'ok')->name('a');\n",
            ),
            (
                "routes/b.php",
                "<?php\nRoute::name('y.')->group(base_path('routes/a.php'));\nRoute::get('/b', fn () => 'ok')->name('b');\n",
            ),
        ],
        &["a", "b", "x.b", "y.a"],
        &["z.a"],
    )
    .await;
}

/// A file outside `routes/` is only loaded through the group that names it,
/// so its routes exist under that group's prefix and not under their bare
/// names.  Resource routes in it take the prefix too.
#[tokio::test]
async fn a_file_loaded_from_outside_the_routes_directory_takes_the_group_prefix() {
    let custom = "\
<?php
Route::get('/dashboard', fn () => 'ok')->name('dash');
Route::resource('widgets', WidgetController::class);
";
    assert_route_names(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::name('admin.')->group(base_path('app/Custom/admin.php'));\n",
            ),
            ("app/Custom/admin.php", custom),
        ],
        &[
            "admin.dash",
            "admin.widgets.index",
            "admin.widgets.create",
            "admin.widgets.store",
            "admin.widgets.show",
            "admin.widgets.edit",
            "admin.widgets.update",
            "admin.widgets.destroy",
        ],
        &["dash", "widgets.index"],
    )
    .await;
}

#[tokio::test]
async fn a_chain_of_loads_outside_the_routes_directory_accumulates_prefixes() {
    assert_route_names(
        &[
            (
                "routes/a.php",
                "<?php\nRoute::name('admin.')->group(base_path('app/Custom/b.php'));\n",
            ),
            (
                "app/Custom/b.php",
                "<?php\nRoute::name('patient.')->group(base_path('app/Custom/c.php'));\n",
            ),
            (
                "app/Custom/c.php",
                "<?php\nRoute::get('/edit', fn () => 'ok')->name('edit');\n",
            ),
        ],
        &["admin.patient.edit"],
        &["edit", "patient.edit"],
    )
    .await;
}

#[tokio::test]
async fn loads_between_a_route_file_and_an_outside_file_terminate() {
    assert_route_names(
        &[
            (
                "routes/a.php",
                "<?php\nRoute::name('x.')->group(base_path('app/Custom/b.php'));\nRoute::get('/a', fn () => 'ok')->name('a');\n",
            ),
            (
                "app/Custom/b.php",
                "<?php\nRoute::name('y.')->group(base_path('routes/a.php'));\nRoute::get('/b', fn () => 'ok')->name('b');\n",
            ),
        ],
        &["a", "x.b"],
        &["z.a"],
    )
    .await;
}

/// A route file in a subdirectory of `routes/` is not loaded by Laravel on
/// its own, so its routes are only known under the group that loads it.
#[tokio::test]
async fn a_subdirectory_route_file_takes_the_prefix_of_the_group_loading_it() {
    assert_route_names(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::name('admin.')->group(__DIR__ . '/admin/users.php');\n",
            ),
            (
                "routes/admin/users.php",
                "<?php\nRoute::get('/users', 'index')->name('users.index');\n",
            ),
        ],
        &["admin.users.index"],
        &["users.index"],
    )
    .await;
}

#[tokio::test]
async fn a_resource_in_a_loaded_file_composes_with_the_load_prefix() {
    assert_route_names(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::name('admin.')->group(base_path('routes/b.php'));\n",
            ),
            (
                "routes/b.php",
                "<?php\nRoute::resource('patient', PatientController::class);\n",
            ),
        ],
        &[
            "admin.patient.index",
            "admin.patient.create",
            "admin.patient.store",
            "admin.patient.show",
            "admin.patient.edit",
            "admin.patient.update",
            "admin.patient.destroy",
        ],
        &["admin.patient.list"],
    )
    .await;
}

// ─── Resource routes ────────────────────────────────────────────────────────

/// `ResourceRegistrar` splits a resource name on `/`, so a leading slash
/// leaves an empty URI prefix and the bare resource name; it never reaches
/// the generated route names.
#[tokio::test]
async fn a_leading_slash_on_a_resource_name_stays_out_of_its_route_names() {
    let routes = "\
<?php
Route::name('api.')->group(function () {
    Route::resource('/leads', LeadController::class)->only(['store', 'update']);
});
";
    assert_route_names(
        &[("routes/web.php", routes)],
        &["api.leads.store", "api.leads.update"],
        &[
            "api.leads.index",
            "api.leads.show",
            "api./leads.store",
            "leads.store",
        ],
    )
    .await;
}

// ─── Go-to-definition ───────────────────────────────────────────────────────

#[tokio::test]
async fn goto_definition_on_a_name_from_a_loaded_file_lands_in_that_file() {
    let locations = definitions_of(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::name('admin.')->group(base_path('routes/web_backstage.php'));\n",
            ),
            ("routes/web_backstage.php", BACKSTAGE_ROUTES),
        ],
        "admin.patient.index",
    )
    .await;

    assert_eq!(locations.len(), 1, "{locations:?}");
    assert!(
        locations[0]
            .uri
            .as_str()
            .ends_with("/routes/web_backstage.php"),
        "should land in the loaded file, got {locations:?}"
    );
    assert_eq!(locations[0].range.start.line, 1);
}

/// The leaf route's name is written as only its own segment; the group's
/// prefix sits elsewhere.  Definition lands on the segment's literal.
#[tokio::test]
async fn goto_definition_on_a_grouped_name_lands_on_its_own_segment() {
    let routes = "\
<?php
Route::name('admin.')->group(function () {
    Route::get('/users', [UserController::class, 'index'])->name('users');
});
";
    let locations = definitions_of(&[("routes/web.php", routes)], "admin.users").await;

    assert_eq!(locations.len(), 1, "{locations:?}");
    let line = routes.lines().nth(2).unwrap();
    let character = line.find("'users'").unwrap() as u32 + 1;
    assert_eq!(locations[0].range.start.line, 2);
    assert_eq!(locations[0].range.start.character, character);
}

/// Registering one name twice is a mistake Laravel lets through; every
/// registration is somewhere the name could have come from.
#[tokio::test]
async fn goto_definition_offers_every_registration_of_a_repeated_name() {
    let routes = "\
<?php
Route::get('/a', [A::class, 'x'])->name('a');
Route::get('/b', [B::class, 'x'])->name('b');
Route::get('/a-also', [A::class, 'y'])->name('a');
";
    let locations = definitions_of(&[("routes/web.php", routes)], "a").await;

    let mut lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    lines.sort_unstable();
    assert_eq!(lines, vec![1, 3], "{locations:?}");
}

// ─── Hover ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn hover_on_a_name_from_a_file_outside_the_routes_directory_names_that_file() {
    let text = hover_of(
        &[
            (
                "routes/web.php",
                "<?php\nRoute::name('admin.')->group(base_path('app/Custom/admin.php'));\n",
            ),
            (
                "app/Custom/admin.php",
                "<?php\nRoute::get('/dashboard', fn () => 'ok')->name('dash');\n",
            ),
        ],
        "admin.dash",
    )
    .await
    .expect("route('admin.dash') should hover");

    assert!(
        text.contains("app/Custom/admin.php"),
        "hover should name the file the route is declared in, got: {text}"
    );
}

#[tokio::test]
async fn hover_on_an_unregistered_name_claims_no_definition() {
    let text = hover_of(
        &[(
            "routes/web.php",
            "<?php\nRoute::get('/', 'home')->name('home');\n",
        )],
        "does.not.exist",
    )
    .await;

    if let Some(text) = text {
        assert!(
            !text.contains("Defined in"),
            "an unregistered route has no definition to name, got: {text}"
        );
    }
}

// ─── Completion ─────────────────────────────────────────────────────────────

/// Resource names written with slashes still complete as the dotted names
/// Laravel generates, never with a slash in them.
#[tokio::test]
async fn completion_offers_resource_names_without_slashes() {
    let routes = "\
<?php
Route::name('api.')->group(function () {
    Route::resource('/leads', LeadController::class);
    Route::apiResource('/photos', PhotoController::class);
});
Route::resource('/bare', BareController::class);
";
    let (backend, _dir, uri, consumer) =
        workspace_calling(&[("routes/web.php", routes)], &[""]).await;
    let position = position_after(&consumer, "route('");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;

    for expected in ["api.leads.index", "api.photos.store", "bare.show"] {
        assert!(
            labels.iter().any(|l| l == expected),
            "expected {expected} among route() completions, got: {labels:?}"
        );
    }
    assert!(
        !labels.iter().any(|l| l.contains('/')),
        "no route name may contain a slash, got: {labels:?}"
    );
}
