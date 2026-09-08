//! Call-site variable inference for Blade templates: variables passed
//! at `view()` call sites (array literals, `compact()`, `->with()`)
//! are injected into the template's scope when it declares no `@var`
//! of its own.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER: &str = r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#;

    const ITEM_CLASS: &str =
        "<?php\nnamespace App;\nclass Item { public string $name; public int $price; }\n";

    async fn hover_type(
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
        match result {
            Some(Hover {
                contents: HoverContents::Markup(m),
                ..
            }) => m.value,
            other => panic!("expected markup hover, got {:?}", other),
        }
    }

    /// A `view('shop', ['item' => $item])` call site types `$item`
    /// inside the template.
    #[tokio::test]
    async fn array_literal_call_site_types_template_variable() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    public function show(): mixed {\n        $item = new Item();\n        return view('shop', ['item' => $item]);\n    }\n}\n",
                ),
                ("resources/views/shop.blade.php", "{{ $item->name }}\n"),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        open_document(
            &backend,
            &controller_uri,
            "php",
            &std::fs::read_to_string(root.join("app/Controller.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        // Completion after `$item->` must offer Item's members.
        let result = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: blade_uri.clone(),
                    },
                    position: Position {
                        line: 0,
                        character: 10,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: Some(CompletionContext {
                    trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                    trigger_character: Some(">".to_string()),
                }),
            })
            .await
            .unwrap();

        let items = match result {
            Some(CompletionResponse::Array(items)) => items,
            Some(CompletionResponse::List(list)) => list.items,
            None => panic!("no completions for injected $item->"),
        };
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"name") && labels.contains(&"price"),
            "expected Item members from call-site inference, got: {:?}",
            labels
        );
    }

    /// `->with('key', $value)` chained onto `view()` and `compact()`
    /// both contribute variables.
    #[tokio::test]
    async fn with_chain_and_compact_call_sites_type_template_variables() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    public function show(): mixed {\n        $item = new Item();\n        $count = 3;\n        return view('shop')->with('item', $item)->with(compact('count'));\n    }\n}\n",
                ),
                (
                    "resources/views/shop.blade.php",
                    "{{ $item->name }}\n{{ $count }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        open_document(
            &backend,
            &controller_uri,
            "php",
            &std::fs::read_to_string(root.join("app/Controller.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        let item_hover = hover_type(&backend, &blade_uri, 0, 4).await;
        assert!(
            item_hover.contains("Item"),
            "hover on $item should show Item, got: {}",
            item_hover
        );

        // The call site passes the literal `3`, so the inferred type is
        // the literal int itself.
        let count_hover = hover_type(&backend, &blade_uri, 1, 4).await;
        assert!(
            count_hover.contains("$count = 3") || count_hover.contains("int"),
            "hover on $count should show the inferred int, got: {}",
            count_hover
        );
    }

    /// A template with its own `@var` declaration keeps it: inference
    /// is skipped entirely (declared sources win).
    #[tokio::test]
    async fn declared_var_shadows_call_site_inference() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    public function show(): mixed {\n        return view('shop', ['item' => 42]);\n    }\n}\n",
                ),
                (
                    "resources/views/shop.blade.php",
                    "@php\n/** @var \\App\\Item $item */\n@endphp\n{{ $item->name }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        open_document(
            &backend,
            &controller_uri,
            "php",
            &std::fs::read_to_string(root.join("app/Controller.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &blade_uri, 3, 4).await;
        assert!(
            hover.contains("Item"),
            "declared @var must win over the int call site, got: {}",
            hover
        );
    }

    /// Multiple call sites union their types per variable.
    #[tokio::test]
    async fn multiple_call_sites_union_types() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/AController.php",
                    "<?php\nnamespace App;\nclass AController {\n    public function show(): mixed {\n        return view('shop', ['subject' => new Item()]);\n    }\n}\n",
                ),
                (
                    "app/BController.php",
                    "<?php\nnamespace App;\nclass BController {\n    public function show(): mixed {\n        return view('shop', ['subject' => 'fallback']);\n    }\n}\n",
                ),
                ("resources/views/shop.blade.php", "{{ $subject }}\n"),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        for controller in ["app/AController.php", "app/BController.php"] {
            let uri = Url::from_file_path(root.join(controller)).unwrap();
            open_document(
                &backend,
                &uri,
                "php",
                &std::fs::read_to_string(root.join(controller)).unwrap(),
            )
            .await;
        }
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        // One call site passes an Item, the other the literal
        // 'fallback' string; the union carries both.
        let hover = hover_type(&backend, &blade_uri, 0, 4).await;
        assert!(
            hover.contains("Item") && (hover.contains("string") || hover.contains("'fallback'")),
            "hover should union both call sites' types, got: {}",
            hover
        );
    }

    /// A data argument that writes no names down still names its
    /// variables in its type, so the template is typed from the shape.
    #[tokio::test]
    async fn a_shaped_data_argument_types_template_variables() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    /** @param array{item: Item} $data */\n    public function show(array $data): mixed {\n        return view('shop', $data);\n    }\n}\n",
                ),
                ("resources/views/shop.blade.php", "{{ $item->name }}\n"),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        open_document(
            &backend,
            &controller_uri,
            "php",
            &std::fs::read_to_string(root.join("app/Controller.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &blade_uri, 0, 4).await;
        assert!(
            hover.contains("Item"),
            "the shape's entry should type the template variable, got: {}",
            hover
        );
    }

    /// Injected variables must not shift diagnostics: an undefined
    /// variable in the template is still reported on the right line,
    /// and injected variables produce no undefined-variable diagnostic.
    #[tokio::test]
    async fn injected_vars_do_not_break_diagnostics() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    public function show(): mixed {\n        return view('shop', ['item' => new Item()]);\n    }\n}\n",
                ),
                (
                    "resources/views/shop.blade.php",
                    "{{ $item->name }}\n{{ $undefined_thing }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        open_document(
            &backend,
            &controller_uri,
            "php",
            &std::fs::read_to_string(root.join("app/Controller.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        // Diagnose the virtual PHP the way the live pipeline does, with
        // ranges translated back to Blade coordinates.
        let virtual_php = backend
            .blade_virtual_php(blade_uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_undefined_variable_diagnostics(
            blade_uri.as_str(),
            &virtual_php,
            &mut diags,
        );
        let undefined: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("Undefined variable"))
            .collect();
        assert!(
            !undefined.iter().any(|d| d.message.contains("$item")),
            "injected $item must not be flagged undefined: {:?}",
            undefined
        );
        assert!(
            undefined
                .iter()
                .any(|d| d.message.contains("$undefined_thing") && d.range.start.line == 1),
            "$undefined_thing must be flagged on Blade line 1: {:?}",
            undefined
        );
    }

    /// A bound attribute on an `<x-…>` component tag types the variable
    /// inside the component's own template, even though the component
    /// declares no `@props` for it.
    #[tokio::test]
    async fn bound_component_attribute_types_the_component_variable() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "resources/views/page.blade.php",
                    "<x-brand.boxes :hairAnalysis=\"$model\" />\n",
                ),
                (
                    "resources/views/components/brand/boxes.blade.php",
                    "{{ $hairAnalysis->name }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let component_uri =
            Url::from_file_path(root.join("resources/views/components/brand/boxes.blade.php"))
                .unwrap();

        // The caller must declare `$model`'s type for inference to pick
        // it up; a `@php` block is the simplest way to do that in a plain
        // view.
        let page_source = "@php\n/** @var \\App\\Item $model */\n@endphp\n<x-brand.boxes :hairAnalysis=\"$model\" />\n";
        open_document(&backend, &page_uri, "blade", page_source).await;
        open_document(
            &backend,
            &component_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/components/brand/boxes.blade.php"))
                .unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &component_uri, 0, 4).await;
        assert!(
            hover.contains("Item"),
            "$hairAnalysis should be typed Item from the tag's bound attribute, got: {}",
            hover
        );
    }

    /// `@class(...)` compiles to the same generic marker call as a bound
    /// attribute that names no parameter, so one written before a
    /// component tag must not shift the index the tag's own bound
    /// attribute is correlated against.
    #[tokio::test]
    async fn a_directive_before_the_tag_does_not_shift_the_bound_attribute_index() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "resources/views/page.blade.php",
                    "<x-brand.boxes :hairAnalysis=\"$model\" />\n",
                ),
                (
                    "resources/views/components/brand/boxes.blade.php",
                    "{{ $hairAnalysis->name }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let component_uri =
            Url::from_file_path(root.join("resources/views/components/brand/boxes.blade.php"))
                .unwrap();

        let page_source = "@php\n/** @var \\App\\Item $model */\n@endphp\n@class(['featured' => true])\n<x-brand.boxes :hairAnalysis=\"$model\" />\n";
        open_document(&backend, &page_uri, "blade", page_source).await;
        open_document(
            &backend,
            &component_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/components/brand/boxes.blade.php"))
                .unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &component_uri, 0, 4).await;
        assert!(
            hover.contains("Item"),
            "@class(...) before the tag must not shift its bound attribute's index, got: {}",
            hover
        );
    }

    /// The inline `@php(…)` directive closes with its own parenthesis and
    /// never writes `@endphp`, so a tag written after it is still a call
    /// site of the component it names.
    #[tokio::test]
    async fn a_tag_after_an_inline_php_directive_is_still_a_call_site() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "resources/views/page.blade.php",
                    "<x-brand.boxes :hairAnalysis=\"$model\" />\n",
                ),
                (
                    "resources/views/components/brand/boxes.blade.php",
                    "{{ $hairAnalysis->name }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let component_uri =
            Url::from_file_path(root.join("resources/views/components/brand/boxes.blade.php"))
                .unwrap();

        // No `@endphp` follows, so mistaking the inline directive for a
        // block opener blanks the rest of the file, tag included.
        let page_source = "@php\n/** @var \\App\\Item $item */\n@endphp\n@php($model = $item)\n<x-brand.boxes :hairAnalysis=\"$model\" />\n";
        open_document(&backend, &page_uri, "blade", page_source).await;
        open_document(
            &backend,
            &component_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/components/brand/boxes.blade.php"))
                .unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &component_uri, 0, 4).await;
        assert!(
            hover.contains("Item"),
            "$hairAnalysis should be typed Item from the tag's bound attribute, got: {}",
            hover
        );
    }

    /// A plain string attribute on an `<x-…>` tag still declares the
    /// variable (as `string`), so the component body does not report it
    /// as undefined.
    #[tokio::test]
    async fn literal_component_attribute_is_not_reported_undefined() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                (
                    "resources/views/page.blade.php",
                    "<x-alert type=\"danger\" />\n",
                ),
                (
                    "resources/views/components/alert.blade.php",
                    "{{ $type }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let component_uri =
            Url::from_file_path(root.join("resources/views/components/alert.blade.php")).unwrap();

        open_document(
            &backend,
            &page_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/page.blade.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &component_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/components/alert.blade.php"))
                .unwrap(),
        )
        .await;

        let virtual_php = backend
            .blade_virtual_php(component_uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_undefined_variable_diagnostics(
            component_uri.as_str(),
            &virtual_php,
            &mut diags,
        );
        let undefined: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("Undefined variable"))
            .collect();
        assert!(
            undefined.is_empty(),
            "$type should be declared from the tag's literal attribute: {:?}",
            undefined
        );
    }

    /// `@props` wins over the call-site-inferred type for the same name,
    /// per the declaration priority chain.
    #[tokio::test]
    async fn props_default_wins_over_component_tag_inference() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                (
                    "resources/views/page.blade.php",
                    "<x-alert type=\"danger\" />\n",
                ),
                (
                    "resources/views/components/alert.blade.php",
                    "@props(['type' => 1])\n{{ $type }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let component_uri =
            Url::from_file_path(root.join("resources/views/components/alert.blade.php")).unwrap();

        open_document(
            &backend,
            &page_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/page.blade.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &component_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/components/alert.blade.php"))
                .unwrap(),
        )
        .await;

        // `@props` types `$type` from its default `1`; the page's literal
        // `"danger"` string attribute must not override it.
        let hover = hover_type(&backend, &component_uri, 1, 4).await;
        assert!(
            hover.contains('1') && !hover.contains("danger"),
            "@props default should win over the tag's literal attribute, got: {}",
            hover
        );
    }

    /// `Blade::anonymousComponentNamespace('components', 'webshop')` makes
    /// `<x-webshop::brand.boxes>` address the plain view
    /// `components.brand.boxes`, so its attributes have to reach that
    /// template even though nothing about the view name mentions the prefix.
    #[tokio::test]
    async fn a_registered_anonymous_prefix_types_the_components_variable() {
        let (backend, dir) = create_psr4_workspace(
            r#"{
                "require": { "laravel/framework": "^11.0" },
                "autoload": {"psr-4": {"App\\": "app/"}}
            }"#,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "bootstrap/providers.php",
                    "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n",
                ),
                (
                    "app/Providers/AppServiceProvider.php",
                    "<?php\nnamespace App\\Providers;\n\
                     use Illuminate\\Support\\Facades\\Blade;\n\
                     class AppServiceProvider {\n\
                         public function boot(): void {\n\
                             Blade::anonymousComponentNamespace('components', 'webshop');\n\
                         }\n\
                     }\n",
                ),
                (
                    "resources/views/page.blade.php",
                    "@php\n/** @var \\App\\Item $model */\n@endphp\n\
                     <x-webshop::brand.boxes :hairAnalysis=\"$model\" />\n",
                ),
                (
                    "resources/views/components/brand/boxes.blade.php",
                    "{{ $hairAnalysis->name }}\n",
                ),
            ],
        );

        // The provider scan is what makes the registration visible.
        backend.initialized(InitializedParams {}).await;

        let root = dir.path();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let component_uri =
            Url::from_file_path(root.join("resources/views/components/brand/boxes.blade.php"))
                .unwrap();
        for uri in [&page_uri, &component_uri] {
            let path = uri.to_file_path().unwrap();
            open_document(
                &backend,
                uri,
                "blade",
                &std::fs::read_to_string(path).unwrap(),
            )
            .await;
        }

        let hover = hover_type(&backend, &component_uri, 0, 4).await;
        assert!(
            hover.contains("Item"),
            "$hairAnalysis should be typed Item from the registered prefix's tag, got: {}",
            hover
        );
    }

    /// Open a workspace where `shop.blade.php` receives an `App\Item` from a
    /// `view()` call but never names the class itself, so `App\Item` appears
    /// in the template's virtual PHP only through the injected prologue.
    async fn workspace_naming_item_only_in_the_prologue()
    -> (phpantom_lsp::Backend, tempfile::TempDir, Url, Url) {
        let (backend, dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    public function show(): mixed {\n        $item = new Item();\n        return view('shop', ['item' => $item]);\n    }\n}\n",
                ),
                ("resources/views/shop.blade.php", "{{ $item->name }}\n"),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let item_uri = Url::from_file_path(root.join("app/Item.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        for rel in ["app/Item.php", "app/Controller.php"] {
            let uri = Url::from_file_path(root.join(rel)).unwrap();
            open_document(
                &backend,
                &uri,
                "php",
                &std::fs::read_to_string(root.join(rel)).unwrap(),
            )
            .await;
        }
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        (backend, dir, item_uri, blade_uri)
    }

    /// Every text edit a workspace edit applies to `uri`, from either the
    /// `changes` map or the `document_changes` operation list.
    fn edits_for(edit: &WorkspaceEdit, uri: &Url) -> Vec<Range> {
        let mut ranges = Vec::new();

        if let Some(changes) = &edit.changes
            && let Some(edits) = changes.get(uri)
        {
            ranges.extend(edits.iter().map(|e| e.range));
        }

        let doc_edits: Vec<&TextDocumentEdit> = match &edit.document_changes {
            Some(DocumentChanges::Edits(edits)) => edits.iter().collect(),
            Some(DocumentChanges::Operations(ops)) => ops
                .iter()
                .filter_map(|op| match op {
                    DocumentChangeOperation::Edit(e) => Some(e),
                    DocumentChangeOperation::Op(_) => None,
                })
                .collect(),
            None => Vec::new(),
        };
        for doc_edit in doc_edits {
            if doc_edit.text_document.uri != *uri {
                continue;
            }
            ranges.extend(doc_edit.edits.iter().map(|e| match e {
                OneOf::Left(e) => e.range,
                OneOf::Right(e) => e.text_edit.range,
            }));
        }

        ranges
    }

    /// A template that names a class only through its injected prologue is
    /// not a reference site: the prologue is not template text, so there is
    /// no Blade position to report.
    #[tokio::test]
    async fn references_skip_a_class_named_only_by_the_prologue() {
        let (backend, _dir, item_uri, blade_uri) =
            workspace_naming_item_only_in_the_prologue().await;

        let locations = backend
            .references(ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: item_uri.clone(),
                    },
                    position: Position {
                        line: 2,
                        character: 7,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
            .await
            .unwrap()
            .unwrap_or_default();

        let in_template: Vec<_> = locations.iter().filter(|l| l.uri == blade_uri).collect();
        assert!(
            in_template.is_empty(),
            "the template never names App\\Item; the prologue is not a reference site: {:?}",
            in_template
        );
    }

    /// Renaming a class a template only receives (never names) must leave the
    /// template alone.  Clamping the prologue match to `0:0` used to produce
    /// an empty-range edit that prepended the new name to the template.
    #[tokio::test]
    async fn rename_does_not_write_into_a_template_that_only_names_the_class_in_its_prologue() {
        let (backend, _dir, item_uri, blade_uri) =
            workspace_naming_item_only_in_the_prologue().await;

        let edit = backend
            .rename(RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: item_uri.clone(),
                    },
                    position: Position {
                        line: 2,
                        character: 7,
                    },
                },
                new_name: "Product".to_string(),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();

        let Some(edit) = edit else {
            return;
        };

        let template_edits = edits_for(&edit, &blade_uri);
        assert!(
            template_edits.is_empty(),
            "renaming App\\Item must not edit a template that only receives it: {:?}",
            template_edits
        );
    }

    /// A render site the receiver's *type* settles feeds the template it
    /// names, the same as a `view()` helper call does.
    #[tokio::test]
    async fn injected_factory_call_site_types_template_variable() {
        const FACTORY: &str = "<?php\nnamespace Illuminate\\Contracts\\View;\n\
            interface Factory { public function make($view, $data = [], $mergeData = []); }\n";
        let (backend, _dir) = create_psr4_workspace(
            r#"{"autoload": {"psr-4": {"App\\": "app/", "Illuminate\\": "stubs/Illuminate/"}}}"#,
            &[
                ("stubs/Illuminate/Contracts/View/Factory.php", FACTORY),
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nuse Illuminate\\Contracts\\View\\Factory;\nclass Controller {\n    public function __construct(private Factory $views) {}\n    public function show(): mixed {\n        $item = new Item();\n        return $this->views->make('shop', ['item' => $item]);\n    }\n}\n",
                ),
                ("resources/views/shop.blade.php", "{{ $item->name }}\n"),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        open_document(
            &backend,
            &controller_uri,
            "php",
            &std::fs::read_to_string(root.join("app/Controller.php")).unwrap(),
        )
        .await;
        open_document(
            &backend,
            &blade_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/shop.blade.php")).unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &blade_uri, 0, 5).await;
        assert!(
            hover.contains("Item"),
            "expected $item typed from the injected factory's call site, got {hover}"
        );
    }

    /// An `@include('partials.row', ['row' => $item])` types `$row` inside
    /// the partial, the same way a controller's `view()` call does.
    #[tokio::test]
    async fn include_directive_types_the_partials_variable() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                ("resources/views/page.blade.php", "\n"),
                (
                    "resources/views/partials/row.blade.php",
                    "{{ $row->name }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let partial_uri =
            Url::from_file_path(root.join("resources/views/partials/row.blade.php")).unwrap();

        let page_source = "@php\n/** @var \\App\\Item $item */\n@endphp\n@include('partials.row', ['row' => $item])\n";
        open_document(&backend, &page_uri, "blade", page_source).await;
        open_document(
            &backend,
            &partial_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/partials/row.blade.php")).unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &partial_uri, 0, 5).await;
        assert!(
            hover.contains("Item"),
            "$row should be typed Item from the @include that renders it, got: {}",
            hover
        );
    }

    /// A partial opened before the page that renders it picks the type up
    /// when the page is opened: the page was not parsed when the partial
    /// first looked for its call sites.
    #[tokio::test]
    async fn a_partial_opened_first_learns_from_the_page_opened_after_it() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                ("resources/views/page.blade.php", "\n"),
                (
                    "resources/views/partials/row.blade.php",
                    "{{ $row->name }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let partial_uri =
            Url::from_file_path(root.join("resources/views/partials/row.blade.php")).unwrap();

        open_document(
            &backend,
            &partial_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/partials/row.blade.php")).unwrap(),
        )
        .await;
        let page_source = "@php\n/** @var \\App\\Item $item */\n@endphp\n@include('partials.row', ['row' => $item])\n";
        open_document(&backend, &page_uri, "blade", page_source).await;

        let hover = hover_type(&backend, &partial_uri, 0, 5).await;
        assert!(
            hover.contains("Item"),
            "$row should be typed Item once the including page is parsed, got: {}",
            hover
        );
    }

    /// `@each` binds the item under the name its third argument spells and
    /// the key beside it, so both are typed from the collection the
    /// rendering template iterates.
    #[tokio::test]
    async fn each_directive_types_the_partials_item_and_key() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                ("resources/views/page.blade.php", "\n"),
                (
                    "resources/views/partials/line.blade.php",
                    "{{ $line->name }}\n{{ $key }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let partial_uri =
            Url::from_file_path(root.join("resources/views/partials/line.blade.php")).unwrap();

        let page_source = "@php\n/** @var array<int, \\App\\Item> $items */\n@endphp\n@each('partials.line', $items, 'line')\n";
        open_document(&backend, &page_uri, "blade", page_source).await;
        open_document(
            &backend,
            &partial_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/partials/line.blade.php")).unwrap(),
        )
        .await;

        let item_hover = hover_type(&backend, &partial_uri, 0, 5).await;
        assert!(
            item_hover.contains("Item"),
            "$line should be typed Item from the @each collection, got: {}",
            item_hover
        );
        let key_hover = hover_type(&backend, &partial_uri, 1, 5).await;
        assert!(
            key_hover.contains("int"),
            "$key should be typed int from the @each collection, got: {}",
            key_hover
        );
    }

    /// A template that renders itself feeds only what its own callers pass:
    /// its recursive `@include` names the very spans its scope would be read
    /// from, so it is skipped rather than read back into it.
    #[tokio::test]
    async fn a_recursive_include_does_not_feed_the_template_itself() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                ("resources/views/page.blade.php", "\n"),
                (
                    "resources/views/menu.blade.php",
                    "{{ $node->name }}\n@include('menu', ['node' => $node])\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let menu_uri = Url::from_file_path(root.join("resources/views/menu.blade.php")).unwrap();

        let page_source =
            "@php\n/** @var \\App\\Item $item */\n@endphp\n@include('menu', ['node' => $item])\n";
        open_document(&backend, &page_uri, "blade", page_source).await;
        open_document(
            &backend,
            &menu_uri,
            "blade",
            &std::fs::read_to_string(root.join("resources/views/menu.blade.php")).unwrap(),
        )
        .await;

        let hover = hover_type(&backend, &menu_uri, 0, 5).await;
        assert!(
            hover.contains("Item"),
            "$node should keep the type the page passes, got: {}",
            hover
        );
    }

    /// Two templates that render each other settle instead of handing the
    /// work back and forth: each is read against the other's scope as it
    /// stands, and the nesting one passes the other does not grow a level
    /// per pass.
    #[tokio::test]
    async fn templates_that_render_each_other_settle() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                (
                    "resources/views/left.blade.php",
                    "@include('right', ['x' => [$y]])\n{{ $y }}\n",
                ),
                (
                    "resources/views/right.blade.php",
                    "@include('left', ['y' => [$x]])\n{{ $x }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        for name in ["left", "right"] {
            let path = format!("resources/views/{name}.blade.php");
            let uri = Url::from_file_path(root.join(&path)).unwrap();
            open_document(
                &backend,
                &uri,
                "blade",
                &std::fs::read_to_string(root.join(&path)).unwrap(),
            )
            .await;
        }

        // Reaching this line at all is the point: an inference that fed
        // itself would never return.  The type is whatever the other
        // template held when it was last read, so only its shape is
        // asserted.
        let left_uri = Url::from_file_path(root.join("resources/views/left.blade.php")).unwrap();
        let hover = hover_type(&backend, &left_uri, 1, 4).await;
        assert!(
            hover.contains("$y"),
            "hover on $y should describe the variable, got: {}",
            hover
        );
    }

    /// A controller's data reaches the partial two renders down: the page it
    /// renders passes the item on with `@include`.
    #[tokio::test]
    async fn a_controllers_data_reaches_a_partial_through_the_page() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    public function show(): mixed {\n        return view('page', ['item' => new Item()]);\n    }\n}\n",
                ),
                (
                    "resources/views/page.blade.php",
                    "@include('partials.row', ['row' => $item])\n",
                ),
                (
                    "resources/views/partials/row.blade.php",
                    "{{ $row->name }}\n",
                ),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let page_uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        let partial_uri =
            Url::from_file_path(root.join("resources/views/partials/row.blade.php")).unwrap();

        for (uri, path, language) in [
            (&controller_uri, "app/Controller.php", "php"),
            (&page_uri, "resources/views/page.blade.php", "blade"),
            (
                &partial_uri,
                "resources/views/partials/row.blade.php",
                "blade",
            ),
        ] {
            open_document(
                &backend,
                uri,
                language,
                &std::fs::read_to_string(root.join(path)).unwrap(),
            )
            .await;
        }

        let hover = hover_type(&backend, &partial_uri, 0, 5).await;
        assert!(
            hover.contains("Item"),
            "$row should be typed Item through the page that includes the partial, got: {}",
            hover
        );
    }

    /// A controller that hands a view an element of a collection typed by
    /// a bare generic class name (`ItemCollection` with `@template TModel
    /// of Item`) must type the template's variable as the bound, not as
    /// the template parameter.  `TModel` names no class, so every member
    /// the template read off it was reported unverifiable.
    #[tokio::test]
    async fn bare_generic_collection_element_types_template_variable() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Item.php", ITEM_CLASS),
                (
                    "app/ItemCollection.php",
                    "<?php\nnamespace App;\n/**\n * @template TModel of Item\n */\nclass ItemCollection {\n    /** @return TModel|null */\n    public function first() { return null; }\n}\n",
                ),
                (
                    "app/Controller.php",
                    "<?php\nnamespace App;\nclass Controller {\n    public function show(ItemCollection $items): mixed {\n        return view('shop', ['item' => $items->first()]);\n    }\n}\n",
                ),
                ("resources/views/shop.blade.php", "{{ $item->name }}\n"),
            ],
        );

        let root = backend.workspace_root().read().clone().unwrap();
        let controller_uri = Url::from_file_path(root.join("app/Controller.php")).unwrap();
        let blade_uri = Url::from_file_path(root.join("resources/views/shop.blade.php")).unwrap();

        for (uri, path, language) in [
            (&controller_uri, "app/Controller.php", "php"),
            (&blade_uri, "resources/views/shop.blade.php", "blade"),
        ] {
            open_document(
                &backend,
                uri,
                language,
                &std::fs::read_to_string(root.join(path)).unwrap(),
            )
            .await;
        }

        let hover = hover_type(&backend, &blade_uri, 0, 5).await;
        assert!(
            hover.contains("Item") && !hover.contains("TModel"),
            "$item should be typed by the TModel bound Item, got: {}",
            hover
        );
    }
}
