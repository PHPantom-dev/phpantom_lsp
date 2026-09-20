use super::*;

/// Wrap a provider `boot()` body so the scanner sees a realistic file.
fn provider(body: &str) -> String {
    format!(
        "<?php\n\
         namespace App\\Providers;\n\
         \n\
         use Illuminate\\Database\\Eloquent\\Relations\\Relation;\n\
         use Illuminate\\Support\\ServiceProvider;\n\
         use App\\Models\\Post;\n\
         use App\\Models\\Video;\n\
         \n\
         class AppServiceProvider extends ServiceProvider\n\
         {{\n\
             public function boot(): void\n\
             {{\n\
                 {body}\n\
             }}\n\
         }}\n"
    )
}

#[test]
fn skips_files_without_a_morph_map_token() {
    let scan = scan_morph_map("<?php\nclass Foo { public function bar(): void {} }\n");
    assert_eq!(scan, MorphMapScan::default());
}

#[test]
fn extracts_keyed_morph_map() {
    let content = provider(
        "Relation::morphMap([\n\
             'post' => Post::class,\n\
             'video' => Video::class,\n\
         ]);",
    );
    let scan = scan_morph_map(&content);

    let pairs: Vec<(&str, &str)> = scan
        .entries
        .iter()
        .map(|e| (e.alias.as_str(), e.target_fqn.as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("post", "App\\Models\\Post"),
            ("video", "App\\Models\\Video"),
        ]
    );
    assert!(!scan.enforced, "morphMap() alone does not enforce the map");
    assert!(scan.table_keyed.is_empty());
}

#[test]
fn records_the_alias_literal_offset() {
    let content = provider("Relation::morphMap(['post' => Post::class]);");
    let scan = scan_morph_map(&content);
    let entry = &scan.entries[0];
    let start = entry.alias_offset as usize;
    assert_eq!(&content[start..start + entry.alias.len()], "post");
}

#[test]
fn enforce_morph_map_marks_the_map_exhaustive() {
    let content = provider("Relation::enforceMorphMap(['post' => Post::class]);");
    let scan = scan_morph_map(&content);
    assert_eq!(scan.entries.len(), 1);
    assert!(scan.enforced);
}

#[test]
fn require_morph_map_enforces_without_contributing_entries() {
    let content =
        provider("Relation::requireMorphMap();\nRelation::morphMap(['post' => Post::class]);");
    let scan = scan_morph_map(&content);
    assert!(scan.enforced);
    assert_eq!(scan.entries.len(), 1);
}

#[test]
fn resolves_an_aliased_relation_import() {
    let content = "<?php\n\
         namespace App\\Providers;\n\
         use Illuminate\\Database\\Eloquent\\Relations\\Relation as EloquentRelation;\n\
         use App\\Models\\Post;\n\
         class AppServiceProvider {\n\
             public function boot(): void {\n\
                 EloquentRelation::morphMap(['post' => Post::class]);\n\
             }\n\
         }\n";
    let scan = scan_morph_map(content);
    assert_eq!(scan.entries.len(), 1);
    assert_eq!(scan.entries[0].target_fqn, "App\\Models\\Post");
}

#[test]
fn resolves_a_fully_qualified_relation_reference() {
    let content = "<?php\n\
         namespace App\\Providers;\n\
         class AppServiceProvider {\n\
             public function boot(): void {\n\
                 \\Illuminate\\Database\\Eloquent\\Relations\\Relation::morphMap([\n\
                     'post' => \\App\\Models\\Post::class,\n\
                 ]);\n\
             }\n\
         }\n";
    let scan = scan_morph_map(content);
    assert_eq!(scan.entries.len(), 1);
    assert_eq!(scan.entries[0].target_fqn, "App\\Models\\Post");
}

#[test]
fn ignores_a_morph_map_on_an_unrelated_relation_class() {
    // A project's own `Relation` class in the current namespace is not
    // Eloquent's, so its `morphMap()` must not seed the index.
    let content = "<?php\n\
         namespace App\\Support;\n\
         use App\\Models\\Post;\n\
         class Bootstrapper {\n\
             public function boot(): void {\n\
                 Relation::morphMap(['post' => Post::class]);\n\
             }\n\
         }\n";
    let scan = scan_morph_map(content);
    assert!(scan.entries.is_empty());
    assert!(!scan.enforced);
}

#[test]
fn accepts_the_legacy_array_syntax() {
    let content = provider("Relation::morphMap(array('post' => Post::class));");
    let scan = scan_morph_map(&content);
    assert_eq!(scan.entries.len(), 1);
    assert_eq!(scan.entries[0].alias, "post");
}

#[test]
fn accepts_a_string_class_name_as_the_map_value() {
    // Single-quoted (single separator) and double-quoted (escaped separator)
    // spellings must both normalize to the same FQN.
    for source in [
        "Relation::morphMap(['post' => 'App\\Models\\Post']);",
        "Relation::morphMap(['post' => \"App\\\\Models\\\\Post\"]);",
    ] {
        let content = provider(source);
        let scan = scan_morph_map(&content);
        assert_eq!(scan.entries.len(), 1, "failed for: {source}");
        assert_eq!(scan.entries[0].target_fqn, "App\\Models\\Post");
    }
}

#[test]
fn collects_list_shorthand_targets_separately() {
    // `Relation::morphMap([Post::class])` keys the map by the model's table
    // name, which the single-file scan cannot know.
    let content = provider("Relation::morphMap([Post::class, Video::class]);");
    let scan = scan_morph_map(&content);
    assert!(scan.entries.is_empty());
    let fqns: Vec<&str> = scan
        .table_keyed
        .iter()
        .map(|t| t.target_fqn.as_str())
        .collect();
    assert_eq!(fqns, vec!["App\\Models\\Post", "App\\Models\\Video"]);
}

#[test]
fn skips_a_non_literal_map_argument() {
    let content = provider("Relation::morphMap($this->morphMapFromConfig());");
    let scan = scan_morph_map(&content);
    assert!(scan.entries.is_empty());
    assert!(scan.table_keyed.is_empty());
}

#[test]
fn skips_interpolated_and_computed_aliases() {
    let content = provider(
        "Relation::morphMap([\n\
             $alias => Post::class,\n\
             'video' => $videoClass,\n\
         ]);",
    );
    let scan = scan_morph_map(&content);
    assert!(scan.entries.is_empty());
}

#[test]
fn morph_map_getter_call_contributes_nothing() {
    let content = provider("$map = Relation::morphMap();");
    let scan = scan_morph_map(&content);
    assert!(scan.entries.is_empty());
    assert!(!scan.enforced);
}

// ─── Index ──────────────────────────────────────────────────────────────────

fn entry(alias: &str, fqn: &str) -> MorphMapEntry {
    MorphMapEntry {
        alias: alias.to_string(),
        target_fqn: fqn.to_string(),
        alias_offset: 0,
    }
}

#[test]
fn index_merges_registrations_across_files() {
    let mut index = LaravelMorphMapIndex::default();
    index.files.set_file(
        "file:///a.php".to_string(),
        MorphMapScan {
            entries: vec![entry("post", "App\\Models\\Post")],
            ..Default::default()
        },
    );
    index.files.set_file(
        "file:///b.php".to_string(),
        MorphMapScan {
            entries: vec![entry("video", "App\\Models\\Video")],
            enforced: true,
            ..Default::default()
        },
    );
    index.rebuild();

    assert_eq!(
        index.get("post").map(|t| t.fqn.as_str()),
        Some("App\\Models\\Post")
    );
    assert_eq!(
        index.get("video").map(|t| t.uri.as_str()),
        Some("file:///b.php")
    );
    assert!(index.is_enforced(), "one enforcing file enforces the map");
}

#[test]
fn index_drops_a_files_contributions_when_it_stops_registering() {
    let mut index = LaravelMorphMapIndex::default();
    let uri = "file:///a.php".to_string();
    index.files.set_file(
        uri.clone(),
        MorphMapScan {
            entries: vec![entry("post", "App\\Models\\Post")],
            enforced: true,
            ..Default::default()
        },
    );
    index.rebuild();
    assert!(index.files.has_uri(&uri));

    index.files.set_file(uri.clone(), MorphMapScan::default());
    index.rebuild();
    assert!(!index.files.has_uri(&uri));
    assert!(index.all_aliases().is_empty());
    assert!(index.get("post").is_none());
    assert!(!index.is_enforced());
}

#[test]
fn index_keeps_the_first_registration_for_a_duplicated_alias() {
    let mut index = LaravelMorphMapIndex::default();
    index.files.set_file(
        "file:///a.php".to_string(),
        MorphMapScan {
            entries: vec![
                entry("post", "App\\Models\\Post"),
                entry("post", "App\\Models\\LegacyPost"),
            ],
            ..Default::default()
        },
    );
    index.rebuild();
    assert_eq!(
        index.get("post").map(|t| t.fqn.as_str()),
        Some("App\\Models\\Post")
    );
}

#[test]
fn index_lists_every_alias() {
    let mut index = LaravelMorphMapIndex::default();
    index.files.set_file(
        "file:///a.php".to_string(),
        MorphMapScan {
            entries: vec![
                entry("post", "App\\Models\\Post"),
                entry("video", "App\\Models\\Video"),
            ],
            ..Default::default()
        },
    );
    index.rebuild();
    let mut aliases = index.all_aliases();
    aliases.sort();
    assert_eq!(aliases, vec!["post".to_string(), "video".to_string()]);
}
