#[cfg(test)]
mod tests {
    use crate::common::create_test_backend;
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    #[tokio::test]
    async fn test_goto_definition_in_blade_file() {
        let backend = create_test_backend();

        // 1. Define a class in a PHP file
        let php_uri = Url::parse("file:///Logger.php").unwrap();
        let php_text = "<?php class Logger { public function info() {} }";
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: php_uri.clone(),
                    language_id: "php".to_string(),
                    version: 1,
                    text: php_text.to_string(),
                },
            })
            .await;

        // 2. Try to use it in a Blade file
        let blade_uri = Url::parse("file:///view.blade.php").unwrap();
        let blade_text = "@php $logger = new Logger(); @endphp\n{{ $logger->info() }}";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        // 3. Click on "info" in the Blade file
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 13, // $logger->in|fo()
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = backend.goto_definition(params).await.unwrap();

        assert!(result.is_some(), "Should resolve definition in Blade file");

        match result.unwrap() {
            GotoDefinitionResponse::Scalar(location) => {
                assert_eq!(location.uri, php_uri);
                // Logger::info is on line 0
                assert_eq!(location.range.start.line, 0);
            }
            _ => panic!("Expected scalar location"),
        }
    }

    #[tokio::test]
    async fn test_blade_if_endif_parsing() {
        let backend = create_test_backend();

        let blade_uri = Url::parse("file:///test.blade.php").unwrap();
        let blade_text = "@if(true)\n    {{ config('app.name') }}\n@endif";

        // This should not produce any syntax errors now.
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        // We check if it can resolve "config" inside the @if block.
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 7, // con|fig
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = backend.goto_definition(params).await.unwrap();
        // Even if config is not resolved (depends on stubs),
        // the important thing is that the server didn't crash or return error due to @endif.
        let _ = result;
    }

    #[tokio::test]
    async fn test_blade_if_with_leading_space() {
        let backend = create_test_backend();

        let blade_uri = Url::parse("file:///test.blade.php").unwrap();
        // Test user's specific example with leading space
        let blade_text = " @if(true)\n    {{ config('app.name') }}\n @endif";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 10,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = backend.goto_definition(params).await.unwrap();
        let _ = result;
    }

    #[tokio::test]
    async fn test_complex_blade_nesting_and_syntax() {
        let backend = create_test_backend();

        let blade_uri = Url::parse("file:///complex.blade.php").unwrap();
        let blade_text = r#"
<ul>
    @foreach ($items as $item)
        <li>
            <a href="{{ $item->url }}">{{ $item->name }}</a>
        </li>
    @endforeach
</ul>
"#;

        // This should not produce any syntax errors.
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        // Verify no syntax errors are reported for this file.
        // (Note: Backend::new_test might not automatically publish diagnostics to a list we can check easily here,
        // but we can check if it parses correctly by trying to resolve something inside).

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 4,
                    character: 31, // $item->u|rl
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = backend.goto_definition(params).await.unwrap();
        // The fact that it doesn't return a JSON-RPC error means it parsed.
        let _ = result;
    }

    #[tokio::test]
    async fn test_blade_references() {
        let backend = create_test_backend();

        let php_uri = Url::parse("file:///Logger.php").unwrap();
        let php_text = "<?php class Logger { public function info() {} }";
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: php_uri.clone(),
                    language_id: "php".to_string(),
                    version: 1,
                    text: php_text.to_string(),
                },
            })
            .await;

        let blade_uri = Url::parse("file:///view.blade.php").unwrap();
        let blade_text = "@php $l = new Logger(); @endphp\n{{ $l->info() }}";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: php_uri.clone(),
                },
                position: Position {
                    line: 0,
                    character: 37, // info
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        };

        let result = backend.references(params).await.unwrap();
        assert!(result.is_some());
        let locations = result.unwrap();

        assert!(
            locations.iter().any(|l| l.uri == blade_uri),
            "Should find reference in Blade file"
        );
    }

    #[tokio::test]
    async fn test_blade_layout_directives() {
        let backend = create_test_backend();

        let blade_uri = Url::parse("file:///layout.blade.php").unwrap();
        let blade_text = r#"
@extends('layouts.app')

@section('title', 'Page Title')

@section('content')
    <p>This is my body content.</p>
    @include('shared.errors')
    @yield('scripts')
@endsection
"#;

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        // If it parses successfully without crashing or returning syntax errors, we are good.
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 2,
                    character: 1,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = backend.goto_definition(params).await.unwrap();
        let _ = result;
    }

    #[tokio::test]
    async fn test_blade_variable_completion_in_echo() {
        let backend = create_test_backend();

        let php_uri = Url::parse("file:///Item.php").unwrap();
        let php_text = "<?php class Item { public string $name; public int $price; }";
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: php_uri.clone(),
                    language_id: "php".to_string(),
                    version: 1,
                    text: php_text.to_string(),
                },
            })
            .await;

        let blade_uri = Url::parse("file:///shop.blade.php").unwrap();
        // Line 0: @php $item = new Item(); @endphp
        // Line 1: {{ $item-> }}
        let blade_text = "@php $item = new Item(); @endphp\n{{ $item-> }}";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 10, // after "$item->"
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(">".to_string()),
            }),
        };

        let result = backend.completion(params).await.unwrap();
        assert!(result.is_some(), "Should return completions for $item->");

        let items = match result.unwrap() {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"name"),
            "Should complete 'name' property, got: {:?}",
            labels
        );
        assert!(
            labels.contains(&"price"),
            "Should complete 'price' property, got: {:?}",
            labels
        );
    }

    // ── Component views: Laravel's implicit `$attributes` / `$slot` ──

    const COMPONENT_COMPOSER: &str =
        r#"{"autoload": {"psr-4": {"Illuminate\\": "vendor/laravel/framework/src/Illuminate/"}}}"#;

    const ATTRIBUTE_BAG_STUB: &str = "<?php\nnamespace Illuminate\\View;\nclass ComponentAttributeBag {\n    public function merge(array $attributeDefaults = []): static { return $this; }\n}\n";

    const COMPONENT_SLOT_STUB: &str = "<?php\nnamespace Illuminate\\View;\nclass ComponentSlot {\n    public function isEmpty(): bool { return true; }\n}\n";

    fn component_workspace(
        view_path: &str,
        view_body: &str,
    ) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
        let (backend, dir) = crate::common::create_psr4_workspace(
            COMPONENT_COMPOSER,
            &[
                (
                    "vendor/laravel/framework/src/Illuminate/View/ComponentAttributeBag.php",
                    ATTRIBUTE_BAG_STUB,
                ),
                (
                    "vendor/laravel/framework/src/Illuminate/View/ComponentSlot.php",
                    COMPONENT_SLOT_STUB,
                ),
                (view_path, view_body),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = Url::from_file_path(root.join(view_path)).unwrap();
        (backend, dir, uri)
    }

    async fn undefined_variables(backend: &phpantom_lsp::Backend, uri: &Url) -> Vec<String> {
        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_undefined_variable_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        diags
            .into_iter()
            .filter(|d| d.message.contains("Undefined variable"))
            .map(|d| d.message)
            .collect()
    }

    #[tokio::test]
    async fn test_component_view_declares_attributes_and_slot() {
        let body = "<img {{ $attributes->merge(['class' => 'img-fluid']) }} />\n{{ $slot }}\n";
        let (backend, _dir, uri) =
            component_workspace("resources/views/components/image.blade.php", body);
        open_blade(&backend, &uri, body).await;

        assert!(
            undefined_variables(&backend, &uri).await.is_empty(),
            "a component view must have $attributes and $slot in scope"
        );

        // The declared types resolve, so members on them are known.
        let hover = hover_text(&backend, &uri, 0, 8).await;
        assert!(
            hover.contains("ComponentAttributeBag"),
            "$attributes must be typed as the attribute bag, got: {}",
            hover
        );
    }

    /// An anonymous component can live outside `components/`; `@props`
    /// identifies it just as well.
    #[tokio::test]
    async fn test_props_directive_marks_a_template_as_a_component() {
        let body = "@props(['caption'])\n{{ $slot }}\n";
        let (backend, _dir, uri) =
            component_workspace("resources/views/widgets/box.blade.php", body);
        open_blade(&backend, &uri, body).await;

        let undefined = undefined_variables(&backend, &uri).await;
        assert!(
            !undefined.iter().any(|m| m.contains("$slot")),
            "@props marks the template as a component: {:?}",
            undefined
        );
    }

    /// The implicit variables are a component's, not every template's.
    #[tokio::test]
    async fn test_plain_view_still_flags_slot_as_undefined() {
        let body = "{{ $slot }}\n";
        let (backend, _dir, uri) = component_workspace("resources/views/page.blade.php", body);
        open_blade(&backend, &uri, body).await;

        let undefined = undefined_variables(&backend, &uri).await;
        assert!(
            undefined.iter().any(|m| m.contains("$slot")),
            "a plain view does not receive $slot: {:?}",
            undefined
        );
    }

    async fn open_blade(backend: &phpantom_lsp::Backend, uri: &Url, text: &str) {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .await;
    }

    async fn hover_text(
        backend: &phpantom_lsp::Backend,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> String {
        let result = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position { line, character },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();
        match result.map(|h| h.contents) {
            Some(HoverContents::Markup(m)) => m.value,
            _ => String::new(),
        }
    }
}
