use super::*;
use crate::test_fixtures::make_backend;

#[test]
fn view_parameter_reads_keep_template_scope_and_reject_missing_or_stale_maps() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.view.php");
    let uri = crate::util::path_to_uri(&path);
    assert!(backend.tempest_view_input_reads(&uri, "<?php").is_empty());
    let source = "<p>{{ $title }}</p>";
    std::fs::write(&path, source).unwrap();
    backend.update_ast(&uri, source);
    let php = backend.analysable_content(&uri).unwrap();
    let reads = backend.tempest_view_input_reads(&uri, &php);
    assert_eq!(
        reads.keys().map(Atom::as_str).collect::<Vec<_>>(),
        ["title"]
    );
    assert!(
        backend
            .tempest_view_input_reads(&uri, "<?php stale();")
            .is_empty()
    );
}

#[test]
fn view_argument_queries_handle_uninferred_reads_and_disabled_projects() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let view_path = dir.path().join("page.view.php");
    let view_uri = crate::util::path_to_uri(&view_path);
    let caller_path = dir.path().join("caller.php");
    let caller_uri = crate::util::path_to_uri(&caller_path);
    let caller = "<?php \\Tempest\\View\\view('./page.view.php', title: 'Title');";
    let template = "{{ $title }}";
    std::fs::write(&view_path, template).unwrap();
    std::fs::write(&caller_path, caller).unwrap();
    backend.update_ast(&caller_uri, caller);
    backend.update_ast(&view_uri, template);
    let at = LineIndex::new(caller).position(caller.find("title:").unwrap());
    let map = backend.symbol_map_for(&caller_uri).unwrap();
    let offset = position_to_offset(caller, at);
    let call = map.find_enclosing_call_site(offset).unwrap();
    let file = backend.file_context(&caller_uri);
    assert!(
        is_view_function(call.call_expression, &file, offset),
        "{}",
        call.call_expression
    );
    assert!(
        call_target(&backend, &caller_uri, caller, &file, call.args_start).is_some(),
        "no call target at {}",
        call.args_start
    );
    assert!(
        backend
            .tempest_view_argument_target(&caller_uri, caller, at)
            .is_some(),
        "no argument target"
    );
    let hover = backend
        .tempest_view_argument_hover(&caller_uri, caller, at)
        .unwrap();
    assert!(
        matches!(hover.contents, Contents::MarkupContent(markup) if markup.value.contains("View parameter"))
    );
    assert!(
        backend
            .tempest_view_argument_definition(&caller_uri, "<?php", at)
            .is_none()
    );
    let context = NamedArgContext {
        call_expression: "\\Tempest\\View\\view".into(),
        existing_named_args: Vec::new(),
        positional_count: 0,
        prefix: "invalid".into(),
        args_text: caller.into(),
    };
    assert!(
        backend
            .tempest_view_named_completions(
                &caller_uri,
                &context,
                caller,
                Position::default(),
                &FileContext::default()
            )
            .is_none()
    );
    let stale = caller.replace("title:", "wrong:");
    assert!(
        backend
            .tempest_view_argument_hover(&caller_uri, &stale, at)
            .is_none()
    );
    backend.set_is_tempest(false);
    assert!(
        backend
            .tempest_view_argument_definition(&caller_uri, caller, at)
            .is_none()
    );
    assert!(
        backend
            .tempest_view_argument_hover(&caller_uri, caller, at)
            .is_none()
    );
    assert!(
        backend
            .tempest_view_named_completions(
                &caller_uri,
                &context,
                caller,
                at,
                &FileContext::default()
            )
            .is_none()
    );
}

#[test]
fn ordinary_php_views_use_their_file_scope() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.php");
    let uri = crate::util::path_to_uri(&path);
    let source =
        "<?php class Local { public string $title; } /** @var string $title */ echo $title;";
    std::fs::write(&path, source).unwrap();
    backend.update_ast(&uri, source);
    let reads = backend.tempest_view_input_reads(&uri, source);
    assert_eq!(reads.len(), 1);
    assert!(reads.contains_key(&atom("title")));
}
