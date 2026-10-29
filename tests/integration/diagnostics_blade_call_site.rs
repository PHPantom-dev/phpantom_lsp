//! `view()` call sites are checked against the contract the template they
//! name declares: a declared variable that is not passed, a value whose
//! type the declaration does not accept, and a variable nothing in the
//! template reads are each reported.
//!
//! A template that declares no contract is not checked at all, and every
//! check stands down as soon as the data involved stops being readable.

#[cfg(test)]
mod tests {
    use crate::common::message_text;
    use crate::common::{USER_MODEL_STUB, create_psr4_workspace, open_document};
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;

    const COMPOSER: &str = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": { "psr-4": { "App\\": "app/", "Illuminate\\": "illuminate/" } }
    }"#;

    /// A template declaring `$title` (string) and `$user` (User).
    const PROFILE: &str = "@php\n\
        /**\n\
         * @bladestan-signature\n\
         * @var string $title\n\
         * @var \\App\\Models\\User $user\n\
         */\n\
        @endphp\n\
        <h1>{{ $title }}</h1>\n\
        <p>{{ $user->email }}</p>\n";

    const VIEW_PROVIDER_LIST: &str =
        "<?php\nreturn [\n    App\\Providers\\ViewServiceProvider::class,\n];\n";

    /// A provider composing `$siteName` into the layout that declares it.
    const LAYOUT_COMPOSER: &str = r#"<?php
namespace App\Providers;

use Illuminate\Support\Facades\View;

class ViewServiceProvider
{
    public function boot(): void
    {
        View::composer('layouts.app', function ($view) {
            $view->with('siteName', 'Acme');
        });
    }
}
"#;

    fn workspace(files: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
        let mut all = vec![("app/Models/User.php", USER_MODEL_STUB)];
        all.extend_from_slice(files);
        create_psr4_workspace(COMPOSER, &all)
    }

    /// Collect the call-site diagnostics for one file, as
    /// `(code, message)` pairs in report order.
    async fn call_site_diagnostics(
        backend: &phpantom_lsp::Backend,
        dir: &tempfile::TempDir,
        relative: &str,
        language_id: &str,
    ) -> Vec<(String, String)> {
        backend.initialized(InitializedParams {}).await;
        let path = dir.path().join(relative);
        let text = std::fs::read_to_string(&path).unwrap();
        let uri = Uri::from_file_path(&path).unwrap();
        open_document(backend, &uri, language_id, &text).await;

        // A Blade file is analysed through its virtual PHP, which is what
        // the symbol map and the source map are both keyed to.
        let effective = backend.blade_virtual_php(uri.as_str()).unwrap_or(text);
        let uri = uri.to_string();
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(&uri, &effective, &mut diags);
        diags
            .into_iter()
            .filter_map(|d| match &d.code {
                Some(Code::String(code))
                    if code.ends_with("_view_variable")
                        || code == "type_mismatch_view_variable" =>
                {
                    Some((code.clone(), message_text(&d.message).to_string()))
                }
                _ => None,
            })
            .collect()
    }

    /// A controller that renders `profile` with `data` as its data argument.
    fn controller(data: &str) -> String {
        format!(
            "<?php\nnamespace App;\nuse App\\Models\\User;\nclass ProfileController {{\n    public function show(User $user): mixed {{\n        return view('profile'{data});\n    }}\n}}\n"
        )
    }

    /// A controller that renders `profile` with a variable of type `shape`
    /// as its data argument, so the names it passes are only in its type.
    fn shaped_controller(shape: &str) -> String {
        format!(
            "<?php\nnamespace App;\nclass ProfileController {{\n    /** @param {shape} $data */\n    public function show(array $data): mixed {{\n        return view('profile', $data);\n    }}\n}}\n"
        )
    }

    #[tokio::test]
    async fn a_complete_call_site_reports_nothing() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &controller(", ['title' => 'Profile', 'user' => $user]"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(
            diags.is_empty(),
            "expected a clean call site, got {diags:?}"
        );
    }

    #[tokio::test]
    async fn a_declared_variable_that_is_not_passed_is_reported() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &controller(", ['title' => 'Profile']"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$user") && diags[0].1.contains("profile"),
            "message should name the variable and the view, got {:?}",
            diags[0].1
        );
    }

    #[tokio::test]
    async fn a_call_site_with_no_data_at_all_is_reported() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            ("app/ProfileController.php", &controller("")),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(
            diags.len(),
            2,
            "both declared variables are missing: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .all(|(code, _)| code == "missing_view_variable")
        );
    }

    #[tokio::test]
    async fn a_value_of_the_wrong_type_is_reported() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &controller(", ['title' => 42, 'user' => $user]"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "type_mismatch_view_variable");
        assert!(
            diags[0].1.contains("$title") && diags[0].1.contains("string"),
            "message should name the variable and the declared type, got {:?}",
            diags[0].1
        );
    }

    #[tokio::test]
    async fn a_variable_the_template_never_reads_is_reported() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &controller(", ['title' => 'Profile', 'user' => $user, 'usre' => $user]"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "unused_view_variable");
        assert!(
            diags[0].1.contains("$usre"),
            "message should name the typo, got {:?}",
            diags[0].1
        );
    }

    /// A partial three levels down is still handed the whole data array,
    /// so a name only it reads is not unwanted.
    #[tokio::test]
    async fn a_variable_an_included_partial_reads_is_accepted() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/profile.blade.php",
                &format!("{PROFILE}@include('partials.badge')\n"),
            ),
            ("resources/views/partials/badge.blade.php", "{{ $badge }}\n"),
            (
                "app/ProfileController.php",
                &controller(", ['title' => 'Profile', 'user' => $user, 'badge' => 'new']"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// An include whose name is built at runtime hides a whole template,
    /// so nothing about the extra names can be concluded.
    #[tokio::test]
    async fn a_dynamic_include_stands_the_unknown_check_down() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/profile.blade.php",
                &format!("{PROFILE}@include($partial)\n"),
            ),
            (
                "app/ProfileController.php",
                &controller(", ['title' => 'Profile', 'user' => $user, 'whatever' => 1]"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// A layout renders from the child's data, so what it declares the
    /// caller has to supply.
    #[tokio::test]
    async fn a_layouts_declaration_is_part_of_the_childs_contract() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/layouts/app.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $siteName\n */\n@endphp\n<title>{{ $siteName }}</title>\n@yield('body')\n",
            ),
            (
                "resources/views/page.blade.php",
                "@extends('layouts.app')\n@php\n/**\n * @bladestan-signature\n * @var string $title\n */\n@endphp\n@section('body'){{ $title }}@endsection\n",
            ),
            (
                "app/PageController.php",
                "<?php\nnamespace App;\nclass PageController {\n    public function show(): mixed {\n        return view('page', ['title' => 'Hi']);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/PageController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$siteName"),
            "message should name the layout's variable, got {:?}",
            diags[0].1
        );
    }

    /// A view composer is registered on the template that reads the
    /// variable, which is normally the layout, so the exemption it earns has
    /// to reach the children the declaration reaches. Nothing a caller of
    /// the child writes could clear the name otherwise.
    #[tokio::test]
    async fn a_layouts_composed_variable_is_not_the_childs_callers_to_pass() {
        let (backend, dir) = workspace(&[
            ("bootstrap/providers.php", VIEW_PROVIDER_LIST),
            ("app/Providers/ViewServiceProvider.php", LAYOUT_COMPOSER),
            (
                "resources/views/layouts/app.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $siteName\n */\n@endphp\n<title>{{ $siteName }}</title>\n@yield('body')\n",
            ),
            (
                "resources/views/page.blade.php",
                "@extends('layouts.app')\n@php\n/**\n * @bladestan-signature\n * @var string $title\n */\n@endphp\n@section('body'){{ $title }}@endsection\n",
            ),
            (
                "app/PageController.php",
                "<?php\nnamespace App;\nclass PageController {\n    public function show(): mixed {\n        return view('page', ['title' => 'Hi']);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/PageController.php", "php").await;
        assert!(
            diags.is_empty(),
            "the composer supplies the layout's variable, got {diags:?}"
        );
    }

    /// A template that declares nothing has no contract to be judged
    /// against, which is what keeps the whole check opt-in.
    #[tokio::test]
    async fn a_template_without_a_signature_is_not_checked() {
        let (backend, dir) = workspace(&[
            ("resources/views/plain.blade.php", "<h1>{{ $title }}</h1>\n"),
            (
                "app/PlainController.php",
                "<?php\nnamespace App;\nclass PlainController {\n    public function show(): mixed {\n        return view('plain', ['anything' => 1]);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/PlainController.php", "php").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// A data array built from a variable hides the names it carries, so
    /// neither a missing nor an unwanted one can be concluded.
    #[tokio::test]
    async fn an_unreadable_data_argument_stands_the_checks_down() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nclass ProfileController {\n    public function show(array $data): mixed {\n        return view('profile', $data);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// `compact()` names the variables it passes just as an array literal
    /// does.
    #[tokio::test]
    async fn compact_is_read_as_the_data_it_passes() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nuse App\\Models\\User;\nclass ProfileController {\n    public function show(User $user): mixed {\n        $title = 'Profile';\n        return view('profile', compact('title', 'user'));\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(
            diags.is_empty(),
            "expected a clean call site, got {diags:?}"
        );
    }

    /// Data added over a chain is one call site's worth, not several.
    #[tokio::test]
    async fn a_with_chain_completes_the_call_site() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nuse App\\Models\\User;\nclass ProfileController {\n    public function show(User $user): mixed {\n        return view('profile')->with('title', 'Profile')->withUser($user);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(
            diags.is_empty(),
            "expected a clean call site, got {diags:?}"
        );
    }

    #[tokio::test]
    async fn view_make_is_checked_like_the_helper() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nuse Illuminate\\Support\\Facades\\View;\nclass ProfileController {\n    public function show(): mixed {\n        return View::make('profile', ['title' => 'Profile']);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
    }

    /// `Route::view()` names its template second, and its data third.
    #[tokio::test]
    async fn route_view_is_checked_like_the_helper() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "routes/web.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::view('/profile', 'profile', ['title' => 'Profile']);\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "routes/web.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(diags[0].1.contains("$user"), "got {:?}", diags[0].1);
    }

    /// Laravel merges a component's public members into its view's data, so
    /// a `render()` that passes nothing still satisfies the contract.
    #[tokio::test]
    async fn a_components_own_members_satisfy_the_view_it_renders() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/View/Components/ProfileCard.php",
                "<?php\nnamespace App\\View\\Components;\nuse App\\Models\\User;\nuse Illuminate\\View\\Component;\nclass ProfileCard extends Component {\n    public string $title = '';\n    public ?User $user = null;\n    public function render(): mixed {\n        return view('profile');\n    }\n}\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "app/View/Components/ProfileCard.php", "php")
                .await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// Laravel adds the attribute bag, the default slot, and the component's
    /// name to the data of the view a class component renders, so a template
    /// declaring them is not short of anything the `render()` leaves out.
    #[tokio::test]
    async fn a_components_render_supplies_the_attribute_bag_and_slot() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/components/products/ean-code.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\Illuminate\\View\\ComponentAttributeBag $attributes\n * @var mixed $slot\n * @var string $componentName\n * @var string $value\n */\n@endphp\n<span {{ $attributes }}>{{ $value }}{{ $slot }}{{ $componentName }}</span>\n",
            ),
            (
                "app/Contexts/Webshop/View/Components/Products/EanCode.php",
                "<?php\nnamespace App\\Contexts\\Webshop\\View\\Components\\Products;\nuse Illuminate\\View\\Component;\nclass EanCode extends Component {\n    private string $raw = '';\n    public function render(): mixed {\n        return view('components.products.ean-code', ['value' => $this->raw]);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(
            &backend,
            &dir,
            "app/Contexts/Webshop/View/Components/Products/EanCode.php",
            "php",
        )
        .await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// A plain controller's properties never reach the view, so they excuse
    /// nothing.
    #[tokio::test]
    async fn a_plain_classs_properties_do_not_satisfy_the_view_it_renders() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nuse App\\Models\\User;\nclass ProfileController {\n    public string $title = '';\n    public ?User $user = null;\n    public function show(): mixed {\n        return view('profile');\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 2, "got {diags:?}");
    }

    /// The public surface a component inherits from `Illuminate\View\Component`
    /// is plumbing rather than view data, and a method that takes an argument
    /// reaches the view as a closure no template can call, so neither excuses
    /// a call site from passing a variable of that name.
    #[tokio::test]
    async fn a_components_framework_surface_satisfies_nothing() {
        let (backend, dir) = create_psr4_workspace(
            r#"{
                "require": { "laravel/framework": "^11.0" },
                "autoload": { "psr-4": {
                    "App\\": "app/",
                    "Illuminate\\": "stubs/Illuminate/"
                } }
            }"#,
            &[
                (
                    "stubs/Illuminate/View/Component.php",
                    "<?php\nnamespace Illuminate\\View;\nabstract class Component {\n    public function data(): array { return []; }\n    public function render() {}\n}\n",
                ),
                (
                    "resources/views/card.blade.php",
                    "@php\n/**\n * @bladestan-signature\n * @var array $data\n * @var string $label\n */\n@endphp\n{{ count($data) }}{{ $label }}\n",
                ),
                (
                    "app/View/Components/ProfileCard.php",
                    "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass ProfileCard extends Component {\n    public function label(string $suffix): string { return $suffix; }\n    public function render(): mixed {\n        return view('card');\n    }\n}\n",
                ),
            ],
        );
        let diags =
            call_site_diagnostics(&backend, &dir, "app/View/Components/ProfileCard.php", "php")
                .await;
        assert_eq!(diags.len(), 2, "got {diags:?}");
        assert!(
            diags.iter().any(|(_, message)| message.contains("$data")),
            "got {diags:?}"
        );
        assert!(
            diags.iter().any(|(_, message)| message.contains("$label")),
            "got {diags:?}"
        );
    }

    /// Livewire hands its view the properties the subclass declares, judged
    /// by where they are declared rather than by name, so a property is
    /// still view data when the base class has a *method* of that name
    /// (`Livewire\Component::id()` against a component's own `$id`).
    #[tokio::test]
    async fn a_livewire_property_named_after_a_framework_method_is_still_supplied() {
        let (backend, dir) = create_psr4_workspace(
            r#"{
                "require": { "laravel/framework": "^11.0" },
                "autoload": { "psr-4": {
                    "App\\": "app/",
                    "Livewire\\": "stubs/Livewire/"
                } }
            }"#,
            &[
                (
                    "stubs/Livewire/Component.php",
                    "<?php\nnamespace Livewire;\nabstract class Component {\n    public function id() {}\n}\n",
                ),
                (
                    "resources/views/panel.blade.php",
                    "@php\n/**\n * @bladestan-signature\n * @var string $id\n */\n@endphp\n<div id=\"{{ $id }}\"></div>\n",
                ),
                (
                    "app/Livewire/ShowPanel.php",
                    "<?php\nnamespace App\\Livewire;\nuse Livewire\\Component;\nclass ShowPanel extends Component {\n    public string $id = '';\n    public function render(): mixed {\n        return view('panel');\n    }\n}\n",
                ),
            ],
        );
        let diags =
            call_site_diagnostics(&backend, &dir, "app/Livewire/ShowPanel.php", "php").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// A Livewire component's public methods are actions, not view data, so
    /// one that shares a name with a declared variable excuses nothing.
    #[tokio::test]
    async fn a_livewire_components_actions_satisfy_nothing() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/Livewire/ShowProfile.php",
                "<?php\nnamespace App\\Livewire;\nuse App\\Models\\User;\nuse Livewire\\Component;\nclass ShowProfile extends Component {\n    public string $title = '';\n    public function user(): ?User { return null; }\n    public function render(): mixed {\n        return view('profile');\n    }\n}\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "app/Livewire/ShowProfile.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert!(diags[0].1.contains("$user"), "got {diags:?}");
    }

    /// `$loop` is Blade's own object however a call site writes it down, so
    /// the type a template declares for it is not the caller's to satisfy.
    #[tokio::test]
    async fn blades_own_loop_variable_is_not_type_checked() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/row.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\LoopStub $loop\n */\n@endphp\n<td>{{ $loop->index }}</td>\n",
            ),
            (
                "app/Models/LoopStub.php",
                "<?php\nnamespace App\\Models;\nclass LoopStub { public int $index = 0; }\n",
            ),
            (
                "resources/views/table.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, string> $rows\n */\n@endphp\n@foreach ($rows as $row)\n@include('partials.row', ['loop' => $loop])\n@endforeach\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/table.blade.php", "blade").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// `@include` inside a template that declares its own contract is
    /// judged against what that contract puts in scope.
    #[tokio::test]
    async fn an_include_is_checked_against_the_including_templates_scope() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/card.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $user\n * @var string $caption\n */\n@endphp\n<p>{{ $user->email }} {{ $caption }}</p>\n",
            ),
            (
                "resources/views/page.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $user\n */\n@endphp\n@include('partials.card')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/page.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$caption"),
            "the includer already has $user in scope; only $caption is short, got {:?}",
            diags[0].1
        );
    }

    /// A prop with a default is in the component's scope whether or not its
    /// body ever writes the name, so a partial it includes gets it from
    /// there.
    #[tokio::test]
    async fn a_defaulted_prop_is_in_scope_for_an_include_that_forwards_it() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/badge.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $size\n */\n@endphp\n<span class=\"badge-{{ $size }}\"></span>\n",
            ),
            (
                "resources/views/components/card.blade.php",
                "@props(['size' => 'md'])\n@php\n/**\n * @bladestan-signature\n * @var string $heading\n */\n@endphp\n<h2>{{ $heading }}</h2>\n@include('partials.badge')\n",
            ),
        ]);
        let diags = call_site_diagnostics(
            &backend,
            &dir,
            "resources/views/components/card.blade.php",
            "blade",
        )
        .await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// Without a contract of its own, an including template's inbound data
    /// is whatever its callers happened to pass, so nothing is missing.
    #[tokio::test]
    async fn an_include_from_an_undeclared_template_is_not_checked_for_missing_data() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/card.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $caption\n */\n@endphp\n<p>{{ $caption }}</p>\n",
            ),
            (
                "resources/views/page.blade.php",
                "<h1>Page</h1>\n@include('partials.card')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/page.blade.php", "blade").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// `@extendsFirst` picks the first candidate layout that exists, and
    /// that layout renders from the child's data exactly as `@extends`
    /// would, so what it declares the caller has to supply.
    #[tokio::test]
    async fn a_layout_chosen_with_extends_first_is_part_of_the_childs_contract() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/layouts/app.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $siteName\n */\n@endphp\n<title>{{ $siteName }}</title>\n@yield('body')\n",
            ),
            (
                "resources/views/page.blade.php",
                "@extendsFirst(['themes.dark', 'layouts.app'])\n@php\n/**\n * @bladestan-signature\n * @var string $title\n */\n@endphp\n@section('body'){{ $title }}@endsection\n",
            ),
            (
                "app/PageController.php",
                "<?php\nnamespace App;\nclass PageController {\n    public function show(): mixed {\n        return view('page', ['title' => 'Hi']);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/PageController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$siteName"),
            "message should name the layout's variable, got {:?}",
            diags[0].1
        );
    }

    /// Blade renders whichever candidate exists, so a variable *any* of
    /// them reads is one the call site may pass. The walk still closes
    /// over the candidates it could read, so a name none of them mentions
    /// is reported.
    #[tokio::test]
    async fn a_variable_a_candidate_layout_reads_is_accepted() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/themes/dark.blade.php",
                "<title>{{ $siteName }}</title>\n@yield('body')\n",
            ),
            (
                "resources/views/layouts/app.blade.php",
                "<h1>{{ $brand }}</h1>\n@yield('body')\n",
            ),
            (
                "resources/views/page.blade.php",
                "@extendsFirst(['themes.dark', 'layouts.app'])\n@php\n/**\n * @bladestan-signature\n * @var string $title\n */\n@endphp\n@section('body'){{ $title }}@endsection\n",
            ),
            (
                "app/PageController.php",
                "<?php\nnamespace App;\nclass PageController {\n    public function show(): mixed {\n        return view('page', ['title' => 'Hi', 'siteName' => 'Acme', 'brand' => 'Acme', 'nope' => 1]);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/PageController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "unused_view_variable");
        assert!(
            diags[0].1.contains("$nope"),
            "only the name no candidate reads is unknown, got {:?}",
            diags[0].1
        );
    }

    /// `@each` binds the entry under the name its third argument spells,
    /// with the collection's element type, so a partial declaring that
    /// variable is satisfied.  `$key` comes free with it.
    #[tokio::test]
    async fn each_binds_the_item_under_the_name_it_spells() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/row.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $row\n */\n@endphp\n<td>{{ $row->email }}</td>\n",
            ),
            (
                "resources/views/table.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, \\App\\Models\\User> $users\n */\n@endphp\n@each('partials.row', $users, 'row')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/table.blade.php", "blade").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// The item's type is the collection's element type, so a collection of
    /// the wrong thing is reported against the partial's declaration.
    #[tokio::test]
    async fn each_reports_an_item_type_the_partial_does_not_accept() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/row.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $row\n */\n@endphp\n<td>{{ $row->email }}</td>\n",
            ),
            (
                "resources/views/table.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, string> $names\n */\n@endphp\n@each('partials.row', $names, 'row')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/table.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "type_mismatch_view_variable");
        assert!(
            diags[0].1.contains("$row") && diags[0].1.contains("string"),
            "message should name the item and the element type, got {:?}",
            diags[0].1
        );
    }

    /// `$key` is Blade's to bind, so a partial with no use for it is not
    /// being handed something unwanted.
    #[tokio::test]
    async fn each_does_not_report_its_key_as_unknown() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/row.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $row\n */\n@endphp\n<td>{{ $row }}</td>\n",
            ),
            (
                "resources/views/table.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, string> $names\n */\n@endphp\n@each('partials.row', $names, 'row')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/table.blade.php", "blade").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// `$key` carries the collection's key type, so a partial declaring it
    /// as something the keys are not is reported.
    #[tokio::test]
    async fn each_types_its_key_from_the_collections_keys() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/row.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $row\n * @var string $key\n */\n@endphp\n<td>{{ $key }}: {{ $row }}</td>\n",
            ),
            (
                "resources/views/table.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, string> $names\n */\n@endphp\n@each('partials.row', $names, 'row')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/table.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "type_mismatch_view_variable");
        assert!(
            diags[0].1.contains("$key"),
            "message should name the key, got {:?}",
            diags[0].1
        );
    }

    /// An `@each` partial sees only the item and the key, so a variable the
    /// surrounding template holds does not excuse the call from passing it.
    #[tokio::test]
    async fn each_does_not_forward_the_surrounding_templates_scope() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/row.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $row\n * @var string $caption\n */\n@endphp\n<td>{{ $caption }}: {{ $row }}</td>\n",
            ),
            (
                "resources/views/table.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, string> $names\n * @var string $caption\n */\n@endphp\n@each('partials.row', $names, 'row')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/table.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$caption"),
            "message should name the variable the partial is short of, got {:?}",
            diags[0].1
        );
    }

    /// A `@component` view is rendered from the array it is passed alone, so
    /// a variable the surrounding template holds does not excuse the call
    /// from passing it.
    #[tokio::test]
    async fn component_does_not_forward_the_surrounding_templates_scope() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/alert.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $title\n */\n@endphp\n<b>{{ $title }}</b>\n",
            ),
            (
                "resources/views/page.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $title\n */\n@endphp\n@component('alert')\n@endcomponent\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/page.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(diags[0].1.contains("$title"), "got {:?}", diags[0].1);
    }

    /// A data argument that writes no names down still passes the ones its
    /// type spells out, so a site built from a variable is checked on them.
    #[tokio::test]
    async fn a_shaped_data_argument_satisfies_the_contract() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &shaped_controller("array{title: string, user: \\App\\Models\\User}"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(
            diags.is_empty(),
            "expected a clean call site, got {diags:?}"
        );
    }

    #[tokio::test]
    async fn a_shaped_data_argument_short_of_a_declared_name_is_reported() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &shaped_controller("array{title: string}"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$user"),
            "message should name the variable the shape is short of, got {:?}",
            diags[0].1
        );
    }

    #[tokio::test]
    async fn a_shaped_data_argument_of_the_wrong_type_is_reported() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &shaped_controller("array{title: int, user: \\App\\Models\\User}"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "type_mismatch_view_variable");
        assert!(
            diags[0].1.contains("$title"),
            "message should name the variable whose type is wrong, got {:?}",
            diags[0].1
        );
    }

    /// An optional key may or may not be there when the render happens, so
    /// it is no more passed than an absent one.
    #[tokio::test]
    async fn an_optional_key_of_a_shaped_data_argument_is_still_missing() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &shaped_controller("array{title: string, user?: \\App\\Models\\User}"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$user"),
            "message should name the optional key, got {:?}",
            diags[0].1
        );
    }

    #[tokio::test]
    async fn a_key_of_a_shaped_data_argument_the_template_never_reads_is_reported() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                &shaped_controller("array{title: string, user: \\App\\Models\\User, extra: int}"),
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "unused_view_variable");
        assert!(
            diags[0].1.contains("$extra"),
            "message should name the key nothing reads, got {:?}",
            diags[0].1
        );
    }

    /// A `->with()` whose argument names nothing itself is read off its type
    /// just as a `view()` data argument is.
    #[tokio::test]
    async fn a_shaped_with_argument_completes_the_call_site() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nclass ProfileController {\n    /** @param array{user: \\App\\Models\\User} $extra */\n    public function show(array $extra): mixed {\n        return view('profile', ['title' => 'Profile'])->with($extra);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(
            diags.is_empty(),
            "expected a clean call site, got {diags:?}"
        );
    }

    /// The factory converts an `Arrayable` before rendering, so what its
    /// `toArray()` returns is the data the template receives.
    #[tokio::test]
    async fn an_arrayable_data_argument_is_read_through_to_array() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "illuminate/Contracts/Support/Arrayable.php",
                "<?php\nnamespace Illuminate\\Contracts\\Support;\ninterface Arrayable { public function toArray(): array; }\n",
            ),
            (
                "app/ProfileData.php",
                "<?php\nnamespace App;\nuse Illuminate\\Contracts\\Support\\Arrayable;\nclass ProfileData implements Arrayable {\n    /** @return array{title: string} */\n    public function toArray(): array { return ['title' => 'Profile']; }\n}\n",
            ),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nclass ProfileController {\n    public function show(ProfileData $data): mixed {\n        return view('profile', $data);\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$user"),
            "the shape carries $title; only $user is short, got {:?}",
            diags[0].1
        );
    }

    /// A data argument whose type says nothing about its keys hides the
    /// names it carries, so every check still stands down.
    #[tokio::test]
    async fn a_shapeless_data_argument_stands_the_checks_down() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", PROFILE),
            (
                "app/ProfileController.php",
                "<?php\nnamespace App;\nclass ProfileController {\n    public function show(array $data): mixed {\n        return view('profile', array_merge($data, ['title' => 'Profile']));\n    }\n}\n",
            ),
        ]);
        let diags = call_site_diagnostics(&backend, &dir, "app/ProfileController.php", "php").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// An `@include` inherits the scope it is written in, so the type the
    /// surrounding template holds is the one the partial receives.
    #[tokio::test]
    async fn an_include_reports_an_inherited_type_the_partial_does_not_accept() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/card.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $user\n */\n@endphp\n<p>{{ $user->email }}</p>\n",
            ),
            (
                "resources/views/page.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $user\n */\n@endphp\n@include('partials.card')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/page.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "type_mismatch_view_variable");
        assert!(
            diags[0].1.contains("$user") && diags[0].1.contains("surrounding scope"),
            "message should name the variable and where it came from, got {:?}",
            diags[0].1
        );
    }

    /// A name the surrounding template binds itself is inherited with the
    /// type it was bound to, not merely as a name.
    #[tokio::test]
    async fn an_include_reports_an_inherited_loop_binding_of_the_wrong_type() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/card.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $row\n */\n@endphp\n<p>{{ $row->email }}</p>\n",
            ),
            (
                "resources/views/page.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, string> $rows\n */\n@endphp\n@foreach ($rows as $row)\n@include('partials.card')\n@endforeach\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/page.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "type_mismatch_view_variable");
        assert!(
            diags[0].1.contains("$row"),
            "message should name the loop binding, got {:?}",
            diags[0].1
        );
    }

    /// The directive is written under a check that proves the variable,
    /// and Blade compiles that check into the same `if` any PHP walker
    /// reads, so the partial receives the narrowed type rather than the
    /// nullable one the template declared.
    #[tokio::test]
    async fn an_include_under_a_guard_inherits_the_narrowed_type() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/card.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $user\n */\n@endphp\n<p>{{ $user->email }}</p>\n",
            ),
            (
                "resources/views/page.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User|null $user\n */\n@endphp\n@if ($user)\n@include('partials.card')\n@endif\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/page.blade.php", "blade").await;
        assert!(
            diags.is_empty(),
            "the guard proves the user is there, got {diags:?}"
        );
    }

    /// The same template without the guard still reports, so the check is
    /// reading the flow rather than being switched off.
    #[tokio::test]
    async fn an_unguarded_include_still_reports_the_nullable_type() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/card.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $user\n */\n@endphp\n<p>{{ $user->email }}</p>\n",
            ),
            (
                "resources/views/page.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User|null $user\n */\n@endphp\n@include('partials.card')\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/page.blade.php", "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "type_mismatch_view_variable");
    }

    /// An item name built at runtime binds a variable whose name cannot be
    /// known, so nothing about what the partial is short of follows.
    #[tokio::test]
    async fn a_dynamic_each_item_name_stands_the_missing_check_down() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/partials/row.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var string $row\n */\n@endphp\n<td>{{ $row }}</td>\n",
            ),
            (
                "resources/views/table.blade.php",
                "@php\n/**\n * @bladestan-signature\n * @var array<int, string> $names\n * @var string $itemName\n */\n@endphp\n@each('partials.row', $names, $itemName)\n",
            ),
        ]);
        let diags =
            call_site_diagnostics(&backend, &dir, "resources/views/table.blade.php", "blade").await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// The innermost partial of a nested include chain: it declares three
    /// variables, none of which the template that includes it declares.
    const WP_SCRIPT: &str = "@php\n/**\n * @var \\App\\Models\\User $server\n * @var string $VERSION\n * @var array $data\n */\n@endphp\n<script>{{ $server->email }} {{ $VERSION }} {{ count($data) }}</script>\n";

    /// The middle template: it types the one variable it reads itself, and
    /// hands the partial its whole scope.
    const APP_SCRIPTS: &str = "@php\n/** @var string $VERSION */\n@endphp\n<p>{{ $VERSION }}</p>\n@include('scripts.apps.wp')\n";

    /// The diagnostics of the middle template of a chain whose outer end
    /// is `outer`, written either as a template or as a controller.
    async fn middle_of_chain_diagnostics(outer: (&str, &str)) -> Vec<(String, String)> {
        let (backend, dir) = workspace(&[
            ("resources/views/scripts/apps/wp.blade.php", WP_SCRIPT),
            ("resources/views/scripts/apps.blade.php", APP_SCRIPTS),
            outer,
        ]);
        // The outer end has to be parsed to be found rendering the middle.
        let (outer_path, outer_text) = outer;
        let language_id = match outer_path.ends_with(".blade.php") {
            true => "blade",
            false => "php",
        };
        let outer_uri = Uri::from_file_path(dir.path().join(outer_path)).unwrap();
        open_document(&backend, &outer_uri, language_id, outer_text).await;
        call_site_diagnostics(
            &backend,
            &dir,
            "resources/views/scripts/apps.blade.php",
            "blade",
        )
        .await
    }

    /// An `@include` without data hands the partial everything in scope,
    /// and the scope of an included template is what *its* includer holds,
    /// so a variable two levels up still reaches the innermost partial.
    #[tokio::test]
    async fn a_nested_include_inherits_what_the_outer_template_holds() {
        let diags = middle_of_chain_diagnostics((
            "resources/views/servers/show.blade.php",
            "@php\n/**\n * @bladestan-signature\n * @var \\App\\Models\\User $server\n * @var string $VERSION\n * @var array $data\n */\n@endphp\n@include('scripts.apps')\n",
        ))
        .await;
        assert!(
            diags.is_empty(),
            "the outer template holds every variable the partial declares, got {diags:?}"
        );
    }

    /// The same holds for what a controller passes the middle template:
    /// the names it hands over are in scope for the partial even though
    /// the middle template never declares them.
    #[tokio::test]
    async fn a_nested_include_inherits_what_the_controller_passes() {
        let diags = middle_of_chain_diagnostics((
            "app/ScriptController.php",
            "<?php\nnamespace App;\nuse App\\Models\\User;\nclass ScriptController {\n    public function show(User $server): mixed {\n        return view('scripts.apps', ['server' => $server, 'VERSION' => '1.0', 'data' => []]);\n    }\n}\n",
        ))
        .await;
        assert!(
            diags.is_empty(),
            "the controller passes every variable the partial declares, got {diags:?}"
        );
    }

    /// A name no render of the middle template hands it is still missing
    /// from the partial.
    #[tokio::test]
    async fn a_name_no_caller_hands_the_includer_is_still_missing() {
        let diags = middle_of_chain_diagnostics((
            "app/ScriptController.php",
            "<?php\nnamespace App;\nuse App\\Models\\User;\nclass ScriptController {\n    public function show(User $server): mixed {\n        return view('scripts.apps', ['server' => $server, 'VERSION' => '1.0']);\n    }\n}\n",
        ))
        .await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$data"),
            "only $data is passed by no caller, got {:?}",
            diags[0].1
        );
    }

    /// A caller whose data cannot be read may be handing the middle
    /// template anything, so nothing the partial declares is provably
    /// missing.
    #[tokio::test]
    async fn an_includer_rendered_with_unreadable_data_stands_the_missing_check_down() {
        let diags = middle_of_chain_diagnostics((
            "app/ScriptController.php",
            "<?php\nnamespace App;\nclass ScriptController {\n    public function show(array $payload): mixed {\n        return view('scripts.apps', $payload);\n    }\n}\n",
        ))
        .await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// An outer template that declares no contract hands the middle one an
    /// unknown scope, so the check stands down rather than guessing.
    #[tokio::test]
    async fn an_includer_included_from_an_undeclared_template_is_not_checked_for_missing_data() {
        let diags = middle_of_chain_diagnostics((
            "resources/views/servers/show.blade.php",
            "<h1>Server</h1>\n@include('scripts.apps')\n",
        ))
        .await;
        assert!(diags.is_empty(), "expected no report, got {diags:?}");
    }

    /// A template reachable along many include paths is read once per
    /// lookup, not once per path: six levels of six templates, each
    /// including every template one level below, hand the bottom level
    /// what the top level holds.
    #[tokio::test]
    async fn a_wide_include_lattice_is_read_once_per_template() {
        const WIDTH: usize = 6;
        const DEPTH: usize = 6;
        let path = |level: usize, i: usize| format!("resources/views/l{level}/t{i}.blade.php");
        let mut files: Vec<(String, String)> = vec![(
            "resources/views/leaf.blade.php".to_string(),
            "@php\n/**\n * @var string $title\n * @var string $a\n * @var string $b\n */\n@endphp\n<p>{{ $title }} {{ $a }} {{ $b }}</p>\n".to_string(),
        )];
        for level in 0..DEPTH {
            for i in 0..WIDTH {
                // The top level declares what the leaf reads, except for
                // one template that never holds `$b`.
                let header = match (level, i) {
                    (0, 0) => "@php\n/**\n * @bladestan-signature\n * @var string $title\n * @var string $a\n */\n@endphp\n".to_string(),
                    (0, _) => "@php\n/**\n * @bladestan-signature\n * @var string $title\n * @var string $a\n * @var string $b\n */\n@endphp\n".to_string(),
                    _ => "@php\n/** @var string $title */\n@endphp\n".to_string(),
                };
                let includes: String = match level + 1 < DEPTH {
                    true => (0..WIDTH)
                        .map(|j| format!("@include('l{}.t{j}')\n", level + 1))
                        .collect(),
                    false => "@include('leaf')\n".to_string(),
                };
                files.push((
                    path(level, i),
                    format!("{header}<p>{{{{ $title }}}}</p>\n{includes}"),
                ));
            }
        }
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        let (backend, dir) = workspace(&borrowed);
        // Every includer has to be parsed to be found rendering the level
        // below it.
        for (relative, text) in &files[1..] {
            let uri = Uri::from_file_path(dir.path().join(relative)).unwrap();
            open_document(&backend, &uri, "blade", text).await;
        }
        let diags = call_site_diagnostics(&backend, &dir, &path(DEPTH - 1, 0), "blade").await;
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].0, "missing_view_variable");
        assert!(
            diags[0].1.contains("$b") && !diags[0].1.contains("$a"),
            "only $b fails to reach the leaf along every path, got {:?}",
            diags[0].1
        );
    }
}
