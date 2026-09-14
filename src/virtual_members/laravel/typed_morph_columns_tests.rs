use super::*;
use crate::atom::atom;
use crate::test_fixtures::make_class;

fn symbol_map(content: &str) -> Arc<SymbolMap> {
    crate::parser::with_parsed_program(content, "morph_test", |program, content| {
        Arc::new(crate::symbol_map::extract_symbol_map(program, content))
    })
}

#[test]
fn morph_column_cache_rejects_missing_changed_and_outdated_sources() {
    let backend = Backend::new_headless();
    let uri = "file:///phpantom-morph-cache.php";
    let source = "<?php class Comment extends \\Illuminate\\Database\\Eloquent\\Model { public function subject() { return $this->morphTo(); } } Comment::where('subject_type', 'post');";
    let map = symbol_map(source);
    assert!(backend.morph_column_spans_for(uri, &map).is_empty());
    backend
        .open_files
        .write()
        .insert(uri.to_string(), Arc::new(source.to_string()));
    backend.update_ast(uri, source);
    let map = backend.symbol_map_for(uri).unwrap();
    let original = backend.morph_column_spans_for(uri, &map);
    assert_eq!(original.len(), 1);
    assert!(Arc::ptr_eq(
        &original,
        &backend.morph_column_spans_for(uri, &map)
    ));

    backend.symbols.note_class_lookup_change();
    let refreshed = backend.morph_column_spans_for(uri, &map);
    assert_eq!(refreshed.len(), 1);
    assert!(!Arc::ptr_eq(&original, &refreshed));
    let replacement = symbol_map(source);
    assert!(!Arc::ptr_eq(
        &refreshed,
        &backend.morph_column_spans_for(uri, &replacement)
    ));

    backend.open_files.write().insert(
        uri.to_string(),
        Arc::new(source.replace("'post'", "'user'")),
    );
    assert!(backend.morph_column_spans_for(uri, &map).is_empty());
    backend
        .open_files
        .write()
        .insert(uri.to_string(), Arc::new(format!("{source}\n")));
    assert!(backend.morph_column_spans_for(uri, &map).is_empty());
    backend
        .blade_virtual_content
        .write()
        .insert(uri.to_string(), source.to_string());
    assert_eq!(backend.morph_column_spans_for(uri, &map).len(), 1);
    assert!(
        backend
            .morph_column_spans_for(uri, &symbol_map("<?php echo 1;"))
            .is_empty()
    );
}

#[test]
fn morph_columns_reject_untyped_and_missing_models() {
    let loader = |_: &str| None;
    let ctx = ResolutionCtx {
        current_class: None,
        all_classes: &[],
        content: "",
        cursor_offset: 0,
        class_loader: &loader,
        backend: None,
        laravel_macro_this_resolver: None,
        resolved_class_cache: None,
        function_loader: None,
        scope_var_resolver: None,
        is_in_static_method: false,
        preserve_static: false,
    };
    let map = symbol_map("<?php Comment::where('subject_type', 'post');");
    let site = &map.morph_column_sites[0];
    for ty in [
        PhpType::mixed(),
        PhpType::int(),
        PhpType::named(atom(super::super::ELOQUENT_MODEL_FQN)),
        PhpType::generic(super::super::ELOQUENT_BUILDER_FQN, vec![PhpType::mixed()]),
        PhpType::generic(
            super::super::ELOQUENT_BUILDER_FQN,
            vec![PhpType::named(atom(super::super::ELOQUENT_BUILDER_FQN))],
        ),
    ] {
        assert!(!has_morph_column(&ty, site, &ctx), "{ty}");
    }
}

#[test]
fn morph_qualified_columns_require_a_known_effective_table() {
    let mut model = make_class("Comment");
    model.laravel_mut();
    assert!(model_table_matches(&model, "comments", &|_| None));
    assert!(!model_table_matches(&model, "posts", &|_| None));
    model.laravel_mut().has_get_table_method = true;
    assert!(!model_table_matches(&model, "comments", &|_| None));
    model.laravel_mut().has_get_table_method = false;
    model.parent_class = Some(atom("Missing"));
    assert!(!model_table_matches(&model, "comments", &|_| None));
    model.parent_class = Some(atom("Comment"));
    let cyclic = Arc::new(model.clone());
    assert!(!model_table_matches(&model, "comments", &|_| Some(
        Arc::clone(&cyclic)
    )));

    model.parent_class = None;
    model.used_traits.push(atom("TableSource"));
    let mut table_source = make_class("TableSource");
    table_source.laravel_mut().table_name = Some("entries".to_string());
    let table_source = Arc::new(table_source);
    assert!(model_table_matches(&model, "entries", &|_| Some(
        Arc::clone(&table_source)
    )));
    let mut dynamic_source = (*table_source).clone();
    dynamic_source.laravel_mut().has_get_table_method = true;
    let dynamic_source = Arc::new(dynamic_source);
    assert!(!model_table_matches(&model, "entries", &|_| Some(
        Arc::clone(&dynamic_source)
    )));
    assert!(model_table_matches(&model, "comments", &|_| None));

    model.used_traits.clear();
    model.laravel_mut().table_name = Some("own_table".to_string());
    model.parent_class = Some(atom("ParentModel"));
    let mut parent = make_class("ParentModel");
    parent.laravel_mut().table_name = Some("parent_table".to_string());
    let parent = Arc::new(parent);
    assert!(model_table_matches(&model, "own_table", &|_| Some(
        Arc::clone(&parent)
    )));
    assert!(!model_table_matches(&model, "parent_table", &|_| Some(
        Arc::clone(&parent)
    )));

    let base = make_class(super::super::ELOQUENT_MODEL_FQN);
    assert!(collect_table_metadata(
        &base,
        "models",
        &|_| None,
        &mut None,
        0
    ));
}

#[test]
fn morph_column_confirmation_reads_trait_table_metadata_from_source() {
    let backend = Backend::new_test();
    let uri = "file:///phpantom-morph-trait-table.php";
    let content = r#"<?php
trait TableSource { protected $table = 'entries'; }
trait Unrelated {}
class Comment extends \Illuminate\Database\Eloquent\Model {
    use TableSource, Unrelated;
    public function subject() { return $this->morphTo(); }
}
Comment::where('entries.subject_type', 'post');
Comment::where('comments.subject_type', 'post');
"#;
    backend
        .open_files
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    backend.update_ast(uri, content);
    let map = backend.symbol_map_for(uri).unwrap();
    let spans = backend.morph_column_spans_for(uri, &map);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].start as usize, content.find("'post'").unwrap() + 1);
}

#[test]
fn morph_column_confirmation_preserves_array_values_across_nested_calls() {
    let backend = Backend::new_test();
    let uri = "file:///phpantom-morph-array.php";
    let content = r#"<?php
class Comment extends \Illuminate\Database\Eloquent\Model {
    public function subject() { return $this->morphTo(); }
}
class Other extends \Illuminate\Database\Eloquent\Model {}
Comment::whereIn('subject_type', [
    'post',
    Other::whereIn('subject_type', ['unrelated']),
    Comment::whereIn('subject_type', ['nested']),
    Comment::whereIn('other_type', ['invalid']),
    'post',
    'video',
]);
"#;
    backend
        .open_files
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    backend.update_ast(uri, content);
    let map = backend.symbol_map_for(uri).unwrap();
    let spans = backend.morph_column_spans_for(uri, &map);
    let aliases: Vec<_> = spans
        .iter()
        .map(|span| &content[span.start as usize..span.end as usize])
        .collect();
    assert_eq!(aliases, ["post", "nested", "post", "video"]);
}
