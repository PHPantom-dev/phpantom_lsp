use super::aliases::Aliases;
use super::views::{controller_name, is_absolute_view_path, normalize_view_path, view_key};

#[test]
fn controller_names_follow_the_renderer() {
    for (class, expected) in [
        (
            "App\\Controller\\FooBar\\BazController",
            Some("foo-bar/baz"),
        ),
        (
            "App\\Controllers\\FooBar\\BazController",
            Some("foo-bar/baz"),
        ),
        (
            "App\\AllControllers\\MyController\\FooBar\\BazController",
            Some("foo-bar/baz"),
        ),
        (
            "App\\AllControllers\\MyController\\BazController",
            Some("baz"),
        ),
        ("Path\\To\\File\\BlogController", Some("blog")),
        ("App\\Web\\Blog\\PostController", Some("post")),
        ("App\\Web\\Blog\\Action", None),
    ] {
        assert_eq!(controller_name(class).as_deref(), expected, "for {class}");
    }
}

#[test]
fn view_names_join_the_path_they_are_relative_to() {
    let base = Some("@views/blog");
    let dir = Some("/app/resources/views/blog");
    assert_eq!(
        view_key("index", base, None).as_deref(),
        Some("@views/blog/index")
    );
    assert_eq!(
        view_key("../shared/menu", base, dir).as_deref(),
        Some("/app/resources/views/shared/menu")
    );
    assert_eq!(
        view_key("./_card", base, dir).as_deref(),
        Some("/app/resources/views/blog/_card")
    );
    assert_eq!(
        view_key("/abs/view", None, None).as_deref(),
        Some("/abs/view")
    );
    // Under the view's base path, or relative to a template the call is
    // not in, a name names nothing that can be told.
    assert_eq!(view_key("//layout", base, dir), None);
    assert_eq!(view_key("./_card", base, None), None);
    assert_eq!(view_key("index", None, None), None);
    assert_eq!(normalize_view_path("@views/../x"), "@views/x");
}

#[test]
fn windows_drive_paths_are_absolute_view_paths() {
    let dir = Some(r"C:\app\resources\views\blog");
    assert_eq!(
        view_key("../shared/menu", None, dir).as_deref(),
        Some("C:/app/resources/views/shared/menu")
    );
    assert_eq!(
        view_key(r"D:\abs\view", None, None).as_deref(),
        Some("D:/abs/view")
    );
    assert!(is_absolute_view_path(r"C:\app"));
    assert!(is_absolute_view_path("c:/app"));
    assert!(!is_absolute_view_path("app/views"));
    assert!(!is_absolute_view_path("@views/x"));
    // `..` never climbs past the drive.
    assert_eq!(normalize_view_path("C:/../x"), "C:/x");
}

#[test]
fn aliases_resolve_through_each_other() {
    let aliases = Aliases::from_entries(&[
        ("@root", "/app"),
        ("@views", "@root/resources/views"),
        ("@views/admin", "/elsewhere/admin"),
        ("@loop", "@loop/again"),
    ]);
    assert_eq!(
        aliases.resolve("@views/blog/index").as_deref(),
        Some("/app/resources/views/blog/index")
    );
    assert_eq!(
        aliases.resolve("@views/admin/edit").as_deref(),
        Some("/elsewhere/admin/edit")
    );
    assert_eq!(aliases.resolve("@viewsx/index"), None);
    assert_eq!(aliases.resolve("@unknown/x"), None);
    assert_eq!(aliases.resolve("@loop/x"), None);
    assert_eq!(aliases.resolve("/plain").as_deref(), Some("/plain"));
}

#[test]
fn an_alias_is_the_name_a_string_starts_with() {
    use super::aliases::alias_prefix;
    assert_eq!(alias_prefix("@runtime/logs"), Some("@runtime"));
    assert_eq!(alias_prefix("@views"), Some("@views"));
    assert_eq!(alias_prefix("@yii-app.x_y/z"), Some("@yii-app.x_y"));
    assert_eq!(alias_prefix("@"), None);
    assert_eq!(alias_prefix("@/x"), None);
    assert_eq!(alias_prefix("@ x"), None);
    assert_eq!(alias_prefix("user@example.com"), None);
}

/// A project whose merge plan lists params files that override and add to
/// each other.
fn params_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let write = |path: &str, content: &str| {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    };
    write(
        "composer.json",
        r#"{"extra": {"config-plugin-file": "config/configuration.php"}}"#,
    );
    write(
        "config/configuration.php",
        r#"<?php
return [
    'config-plugin' => [
        'params' => ['common/params.php', '?common/missing.php'],
        'params-web' => ['$params', 'web/params.php'],
        'di' => 'common/di/*.php',
        'routes' => 'common/routes.php',
    ],
    'config-plugin-environments' => [
        Environment::DEV => ['params' => ['dev/params.php']],
    ],
    'config-plugin-options' => ['source-directory' => 'config'],
];
"#,
    );
    write(
        "config/common/params.php",
        r#"<?php
return [
    'app' => ['name' => 'Demo', 'debug' => false, 'tags' => ['a', 'b']],
    'mailer' => ['from' => 'a@example.com'],
    'spread' => [...$other, 'known' => 1],
    'list' => [],
];
"#,
    );
    write(
        "config/web/params.php",
        r#"<?php
return [
    'app' => ['debug' => true, 'theme' => 'dark'],
    'mailer' => null,
];
"#,
    );
    write(
        "config/dev/params.php",
        r#"<?php
return ['app' => ['name' => 'Demo (dev)'], 'traceLink' => 'x'];
"#,
    );
    write("config/common/di/a.php", "<?php return [];");
    write("config/common/routes.php", "<?php return [];");
    dir
}

#[test]
fn the_merge_plan_lists_the_files_handed_params() {
    use super::params::ConfigPlan;
    let dir = params_project();
    let plan = ConfigPlan::load(dir.path());
    let config = dir.path().join("config");
    assert!(plan.receives_params(&config.join("common/di/a.php")));
    assert!(plan.receives_params(&config.join("common/routes.php")));
    assert!(!plan.receives_params(&config.join("common/params.php")));
    assert!(!plan.receives_params(&config.join("configuration.php")));
}

#[test]
fn params_merge_by_rank() {
    use super::params::{ConfigPlan, Params, param_key};
    let dir = params_project();
    let params = Params::from_plan(&ConfigPlan::load(dir.path()));
    let declares = |keys: &[&str]| params.declares(&param_key(keys));
    let declared_in = |keys: &[&str]| -> Vec<String> {
        params
            .get(&param_key(keys))
            .unwrap()
            .declarations()
            .iter()
            .map(|d| {
                d.file
                    .strip_prefix(dir.path().join("config"))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    };

    // Arrays merge, and every file that adds to one declares it.
    assert_eq!(declares(&["app", "theme"]), Some(true));
    assert_eq!(declares(&["app", "name"]), Some(true));
    assert_eq!(
        declared_in(&["app"]),
        ["common/params.php", "web/params.php", "dev/params.php"]
    );
    // A later value replaces an earlier one.
    assert_eq!(declared_in(&["app", "debug"]), ["web/params.php"]);
    assert_eq!(declared_in(&["app", "name"]), ["dev/params.php"]);
    assert_eq!(declares(&["mailer", "from"]), None);
    // An unknown key is told only where every key beside it is known.
    assert_eq!(declares(&["nope"]), Some(false));
    assert_eq!(declares(&["app", "nope"]), Some(false));
    assert_eq!(declares(&["spread", "anything"]), None);
    assert_eq!(declares(&["list", "anything"]), None);
    assert_eq!(declares(&["nope", "deeper"]), None);

    assert_eq!(
        params.php_type().unwrap().to_string(),
        "array{app: array{name: string, debug: bool, tags: list<string>, theme: string}, \
         mailer: null, spread: array{known: int}, list: array, traceLink?: string}"
    );
}

#[test]
fn an_asset_entry_is_keyed_under_its_bundle_directory() {
    use super::assets::{asset_file_key, split_asset_file_key};
    let key = asset_file_key("@assetsSource/main", "css/site.css").unwrap();
    assert_eq!(
        split_asset_file_key(&key),
        ("@assetsSource/main", "css/site.css")
    );
    assert_eq!(
        asset_file_key("@assetsSource/main", "https://cdn.example.com/x.css"),
        None
    );
    assert_eq!(
        asset_file_key("@assetsSource/main", "//cdn.example.com/x.css"),
        None
    );
    assert_eq!(asset_file_key("@assetsSource/main", "/favicon.css"), None);
    assert_eq!(asset_file_key("@assetsSource/main", ""), None);
}
