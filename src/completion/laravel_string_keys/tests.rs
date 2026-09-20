use super::enumerate::extract_lang_file_stem;
use super::*;
use tower_lsp::lsp_types::Position;

fn detect_at_end<'a>(content: &'a str, value: &str) -> Option<LaravelStringKeyContext<'a>> {
    let cursor = content.rfind(value)? + value.len();
    detect_laravel_string_key_context(
        content,
        crate::text_position::offset_to_position(content, cursor),
    )
}

fn storage_source(expression: &str) -> String {
    format!("<?php\nuse Illuminate\\Support\\Facades\\Storage;\n{expression};\n")
}

fn completion_response_items(response: CompletionResponse) -> Vec<CompletionItem> {
    match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    }
}

/// Three kinds are recorded as spans but completed from somewhere else,
/// or not at all, so they must offer nothing here rather than an empty
/// list dressed up as the answer.
#[test]
fn the_kinds_completed_elsewhere_offer_no_candidates() {
    let backend = crate::test_fixtures::make_backend();
    for kind in [
        LaravelStringKind::Section,
        LaravelStringKind::Stack,
        LaravelStringKind::ContainerBinding,
    ] {
        assert!(
            backend.string_key_candidates(&kind, None, "").is_empty(),
            "{kind:?} should offer no candidates"
        );
    }
}

#[test]
fn configured_resource_candidates_include_database_roles_and_null_drivers() {
    let backend = crate::test_fixtures::make_backend();
    backend.laravel_string_key_cache.write().config_keys = Some(std::sync::Arc::new(vec![
        "cache.stores.redis".to_string(),
        "database.connections.mysql".to_string(),
        "queue.connections.sync".to_string(),
    ]));

    assert_eq!(
        backend.string_key_candidates(&LaravelStringKind::Config, None, ""),
        [
            "cache.stores.redis",
            "database.connections.mysql",
            "queue.connections.sync",
        ]
    );
    assert_eq!(
        backend.string_key_candidates(
            &LaravelStringKind::ConfigResource(LaravelConfigResource::DatabaseConnection,),
            Some("database.connections."),
            "mysql::",
        ),
        ["mysql::read", "mysql::write", "mysql::direct"]
    );
    for (resource, config_prefix, expected) in [
        (
            LaravelConfigResource::CacheStore,
            "cache.stores.",
            vec!["null", "redis"],
        ),
        (
            LaravelConfigResource::QueueConnection,
            "queue.connections.",
            vec!["null", "sync"],
        ),
    ] {
        assert_eq!(
            backend.string_key_candidates(
                &LaravelStringKind::ConfigResource(resource),
                Some(config_prefix),
                "",
            ),
            expected,
        );
    }
}

/// Whatever a key names decides the icon beside it.
#[test]
fn a_string_key_is_iconed_by_what_it_names() {
    use tower_lsp::lsp_types::CompletionItemKind;
    for (kind, expected) in [
        (LaravelStringKind::Config, CompletionItemKind::PROPERTY),
        (
            LaravelStringKind::ConfigResource(LaravelConfigResource::CacheStore),
            CompletionItemKind::PROPERTY,
        ),
        (LaravelStringKind::View, CompletionItemKind::FILE),
        (LaravelStringKind::Trans, CompletionItemKind::TEXT),
        (
            LaravelStringKind::MorphAlias,
            CompletionItemKind::ENUM_MEMBER,
        ),
        (LaravelStringKind::GateAbility, CompletionItemKind::METHOD),
        (LaravelStringKind::Route, CompletionItemKind::VALUE),
        (LaravelStringKind::Command, CompletionItemKind::VALUE),
        (LaravelStringKind::Section, CompletionItemKind::VALUE),
        (LaravelStringKind::Stack, CompletionItemKind::VALUE),
        (
            LaravelStringKind::ContainerBinding,
            CompletionItemKind::VALUE,
        ),
    ] {
        assert_eq!(string_key_item_kind(&kind), expected, "for {kind:?}");
    }
}

#[test]
fn detects_route_call() {
    let content = "<?php\nroute('user.');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("user.").unwrap() as u32 + 5;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect route() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Route));
    assert_eq!(ctx.prefix, "user.");
}

#[test]
fn detects_instance_route_call() {
    let content = "<?php\n$redirect->route('user.');\n";
    let ctx = detect_at_end(content, "user.").expect("should detect ->route() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Route));
    assert_eq!(ctx.prefix, "user.");
}

#[test]
fn detects_to_route_call() {
    let content = "<?php\nto_route('home');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("home").unwrap() as u32 + 2;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect to_route() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Route));
    assert_eq!(ctx.prefix, "ho");
}

/// Every call that names a route completes route names, whichever of
/// the URL-building helpers it hangs off.
#[test]
fn detects_the_route_helpers_beyond_the_route_function() {
    for call in [
        "URL::signedRoute('orders.",
        "Redirect::temporarySignedRoute('orders.",
        "Response::redirectToRoute('orders.",
        "Route::is('orders.",
        "redirect()->route('orders.",
        "$request->routeIs('orders.",
    ] {
        let content = format!("<?php\n{call}");
        let col = content.lines().nth(1).unwrap().len() as u32;
        let ctx = detect_laravel_string_key_context(&content, Position::new(1, col))
            .unwrap_or_else(|| panic!("should detect `{call}` as a route context"));
        assert!(matches!(ctx.kind, LaravelStringKind::Route), "for `{call}`");
        assert_eq!(ctx.prefix, "orders.", "for `{call}`");
    }
}

/// A `getMany()` key is the first entry of an array literal rather than
/// the first argument, and `Env::get()` names an environment variable.
#[test]
fn detects_the_list_and_environment_call_shapes() {
    let content = "<?php\nConfig::getMany(['app.";
    let col = content.lines().nth(1).unwrap().len() as u32;
    let ctx = detect_laravel_string_key_context(content, Position::new(1, col))
        .expect("should detect getMany() as a config context");
    assert!(matches!(ctx.kind, LaravelStringKind::Config));
    assert_eq!(ctx.prefix, "app.");

    for call in ["Env::get('APP_", "env('APP_"] {
        let content = format!("<?php\n{call}");
        let col = content.lines().nth(1).unwrap().len() as u32;
        let ctx = detect_laravel_string_key_context(&content, Position::new(1, col))
            .unwrap_or_else(|| panic!("should detect `{call}` as an environment context"));
        assert!(matches!(ctx.kind, LaravelStringKind::Env), "for `{call}`");
        assert_eq!(ctx.prefix, "APP_", "for `{call}`");
    }
}

/// `#[RedirectToRoute]` names a route rather than a config key, but only
/// where the file imports Laravel's attribute.
#[test]
fn detects_the_redirect_to_route_attribute() {
    let content = "<?php\nuse Illuminate\\Foundation\\Http\\Attributes\\RedirectToRoute;\n\
                   #[RedirectToRoute('lo";
    let col = content.lines().nth(2).unwrap().len() as u32;
    let ctx = detect_laravel_string_key_context(content, Position::new(2, col))
        .expect("should detect the attribute as a route context");
    assert!(matches!(ctx.kind, LaravelStringKind::Route));
    assert_eq!(ctx.prefix, "lo");

    let unimported = "<?php\n#[RedirectToRoute('lo";
    let col = unimported.lines().nth(1).unwrap().len() as u32;
    assert!(
        detect_laravel_string_key_context(unimported, Position::new(1, col)).is_none(),
        "an attribute of the same name from elsewhere names no route"
    );
}

/// The preprocessor compiles Blade's render directives into marker
/// calls, so completion inside `@include('` and `@each('` reaches the
/// view index through those names.
#[test]
fn detects_the_blade_render_directive_markers() {
    for marker in ["blade_view_directive", "blade_each_directive"] {
        let content = format!("<?php\n{marker} ('partials.');\n");
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.find("partials.").unwrap() as u32 + 9;
        let ctx = detect_laravel_string_key_context(&content, Position::new(1, col));
        let ctx = ctx.unwrap_or_else(|| panic!("should detect {marker} context"));
        assert!(matches!(ctx.kind, LaravelStringKind::View));
        assert_eq!(ctx.prefix, "partials.");
    }
}

/// `@includeFirst`, `@componentFirst`, `@extendsFirst` and `@canany`
/// name their candidates inside an array literal, so the marker calls
/// they compile to have to complete there and not only for a plain
/// first argument.
#[test]
fn detects_the_blade_markers_that_list_their_names_in_an_array() {
    for (marker, value, kind) in [
        ("blade_view_directive", "partials.", LaravelStringKind::View),
        ("blade_can_directive", "upd", LaravelStringKind::GateAbility),
    ] {
        let content = format!("<?php\n{marker}(['{value}']);\n");
        let cursor = content.find(value).unwrap() + value.len();
        let ctx = detect_laravel_string_key_context(
            &content,
            crate::text_position::offset_to_position(&content, cursor),
        )
        .unwrap_or_else(|| panic!("should detect {marker} array context"));
        assert_eq!(ctx.kind, kind);
        assert_eq!(ctx.prefix, value);
    }
}

#[test]
fn detects_artisan_call_command() {
    let content = "<?php\nArtisan::call('app:');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("app:").unwrap() as u32 + 4;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect Artisan::call() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Command));
}

#[test]
fn detects_this_call_command() {
    let content = "<?php\n$this->call('app:');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("app:").unwrap() as u32 + 4;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect $this->call() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Command));
}

#[test]
fn rejects_non_this_call() {
    // `->call()` on an arbitrary object is not a command reference.
    let content = "<?php\n$service->call('doSomething');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("doSomething").unwrap() as u32 + 2;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    assert!(
        ctx.is_none(),
        "->call() on a non-$this receiver should not match"
    );
}

#[test]
fn detects_gate_facade_ability() {
    let content = "<?php\nGate::allows('upd');\n";
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("upd").unwrap() as u32 + 3;
    let ctx = detect_laravel_string_key_context(content, Position::new(1, col))
        .expect("should detect Gate::allows() context");
    assert!(matches!(ctx.kind, LaravelStringKind::GateAbility));
    assert_eq!(ctx.prefix, "upd");
}

/// Every entry point that names an ability completes from the same set.
#[test]
fn detects_every_ability_call_shape() {
    for call in [
        // The facade's own API, including the registration itself.
        "Gate::allows('upd'",
        "Gate::denies('upd'",
        "Gate::check('upd'",
        "Gate::any('upd'",
        "Gate::none('upd'",
        "Gate::authorize('upd'",
        "Gate::inspect('upd'",
        "Gate::has('upd'",
        "Gate::define('upd'",
        // A chain rooted at the facade.
        "Gate::forUser($user)->allows('upd'",
        "Gate::forUser($user)->authorize('upd'",
        "Gate::forUser($user)->has('upd'",
        "Gate::forUser($user)->inspect('upd'",
        // A controller's own helper.
        "$this->authorize('upd'",
        // The user the check is about.
        "$user->can('upd'",
        "$user->cannot('upd'",
        "$user->canAny('upd'",
        // A route registration.
        "Route::get('/p', $a)->can('upd'",
        // The Blade `@can` directive, after preprocessing.
        "blade_can_directive('upd'",
    ] {
        let content = format!("<?php\n{call});\n");
        let line_text = content.lines().nth(1).unwrap();
        let col = line_text.rfind("upd").unwrap() as u32 + 3;
        let ctx = detect_laravel_string_key_context(&content, Position::new(1, col))
            .unwrap_or_else(|| panic!("`{call}` should offer abilities"));
        assert!(
            matches!(ctx.kind, LaravelStringKind::GateAbility),
            "`{call}` should offer abilities, got {:?}",
            ctx.kind
        );
        assert_eq!(ctx.prefix, "upd", "for `{call}`");
    }
}

#[test]
fn detects_user_can_ability() {
    let content = "<?php\n$user->can('upd', $post);\n";
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("upd").unwrap() as u32 + 3;
    let ctx = detect_laravel_string_key_context(content, Position::new(1, col))
        .expect("should detect $user->can() context");
    assert!(matches!(ctx.kind, LaravelStringKind::GateAbility));
}

#[test]
fn rejects_can_on_an_unrelated_receiver() {
    let content = "<?php\n$rules->can('read');\n";
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("read").unwrap() as u32 + 4;
    assert!(
        detect_laravel_string_key_context(content, Position::new(1, col)).is_none(),
        "->can() on a receiver that is not a user must not match"
    );
}

/// A `Gate::` call earlier in the file must not turn an unrelated
/// `->has()` several statements later into an ability check.
#[test]
fn a_gate_call_in_an_earlier_statement_does_not_leak() {
    let content = "<?php\nGate::allows('update');\n$bag->has('key');\n";
    let line_text = content.lines().nth(2).unwrap();
    let col = line_text.find("key").unwrap() as u32 + 3;
    assert!(
        detect_laravel_string_key_context(content, Position::new(2, col)).is_none(),
        "an earlier Gate:: statement must not reach a later chain"
    );
}

#[test]
fn detects_config_call() {
    let content = "<?php\nconfig('app.');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("app.").unwrap() as u32 + 4;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect config() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Config));
    assert_eq!(ctx.prefix, "app.");
}

#[test]
fn detects_config_static_get() {
    let content = "<?php\nConfig::get('app.name');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("app.").unwrap() as u32 + 4;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect Config::get() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Config));
    assert_eq!(ctx.prefix, "app.");
}

#[test]
fn detects_static_config_mutators_and_translation_queries() {
    for method in ["prepend", "push"] {
        let content = format!("<?php\nConfig::{method}('app.providers');\n");
        let ctx = detect_at_end(&content, "app.providers")
            .unwrap_or_else(|| panic!("should detect Config::{method}() context"));
        assert_eq!(ctx.kind, LaravelStringKind::Config);
    }

    for method in ["get", "has", "hasForLocale", "choice"] {
        let content = format!("<?php\nLang::{method}('messages.saved');\n");
        let ctx = detect_at_end(&content, "messages.saved")
            .unwrap_or_else(|| panic!("should detect Lang::{method}() context"));
        assert_eq!(ctx.kind, LaravelStringKind::Trans);
    }
}

#[test]
fn storage_argument_scanning_preserves_existing_static_contexts() {
    for (expression, expected_kind, expected_prefix) in [
        (
            "DB::connection('primary')",
            LaravelStringKind::ConfigResource(LaravelConfigResource::DatabaseConnection),
            Some("database.connections."),
        ),
        (
            "Model::getActualClassNameForMorph('post')",
            LaravelStringKind::MorphAlias,
            None,
        ),
    ] {
        let content = format!("<?php\n{expression};\n");
        let value = if expected_prefix.is_some() {
            "primary"
        } else {
            "post"
        };
        let ctx = detect_at_end(&content, value)
            .unwrap_or_else(|| panic!("should preserve `{expression}` completion"));
        assert_eq!(ctx.kind, expected_kind);
        assert_eq!(ctx.config_sub_prefix, expected_prefix);
    }
}

#[test]
fn detects_storage_disk_scalar_methods_and_named_arguments() {
    for expression in [
        "Storage::disk('arch')",
        "Storage::disk(name: 'arch')",
        "Storage::fake('arch')",
        "Storage::fake(disk: 'arch')",
        "Storage::persistentFake('arch')",
        "Storage::persistentFake(disk: 'arch')",
        "Storage::forgetDisk('arch')",
        "Storage::forgetDisk(disk: 'arch')",
    ] {
        let content = storage_source(expression);
        let ctx = detect_at_end(&content, "arch")
            .unwrap_or_else(|| panic!("should detect `{expression}` as a disk context"));
        assert!(matches!(
            ctx.kind,
            LaravelStringKind::ConfigResource(LaravelConfigResource::StorageDisk)
        ));
        assert_eq!(ctx.prefix, "arch", "prefix for `{expression}`");
        assert_eq!(
            ctx.config_sub_prefix,
            Some("filesystems.disks."),
            "config subtree for `{expression}`"
        );
    }

    for expression in [
        "\\Illuminate\\Support\\Facades\\Storage::disk(name: 'arch')",
        "#[\\Illuminate\\Container\\Attributes\\Storage(disk: 'arch')] class Consumer {}",
    ] {
        let content = format!("<?php\n{expression};\n");
        let ctx = detect_at_end(&content, "arch")
            .unwrap_or_else(|| panic!("should detect `{expression}` as a disk context"));
        assert!(matches!(
            ctx.kind,
            LaravelStringKind::ConfigResource(LaravelConfigResource::StorageDisk)
        ));
        assert_eq!(ctx.config_sub_prefix, Some("filesystems.disks."));
    }
}

#[test]
fn detects_each_direct_config_resource_family() {
    for (expression, expected) in [
        ("auth('name')", LaravelConfigResource::AuthGuard),
        ("Auth::guard('name')", LaravelConfigResource::AuthGuard),
        ("Cache::store('name')", LaravelConfigResource::CacheStore),
        ("Log::channel('name')", LaravelConfigResource::LogChannel),
        ("Log::stack(['name'])", LaravelConfigResource::LogChannel),
        (
            "DB::connection('name')",
            LaravelConfigResource::DatabaseConnection,
        ),
        (
            "Queue::connection('name')",
            LaravelConfigResource::QueueConnection,
        ),
        ("Mail::mailer('name')", LaravelConfigResource::Mailer),
        (
            "Broadcast::connection('name')",
            LaravelConfigResource::BroadcastConnection,
        ),
    ] {
        let content = format!("<?php\n{expression};\n");
        let ctx = detect_at_end(&content, "name")
            .unwrap_or_else(|| panic!("should detect `{expression}`"));
        assert_eq!(ctx.kind, LaravelStringKind::ConfigResource(expected));
    }
}

#[test]
fn auth_middleware_completion_replaces_only_the_current_guard() {
    let content = "<?php\nRoute::middleware('auth:web, admin');\n";
    let ctx = detect_at_end(content, "admin").expect("auth middleware guard context");
    assert_eq!(
        ctx.kind,
        LaravelStringKind::ConfigResource(LaravelConfigResource::AuthGuard)
    );
    assert_eq!(ctx.prefix, "admin");
    assert_eq!(
        &content[ctx.content_start_offset..content.find("admin").unwrap() + 5],
        " admin"
    );

    for literal in ["signed:admin", "AUTH:admin", "auth"] {
        let content = format!("<?php\nRoute::middleware('{literal}');\n");
        assert!(detect_at_end(&content, literal).is_none(), "`{literal}`");
    }
}

#[test]
fn facade_chain_detection_follows_the_receiver_spine_only() {
    for chain in [
        "Route::get('/', fn () => null)->",
        "  Route \n ::get('/', fn () => null)->",
    ] {
        let content = format!("<?php {chain}");
        assert!(chain_starts_at_laravel_facade(
            &content, chain, 6, None, "Route", None,
        ));
    }

    for chain in [
        "factory(Route::class)->",
        "Route::class && factory()->",
        "Acme\\Route::get()->",
        "Factory::get()->Route::get()->",
        "::get()->",
        "Route->get()->",
    ] {
        let content = format!("<?php {chain}");
        assert!(!chain_starts_at_laravel_facade(
            &content, chain, 6, None, "Route", None,
        ));
    }
}

#[test]
fn attribute_groups_select_only_a_top_level_attribute_class() {
    for callable in [
        "#[ Cache",
        "#[Deprecated, Cache",
        "#[Deprecated(']'), Cache",
        "#[Deprecated('#['), Cache",
        "#[Deprecated(values: ['x, y']), Cache",
        "#[Deprecated(new class {}), Cache",
        "$value = \"#[ignored]\";\n#[ Cache",
        "# #[ignored]\n#[ Cache",
    ] {
        let start = callable.rfind("Cache").unwrap();
        assert_eq!(attribute_class_start(callable, start), Some(start));
    }
    for callable in [
        "#[Deprecated(Cache",
        "#[Deprecated([Cache",
        "#[Deprecated], Cache",
        "#[Deprecated] function example() { Cache",
        "// #[\nCache",
        "/* #[ */ Cache",
        "Cache",
    ] {
        let start = callable.rfind("Cache").unwrap();
        assert_eq!(attribute_class_start(callable, start), None);
    }
}

#[test]
fn receiver_chain_boundaries_ignore_nested_statements_and_newlines() {
    for prefix in [
        "<?php\nRoute::get('/')\n ->name('x')\n ->",
        "<?php\nRoute::get('/', function () { return 1; })->",
        "<?php\nRoute::get('/', /* ) ] } */ fn () => null) // keep chaining\n ->",
        "<?= $rendered ?><?php Route::get('/')->",
    ] {
        let start = receiver_chain_start(prefix);
        assert_eq!(prefix[start..].trim_start().get(..5), Some("Route"));
    }
    let prefix = "<?php\nif ($ready) {}\nunrelated()->";
    let start = receiver_chain_start(prefix);
    assert_eq!(prefix[start..].trim_start(), "unrelated()->");

    let prefix = "<?php\n\"ignored; }\";\n# ignored }\n;\nRoute::get('/')->";
    let start = receiver_chain_start(prefix);
    assert_eq!(prefix[start..].trim_start(), "Route::get('/')->");
}

#[test]
fn receiver_spine_suffix_accepts_nested_and_legacy_chain_shapes() {
    for suffix in [
        "get([fn () => ['value']])[0]::next()?->tail",
        "get([\"close ) ] }\"])->tail",
        "get(){0}->tail",
        "get(function () { return ['}']; })->tail",
        "get('/', /* ) ] } */ fn () => null) // continue\n ->tail",
        "get() # continue\n ->tail",
    ] {
        assert!(is_method_chain_suffix(suffix), "suffix: {suffix}");
    }
    for suffix in [
        "get() + unrelated()",
        "get('unterminated)",
        "get()->'invalid'",
        "get()->\"invalid\"",
        "get([missing)",
        "get()->a()->b()->c()->d()->target",
        "get()?->a()?->b()?->c()?->d()?->target",
    ] {
        assert!(!is_method_chain_suffix(suffix), "suffix: {suffix}");
    }
}

#[test]
fn storage_facade_resolution_accepts_imports_and_rejects_homonyms() {
    for content in [
        "<?php\nStorage::disk('arch');\n",
        "<?php\nIlluminate\\Support\\Facades\\Storage::disk('arch');\n",
        "<?php\nnamespace App;\n\\Illuminate\\Support\\Facades\\Storage::disk('arch');\n",
        "<?php\nuse Vendor\\Unrelated;\nStorage::disk('arch');\n",
        "<?php\nuse Illuminate\\Support\\Facades\\Storage as Disks;\nDisks::disk('arch');\n",
        "<?php\nuse Illuminate\\Support\\Facades\\{Storage as Disks, Cache};\nDisks::disk('arch');\n",
        "<?php\nuse Vendor\\Package\\{Cache};\nuse Illuminate\\Support\\Facades\\Storage as Disks;\nDisks::disk('arch');\n",
        "<?php\nnamespace App;\nuse Vendor\\Storage;\n\\Storage::disk('arch');\n",
    ] {
        assert!(
            detect_at_end(content, "arch").is_some(),
            "source: {content}"
        );
    }

    for content in [
        "<?php\nVendor\\Storage::disk('arch');\n",
        "<?php\n\\Acme\\Storage::disk('arch');\n",
        "<?php\nnamespace App;\nStorage::disk('arch');\n",
        "<?php\nuse Vendor\\Storage;\nStorage::disk('arch');\n",
        "<?php\nnamespace App;\nuse Vendor\\{Storage as Disks};\nDisks::disk('arch');\n",
        "<?php\nnamespace App;\nuse Illuminate\\Support\\Facades\\{Storage;\nStorage::disk('arch');\n",
        "<?php\nnamespace App;\nclass Storage {}\nStorage::disk('arch');\n",
    ] {
        assert!(
            detect_at_end(content, "arch").is_none(),
            "source: {content}"
        );
    }

    assert_eq!(
        imported_item_target(
            "Storage nope",
            None,
            "Illuminate\\Support\\Facades",
            &["Storage"],
            "Storage",
        ),
        None
    );
    assert_eq!(
        imported_item_target(
            "Storage as Disks trailing",
            None,
            "Illuminate\\Support\\Facades",
            &["Storage"],
            "Disks",
        ),
        None
    );
}

#[test]
fn storage_facade_completion_respects_import_bindings_in_namespaces() {
    for (import, local_name) in [
        ("use Illuminate\\Support\\Facades\\Storage;", "Storage"),
        ("use Illuminate\\Support\\Facades\\{Storage};", "Storage"),
        (
            "use Illuminate\\Support\\Facades\\{Cache, Storage};",
            "Storage",
        ),
        (
            "use Illuminate\\Support\\Facades\\{\n    Cache,\n    Storage,\n};",
            "Storage",
        ),
        (
            "use Illuminate\\Support\\Facades\\Storage as Disks;",
            "Disks",
        ),
        (
            "use Illuminate\\Support\\Facades\\{Storage as Disks, Cache};",
            "Disks",
        ),
    ] {
        for class_name in ["Storage", "Disks"] {
            let content = format!("<?php\nnamespace App;\n{import}\n{class_name}::disk('arch');\n");
            let expected = (class_name == local_name).then_some(LaravelStringKind::ConfigResource(
                LaravelConfigResource::StorageDisk,
            ));
            assert_eq!(
                detect_at_end(&content, "arch").map(|context| context.kind),
                expected,
                "source: {content}"
            );
        }
    }
}

#[test]
fn storage_facade_completion_rejects_unrelated_and_malformed_imports() {
    for import in [
        "use Vendor\\Storage;",
        "use Vendor\\Package\\{Storage};",
        "use Illuminate\\Support\\Facades\\Storage nope;",
        "use Illuminate\\Support\\Facades\\Storage as;",
        "use Illuminate\\Support\\Facades\\Storage as Disks trailing;",
        "use Illuminate\\Support\\Facades\\{Storage;",
        "use function Illuminate\\Support\\Facades\\Storage;",
    ] {
        for class_name in ["Storage", "Disks"] {
            let content = format!("<?php\nnamespace App;\n{import}\n{class_name}::disk('arch');\n");
            assert!(
                detect_at_end(&content, "arch").is_none(),
                "source: {content}"
            );
        }
    }
}

#[test]
fn authoritative_short_attribute_name_is_not_assumed_to_be_framework_class() {
    let reference = ResolvedClassReference {
        written: "Cache",
        semantic: "Cache",
        semantic_is_authoritative: true,
    };
    assert_eq!(
        resolve_known_class_reference(
            "<?php #[Cache('redis')]",
            reference,
            "Illuminate\\Container\\Attributes",
            &["Cache"],
            false,
            None,
        ),
        None
    );
}

#[test]
fn detects_every_forget_disk_array_value_and_spelling() {
    for expression in [
        "Storage::forgetDisk(['archive', 'backup'])",
        "Storage::forgetDisk(array('archive', 'backup'))",
        "Storage::forgetDisk(disk: ['archive', 'backup'])",
        "Storage::forgetDisk(disk: array('archive', 'backup'))",
    ] {
        let content = storage_source(expression);
        for value in ["archive", "backup"] {
            let cursor = content.find(value).unwrap() + value.len();
            let ctx = detect_laravel_string_key_context(
                &content,
                crate::text_position::offset_to_position(&content, cursor),
            )
            .unwrap_or_else(|| panic!("should detect `{value}` in `{expression}`"));
            assert!(matches!(
                ctx.kind,
                LaravelStringKind::ConfigResource(LaravelConfigResource::StorageDisk)
            ));
            assert_eq!(ctx.prefix, value);
            assert_eq!(ctx.config_sub_prefix, Some("filesystems.disks."));
        }
    }
}

#[test]
fn forget_disk_array_completion_filters_and_edits_the_current_value() {
    let backend = crate::Backend::new_test();
    backend.laravel_string_key_cache.write().config_keys = Some(std::sync::Arc::new(vec![
        "cache.stores.archive".to_string(),
        "filesystems.disks.archive".to_string(),
        "filesystems.disks.archive.driver".to_string(),
        "filesystems.disks.backup".to_string(),
    ]));

    let content = "<?php\nuse Illuminate\\Support\\Facades\\Storage;\nStorage::forgetDisk(['backup', 'ar']);\n";
    let cursor = content.rfind("ar").unwrap() + 2;
    let position = crate::text_position::offset_to_position(content, cursor);
    let response = backend
        .try_laravel_string_key_completion(content, position)
        .expect("the array value should offer disk names");
    let items = completion_response_items(response);
    assert_eq!(
        items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        vec!["archive"]
    );
    assert_eq!(
        items[0].text_edit,
        Some(CompletionTextEdit::Edit(TextEdit {
            range: Range::new(
                crate::text_position::offset_to_position(content, cursor - 2),
                position,
            ),
            new_text: "archive".to_string(),
        }))
    );
}

#[test]
fn completion_response_item_helper_accepts_list_responses() {
    let items = completion_response_items(CompletionResponse::List(CompletionList {
        is_incomplete: false,
        items: vec![CompletionItem::default()],
    }));
    assert_eq!(items.len(), 1);
}

#[test]
fn forget_disk_arrays_complete_values_but_not_associative_keys() {
    let content = "<?php\nuse Illuminate\\Support\\Facades\\Storage;\nStorage::forgetDisk(['alias' => 'archive']);\n";
    let key_cursor = content.find("alias").unwrap() + "alias".len();
    assert!(
        detect_laravel_string_key_context(
            content,
            crate::text_position::offset_to_position(content, key_cursor),
        )
        .is_none(),
        "an associative key is not a disk name"
    );

    let value = detect_at_end(content, "archive").expect("the array value is a disk name");
    assert!(matches!(
        value.kind,
        LaravelStringKind::ConfigResource(LaravelConfigResource::StorageDisk)
    ));
    assert_eq!(value.prefix, "archive");
}

#[test]
fn rejects_invalid_resource_argument_names_and_shapes() {
    for expression in [
        "auth(['archive'])",
        "Storage::disk(['archive'])",
        "Storage::fake(['archive'])",
        "Storage::persistentFake(['archive'])",
        "Storage::forgetDisk([['archive']])",
        "Storage::disk(disk: 'archive')",
        "Storage::disk(NAME: 'archive')",
        "Storage::fake(name: 'archive')",
        "Storage::persistentFake(name: 'archive')",
        "Storage::forgetDisk(name: 'archive')",
        "#[\\Illuminate\\Container\\Attributes\\Storage(name: 'archive')] class C {}",
        "#[\\Illuminate\\Container\\Attributes\\Storage(disk: ['archive'])] class C {}",
        "Storage::extend('archive', fn () => null)",
        "Config::get(['archive'])",
        "route(name: 'archive')",
    ] {
        let content = storage_source(expression);
        assert!(
            detect_at_end(&content, "archive").is_none(),
            "`{expression}` must not offer disk completion"
        );
    }
}

#[test]
fn storage_argument_scanners_handle_nested_and_malformed_php() {
    let before_named = r#"Storage::fake(config: ['message' => 'it\'s', 'factory' => wrap(fn () => new class {})], disk:"#;
    assert_eq!(
        callable_before_scalar_argument(before_named),
        Some(("Storage::fake", Some("disk")))
    );

    let nested = r#"<?php
use Illuminate\Support\Facades\Storage;
Storage::forgetDisk([['nested'], wrap(fn () => new class {}), 'it\'s', 'archive']);"#;
    let ctx = detect_at_end(nested, "archive")
        .expect("balanced nested expressions must not hide the outer array");
    assert_eq!(ctx.config_sub_prefix, Some("filesystems.disks."));

    assert!(callable_before_scalar_argument("Storage::disk(:").is_none());
    assert!(callable_before_scalar_argument("Storage::disk(name: value").is_none());
    assert!(callable_before_scalar_argument("orphan name:").is_none());
    assert!(enclosing_call_open_paren("completed(); orphan").is_none());
    assert!(enclosing_call_open_paren("orphan").is_none());
    assert!(callable_before_array_argument("factory('archive").is_none());
    assert!(callable_before_array_argument("{ 'archive").is_none());
    assert!(callable_before_array_argument("broken; 'archive").is_none());
    assert!(callable_before_array_argument("orphan").is_none());

    let escaped_key = r#"'key\'part' => 'archive'"#;
    assert!(string_literal_is_array_key(
        escaped_key,
        "'key".len(),
        b'\''
    ));
    assert!(!string_literal_is_array_key("'key\n", "'key".len(), b'\''));
    assert!(!string_literal_is_array_key("'key", "'key".len(), b'\''));
    assert!(string_literal_is_array_key(
        "'key\npart'\n => 'archive'",
        "'key".len(),
        b'\'',
    ));
    assert!(string_literal_is_array_key(
        "'key' /* why */\n // still a key\n # also trivia\n => 'archive'",
        "'key".len(),
        b'\'',
    ));
    assert!(!string_literal_is_array_key(
        "'value' /* unterminated",
        "'value".len(),
        b'\'',
    ));
}

#[test]
fn detects_view_call() {
    let content = "<?php\nview('users.');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("users.").unwrap() as u32 + 6;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect view() context");
    assert!(matches!(ctx.kind, LaravelStringKind::View));
}

#[test]
fn detects_trans_double_underscore() {
    let content = "<?php\n__('messages.');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("messages.").unwrap() as u32 + 9;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect __() context");
    assert!(matches!(ctx.kind, LaravelStringKind::Trans));
}

#[test]
fn detects_empty_prefix() {
    let content = "<?php\nroute('');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("''").unwrap() as u32 + 1;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect empty prefix");
    assert!(matches!(ctx.kind, LaravelStringKind::Route));
    assert_eq!(ctx.prefix, "");
}

#[test]
fn rejects_second_arg() {
    let content = "<?php\nroute('name', 'param');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("param").unwrap() as u32 + 2;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    assert!(ctx.is_none(), "Second argument should not match");
}

#[test]
fn rejects_non_laravel_function() {
    let content = "<?php\nfoo('bar');\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("bar").unwrap() as u32 + 1;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    assert!(ctx.is_none(), "Non-Laravel function should not match");
}

#[test]
fn lang_file_stem_extraction() {
    assert_eq!(
        extract_lang_file_stem("file:///app/lang/en/messages.php"),
        Some("messages".to_string())
    );
    assert_eq!(
        extract_lang_file_stem("file:///app/resources/lang/en/validation.php"),
        Some("validation".to_string())
    );
}

#[test]
fn detects_config_attribute_with_import() {
    let content =
        "<?php\nuse Illuminate\\Container\\Attributes\\Config;\n#[Config('app.timezone')]\n";
    let line = 2;
    let line_text = content.lines().nth(2).unwrap();
    let col = line_text.find("app.timezone").unwrap() as u32 + 12;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect #[Config()] with verified import");
    assert!(matches!(ctx.kind, LaravelStringKind::Config));
    assert_eq!(ctx.prefix, "app.timezone");
}

#[test]
fn rejects_config_attribute_without_import() {
    let content = "<?php\n#[Config('app.timezone')]\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("app.timezone").unwrap() as u32 + 12;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    assert!(
        ctx.is_none(),
        "Should reject #[Config()] without verified import"
    );
}

#[test]
fn unrelated_attribute_does_not_break_detection() {
    let content = "<?php\nclass Foo {\n    #[Override]\n    public function bar(): void {\n        route('');\n    }\n}\n";
    let line = 4;
    let line_text = content.lines().nth(line).unwrap();
    let col = line_text.find("''").unwrap() as u32 + 1;
    let ctx = detect_laravel_string_key_context(content, Position::new(line as u32, col));
    assert!(
        ctx.is_some(),
        "route('') must be detected even when #[Override] exists earlier in the file"
    );
    assert!(matches!(ctx.unwrap().kind, LaravelStringKind::Route));
}

#[test]
fn detects_fqn_config_attribute() {
    let content = "<?php\n#[\\Illuminate\\Container\\Attributes\\Config('app.')]\n";
    let line = 1;
    let line_text = content.lines().nth(1).unwrap();
    let col = line_text.find("app.").unwrap() as u32 + 4;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    let ctx = ctx.expect("should detect FQN #[Config()] attribute");
    assert!(matches!(ctx.kind, LaravelStringKind::Config));
    assert_eq!(ctx.prefix, "app.");
}

#[test]
fn detects_route_in_module_controller() {
    let content = "<?php\n\
\n\
namespace Acme\\User\\Http\\Controllers;\n\
\n\
use App\\Http\\Controllers\\Abstracts\\BaseController;\n\
use Illuminate\\Http\\RedirectResponse;\n\
use Illuminate\\Http\\Request;\n\
\n\
final class UserPermissionController extends BaseController\n\
{\n\
public function copy(Request $request): RedirectResponse\n\
{\n\
    route('');\n\
\n\
    return to_route('admin::user.permissions.edit', 1);\n\
}\n\
}\n";
    // route('') is on line 12 (0-indexed), cursor at character 15 (between quotes)
    let line = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("route('')"))
        .map(|(i, _)| i as u32)
        .expect("should find route('') line");
    let line_text = content.lines().nth(line as usize).unwrap();
    let col = line_text.find("''").unwrap() as u32 + 1;
    let ctx = detect_laravel_string_key_context(content, Position::new(line, col));
    assert!(
        ctx.is_some(),
        "should detect route('') context in module controller at line {}, col {}",
        line,
        col,
    );
    let ctx = ctx.unwrap();
    assert!(matches!(ctx.kind, LaravelStringKind::Route));
}

#[test]
fn route_completion_end_to_end() {
    let backend = crate::Backend::new_test();

    let route_uri = "file:///app/routes/web.php";
    let route_content = "<?php\n\
        use Illuminate\\Support\\Facades\\Route;\n\
        Route::get('/home', fn() => 'home')->name('home');\n\
        Route::get('/about', fn() => 'about')->name('about');\n";
    backend.open_files.write().insert(
        route_uri.to_string(),
        std::sync::Arc::new(route_content.to_string()),
    );
    backend.update_ast(route_uri, route_content);

    let test_content = "<?php\nroute('');\n";
    backend.update_ast("file:///app/Http/Controllers/Test.php", test_content);

    let names = backend.cached_route_names();
    assert!(
        names.contains(&"home".to_string()),
        "cached_route_names should contain 'home', got: {:?}",
        names
    );

    let response = backend.try_laravel_string_key_completion(test_content, Position::new(1, 7));
    assert!(
        response.is_some(),
        "try_laravel_string_key_completion should return Some for route('')"
    );
    if let Some(CompletionResponse::Array(items)) = response {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"home"),
            "completion should include 'home', got: {:?}",
            labels
        );
        assert!(
            labels.contains(&"about"),
            "completion should include 'about', got: {:?}",
            labels
        );
    }
}

/// Concurrent first callers must share one build, not run one each:
/// every enumeration behind these accessors walks the workspace from
/// disk, and the diagnostic pass hits them from all N workers at once.
#[test]
fn concurrent_first_callers_build_the_enumeration_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let backend = crate::Backend::new_test();
    let builds = AtomicUsize::new(0);
    let build_lock = parking_lot::Mutex::new(());

    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let backend = &backend;
                let builds = &builds;
                let build_lock = &build_lock;
                scope.spawn(move || {
                    backend.cached_laravel_enumeration(
                        build_lock,
                        |cache| cache.view_names.clone(),
                        |cache, names| cache.view_names = Some(names),
                        || {
                            builds.fetch_add(1, Ordering::SeqCst);
                            // Long enough that an unguarded
                            // check-then-fill has every thread miss.
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            vec!["home".to_string()]
                        },
                    )
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    assert_eq!(
        builds.load(Ordering::SeqCst),
        1,
        "the enumeration must be built once and shared, not once per caller"
    );
    for names in &results {
        assert_eq!(names, &vec!["home".to_string()]);
    }
}
