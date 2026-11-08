//! Documentation is shared by Tempest markup completion and hover.

use crate::common::{
    completion_response, create_psr4_workspace, create_test_backend, hover_response, hover_text,
    open_php, position_after, position_of, response_items, workspace_uri,
};
use tower_lsp_server::gen_lsp_types::*;

const DIRECTIVES: [(&str, &str, &str); 8] = [
    (":if", "condition", "control-structures"),
    (":elseif", "condition", "control-structures"),
    (":else", "branch", "control-structures"),
    (":isset", "isset", "control-structures"),
    (":foreach", "item", "control-structures"),
    (":forelse", "empty", "control-structures"),
    (":apply", "attributes", "dynamic-attributes"),
    (":as", "tag", "expression-attributes"),
];

const COMPONENTS: [(&str, &str); 10] = [
    ("x-base", "Tailwind"),
    ("x-form", "POST"),
    ("x-input", "validation"),
    ("x-submit", "Submit"),
    ("x-icon", "Iconify"),
    ("x-vite-tags", "entrypoints"),
    ("x-template", "wrapper"),
    ("x-slot", "slot"),
    ("x-markdown", "Markdown"),
    ("x-component", "selected"),
];

#[tokio::test]
async fn tempest_builtin_directives_share_completion_and_hover_documentation() {
    let backend = create_test_backend();
    let uri = Uri::from_file_path(std::env::temp_dir().join("directive-docs.view.php")).unwrap();
    for (name, description, anchor) in DIRECTIVES {
        let source = format!("<!-- 📚 -->\r\n<div {name}=\"true\"></div>");
        open_php(&backend, &uri, &source).await;
        let start = position_of(&source, name);
        let end = position_after(&source, name);
        let response = completion_response(&backend, &uri, end.line, end.character, None).await;
        let items = response_items(response).unwrap();
        let item = items.iter().find(|item| item.label == name).unwrap();
        let Some(Documentation::MarkupContent(documentation)) = &item.documentation else {
            panic!("Missing documentation for {name}");
        };
        assert_eq!(documentation.kind, MarkupKind::Markdown);
        assert!(
            documentation.value.contains(description),
            "{documentation:?}"
        );
        assert!(documentation.value.contains(&format!("view-spec#{anchor}")));
        for at in [start, end] {
            let hover = hover_response(&backend, &uri, at.line, at.character)
                .await
                .unwrap();
            assert_eq!(hover_text(&hover), documentation.value);
            assert_eq!(hover.range, Some(Range { start, end }));
        }
    }
}

#[tokio::test]
async fn tempest_builtin_components_document_opening_closing_and_incomplete_tags() {
    let files: Vec<_> = COMPONENTS
        .iter()
        .map(|(name, _)| (format!("app/{name}.view.php"), "<p>Component</p>"))
        .collect();
    let files: Vec<_> = files
        .iter()
        .map(|(path, source)| (path.as_str(), *source))
        .collect();
    let (backend, _dir) =
        create_psr4_workspace(r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#, &files);
    let uri = workspace_uri(&backend, "app/documentation.view.php");
    for (name, description) in COMPONENTS {
        for source in [format!("📚 <{name}></{name}>"), format!("<{name}")] {
            open_php(&backend, &uri, &source).await;
            let at = position_after(&source, &format!("<{name}"));
            let response = completion_response(&backend, &uri, at.line, at.character, None).await;
            let items = response_items(response).unwrap();
            let item = items.iter().find(|item| item.label == name).unwrap();
            let Some(Documentation::MarkupContent(documentation)) = &item.documentation else {
                panic!("Missing documentation for {name}");
            };
            assert!(
                documentation.value.contains(description),
                "{documentation:?}"
            );
            assert!(documentation.value.contains(&format!("view-spec#{name}")));
            for (offset, _) in source.match_indices(name) {
                let start = position_after(&source, &source[..offset]);
                let end = Position::new(start.line, start.character + name.len() as u32);
                for at in [start, end] {
                    let hover = hover_response(&backend, &uri, at.line, at.character)
                        .await
                        .unwrap();
                    assert_eq!(hover_text(&hover), documentation.value);
                    assert_eq!(hover.range, Some(Range { start, end }));
                }
            }
        }
    }
}

#[tokio::test]
async fn tempest_builtin_documentation_leaves_other_markup_and_php_alone() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[(
            "app/x-custom.view.php",
            "<?php /** @var string $xForm */ ?>{{ $xForm }}",
        )],
    );
    let uri = workspace_uri(&backend, "app/documentation.view.php");
    let source = r#"<?php $condition = true; ?>
<!-- <x-template :if="true" /> -->
{{-- <x-slot /> --}}
<x-custom x-form="local" ::if="frontend" :unknown="1" :if="$condition" title="x-form" />
<div>No documentation</div>
</x-template :if>
"#;
    open_php(&backend, &uri, source).await;
    for needle in [
        "<!-- <x-tem",
        "<!-- <x-template :i",
        "{{-- <x-sl",
        "<x-cus",
        "::i",
        ":unk",
        "<di",
        "No doc",
        "</x-template :i",
        "title=\"x-fo",
    ] {
        let at = position_after(source, needle);
        assert!(
            hover_response(&backend, &uri, at.line, at.character)
                .await
                .is_none(),
            "{needle}"
        );
    }
    let at = position_after(source, ":if=\"$cond");
    let hover = hover_response(&backend, &uri, at.line, at.character)
        .await
        .unwrap();
    assert!(hover_text(&hover).contains("$condition"));
    assert!(!hover_text(&hover).contains("view-spec"));
    let at = position_after(source, "x-form");
    let hover = hover_response(&backend, &uri, at.line, at.character)
        .await
        .unwrap();
    assert!(hover_text(&hover).contains("$xForm"));
    assert!(!hover_text(&hover).contains("view-spec"));
    let response = completion_response(&backend, &uri, at.line, at.character, None).await;
    let items = response_items(response).unwrap();
    assert!(
        items
            .iter()
            .find(|item| item.label == "x-form")
            .unwrap()
            .documentation
            .is_none()
    );

    let source = "<x-";
    open_php(&backend, &uri, source).await;
    let response = completion_response(&backend, &uri, 0, 3, None).await;
    let items = response_items(response).unwrap();
    assert!(!items.iter().any(|item| item.label == "x-form"));
    assert!(
        items
            .iter()
            .find(|item| item.label == "x-custom")
            .unwrap()
            .documentation
            .is_none()
    );

    let php = workspace_uri(&backend, "app/ordinary.php");
    let source = "<x-template :if=\"true\" />";
    open_php(&backend, &php, source).await;
    assert!(hover_response(&backend, &php, 0, 5).await.is_none());
}
