use super::*;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn world(root: &Path, inertia: InertiaPages) -> AssetWorld {
    let public = root.join("public");
    AssetWorld {
        root: Some(root.to_path_buf()),
        has_public: public.is_dir(),
        mix: (!public.join("hot").exists())
            .then(|| MixManifest::read(root))
            .flatten(),
        has_vite: has_vite_config(root),
        inertia: Some(Arc::new(inertia)),
    }
}

#[test]
fn inertia_config_falls_back_to_the_conventions() {
    let root = Path::new("/project");
    let pages = inertia_pages_from_config(None, root);
    assert_eq!(
        pages.roots,
        vec![
            root.join("resources/js/Pages"),
            root.join("resources/js/pages")
        ]
    );
    assert!(pages.extensions.iter().any(|e| e == "vue"));
    assert!(pages.extensions.iter().any(|e| e == "tsx"));
}

#[test]
fn inertia_config_reads_the_newest_keys_first() {
    let root = Path::new("/project");
    let config = "<?php
return [
    'pages' => [
        'paths' => [resource_path('js/Views'), base_path('modules/Pages')],
        'extensions' => ['.svelte'],
    ],
    'testing' => [
        'page_paths' => [resource_path('js/Pages')],
        'page_extensions' => ['vue'],
    ],
];
";
    let pages = inertia_pages_from_config(Some(config), root);
    assert_eq!(
        pages.roots,
        vec![root.join("resources/js/Views"), root.join("modules/Pages")]
    );
    assert_eq!(pages.extensions, vec!["svelte".to_string()]);
}

#[test]
fn inertia_config_reads_the_testing_keys_of_older_releases() {
    let root = Path::new("/project");
    let config = "<?php
return [
    'testing' => [
        'ensure_pages_exist' => true,
        'page_paths' => [realpath(resource_path('js/Screens'))],
        'page_extensions' => ['tsx'],
    ],
];
";
    let pages = inertia_pages_from_config(Some(config), root);
    assert_eq!(pages.roots, vec![root.join("resources/js/Screens")]);
    assert_eq!(pages.extensions, vec!["tsx".to_string()]);
}

#[test]
fn an_asset_is_judged_against_public() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "public/css/app.css", "");
    let world = world(dir.path(), InertiaPages::default());

    assert_eq!(world.judge(StringKeyKind::Asset, "css/app.css"), Some(true));
    assert_eq!(
        world.judge(StringKeyKind::Asset, "css/app.css?v=3"),
        Some(true),
        "a cache-busting query string is not part of the path"
    );
    assert_eq!(
        world.judge(StringKeyKind::Asset, "css"),
        Some(true),
        "a directory is a valid prefix to link under"
    );
    assert_eq!(world.judge(StringKeyKind::Asset, "css/ap.css"), Some(false));
    assert_eq!(
        world.judge(StringKeyKind::Asset, "storage/avatars/1.png"),
        None,
        "uploads behind `storage:link` only exist at runtime"
    );
    assert_eq!(
        world.judge(StringKeyKind::Asset, "https://cdn.example.com/app.css"),
        None
    );
}

#[test]
fn an_asset_is_not_judged_without_public() {
    let dir = tempfile::tempdir().unwrap();
    let world = world(dir.path(), InertiaPages::default());
    assert_eq!(world.judge(StringKeyKind::Asset, "css/app.css"), None);
}

#[test]
fn a_mix_key_is_judged_against_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "public/mix-manifest.json",
        r#"{"\/js\/app.js": "\/js\/app.js?id=abc"}"#,
    );
    let world = world(dir.path(), InertiaPages::default());
    assert_eq!(
        world.judge(StringKeyKind::MixAsset, "js/app.js"),
        Some(true)
    );
    assert_eq!(
        world.judge(StringKeyKind::MixAsset, "/js/app.js"),
        Some(true)
    );
    assert_eq!(
        world.judge(StringKeyKind::MixAsset, "js/ap.js"),
        Some(false)
    );

    write(dir.path(), "public/hot", "http://localhost:8080");
    let world = self::world(dir.path(), InertiaPages::default());
    assert_eq!(
        world.judge(StringKeyKind::MixAsset, "js/ap.js"),
        None,
        "the dev server answers without the manifest"
    );
}

#[test]
fn a_vite_entry_is_judged_only_in_a_vite_project() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "resources/js/app.js", "");
    let world_without = world(dir.path(), InertiaPages::default());
    assert_eq!(
        world_without.judge(StringKeyKind::ViteAsset, "resources/js/missing.js"),
        None
    );

    write(dir.path(), "vite.config.js", "export default {}");
    let world = world(dir.path(), InertiaPages::default());
    assert_eq!(
        world.judge(StringKeyKind::ViteAsset, "resources/js/app.js"),
        Some(true)
    );
    assert_eq!(
        world.judge(StringKeyKind::ViteAsset, "resources/js/missing.js"),
        Some(false)
    );
}

#[test]
fn an_inertia_page_is_judged_against_the_page_directories() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "resources/js/Pages/Users/Index.vue", "");
    let pages = inertia_pages_from_config(None, dir.path());
    let world = world(dir.path(), pages);
    assert_eq!(
        world.judge(StringKeyKind::InertiaPage, "Users/Index"),
        Some(true)
    );
    assert_eq!(
        world.judge(StringKeyKind::InertiaPage, "Users/Show"),
        Some(false)
    );
}

#[test]
fn an_inertia_page_is_not_judged_without_a_page_directory() {
    let dir = tempfile::tempdir().unwrap();
    let pages = inertia_pages_from_config(None, dir.path());
    let world = world(dir.path(), pages);
    assert_eq!(world.judge(StringKeyKind::InertiaPage, "Dashboard"), None);
}

#[test]
fn page_names_and_new_pages_follow_the_existing_pages() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "resources/js/Pages/Dashboard.tsx", "");
    write(dir.path(), "resources/js/Pages/Users/Index.tsx", "");
    write(dir.path(), "resources/js/Pages/Users/README.md", "");
    let pages = inertia_pages_from_config(None, dir.path());

    assert_eq!(
        pages.page_names(),
        vec!["Dashboard".to_string(), "Users/Index".to_string()]
    );
    let new_file = pages.new_page_file("Users/Show").unwrap();
    assert!(
        new_file.ends_with(Path::new("Users").join("Show.tsx")),
        "{new_file:?}"
    );
}
