//! Integration tests for `context.only`, the kind filter a client puts on
//! a code action request: only actions of the requested kinds (or kinds
//! under them) come back, and the advertised kinds cover every action.

use crate::common::{
    create_test_backend, find_action, get_code_actions_in_range, get_code_actions_of_kinds,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const URI: &str = "file:///test.php";

/// An unused import (a `quickfix` and a `source.organizeImports` action)
/// and a concatenation (`refactor.rewrite` and `refactor.extract`
/// actions when selected).
const CONTENT: &str = "<?php\nuse Foo\\Bar;\n\nfunction greet(string $name): string {\n    return 'Hello ' . $name;\n}\n";

const REMOVE_ONE: &str = "Remove unused import";
const REMOVE_ALL: &str = "Remove all unused imports";
const INTERPOLATE: &str = "Convert to string interpolation";
const EXTRACT: &str = "Extract variable";

fn import_range() -> Range {
    Range::new(Position::new(1, 4), Position::new(1, 4))
}

fn concat_range() -> Range {
    Range::new(Position::new(4, 11), Position::new(4, 27))
}

fn backend() -> Backend {
    let backend = create_test_backend();
    backend.update_ast(URI, CONTENT);
    backend
}

fn kinds(actions: &[CodeActionResponse]) -> Vec<String> {
    actions
        .iter()
        .map(|action| match action {
            CodeActionResponse::CodeAction(action) => action
                .kind
                .as_ref()
                .map_or_else(String::new, |kind| kind.as_str().to_string()),
            CodeActionResponse::Command(_) => String::new(),
        })
        .collect()
}

#[test]
fn unfiltered_request_offers_every_kind() {
    let backend = backend();
    let on_import = get_code_actions_in_range(&backend, URI, CONTENT, import_range());
    assert!(find_action(&on_import, REMOVE_ONE).is_some());
    assert!(find_action(&on_import, REMOVE_ALL).is_some());

    let on_concat = get_code_actions_in_range(&backend, URI, CONTENT, concat_range());
    assert!(find_action(&on_concat, INTERPOLATE).is_some());
    assert!(find_action(&on_concat, EXTRACT).is_some());
}

#[test]
fn quickfix_only_drops_the_other_kinds_of_one_collector() {
    let backend = backend();
    let actions = get_code_actions_of_kinds(
        &backend,
        URI,
        CONTENT,
        import_range(),
        vec![CodeActionKind::QuickFix],
    );
    assert!(find_action(&actions, REMOVE_ONE).is_some());
    assert!(find_action(&actions, REMOVE_ALL).is_none());
    assert!(
        kinds(&actions).iter().all(|kind| kind == "quickfix"),
        "{:?}",
        kinds(&actions)
    );
}

#[test]
fn organize_imports_only_offers_the_bulk_removal() {
    let backend = backend();
    let actions = get_code_actions_of_kinds(
        &backend,
        URI,
        CONTENT,
        import_range(),
        vec![CodeActionKind::SourceOrganizeImports],
    );
    assert!(find_action(&actions, REMOVE_ALL).is_some());
    assert!(find_action(&actions, REMOVE_ONE).is_none());
    assert!(
        kinds(&actions)
            .iter()
            .all(|kind| kind == "source.organizeImports"),
        "{:?}",
        kinds(&actions)
    );
}

#[test]
fn parent_kind_matches_its_sub_kinds() {
    let backend = backend();
    let actions = get_code_actions_of_kinds(
        &backend,
        URI,
        CONTENT,
        concat_range(),
        vec![CodeActionKind::Refactor],
    );
    assert!(find_action(&actions, INTERPOLATE).is_some());
    assert!(find_action(&actions, EXTRACT).is_some());
    assert!(
        kinds(&actions)
            .iter()
            .all(|kind| kind.starts_with("refactor")),
        "{:?}",
        kinds(&actions)
    );
}

#[test]
fn sub_kind_does_not_match_a_sibling() {
    let backend = backend();
    let actions = get_code_actions_of_kinds(
        &backend,
        URI,
        CONTENT,
        concat_range(),
        vec![CodeActionKind::RefactorExtract],
    );
    assert!(find_action(&actions, EXTRACT).is_some());
    assert!(find_action(&actions, INTERPOLATE).is_none());
}

#[test]
fn kind_prefix_without_a_dot_does_not_match() {
    let backend = backend();
    let actions = get_code_actions_of_kinds(
        &backend,
        URI,
        CONTENT,
        concat_range(),
        vec![CodeActionKind::new("refactor.ext")],
    );
    assert!(actions.is_empty(), "{:?}", kinds(&actions));
}

#[test]
fn several_requested_kinds_are_all_offered() {
    let backend = backend();
    let actions = get_code_actions_of_kinds(
        &backend,
        URI,
        CONTENT,
        import_range(),
        vec![
            CodeActionKind::QuickFix,
            CodeActionKind::SourceOrganizeImports,
        ],
    );
    assert!(find_action(&actions, REMOVE_ONE).is_some());
    assert!(find_action(&actions, REMOVE_ALL).is_some());
}

#[tokio::test]
async fn advertised_kinds_cover_every_offered_action() {
    let backend = backend();
    let init = backend
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    let Some(CodeActionProvider::CodeActionOptions(options)) =
        init.capabilities.code_action_provider
    else {
        panic!("code actions should be advertised with options");
    };
    let advertised = options.code_action_kinds.unwrap_or_default();

    let mut actions = get_code_actions_in_range(&backend, URI, CONTENT, import_range());
    actions.extend(get_code_actions_in_range(
        &backend,
        URI,
        CONTENT,
        concat_range(),
    ));
    for kind in kinds(&actions) {
        assert!(
            advertised.iter().any(|declared| {
                let declared = declared.as_str();
                kind == declared || kind.starts_with(&format!("{declared}."))
            }),
            "{kind} is offered but not advertised"
        );
    }
}
