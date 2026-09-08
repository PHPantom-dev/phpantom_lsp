//! A component tag names a class (or a template) nowhere written in the
//! template's PHP, so following one is its own resolution path.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document, open_php};
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

    /// A project holding one component of every shape a tag can name, with
    /// `page.blade.php` carrying the template under test.
    fn workspace(template: &str) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
        let (backend, dir) = create_psr4_workspace(
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
                    "app/View/Components/Forms/DatePicker.php",
                    "<?php\nnamespace App\\View\\Components\\Forms;\n\
                     use Illuminate\\View\\Component;\n\
                     class DatePicker extends Component {\n\
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
                    "<div>{{ $slot }}</div>\n",
                ),
                ("resources/views/page.blade.php", template),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        (backend, dir, uri)
    }

    /// The file go-to-definition answers with, relative to the project
    /// root, or `None` when it answers with nothing.
    async fn definition(
        backend: &phpantom_lsp::Backend,
        dir: &tempfile::TempDir,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Option<String> {
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
        let location = match response? {
            GotoDefinitionResponse::Scalar(location) => location,
            GotoDefinitionResponse::Array(locations) => locations.into_iter().next()?,
            GotoDefinitionResponse::Link(links) => {
                let link = links.into_iter().next()?;
                Location {
                    uri: link.target_uri,
                    range: link.target_range,
                }
            }
        };
        let root = dir.path().to_string_lossy().to_string();
        Some(
            location
                .uri
                .to_file_path()
                .unwrap()
                .to_string_lossy()
                .trim_start_matches(&root)
                .trim_start_matches('/')
                .to_string(),
        )
    }

    #[tokio::test]
    async fn a_component_tag_leads_to_the_class_backing_it() {
        let template = "<x-alert>hi</x-alert>\n";
        let (backend, dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        // On the `l` of `alert`.
        assert_eq!(
            definition(&backend, &dir, &uri, 0, 5).await.as_deref(),
            Some("app/View/Components/Alert.php")
        );
    }

    /// A nested component is addressed by the dotted, kebab-cased form of
    /// the path its class sits at.
    #[tokio::test]
    async fn a_nested_component_tag_leads_to_its_class() {
        let template = "<x-forms.date-picker />\n";
        let (backend, dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        assert_eq!(
            definition(&backend, &dir, &uri, 0, 12).await.as_deref(),
            Some("app/View/Components/Forms/DatePicker.php")
        );
    }

    #[tokio::test]
    async fn a_livewire_tag_leads_to_the_livewire_class() {
        let template = "<livewire:counter />\n";
        let (backend, dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        assert_eq!(
            definition(&backend, &dir, &uri, 0, 13).await.as_deref(),
            Some("app/Livewire/Counter.php")
        );
    }

    /// An anonymous component has no class to point at, so the template
    /// Laravel renders in its place is the definition.
    #[tokio::test]
    async fn an_anonymous_component_tag_leads_to_its_template() {
        let template = "<x-banner>hi</x-banner>\n";
        let (backend, dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        assert_eq!(
            definition(&backend, &dir, &uri, 0, 5).await.as_deref(),
            Some("resources/views/components/banner.blade.php")
        );
    }

    #[tokio::test]
    async fn a_tag_no_component_answers_for_leads_nowhere() {
        let template = "<x-nonexistent />\n";
        let (backend, dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        assert_eq!(definition(&backend, &dir, &uri, 0, 5).await, None);
    }

    /// The tag's attributes are ordinary markup, not another chance to
    /// name the component.
    #[tokio::test]
    async fn an_attribute_is_not_the_component_name() {
        let template = "<x-alert type=\"danger\" />\n";
        let (backend, dir, uri) = workspace(template);
        open_document(&backend, &uri, "blade", template).await;

        assert_eq!(definition(&backend, &dir, &uri, 0, 11).await, None);
    }

    /// A `{{ }}` echo delimiter has no position in the underlying PHP
    /// expression it wraps, since it's stripped away when Blade compiles
    /// the template down to an `e(...)` call. Go-to-definition on the
    /// delimiter itself must agree with what hovering it reports (`e()`),
    /// not fall through to the offset mapping and land on whatever
    /// expression happens to start where the delimiter was.
    #[tokio::test]
    async fn an_echo_delimiter_leads_to_e_not_to_the_wrapped_call() {
        let composer = r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#;
        let e_helper = "<?php\n\
             function e(mixed $value, bool $doubleEncode = true): string {\n\
                 return htmlspecialchars((string) $value, ENT_QUOTES, 'UTF-8', $doubleEncode);\n\
             }\n";
        let route_helper = "<?php\nfunction route(string $name): string {\n    return $name;\n}\n";
        let template = "{{ route('pages.index') }}\n";
        let (backend, dir) = create_psr4_workspace(
            composer,
            &[
                ("app/echo_helper.php", e_helper),
                ("app/route_helper.php", route_helper),
                ("resources/views/page.blade.php", template),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();

        for (rel_path, content) in [
            ("app/echo_helper.php", e_helper),
            ("app/route_helper.php", route_helper),
        ] {
            let uri = Url::from_file_path(root.join(rel_path)).unwrap();
            open_php(&backend, &uri, content).await;
        }

        let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        open_document(&backend, &uri, "blade", template).await;

        // On the first `{` of the `{{` opening the echo.
        assert_eq!(
            definition(&backend, &dir, &uri, 0, 0).await.as_deref(),
            Some("app/echo_helper.php")
        );
        // On the `}}` closing it.
        assert_eq!(
            definition(&backend, &dir, &uri, 0, 24).await.as_deref(),
            Some("app/echo_helper.php")
        );
    }
}
