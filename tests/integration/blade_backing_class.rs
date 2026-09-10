//! The class backing a component view puts its public members in the
//! template's scope: a Blade component's public properties and
//! argument-less public methods, and a Livewire component's public
//! properties plus the component instance.

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

    /// The framework members a component inherits but never exposes.
    const COMPONENT_STUB: &str = "<?php\nnamespace Illuminate\\View;\n\
        abstract class Component {\n\
            public $componentName;\n\
            public $attributes;\n\
            public function render() {}\n\
            public function data() {}\n\
            public function shouldRender() {}\n\
        }\n";

    const INVOKABLE_STUB: &str = "<?php\nnamespace Illuminate\\View;\n\
        class InvokableComponentVariable {\n\
            public function __invoke() {}\n\
            public function __toString(): string { return ''; }\n\
        }\n";

    const LIVEWIRE_STUB: &str = "<?php\nnamespace Livewire;\n\
        abstract class Component {\n\
            public function render() {}\n\
            public function dispatch(string $event) {}\n\
        }\n";

    const ORDER_CLASS: &str =
        "<?php\nnamespace App\\Models;\nclass Order { public string $reference = ''; }\n";

    async fn open_template(
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
            other => panic!("expected markup hover, got {:?}", other),
        }
    }

    fn undefined_variables(backend: &phpantom_lsp::Backend, uri: &Url) -> Vec<String> {
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

    fn component_workspace(
        template: &str,
    ) -> (phpantom_lsp::Backend, tempfile::TempDir, std::path::PathBuf) {
        let (backend, dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Illuminate/View/Component.php", COMPONENT_STUB),
                (
                    "stubs/Illuminate/View/InvokableComponentVariable.php",
                    INVOKABLE_STUB,
                ),
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                ("app/Models/Order.php", ORDER_CLASS),
                (
                    "app/View/Components/OrderCard.php",
                    "<?php\nnamespace App\\View\\Components;\n\
                     use App\\Models\\Order;\n\
                     use Illuminate\\View\\Component;\n\
                     class OrderCard extends Component {\n\
                         public function __construct(public Order $order, public string $label = 'Order') {}\n\
                         public function total(): string { return '0.00'; }\n\
                         public function formatted(string $currency): string { return ''; }\n\
                         protected function internal(): string { return ''; }\n\
                         public static function make(): string { return ''; }\n\
                         public function render() {}\n\
                     }\n",
                ),
                ("resources/views/components/order-card.blade.php", template),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        (backend, dir, root)
    }

    /// A public property of the backing class types the matching variable
    /// in the component's own template.
    #[tokio::test]
    async fn a_public_property_types_the_template_variable() {
        let (backend, _dir, root) = component_workspace("<p>{{ $order->reference }}</p>\n");
        let uri = open_template(
            &backend,
            &root,
            "resources/views/components/order-card.blade.php",
        )
        .await;

        let hover = hover_text(&backend, &uri, 0, 8).await;
        assert!(
            hover.contains("App\\Models") && hover.contains("Order"),
            "$order should be typed from the backing class, got: {hover}"
        );
        assert!(
            undefined_variables(&backend, &uri).is_empty(),
            "no variable of the backing class may be reported undefined: {:?}",
            undefined_variables(&backend, &uri)
        );
    }

    /// Blade merges an argument-less public method into the view data as
    /// an invokable variable, so both `{{ $total }}` and `{{ $total() }}`
    /// are legitimate. A method that requires an argument is not a
    /// variable at all.
    #[tokio::test]
    async fn argument_less_methods_become_variables_and_others_do_not() {
        let (backend, _dir, root) =
            component_workspace("{{ $total }}\n{{ $total() }}\n{{ $formatted }}\n");
        let uri = open_template(
            &backend,
            &root,
            "resources/views/components/order-card.blade.php",
        )
        .await;

        let hover = hover_text(&backend, &uri, 0, 7).await;
        assert!(
            hover.contains("InvokableComponentVariable"),
            "$total should be the wrapper Blade merges in, got: {hover}"
        );

        let undefined = undefined_variables(&backend, &uri);
        assert!(
            !undefined.iter().any(|m| m.contains("$total")),
            "$total is view data: {undefined:?}"
        );
        assert!(
            undefined.iter().any(|m| m.contains("$formatted")),
            "a method that requires an argument is not view data: {undefined:?}"
        );
    }

    /// Framework plumbing (`render()`, `data()`), non-public members, and
    /// static members are not view data.
    #[tokio::test]
    async fn framework_and_non_public_members_stay_out_of_scope() {
        let (backend, _dir, root) =
            component_workspace("{{ $render }}{{ $data }}{{ $internal }}{{ $make }}\n");
        let uri = open_template(
            &backend,
            &root,
            "resources/views/components/order-card.blade.php",
        )
        .await;

        let undefined = undefined_variables(&backend, &uri);
        for name in ["$render", "$data", "$internal", "$make"] {
            assert!(
                undefined.iter().any(|m| m.contains(name)),
                "{name} must not be declared by the backing class: {undefined:?}"
            );
        }
    }

    /// The template's own `@props` default wins over the class member of
    /// the same name: the template is closer to the body than the class.
    #[tokio::test]
    async fn a_props_entry_wins_over_the_class_member() {
        let (backend, _dir, root) =
            component_workspace("@props(['label' => 0])\n{{ $label }}\n{{ $order }}\n");
        let uri = open_template(
            &backend,
            &root,
            "resources/views/components/order-card.blade.php",
        )
        .await;

        let hover = hover_text(&backend, &uri, 1, 7).await;
        assert!(
            !hover.contains("string"),
            "the @props default should type $label, not the class property: {hover}"
        );
        // The names @props leaves out still come from the class.
        let order = hover_text(&backend, &uri, 2, 7).await;
        assert!(
            order.contains("App\\Models") && order.contains("Order"),
            "$order should still come from the backing class, got: {order}"
        );
    }

    /// A template whose view name has no backing class is unaffected.
    #[tokio::test]
    async fn an_anonymous_component_gets_no_class_members() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Illuminate/View/Component.php", COMPONENT_STUB),
                ("app/Models/Order.php", ORDER_CLASS),
                (
                    "resources/views/components/plain.blade.php",
                    "{{ $order }}\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/components/plain.blade.php",
        )
        .await;

        assert!(
            undefined_variables(&backend, &uri)
                .iter()
                .any(|m| m.contains("$order")),
            "nothing declares $order in an anonymous component"
        );
    }

    /// A Livewire view gets the component's public properties and the
    /// component instance under the names Livewire binds it to, `$this`
    /// included.
    #[tokio::test]
    async fn a_livewire_view_gets_the_component_scope() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                ("app/Models/Order.php", ORDER_CLASS),
                (
                    "app/Livewire/OrderList.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use App\\Models\\Order;\n\
                     use Livewire\\Component;\n\
                     class OrderList extends Component {\n\
                         public Order $selected;\n\
                         public int $page = 1;\n\
                         public function reload(): void {}\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/livewire/order-list.blade.php",
                    "{{ $selected->reference }}\n{{ $page }}\n{{ $_instance }}\n{{ $this->page }}\n{{ $__livewire }}\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/livewire/order-list.blade.php",
        )
        .await;

        let hover = hover_text(&backend, &uri, 0, 7).await;
        assert!(
            hover.contains("App\\Models") && hover.contains("Order"),
            "$selected should be typed from the Livewire class, got: {hover}"
        );
        let instance = hover_text(&backend, &uri, 2, 7).await;
        assert!(
            instance.contains("App\\Livewire") && instance.contains("OrderList"),
            "$_instance is the component itself, got: {instance}"
        );
        let this_member = hover_text(&backend, &uri, 3, 11).await;
        assert!(
            this_member.contains("int"),
            "$this->page reads the component's own property, got: {this_member}"
        );
        // Livewire binds the instance under both of its own names, not
        // just `$this`.
        let aliased = hover_text(&backend, &uri, 4, 7).await;
        assert!(
            aliased.contains("App\\Livewire") && aliased.contains("OrderList"),
            "$__livewire is the component too, got: {aliased}"
        );
        assert!(
            undefined_variables(&backend, &uri).is_empty(),
            "the Livewire scope covers every variable used: {:?}",
            undefined_variables(&backend, &uri)
        );
    }

    /// Livewire renders its view with the component bound to `$this`, so
    /// the view reaches the component's properties, its actions (what
    /// `wire:click` calls), and whatever it inherits.
    #[tokio::test]
    async fn a_livewire_view_resolves_this_to_the_component() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                ("app/Models/Order.php", ORDER_CLASS),
                (
                    "app/Livewire/OrderList.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use App\\Models\\Order;\n\
                     use Livewire\\Component;\n\
                     class OrderList extends Component {\n\
                         public Order $selected;\n\
                         public function reload(): Order { return $this->selected; }\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/livewire/order-list.blade.php",
                    "<div wire:click=\"reload\">\n\
                     {{ $this->selected->reference }}\n\
                     {{ $this->reload()->reference }}\n\
                     {{ $this->dispatch('saved') }}\n\
                     </div>\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/livewire/order-list.blade.php",
        )
        .await;

        let property = hover_text(&backend, &uri, 1, 12).await;
        assert!(
            property.contains("Order $selected"),
            "$this->selected is the component's own property, got: {property}"
        );
        let action = hover_text(&backend, &uri, 2, 12).await;
        assert!(
            action.contains("reload") && action.contains("Order"),
            "$this->reload() is a component action, got: {action}"
        );
        let inherited = hover_text(&backend, &uri, 3, 12).await;
        assert!(
            inherited.contains("dispatch"),
            "$this reaches what the component inherits, got: {inherited}"
        );

        // Nothing on `$this` may be reported as a member the component
        // does not have.
        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_unknown_member_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        assert!(
            diags.is_empty(),
            "the component's own members must resolve: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// `$this->` in a Livewire view completes the component's members.
    #[tokio::test]
    async fn completion_on_this_offers_the_component_members() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                (
                    "app/Livewire/Counter.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use Livewire\\Component;\n\
                     class Counter extends Component {\n\
                         public int $count = 0;\n\
                         public function increment(): void {}\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/livewire/counter.blade.php",
                    "{{ $this-> }}\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/livewire/counter.blade.php",
        )
        .await;

        let result = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
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
        let labels: Vec<String> = match result {
            Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
            Some(CompletionResponse::List(list)) => {
                list.items.into_iter().map(|i| i.label).collect()
            }
            None => Vec::new(),
        };
        for name in ["count", "increment()", "dispatch($event)"] {
            assert!(
                labels.iter().any(|l| l == name),
                "$this-> should offer {name}: {labels:?}"
            );
        }
        assert!(
            !labels.iter().any(|l| l.starts_with("__blade")),
            "the synthesized wrapper is not a member of the component: {labels:?}"
        );
    }

    /// Go-to-definition on a `$this` member in a Livewire view lands on
    /// the component class, not on the synthesized wrapper.
    #[tokio::test]
    async fn go_to_definition_on_a_this_member_lands_on_the_component() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                (
                    "app/Livewire/Counter.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use Livewire\\Component;\n\
                     class Counter extends Component {\n\
                         public int $count = 0;\n\
                         public function increment(): void {}\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/livewire/counter.blade.php",
                    "<button wire:click=\"{{ $this->increment() }}\">{{ $this->count }}</button>\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/livewire/counter.blade.php",
        )
        .await;

        let target = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position {
                        line: 0,
                        character: 34,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        let uri_of = |r: &GotoDefinitionResponse| match r {
            GotoDefinitionResponse::Scalar(l) => l.uri.to_string(),
            GotoDefinitionResponse::Array(ls) => ls[0].uri.to_string(),
            other => panic!("unexpected definition response {other:?}"),
        };
        let target = target.as_ref().map(uri_of).unwrap_or_default();
        assert!(
            target.ends_with("app/Livewire/Counter.php"),
            "increment() is declared on the component, got: {target}"
        );
    }

    /// Wrapping a Livewire view's body in a method must not bury the
    /// imports its `@php` region declares: Laravel compiles those to the
    /// top level of the generated view file.
    #[tokio::test]
    async fn a_livewire_view_still_hoists_its_own_imports() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                ("app/Models/Order.php", ORDER_CLASS),
                (
                    "app/Livewire/Counter.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use Livewire\\Component;\n\
                     class Counter extends Component { public function render() {} }\n",
                ),
                (
                    "resources/views/livewire/counter.blade.php",
                    "@php\nuse App\\Models\\Order;\n$order = new Order();\n@endphp\n\
                     {{ $order->reference }}\n{{ $order->missing() }}\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/livewire/counter.blade.php",
        )
        .await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        let messages: Vec<String> = diags
            .into_iter()
            .map(|d| d.message)
            .filter(|m| !m.contains("Function 'e' not found"))
            .collect();
        assert_eq!(
            messages,
            vec!["Method 'missing' not found on class 'App\\Models\\Order'"],
            "the template's own import must still resolve"
        );
    }

    /// A Blade component's view is rendered by the view engine, where
    /// `$this` is *not* the component, so nothing may be invented for it.
    #[tokio::test]
    async fn a_blade_component_view_does_not_bind_this() {
        let (backend, _dir, root) = component_workspace("{{ $this->label }}\n");
        let uri = open_template(
            &backend,
            &root,
            "resources/views/components/order-card.blade.php",
        )
        .await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        assert!(
            !virtual_php.contains("__blade_scope_"),
            "a Blade component view must keep the plain function wrapper: {virtual_php}"
        );
    }

    /// Livewire exposes public *properties* only: a public method is an
    /// action, reached through the component instance.
    #[tokio::test]
    async fn a_livewire_method_is_not_a_view_variable() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
                (
                    "app/Livewire/Counter.php",
                    "<?php\nnamespace App\\Livewire;\n\
                     use Livewire\\Component;\n\
                     class Counter extends Component {\n\
                         public int $count = 0;\n\
                         public function increment(): void {}\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/livewire/counter.blade.php",
                    "{{ $count }}{{ $increment }}\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/livewire/counter.blade.php",
        )
        .await;

        let undefined = undefined_variables(&backend, &uri);
        assert!(
            !undefined.iter().any(|m| m.contains("$count")),
            "a public property is view data: {undefined:?}"
        );
        assert!(
            undefined.iter().any(|m| m.contains("$increment")),
            "a Livewire action is not a view variable: {undefined:?}"
        );
    }

    /// An index component is addressed by its directory alone
    /// (`<x-card>` → `App\View\Components\Card\Card`), a name no
    /// transform of the view name predicts. Only having seen the file
    /// finds it.
    #[tokio::test]
    async fn an_index_component_backs_its_view() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Illuminate/View/Component.php", COMPONENT_STUB),
                ("app/Models/Order.php", ORDER_CLASS),
                (
                    "app/View/Components/Card/Card.php",
                    "<?php\nnamespace App\\View\\Components\\Card;\n\
                     use App\\Models\\Order;\n\
                     use Illuminate\\View\\Component;\n\
                     class Card extends Component {\n\
                         public function __construct(public Order $order) {}\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/components/card.blade.php",
                    "{{ $order->reference }}\n",
                ),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(&backend, &root, "resources/views/components/card.blade.php").await;

        let hover = hover_text(&backend, &uri, 0, 7).await;
        assert!(
            hover.contains("App\\Models") && hover.contains("Order"),
            "$order should be typed from the index component class, got: {hover}"
        );
        assert!(
            undefined_variables(&backend, &uri).is_empty(),
            "the index component declares $order: {:?}",
            undefined_variables(&backend, &uri)
        );
    }

    /// Every `{{ … }}` compiles to an `echo`, so a guard written inside
    /// one has to narrow the variable it proves — here the component's
    /// own nullable `$countryName`.
    #[tokio::test]
    async fn a_guard_inside_an_interpolation_narrows_the_variable_it_proves() {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("stubs/Illuminate/View/Component.php", COMPONENT_STUB),
                (
                    "app/helpers.php",
                    "<?php\nfunction shout(string $value): string { return $value; }\n",
                ),
                (
                    "app/View/Components/Banner.php",
                    "<?php\nnamespace App\\View\\Components;\n\
                     use Illuminate\\View\\Component;\n\
                     class Banner extends Component {\n\
                         public function __construct(public ?string $countryName = null) {}\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "resources/views/components/banner.blade.php",
                    "@php\n\
                     /**\n\
                      * @bladestan-signature\n\
                      * @var string|null $countryName\n\
                      */\n\
                     @endphp\n\
                     {{ $countryName ? shout($countryName) : '' }}\n",
                ),
            ],
        );
        backend.initialized(InitializedParams {}).await;
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = open_template(
            &backend,
            &root,
            "resources/views/components/banner.blade.php",
        )
        .await;

        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        let type_errors: Vec<String> = diags
            .into_iter()
            .filter(|d| {
                d.code.as_ref().is_some_and(
                    |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_argument"),
                )
            })
            .map(|d| d.message)
            .collect();
        assert!(
            type_errors.is_empty(),
            "the truthy arm proves $countryName is a string: {type_errors:?}"
        );
    }
}
