//! Component tags (`<x-…>`, `<livewire:…>`) resolve to the classes behind
//! them, so `$component` inside a tag body carries that class's members.

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
            public $attributes;\n\
            public function render() {}\n\
        }\n";

    const ANONYMOUS_STUB: &str = "<?php\nnamespace Illuminate\\View;\n\
        class AnonymousComponent extends Component {\n\
            public function anonymousMarker(): string { return ''; }\n\
        }\n";

    const LIVEWIRE_STUB: &str = "<?php\nnamespace Livewire;\n\
        abstract class Component {\n\
            public function render() {}\n\
        }\n";

    /// One component class per naming shape the index has to cover: a
    /// plain one, one in a sub-directory, and an index component
    /// (`Card\Card`, which `<x-card>` reaches).
    fn workspace(template: &str) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
        let (backend, dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Illuminate/View/Component.php", COMPONENT_STUB),
                (
                    "stubs/Illuminate/View/AnonymousComponent.php",
                    ANONYMOUS_STUB,
                ),
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                // Laravel's own container helper, which is how the virtual
                // PHP says "the container builds this parameter".
                (
                    "stubs/helpers.php",
                    "<?php\nfunction resolve(string $abstract, array $parameters = []) {}\n",
                ),
                (
                    "app/View/Components/Alert.php",
                    "<?php\nnamespace App\\View\\Components;\n\
                     use Illuminate\\View\\Component;\n\
                     class Alert extends Component {\n\
                         public function __construct(public string $type) {}\n\
                         public function severity(): string { return ''; }\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "app/Services/FieldService.php",
                    "<?php\nnamespace App\\Services;\nclass FieldService {}\n",
                ),
                // Required parameters the container can build on its own:
                // Laravel resolves the class-typed one and passes `null`
                // for the nullable one when the tag leaves them out.
                (
                    "app/View/Components/Forms/Input.php",
                    "<?php\nnamespace App\\View\\Components\\Forms;\n\
                     use App\\Services\\FieldService;\n\
                     use Illuminate\\View\\Component;\n\
                     class Input extends Component {\n\
                         public function __construct(\n\
                             public FieldService $service,\n\
                             public ?string $label,\n\
                             public string $hint = '',\n\
                         ) {}\n\
                         public function placeholder(): string { return ''; }\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "app/View/Components/Card/Card.php",
                    "<?php\nnamespace App\\View\\Components\\Card;\n\
                     use Illuminate\\View\\Component;\n\
                     class Card extends Component {\n\
                         public function heading(): string { return ''; }\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "app/Livewire/Counter.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use App\\Services\\FieldService;\n\
                     use Livewire\\Component;\n\
                     class Counter extends Component {\n\
                         public int $count = 0;\n\
                         public function mount(int $start, FieldService $service): void {}\n\
                         public function increment(): void {}\n\
                         public function render() {}\n\
                     }\n",
                ),
                // An anonymous component: a template with no class, so
                // what it reads comes from the attributes its tags pass.
                (
                    "resources/views/components/banner.blade.php",
                    "<div>{{ $headline }}</div>\n",
                ),
                ("resources/views/page.blade.php", template),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        // Parse the helper up front: a function is only findable once the
        // file declaring it has been indexed, and these workspaces are not
        // walked.
        let helpers = Url::from_file_path(root.join("stubs/helpers.php")).unwrap();
        backend.update_ast(
            helpers.as_str(),
            &std::fs::read_to_string(root.join("stubs/helpers.php")).unwrap(),
        );
        let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        (backend, dir, uri)
    }

    /// Open a second template from the same workspace, for the cases that
    /// need a caller and a callee.
    async fn open_view(
        backend: &phpantom_lsp::Backend,
        root: &std::path::Path,
        relative: &str,
    ) -> Url {
        let uri = Url::from_file_path(root.join(relative)).unwrap();
        let text = std::fs::read_to_string(root.join(relative)).unwrap();
        open_document(backend, &uri, "blade", &text).await;
        uri
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
        match result {
            Some(Hover {
                contents: HoverContents::Markup(m),
                ..
            }) => m.value,
            other => panic!("expected markup hover, got {other:?}"),
        }
    }

    async fn completion_labels(
        backend: &phpantom_lsp::Backend,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Vec<String> {
        let items = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position { line, character },
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
        match items {
            Some(CompletionResponse::Array(items)) => items,
            Some(CompletionResponse::List(list)) => list.items,
            None => Vec::new(),
        }
        .into_iter()
        .map(|item| item.label)
        .collect()
    }

    /// Method labels carry a trailing `()`; property labels do not.
    fn has_member(labels: &[String], name: &str) -> bool {
        labels
            .iter()
            .any(|label| label.trim_end_matches("()") == name)
    }

    /// A bound attribute's expression is an argument of the tag's call,
    /// but it is still emitted where the template wrote it, so hovering
    /// and completing inside `:attr="…"` land on the template's own text
    /// rather than on the call built from it.
    #[tokio::test]
    async fn a_bound_attribute_expression_stays_where_it_is_written() {
        let template = "@php $kind = 'danger'; @endphp\n<x-alert :type=\"$kind\" />\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        // Column 18 is inside the `$kind` in the attribute value.
        let hover = hover_text(&backend, &uri, 1, 18).await;
        assert!(
            hover.contains("$kind"),
            "hovering the bound expression should describe it: {hover}"
        );
    }

    /// The deliverable: `$component->` after a component tag completes
    /// from the class the tag names.
    #[tokio::test]
    async fn a_component_tag_puts_its_class_behind_component() {
        let template = "<x-alert type=\"danger\">\n{{ $component-> }}\n</x-alert>\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let labels = completion_labels(&backend, &uri, 1, 15).await;
        assert!(
            has_member(&labels, "severity"),
            "expected the Alert members, got: {labels:?}"
        );
    }

    /// Every naming shape the component index covers reaches the class it
    /// records: a sub-directory (`<x-forms.input>`) and an index
    /// component (`<x-card>` → `Card\Card`).
    #[tokio::test]
    async fn nested_and_index_components_resolve() {
        for (tag, member) in [("forms.input", "placeholder"), ("card", "heading")] {
            let template = format!("<x-{tag}>\n{{{{ $component-> }}}}\n</x-{tag}>\n");
            let (backend, _dir, uri) = workspace(&template);
            open_document(&backend, &uri, "blade", &template).await;

            let labels = completion_labels(&backend, &uri, 1, 15).await;
            assert!(
                has_member(&labels, member),
                "<x-{tag}> should resolve to the class declaring {member}, got: {labels:?}"
            );
        }
    }

    /// A `<livewire:…>` tag resolves through the Livewire index.
    #[tokio::test]
    async fn a_livewire_tag_puts_its_class_behind_component() {
        let template = "<livewire:counter :count=\"$n\" />\n{{ $component-> }}\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let labels = completion_labels(&backend, &uri, 1, 15).await;
        assert!(
            has_member(&labels, "increment") && has_member(&labels, "count"),
            "expected the Counter members, got: {labels:?}"
        );
    }

    /// A tag that names a template with no class of its own is what
    /// Laravel renders through `AnonymousComponent`.
    #[tokio::test]
    async fn an_anonymous_component_resolves_to_the_framework_class() {
        let template = "<x-banner>\n{{ $component-> }}\n</x-banner>\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let labels = completion_labels(&backend, &uri, 1, 15).await;
        assert!(
            has_member(&labels, "anonymousMarker"),
            "an anonymous component is an AnonymousComponent, got: {labels:?}"
        );
    }

    /// `<x-dynamic-component>` names its target through an attribute, so
    /// there is nothing to resolve — but the attribute expressions are
    /// still parsed, and nothing about the template breaks.
    #[tokio::test]
    async fn a_dynamic_component_resolves_nothing_and_keeps_working() {
        let template = "@php $name = 'alert'; $kind = 'danger'; @endphp\n\
             <x-dynamic-component :component=\"$name\" :type=\"$kind\" />\n\
             <p>{{ $kind }}</p>\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        assert!(
            virtual_php.contains("blade_bound_attr_directive($name);"),
            "the target expression must still be parsed: {virtual_php}"
        );
        assert!(
            !virtual_php.contains("$component = null;"),
            "no component may be resolved for a dynamic tag: {virtual_php}"
        );

        let hover = hover_text(&backend, &uri, 2, 8).await;
        assert!(
            hover.contains("$kind"),
            "the rest of the template must still resolve, got: {hover}"
        );
    }

    /// A tag no index knows degrades to a comment: the template still
    /// preprocesses to valid PHP and reports nothing extra.
    #[tokio::test]
    async fn an_unknown_component_reports_nothing() {
        let template = "<x-not-a-component foo=\"bar\">\n<p>hi</p>\n</x-not-a-component>\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_syntax_error_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        assert!(
            diags.is_empty(),
            "an unknown component must not break the template: {diags:?}"
        );
    }

    /// `$component` is the preprocessor's own, so a template that never
    /// reads it is not holding an unused variable.
    #[tokio::test]
    async fn the_component_variable_is_never_reported_unused() {
        let template = "<x-alert type=\"danger\">hi</x-alert>\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_unused_variable_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        assert!(
            !diags.iter().any(|d| d.message.contains("$component")),
            "$component is synthesized, not authored: {diags:?}"
        );
    }

    /// Every diagnostic the whole pipeline reports on a template, so a
    /// component tag can be held to the same standard as the rest of the
    /// file: nothing extra, and nothing missing.
    fn diagnostics(backend: &phpantom_lsp::Backend, uri: &Url) -> Vec<String> {
        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_syntax_error_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        backend.collect_argument_count_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        diags.into_iter().map(|d| d.message).collect()
    }

    /// Laravel partitions a tag's attributes by the constructor it is
    /// about to call: the ones naming a parameter are its arguments and
    /// the rest go to the component's attribute bag. An attribute meant
    /// for the bag is therefore never a bad argument, however it is
    /// spelled.
    #[tokio::test]
    async fn attributes_the_bag_takes_are_not_arguments() {
        let template =
            "<x-alert type=\"danger\" class=\"m-2\" wire:model=\"x\" data-id=\"3\" disabled />\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        assert!(
            diagnostics(&backend, &uri).is_empty(),
            "only `type` is an argument: {:?}",
            diagnostics(&backend, &uri)
        );
    }

    /// The attributes that *are* arguments are checked as arguments: a
    /// bound one whose expression is the wrong type is reported, the way
    /// it would be at any other call site.
    #[tokio::test]
    async fn a_bound_attribute_is_checked_against_the_parameter() {
        let template = "@php $name = 'oops'; @endphp\n<x-forms.input :service=\"$name\" />\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let diags = diagnostics(&backend, &uri);
        assert!(
            diags
                .iter()
                .any(|d| d.contains("service") && d.contains("FieldService")),
            "a string passed where the constructor wants a service: {diags:?}"
        );
    }

    /// A required parameter no attribute fills is reported, since that is
    /// what Laravel itself fails on when the container cannot build it.
    #[tokio::test]
    async fn a_required_attribute_that_is_missing_is_reported() {
        let template = "<x-alert class=\"m-2\" />\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let diags = diagnostics(&backend, &uri);
        assert!(
            diags.iter().any(|d| d.contains("argument")),
            "a component missing its required attribute: {diags:?}"
        );
    }

    /// Livewire hands a tag's attributes to `mount()`, so they are that
    /// method's arguments and are checked as such.
    #[tokio::test]
    async fn a_livewire_attribute_is_checked_against_mount() {
        let template = "@php $label = 'x'; @endphp\n<livewire:counter :start=\"$label\" />\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let diags = diagnostics(&backend, &uri);
        assert!(
            diags
                .iter()
                .any(|d| d.contains("start") && d.contains("int")),
            "a string passed where mount() wants an int: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.contains("service")),
            "the container fills mount()'s service: {diags:?}"
        );
    }

    /// An anonymous component's variables come from the attributes its
    /// tags pass, correlated against the expressions the caller's virtual
    /// PHP carries. A class-backed tag earlier in the same file turns
    /// *its* bound attributes into arguments rather than into that
    /// sequence, so both sides have to agree on which attributes those
    /// were, or the anonymous component reads the wrong expression.
    #[tokio::test]
    async fn an_argument_attribute_does_not_shift_a_later_tags_inference() {
        let template = "@php $kind = 'danger'; $head = 42; @endphp\n\
             <x-alert :type=\"$kind\" />\n\
             <x-banner :headline=\"$head\" />\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        let root = backend.workspace_root().read().clone().unwrap();
        let banner = open_view(
            &backend,
            &root,
            "resources/views/components/banner.blade.php",
        )
        .await;

        let hover = hover_text(&backend, &banner, 0, 10).await;
        assert!(
            hover.contains("42") && !hover.contains("danger"),
            "$headline is what its own tag passes, not what the tag before \
             it passed: {hover}"
        );
        let _ = uri;
    }

    /// A parameter the tag leaves out that the container *can* build is
    /// not missing at all: Laravel resolves a class-typed one and passes
    /// `null` for a nullable one, so neither is reported.
    #[tokio::test]
    async fn parameters_the_container_fills_are_not_reported_missing() {
        let template = "<x-forms.input />\n";
        let (backend, _dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        assert!(
            diagnostics(&backend, &uri).is_empty(),
            "the container fills these: {:?}",
            diagnostics(&backend, &uri)
        );
    }
}
