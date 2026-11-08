use super::*;
use crate::test_fixtures::make_backend;

#[test]
fn action_literals_require_an_exact_class_or_string() {
    assert_eq!(
        literal(&PhpType::parse("class-string<Routes\\Home>")),
        Some("Routes\\Home".into())
    );
    for unknown in [
        "class-string",
        "class-string<object>",
        "class-string<Routes\\Home|Routes\\Books>",
        "string",
        "int",
        "42",
    ] {
        assert_eq!(literal(&PhpType::parse(unknown)), None, "{unknown}");
    }
}

#[test]
fn placeholder_ranges_skip_regex_quantifiers_and_incomplete_names() {
    let uri = "/{id:\\d{1,4}}/{?name}/{}/{unfinished";
    let names: Vec<_> = placeholders(uri)
        .iter()
        .map(|&(start, end)| &uri[start..end])
        .collect();
    assert_eq!(names, ["id", "name"]);
    assert!(placeholders("/{not-a-name}/plain").is_empty());
}

#[test]
fn route_extraction_requires_semantic_class_names() {
    let source = "<?php class Books { #[Get('/books')] public function index() {} }";
    let arena = mago_allocator::LocalArena::new();
    let file = mago_database::file::FileId::new(b"routes.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file, source.as_bytes());
    let map = crate::symbol_map::extract_symbol_map(program, source);
    assert!(map.tempest_routes.is_empty());
}

#[test]
fn route_navigation_rejects_missing_and_stale_maps() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let uri = crate::util::path_to_uri(&dir.path().join("routes.php"));
    let source = "<?php \\Tempest\\Router\\uri('/books');";
    assert!(
        backend
            .tempest_route_definition(&uri, source, Position::default())
            .is_empty()
    );
    let mut locations = Vec::new();
    backend.extend_tempest_route_parameter_references(&uri, "book", 0, &mut locations);
    assert!(locations.is_empty());
    backend.update_ast(&uri, source);
    assert!(
        backend
            .tempest_route_definition(&uri, "<?php", Position::default())
            .is_empty()
    );
}

#[test]
fn route_string_completion_tolerates_a_buffer_not_yet_indexed() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let uri = crate::util::path_to_uri(&dir.path().join("routes.php"));
    let content = "<?php \\Tempest\\Router\\uri('');";
    let offset = content.find("''").unwrap() + 1;
    let position = crate::text_position::LineIndex::new(content).position(offset);
    let code = crate::completion::source::code_context::code_context_at(content, offset).unwrap();
    let response = backend.tempest_route_string_completion(
        &uri,
        content,
        position,
        &FileContext::default(),
        &code,
    );
    assert!(
        matches!(response, Some(CompletionResponse::CompletionItemList(items)) if items.is_empty())
    );
}

#[test]
fn parameter_references_tolerate_deleted_callers_and_changed_buffers() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let controller_uri = crate::util::path_to_uri(&dir.path().join("Books.php"));
    let controller = "<?php namespace Tempest\\Router; interface Route {} class Get implements Route {} namespace App; class Books { #[\\Tempest\\Router\\Get('/books/{book}')] public function show(string $book): void {} }";
    backend.update_ast(&controller_uri, controller);
    backend.open_files.write().insert(
        controller_uri.clone(),
        std::sync::Arc::new(controller.to_string()),
    );
    let map = backend.symbol_map_for(&controller_uri).unwrap();
    let offset = controller.find("$book").unwrap() as u32;
    let scope = map.find_variable_scope("book", offset);
    let caller = "<?php \\Tempest\\Router\\uri('/books/{book}', book: 'one');";
    let missing_uri = crate::util::path_to_uri(&dir.path().join("deleted.php"));
    backend.update_ast(&missing_uri, caller);
    let changed_uri = crate::util::path_to_uri(&dir.path().join("changed.php"));
    backend.update_ast(&changed_uri, caller);
    backend
        .open_files
        .write()
        .insert(changed_uri, std::sync::Arc::new("<?php".to_string()));
    // A resource provider can disappear while its indexed map is still present.
    backend.update_ast("invalid resource identifier", caller);
    backend.open_files.write().insert(
        "invalid resource identifier".into(),
        std::sync::Arc::new(caller.into()),
    );
    let mut locations = Vec::new();
    backend.extend_tempest_route_parameter_references(
        &controller_uri,
        "book",
        scope,
        &mut locations,
    );
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri.as_str(), controller_uri);
}
