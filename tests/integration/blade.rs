#[cfg(test)]
mod tests {
    use crate::common::{create_test_backend, open_document, open_php};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    #[tokio::test]
    async fn test_goto_definition_in_blade_file() {
        let backend = create_test_backend();

        // 1. Define a class in a PHP file
        let php_uri = Url::parse("file:///Logger.php").unwrap();
        let php_text = "<?php class Logger { public function info() {} }";
        open_php(&backend, &php_uri, php_text).await;

        // 2. Try to use it in a Blade file
        let blade_uri = Url::parse("file:///view.blade.php").unwrap();
        let blade_text = "@php $logger = new Logger(); @endphp\n{{ $logger->info() }}";

        open_document(&backend, &blade_uri, "blade", blade_text).await;

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
    async fn test_inline_php_directive_assignment_updates_scope() {
        let backend = create_test_backend();

        let php_uri = Url::parse("file:///Logger.php").unwrap();
        let php_text = "<?php class Logger { public function info() {} }";
        open_php(&backend, &php_uri, php_text).await;

        let blade_uri = Url::parse("file:///inline.blade.php").unwrap();
        let blade_text = "@php($logger = new Logger())\n{{ $logger->info() }}";

        open_document(&backend, &blade_uri, "blade", blade_text).await;

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

        assert!(
            result.is_some(),
            "Inline @php(...) assignment should update the template scope"
        );
        match result.unwrap() {
            GotoDefinitionResponse::Scalar(location) => {
                assert_eq!(location.uri, php_uri);
                assert_eq!(location.range.start.line, 0);
            }
            _ => panic!("Expected scalar location"),
        }
    }

    /// A standalone `@var` block immediately above an inline `@php(…)`
    /// must not swallow the assignment that follows it.
    #[tokio::test]
    async fn test_inline_php_directive_assignment_after_var_block() {
        let backend = create_test_backend();

        let php_uri = Url::parse("file:///Logger.php").unwrap();
        let php_text = "<?php class Logger { public function info() {} \
                        public function self_(): Logger { return $this; } }";
        open_php(&backend, &php_uri, php_text).await;

        let blade_uri = Url::parse("file:///inline_var.blade.php").unwrap();
        let blade_text = "@php\n/** @var \\Logger $base */\n@endphp\n\
                          @php($logger = $base->self_())\n{{ $logger->info() }}";

        open_document(&backend, &blade_uri, "blade", blade_text).await;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 4,
                    character: 13, // $logger->in|fo()
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = backend.goto_definition(params).await.unwrap();

        assert!(
            result.is_some(),
            "Inline @php(...) assignment below a @var block should still update the scope"
        );
    }

    #[tokio::test]
    async fn test_blade_if_endif_parsing() {
        let backend = create_test_backend();

        let blade_uri = Url::parse("file:///test.blade.php").unwrap();
        let blade_text = "@if(true)\n    {{ config('app.name') }}\n@endif";

        // This should not produce any syntax errors now.
        open_document(&backend, &blade_uri, "blade", blade_text).await;

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

        open_document(&backend, &blade_uri, "blade", blade_text).await;

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
        open_document(&backend, &blade_uri, "blade", blade_text).await;

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
        open_php(&backend, &php_uri, php_text).await;

        let blade_uri = Url::parse("file:///view.blade.php").unwrap();
        let blade_text = "@php $l = new Logger(); @endphp\n{{ $l->info() }}";

        open_document(&backend, &blade_uri, "blade", blade_text).await;

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

        open_document(&backend, &blade_uri, "blade", blade_text).await;

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
        open_php(&backend, &php_uri, php_text).await;

        let blade_uri = Url::parse("file:///shop.blade.php").unwrap();
        // Line 0: @php $item = new Item(); @endphp
        // Line 1: {{ $item-> }}
        let blade_text = "@php $item = new Item(); @endphp\n{{ $item-> }}";

        open_document(&backend, &blade_uri, "blade", blade_text).await;

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
        open_document(&backend, &uri, "blade", body).await;

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
        open_document(&backend, &uri, "blade", body).await;

        let undefined = undefined_variables(&backend, &uri).await;
        assert!(
            !undefined.iter().any(|m| m.contains("$slot")),
            "@props marks the template as a component: {:?}",
            undefined
        );
    }

    /// `@props` declares its keys as local variables (typed from their
    /// default), so a component that only reads its own declared props
    /// never reports them as undefined — regression test for the
    /// multi-line array form used throughout real components.
    #[tokio::test]
    async fn test_props_directive_declares_its_variables() {
        let body = "@props([\n    'caption' => '',\n])\n<span>{{ $caption }}</span>\n";
        let (backend, _dir, uri) =
            component_workspace("resources/views/components/box.blade.php", body);
        open_document(&backend, &uri, "blade", body).await;

        let undefined = undefined_variables(&backend, &uri).await;
        assert!(
            undefined.is_empty(),
            "@props should declare $caption: {:?}",
            undefined
        );

        let hover = hover_text(&backend, &uri, 3, 9).await;
        assert!(
            hover.contains("$caption = ''"),
            "$caption should resolve to its '' default, got: {}",
            hover
        );
    }

    /// The implicit variables are a component's, not every template's.
    #[tokio::test]
    async fn test_plain_view_still_flags_slot_as_undefined() {
        let body = "{{ $slot }}\n";
        let (backend, _dir, uri) = component_workspace("resources/views/page.blade.php", body);
        open_document(&backend, &uri, "blade", body).await;

        let undefined = undefined_variables(&backend, &uri).await;
        assert!(
            undefined.iter().any(|m| m.contains("$slot")),
            "a plain view does not receive $slot: {:?}",
            undefined
        );
    }

    /// `isset($x) &&`/`!isset($x) ||` guards the rest of the same
    /// short-circuit chain, a pattern that shows up constantly in
    /// `@if` conditions.
    #[tokio::test]
    async fn test_isset_guards_short_circuit_chain_in_if_directive() {
        let body = "@if (isset($isOutlet) && $isOutlet == 1)\nyes\n@endif\n@if (!isset($stockGtr0) || $stockGtr0 == 'true')\nyes\n@endif\n";
        let (backend, _dir, uri) = component_workspace("resources/views/page.blade.php", body);
        open_document(&backend, &uri, "blade", body).await;

        let undefined = undefined_variables(&backend, &uri).await;
        assert!(
            undefined.is_empty(),
            "isset()/!isset() should guard the rest of the && / || chain: {:?}",
            undefined
        );
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

    /// A `@var` block at the top of a template stays in scope for the
    /// whole raw `<?php` region, even when a line comment separates it
    /// from the first statement and the use site sits in a later sibling
    /// `if` block.
    #[tokio::test]
    async fn test_var_docblock_survives_line_comment_and_sibling_blocks() {
        let backend = create_test_backend();

        let php_uri = Url::parse("file:///ShowViewModel.php").unwrap();
        let php_text = "<?php\nclass ShowViewModel {\n    public ?string $rawImageUrl = null;\n    public int $ratingCount = 0;\n    public float $ratingScore = 0.0;\n}\n";
        open_php(&backend, &php_uri, php_text).await;

        let uri = Url::parse("file:///show.blade.php").unwrap();
        let body = "@php\n    /**\n     * @var ShowViewModel $model\n     */\n@endphp\n<?php\n// short\n$schema = [];\n\nif ($model->rawImageUrl !== null) {\n    $schema['image'] = $model->rawImageUrl;\n}\n\nif ($model->ratingCount > 0) {\n    $schema['aggregateRating'] = [\n        'ratingValue' => $model->ratingScore,\n    ];\n}\n?>\n";
        open_document(&backend, &uri, "blade", body).await;

        // `'ratingValue' => $model->ratingScore,` — the deepest use site.
        let hover = hover_text(&backend, &uri, 15, 26).await;
        assert!(
            hover.contains("ShowViewModel"),
            "$model must still be typed inside the nested array literal, got: {}",
            hover
        );
    }

    /// A standalone `@var` docblock that carries generic arguments
    /// (`Collection<string, Loaf>`) must narrow a member call through
    /// the class-level `@template`, not just resolve the bare class.
    ///
    /// Every `{{ … }}` echo in a Blade template compiles to `echo e( … );`,
    /// which is a non-expression statement — the forward walker's
    /// diagnostic scope cache recorded a scope snapshot for that statement
    /// *before* applying the standalone `@var`, and never re-recorded it
    /// afterward, so a call site inside the echo's own expression saw
    /// `$byName` typed as bare `Collection` and fell back to `TKey`'s
    /// declared bound (`array-key`) instead of the annotated `string`.
    #[tokio::test]
    async fn test_var_docblock_with_generic_args_narrows_echo_call() {
        let backend = create_test_backend();

        let php_uri = Url::parse("file:///Collection.php").unwrap();
        let php_text = "<?php\nnamespace App\\Models;\nclass Loaf {}\n/**\n * @template TKey of array-key\n * @template TValue\n */\nclass Collection {\n    /** @param TKey|null $key */\n    public function get($key) {}\n}\n";
        open_php(&backend, &php_uri, php_text).await;

        let uri = Url::parse("file:///byname.blade.php").unwrap();
        let body = "@php\n/** @var \\App\\Models\\Collection<string, \\App\\Models\\Loaf> $byName */\n@endphp\n{{ $byName->get([1]) }}\n";
        open_document(&backend, &uri, "blade", body).await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        let messages: Vec<String> = diags.into_iter().map(|d| d.message).collect();

        assert!(
            messages.iter().any(|m| m.contains("expects string|null")),
            "expected TKey to narrow to string via the @var's generic arguments, got: {:?}",
            messages
        );
    }

    /// A component that declares its contract in a docblock and then lists
    /// the same names in `@props` keeps the declared type for *every* key.
    /// `@props` supplies defaults for what the contract leaves out; it never
    /// overrides the contract.
    #[tokio::test]
    async fn test_declared_types_survive_a_props_list() {
        let body = "@php\n/**\n * @var string $poster\n * @var string $video\n */\n@endphp\n@props(['poster', 'video'])\n\n{{ strlen($poster) }}\n{{ strlen($video) }}\n";
        let (backend, _dir, uri) =
            component_workspace("resources/views/components/player.blade.php", body);
        open_document(&backend, &uri, "blade", body).await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        let messages: Vec<String> = diags.into_iter().map(|d| d.message).collect();

        assert!(
            !messages.iter().any(|m| m.contains("got null")),
            "a @props key must not be typed null over its declared type: {:?}",
            messages
        );

        for (line, name) in [(8u32, "$poster"), (9, "$video")] {
            let hover = hover_text(&backend, &uri, line, 12).await;
            assert!(
                hover.contains("string"),
                "{name} must keep its declared type, got: {hover}"
            );
        }
    }

    /// A `@props` entry with a default is typed from that default; an entry
    /// without one is *required*, so its value comes from the caller and it
    /// stays unknown rather than being invented as `null`.
    #[tokio::test]
    async fn test_props_are_typed_from_their_defaults() {
        let body = "@props(['variant' => 'info', 'collapsed' => false, 'tags' => [], 'heading'])\n{{ $variant }}\n{{ $collapsed }}\n{{ $tags }}\n{{ $heading }}\n";
        let (backend, _dir, uri) =
            component_workspace("resources/views/components/panel.blade.php", body);
        open_document(&backend, &uri, "blade", body).await;

        for (line, expected) in [(1u32, "'info'"), (2, "false"), (3, "array")] {
            let hover = hover_text(&backend, &uri, line, 5).await;
            assert!(
                hover.contains(expected),
                "line {line} should be typed {expected}, got: {hover}"
            );
        }

        // A required prop must not read as `null`: that would make every use
        // of it a type error against whatever the caller really passes.
        let hover = hover_text(&backend, &uri, 4, 5).await;
        assert!(
            !hover.contains("null"),
            "a required prop must not be typed null, got: {hover}"
        );
    }

    /// Naming a key in `@props` is what removes it from `$attributes`, so a
    /// declared prop the body never reads is a component-API decision, not a
    /// dead local assignment. Deleting it would change the rendered output.
    #[tokio::test]
    async fn test_a_declared_prop_is_not_an_unused_variable() {
        let body = "@props(['accordionId', 'headingId'])\n\n<button id=\"{{ $headingId }}\" {{ $attributes }}></button>\n";
        let (backend, _dir, uri) =
            component_workspace("resources/views/components/accordion.blade.php", body);
        open_document(&backend, &uri, "blade", body).await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_unused_variable_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        let messages: Vec<String> = diags.into_iter().map(|d| d.message).collect();

        assert!(
            messages.is_empty(),
            "a declared prop must not be reported unused: {:?}",
            messages
        );
    }

    /// A raw `{!! … !!}` echo starts at `{!!` (one brace), so a variable
    /// declared in a `<?php ?>` block and only read by a raw echo is used,
    /// not dead.
    #[tokio::test]
    async fn test_a_variable_read_by_a_raw_echo_is_not_unused() {
        let body = "<?php\n$acmeProfile = \"xxx\";\n?>\n\n{!! $acmeProfile !!}\n";
        let (backend, _dir, uri) = component_workspace("resources/views/profile.blade.php", body);
        open_document(&backend, &uri, "blade", body).await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_unused_variable_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        let messages: Vec<String> = diags.into_iter().map(|d| d.message).collect();

        assert!(
            messages.is_empty(),
            "a variable read by a raw echo must not be reported unused: {:?}",
            messages
        );
    }

    /// `@unless (!$user)` is how Blade spells a doubly negated truthiness
    /// guard, since `@unless` compiles to `if(!…)` and the condition adds a
    /// `!` of its own.  The pair cancels, so the body sees what
    /// `@if ($user)` would give it.
    #[tokio::test]
    async fn test_unless_a_negated_condition_narrows_its_body() {
        let backend = create_test_backend();

        let php_uri = Url::parse("file:///User.php").unwrap();
        let php_text = "<?php\nclass User {\n    public string $name = '';\n}\n";
        open_php(&backend, &php_uri, php_text).await;

        let uri = Url::parse("file:///greeting.blade.php").unwrap();
        let body = "@php\n/** @var ?User $user */\n@endphp\n@unless (!$user)\n    {{ $user->name }}\n@endunless\n";
        open_document(&backend, &uri, "blade", body).await;

        // `{{ $user->name }}` — hover on `$user`.
        let hover = hover_text(&backend, &uri, 4, 9).await;
        assert!(
            hover.contains("$user = User") && !hover.contains("?User"),
            "the doubly negated guard must narrow $user the way `@if ($user)` does, got: {}",
            hover
        );
    }

    #[tokio::test]
    async fn test_lowering_declarations_are_not_workspace_symbols() {
        let backend = create_test_backend();

        let first = Url::parse("file:///page.blade.php").unwrap();
        let second = Url::parse("file:///other.blade.php").unwrap();
        let body = "@can('update', $post)\n    <p>ok</p>\n@endcan\n@section('content')\n@endsection\n@stack('scripts')\n";
        open_document(&backend, &first, "blade", body).await;
        open_document(&backend, &second, "blade", body).await;

        // Keeping them out of the index must not turn every marker call
        // the lowering emits into an unknown function.
        let virtual_php = backend.blade_virtual_php(first.as_str()).unwrap();
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(first.as_str(), &virtual_php, &mut diags);
        let unknown: Vec<String> = diags
            .into_iter()
            .filter(|d| matches!(&d.code, Some(NumberOrString::String(code)) if code == "unknown_function"))
            .map(|d| d.message)
            .collect();
        assert!(
            unknown.is_empty(),
            "marker calls must stay resolvable inside the template: {:?}",
            unknown
        );

        // The wrapper the body is lowered into and the marker functions
        // the directives compile to are the preprocessor's own
        // boilerplate: no file wrote them, so nothing should find them.
        let leaked: Vec<String> = backend
            .handle_workspace_symbol("blade")
            .unwrap_or_default()
            .into_iter()
            .map(|symbol| symbol.name)
            .filter(|name| name.contains("blade_") || name.contains("__blade"))
            .collect();
        assert!(
            leaked.is_empty(),
            "the lowering's own declarations reached workspace symbols: {:?}",
            leaked
        );

        // Nor are they something to complete: a PHP file typing `blade`
        // is not reaching for the lowering's own functions.
        let php_uri = Url::parse("file:///helpers.php").unwrap();
        let php = "<?php\nblade\n";
        open_php(&backend, &php_uri, php).await;
        let completions = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: php_uri },
                    position: Position {
                        line: 1,
                        character: 5,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap();
        let offered: Vec<String> = match completions {
            Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
            Some(CompletionResponse::List(list)) => {
                list.items.into_iter().map(|i| i.label).collect()
            }
            None => Vec::new(),
        };
        assert!(
            !offered
                .iter()
                .any(|label| label.contains("blade_") || label.contains("__blade")),
            "the lowering's own functions were offered as completions: {:?}",
            offered
        );
    }
}
