//! The outline of a Blade template: the sections and stacks it writes and
//! the components it renders, in the template's own coordinates.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER: &str = r#"{"autoload": {"psr-4": {
        "App\\": "app/",
        "Illuminate\\": "stubs/Illuminate/",
        "Livewire\\": "stubs/Livewire/"
    }}}"#;

    const COMPONENT_STUB: &str = "<?php\nnamespace Illuminate\\View;\n\
        abstract class Component {\n\
            public function render() {}\n\
        }\n";

    const LIVEWIRE_STUB: &str = "<?php\nnamespace Livewire;\n\
        abstract class Component {\n\
            public function render() {}\n\
        }\n";

    /// The template a class-based component renders, which is also a
    /// Blade file with an outline of its own.
    const ALERT_TEMPLATE: &str = "@props(['type' => 'info'])\n<div>{{ $slot }}</div>\n";

    /// The outline of `resources/views/page.blade.php`, rendering
    /// `template`.
    async fn outline(template: &str) -> Vec<DocumentSymbol> {
        outline_of("resources/views/page.blade.php", template).await
    }

    /// A workspace with one class-based component, one anonymous
    /// component, and one Livewire component, rendering `template` as
    /// `resources/views/page.blade.php`; the outline of the file at
    /// `relative`.
    async fn outline_of(relative: &str, template: &str) -> Vec<DocumentSymbol> {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Illuminate/View/Component.php", COMPONENT_STUB),
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                (
                    "app/View/Components/Alert.php",
                    "<?php\nnamespace App\\View\\Components;\n\
                     use Illuminate\\View\\Component;\n\
                     class Alert extends Component {\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "app/Livewire/Counter.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use Livewire\\Component;\n\
                     class Counter extends Component {\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/components/banner.blade.php",
                    "<div>{{ $headline }}</div>\n",
                ),
                ("resources/views/components/alert.blade.php", ALERT_TEMPLATE),
                ("resources/views/page.blade.php", template),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = Url::from_file_path(root.join(relative)).unwrap();
        let text = std::fs::read_to_string(root.join(relative)).unwrap();
        open_document(&backend, &uri, "blade", &text).await;

        match backend
            .document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap()
        {
            Some(DocumentSymbolResponse::Nested(symbols)) => symbols,
            None => Vec::new(),
            other => panic!("expected a nested outline, got {other:?}"),
        }
    }

    /// The outline as `(name, detail)` pairs, for readable assertions.
    fn labels(symbols: &[DocumentSymbol]) -> Vec<(String, Option<String>)> {
        symbols
            .iter()
            .map(|symbol| (symbol.name.clone(), symbol.detail.clone()))
            .collect()
    }

    #[tokio::test]
    async fn a_section_is_listed_by_its_name() {
        let symbols = outline("@section('content')\n<p>hi</p>\n@endsection\n").await;
        assert_eq!(
            labels(&symbols),
            [("content".to_string(), Some("@section".to_string()))]
        );
        // It spans the whole block, and selecting it lands on the name.
        assert_eq!(symbols[0].range.start.line, 0);
        assert_eq!(symbols[0].range.end.line, 2);
        assert_eq!(symbols[0].selection_range.start, Position::new(0, 10));
    }

    #[tokio::test]
    async fn a_yield_and_a_stack_are_listed_where_they_stand() {
        let symbols = outline("@yield('content')\n@stack('scripts')\n").await;
        assert_eq!(
            labels(&symbols),
            [
                ("content".to_string(), Some("@yield".to_string())),
                ("scripts".to_string(), Some("@stack".to_string())),
            ]
        );
    }

    #[tokio::test]
    async fn a_push_block_is_listed() {
        let symbols =
            outline("@push('scripts')\n<script src=\"app.js\"></script>\n@endpush\n").await;
        assert_eq!(
            labels(&symbols),
            [("scripts".to_string(), Some("@push".to_string()))]
        );
    }

    #[tokio::test]
    async fn a_component_tag_is_listed_with_the_class_it_resolves_to() {
        let symbols = outline("<x-alert>\n<p>hi</p>\n</x-alert>\n").await;
        assert_eq!(
            labels(&symbols),
            [(
                "x-alert".to_string(),
                Some("App\\View\\Components\\Alert".to_string())
            )]
        );
        assert_eq!(symbols[0].kind, SymbolKind::CLASS);
        assert_eq!(symbols[0].range.end.line, 2);
    }

    /// An anonymous component has no class, so the template Laravel
    /// renders in its place is what the tag resolves to.
    #[tokio::test]
    async fn an_anonymous_component_is_listed_with_its_template() {
        let symbols = outline("<x-banner headline=\"Hi\" />\n").await;
        assert_eq!(
            labels(&symbols),
            [(
                "x-banner".to_string(),
                Some("components.banner".to_string())
            )]
        );
    }

    #[tokio::test]
    async fn a_livewire_tag_is_listed_with_its_class() {
        let symbols = outline("<livewire:counter />\n").await;
        assert_eq!(
            labels(&symbols),
            [(
                "livewire:counter".to_string(),
                Some("App\\Livewire\\Counter".to_string())
            )]
        );
    }

    /// Nothing answers for the tag, so the outline lists it as written
    /// rather than dropping it.
    #[tokio::test]
    async fn an_unresolved_tag_keeps_its_bare_name() {
        let symbols = outline("<x-missing />\n").await;
        assert_eq!(labels(&symbols), [("x-missing".to_string(), None)]);
    }

    #[tokio::test]
    async fn a_tag_written_inside_a_section_is_nested_under_it() {
        let symbols =
            outline("@section('content')\n<x-alert />\n@endsection\n<x-banner />\n").await;
        assert_eq!(
            labels(&symbols),
            [
                ("content".to_string(), Some("@section".to_string())),
                (
                    "x-banner".to_string(),
                    Some("components.banner".to_string())
                ),
            ]
        );
        let children = symbols[0].children.as_ref().expect("the section has a tag");
        assert_eq!(
            labels(children),
            [(
                "x-alert".to_string(),
                Some("App\\View\\Components\\Alert".to_string())
            )]
        );
    }

    /// The nine marker functions the preprocessor declares, and the
    /// function it wraps every template body in, stand behind no template
    /// text at all, so none of them is listed.
    #[tokio::test]
    async fn plain_markup_has_no_outline() {
        assert!(labels(&outline("<h1>Hello</h1>\n<p>{{ $name }}</p>\n").await).is_empty());
    }

    /// A component's own template is preprocessed with a synthesized
    /// subclass of the component around its body, so that `$this` carries
    /// the component's members. That class is prologue too.
    #[tokio::test]
    async fn a_components_own_template_lists_only_what_it_writes() {
        let symbols = outline_of("resources/views/components/alert.blade.php", "").await;
        assert!(labels(&symbols).is_empty());
    }
}
