//! A section is filled in one template and rendered in another, and the
//! only thing linking the two halves is a string. These tests cover
//! following that string across files, completing it from the other half,
//! and reporting a half that pairs with nothing.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER: &str = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": { "psr-4": { "App\\": "app/" } }
    }"#;

    const LAYOUT: &str = "<html>\n\
        <head>@stack('styles')</head>\n\
        <body>\n\
        @yield('content')\n\
        @stack('scripts')\n\
        </body>\n\
        </html>\n";

    const PAGE: &str = "@extends('layouts.app')\n\
        @section('content')\n\
        <p>hi</p>\n\
        @endsection\n\
        @push('scripts')\n\
        <script></script>\n\
        @endpush\n";

    /// A workspace with `layouts.app` and the templates given, and the
    /// backend initialised over it.
    async fn workspace(templates: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
        let mut files = vec![("resources/views/layouts/app.blade.php", LAYOUT)];
        files.extend_from_slice(templates);
        let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
        backend.initialized(InitializedParams {}).await;
        (backend, dir)
    }

    fn uri_of(dir: &tempfile::TempDir, relative: &str) -> Url {
        Url::from_file_path(dir.path().join(relative)).unwrap()
    }

    async fn open(backend: &phpantom_lsp::Backend, dir: &tempfile::TempDir, relative: &str) -> Url {
        let uri = uri_of(dir, relative);
        let text = std::fs::read_to_string(dir.path().join(relative)).unwrap();
        open_document(backend, &uri, "blade", &text).await;
        uri
    }

    /// Every location go-to-definition answers with, as
    /// `(relative path, line, character)`.
    async fn definitions(
        backend: &phpantom_lsp::Backend,
        dir: &tempfile::TempDir,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position { line, character },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        let locations = match response {
            Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
            Some(GotoDefinitionResponse::Array(locations)) => locations,
            Some(GotoDefinitionResponse::Link(links)) => links
                .into_iter()
                .map(|link| Location {
                    uri: link.target_uri,
                    range: link.target_range,
                })
                .collect(),
            None => Vec::new(),
        };
        let root = dir.path().to_string_lossy().to_string();
        locations
            .into_iter()
            .map(|location| {
                let path = location.uri.to_file_path().unwrap();
                let relative = path
                    .to_string_lossy()
                    .trim_start_matches(&root)
                    .trim_start_matches('/')
                    .to_string();
                (
                    relative,
                    location.range.start.line,
                    location.range.start.character,
                )
            })
            .collect()
    }

    async fn completions(
        backend: &phpantom_lsp::Backend,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Vec<CompletionItem> {
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position { line, character },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap();
        match response {
            Some(CompletionResponse::Array(items)) => items,
            Some(CompletionResponse::List(list)) => list.items,
            None => Vec::new(),
        }
    }

    /// The unrendered-section reports for `uri`, as `(code, message)`.
    fn section_diagnostics(backend: &phpantom_lsp::Backend, uri: &Url) -> Vec<(String, String)> {
        let effective = backend.blade_virtual_php(uri.as_str()).unwrap();
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &effective, &mut diags);
        diags
            .into_iter()
            .filter_map(|diagnostic| match &diagnostic.code {
                Some(NumberOrString::String(code)) if code.starts_with("unrendered_blade") => {
                    Some((code.clone(), diagnostic.message))
                }
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_section_leads_to_the_yield_that_renders_it() {
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", PAGE)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        // `@section('content')` on line 1, cursor inside the name.
        let found = definitions(&backend, &dir, &uri, 1, 12).await;
        assert_eq!(
            found,
            [("resources/views/layouts/app.blade.php".to_string(), 3, 8)],
            "the section leads to its @yield"
        );
    }

    #[tokio::test]
    async fn a_push_leads_to_the_stack_that_renders_it() {
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", PAGE)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        // `@push('scripts')` on line 4.
        let found = definitions(&backend, &dir, &uri, 4, 9).await;
        assert_eq!(
            found,
            [("resources/views/layouts/app.blade.php".to_string(), 4, 8)],
            "the push leads to its @stack, not to the @yield above it"
        );
    }

    /// Both halves are Blade files, so the layout's location has to survive
    /// the virtual-PHP translation every location in a template goes
    /// through — including when the layout is open and therefore has a
    /// source map of its own.
    #[tokio::test]
    async fn the_target_lands_on_the_name_with_the_layout_open_too() {
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", PAGE)]).await;
        open(&backend, &dir, "resources/views/layouts/app.blade.php").await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        let found = definitions(&backend, &dir, &uri, 1, 12).await;
        assert_eq!(
            found,
            [("resources/views/layouts/app.blade.php".to_string(), 3, 8)]
        );
    }

    #[tokio::test]
    async fn a_yield_leads_to_the_sections_that_fill_it() {
        let (backend, dir) = workspace(&[
            ("resources/views/page.blade.php", PAGE),
            (
                "resources/views/other.blade.php",
                "@extends('layouts.app')\n@section('content')\nx\n@endsection\n",
            ),
        ])
        .await;
        let uri = open(&backend, &dir, "resources/views/layouts/app.blade.php").await;

        let found = definitions(&backend, &dir, &uri, 3, 8).await;
        assert_eq!(
            found,
            [
                ("resources/views/other.blade.php".to_string(), 1, 10),
                ("resources/views/page.blade.php".to_string(), 1, 10),
            ],
            "every page that fills the section, in a stable order"
        );
    }

    /// A layout that pulls its `<head>` out into a partial still renders
    /// the stacks that partial declares.
    #[tokio::test]
    async fn a_stack_declared_in_an_included_partial_is_found() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/layouts/split.blade.php",
                "@include('partials.head')\n@yield('content')\n",
            ),
            (
                "resources/views/partials/head.blade.php",
                "<head>@stack('styles')</head>\n",
            ),
            (
                "resources/views/split-page.blade.php",
                "@extends('layouts.split')\n@push('styles')\nx\n@endpush\n",
            ),
        ])
        .await;
        let uri = open(&backend, &dir, "resources/views/split-page.blade.php").await;

        let found = definitions(&backend, &dir, &uri, 1, 8).await;
        assert_eq!(
            found,
            [("resources/views/partials/head.blade.php".to_string(), 0, 14)]
        );
        assert!(
            section_diagnostics(&backend, &uri).is_empty(),
            "a stack the layout's partial declares is rendered"
        );
    }

    #[tokio::test]
    async fn a_section_name_completes_from_the_layout() {
        let page = "@extends('layouts.app')\n@section('')\n";
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", page)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        let items = completions(&backend, &uri, 1, 10).await;
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            ["content"],
            "only the names the layout yields, not the stacks it renders"
        );

        let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
            panic!("expected a text edit, got {:?}", items[0].text_edit);
        };
        assert_eq!(
            edit.range,
            Range {
                start: Position::new(1, 10),
                end: Position::new(1, 10),
            },
            "the edit replaces the typed name in the template's own coordinates"
        );
    }

    #[tokio::test]
    async fn a_stack_name_completes_from_the_layout() {
        let page = "@extends('layouts.app')\n@push('')\n";
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", page)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        let items = completions(&backend, &uri, 1, 7).await;
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["scripts", "styles"]);
    }

    /// The other direction: a layout offers the names its own pages fill,
    /// so a second `@yield` is spelled like the `@section` waiting for it.
    #[tokio::test]
    async fn a_yield_name_completes_from_the_pages_below_it() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/page.blade.php",
                "@extends('layouts.app')\n@section('sidebar')\nx\n@endsection\n",
            ),
            ("resources/views/layouts/partial.blade.php", "@yield('')\n"),
        ])
        .await;
        // The page extends `layouts.app`, so the names it fills are offered
        // in `layouts.app` rather than in an unrelated layout.
        let uri = open(&backend, &dir, "resources/views/layouts/app.blade.php").await;
        let items = completions(&backend, &uri, 3, 8).await;
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["sidebar"]);

        let unrelated = open(&backend, &dir, "resources/views/layouts/partial.blade.php").await;
        let items = completions(&backend, &unrelated, 0, 8).await;
        assert!(
            items.is_empty(),
            "a layout nothing extends has no names to offer: {items:?}"
        );
    }

    #[tokio::test]
    async fn a_section_no_layout_yields_is_reported() {
        let page = "@extends('layouts.app')\n@section('sidebar')\nx\n@endsection\n";
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", page)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        let reported = section_diagnostics(&backend, &uri);
        assert_eq!(reported.len(), 1, "one unrendered section: {reported:?}");
        assert_eq!(reported[0].0, "unrendered_blade_section");
        assert!(
            reported[0].1.contains("'sidebar'") && reported[0].1.contains("@yield"),
            "the report names the section and what would render it: {}",
            reported[0].1
        );
    }

    #[tokio::test]
    async fn a_push_to_a_stack_nothing_renders_is_reported() {
        let page = "@extends('layouts.app')\n@push('footer')\nx\n@endpush\n";
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", page)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        let reported = section_diagnostics(&backend, &uri);
        assert_eq!(reported.len(), 1, "one unrendered stack: {reported:?}");
        assert_eq!(reported[0].0, "unrendered_blade_stack");
    }

    #[tokio::test]
    async fn a_section_the_layout_yields_is_not_reported() {
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", PAGE)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;
        assert!(section_diagnostics(&backend, &uri).is_empty());
    }

    /// A layout that asks `@hasSection('sidebar')` consumes the section as
    /// surely as one that yields it.
    #[tokio::test]
    async fn a_section_the_layout_only_asks_about_is_not_reported() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/layouts/asks.blade.php",
                "@hasSection('sidebar')\n<aside>@yield('sidebar')</aside>\n@endif\n",
            ),
            (
                "resources/views/asks-page.blade.php",
                "@extends('layouts.asks')\n@section('sidebar')\nx\n@endsection\n",
            ),
        ])
        .await;
        let uri = open(&backend, &dir, "resources/views/asks-page.blade.php").await;
        assert!(section_diagnostics(&backend, &uri).is_empty());
    }

    /// A template that names no layout is rendered by something the check
    /// cannot see, so what renders its sections is not knowable here.
    #[tokio::test]
    async fn a_template_that_extends_nothing_is_left_alone() {
        let (backend, dir) = workspace(&[(
            "resources/views/partial.blade.php",
            "@section('sidebar')\nx\n@endsection\n",
        )])
        .await;
        let uri = open(&backend, &dir, "resources/views/partial.blade.php").await;
        assert!(section_diagnostics(&backend, &uri).is_empty());
    }

    /// The missing `@yield` could be in the partial the layout includes
    /// under a name only known at runtime.
    #[tokio::test]
    async fn a_layout_that_includes_a_dynamic_partial_is_left_alone() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/layouts/dynamic.blade.php",
                "@include($partial)\n@yield('content')\n",
            ),
            (
                "resources/views/dynamic-page.blade.php",
                "@extends('layouts.dynamic')\n@section('sidebar')\nx\n@endsection\n",
            ),
        ])
        .await;
        let uri = open(&backend, &dir, "resources/views/dynamic-page.blade.php").await;
        assert!(
            section_diagnostics(&backend, &uri).is_empty(),
            "nothing is knowable about a render tree with a hole in it"
        );
    }

    /// A component is rendered by a tag in another template, so its
    /// `@extends` chain is only part of the tree it renders in: the stack
    /// it pushes to is declared by whatever renders the component.
    #[tokio::test]
    async fn a_component_that_extends_a_component_is_left_alone() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/components/options/base.blade.php",
                "@php\n/** @var string $label */\n@endphp\n<div>{{ $label }}</div>\n",
            ),
            (
                "resources/views/components/options/vipps.blade.php",
                "@extends('components.options.base')\n@push('styles')\nx\n@endpush\n",
            ),
        ])
        .await;
        let uri = open(
            &backend,
            &dir,
            "resources/views/components/options/vipps.blade.php",
        )
        .await;
        assert!(
            section_diagnostics(&backend, &uri).is_empty(),
            "the stack is declared by whatever renders the component"
        );
    }

    /// A chain that ends at a partial another template includes ends in the
    /// middle of a render tree, not at the top of one.
    #[tokio::test]
    async fn a_chain_that_ends_in_an_included_partial_is_left_alone() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/inner.blade.php",
                "<div>@yield('content')</div>\n",
            ),
            (
                "resources/views/shell.blade.php",
                "@stack('scripts')\n@include('partials.inner')\n",
            ),
            (
                "resources/views/inner-page.blade.php",
                "@extends('partials.inner')\n@push('scripts')\nx\n@endpush\n",
            ),
        ])
        .await;
        let uri = open(&backend, &dir, "resources/views/inner-page.blade.php").await;
        assert!(
            section_diagnostics(&backend, &uri).is_empty(),
            "the stack is declared above the partial the chain ends at"
        );
    }

    /// A layout that renders a component tag is a hole too: the tag's own
    /// template is not something this walk follows.
    #[tokio::test]
    async fn a_layout_with_a_component_tag_is_left_alone() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/layouts/tagged.blade.php",
                "<x-shell>@yield('content')</x-shell>\n",
            ),
            (
                "resources/views/tagged-page.blade.php",
                "@extends('layouts.tagged')\n@section('sidebar')\nx\n@endsection\n",
            ),
        ])
        .await;
        let uri = open(&backend, &dir, "resources/views/tagged-page.blade.php").await;
        assert!(section_diagnostics(&backend, &uri).is_empty());
    }

    #[tokio::test]
    async fn hover_on_a_section_names_the_template_that_renders_it() {
        let (backend, dir) = workspace(&[("resources/views/page.blade.php", PAGE)]).await;
        let uri = open(&backend, &dir, "resources/views/page.blade.php").await;

        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 12),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();
        let text = match hover {
            Some(Hover {
                contents: HoverContents::Markup(markup),
                ..
            }) => markup.value,
            other => panic!("expected markup hover, got {other:?}"),
        };
        assert!(
            text.contains("Section") && text.contains("layouts/app.blade.php"),
            "unexpected hover: {text}"
        );
    }
}
