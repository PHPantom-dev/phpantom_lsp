//! A template's block directives have to pair up: `@foreach` closes with
//! `@endforeach`, and a block the template opens it also closes. Blade maps
//! each directive to PHP on its own, so neither mistake is reported against
//! the template without this check.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER: &str = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": { "psr-4": { "App\\": "app/" } }
    }"#;

    /// The directive-balance diagnostics for one template, as
    /// `(code, message, line)` in report order.
    async fn balance_diagnostics(template: &str) -> Vec<(String, String, u32)> {
        let relative = "resources/views/page.blade.php";
        let (backend, dir) = create_psr4_workspace(COMPOSER, &[(relative, template)]);
        backend.initialized(InitializedParams {}).await;

        let path = dir.path().join(relative);
        let uri = Url::from_file_path(&path).unwrap();
        open_document(&backend, &uri, "blade", template).await;

        let effective = backend
            .blade_virtual_php(uri.as_str())
            .unwrap_or_else(|| template.to_string());
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &effective, &mut diags);
        diags
            .into_iter()
            .filter_map(|d| match &d.code {
                Some(NumberOrString::String(code))
                    if code.ends_with("_blade_directive") || code.starts_with("unclosed_blade") =>
                {
                    Some((code.clone(), d.message, d.range.start.line))
                }
                _ => None,
            })
            .collect()
    }

    /// The directive that closes a loop is not the one that closes a
    /// conditional, however happily Blade compiles both.
    #[tokio::test]
    async fn a_loop_closed_by_endif_is_reported() {
        let reported = balance_diagnostics(
            "<ul>\n\
             @foreach ($rows as $row)\n\
             <li>{{ $row }}</li>\n\
             @endif\n\
             </ul>\n",
        )
        .await;

        assert_eq!(reported.len(), 1, "one mismatch, one report: {reported:?}");
        assert_eq!(reported[0].0, "mismatched_blade_directive");
        assert!(
            reported[0].1.contains("@endforeach") && reported[0].1.contains("@endif"),
            "the report names both directives: {}",
            reported[0].1
        );
        assert_eq!(reported[0].2, 3, "reported on the closing directive");
    }

    /// A block the template opens and never closes renders only part of
    /// itself.
    #[tokio::test]
    async fn a_block_left_open_at_end_of_file_is_reported() {
        let reported = balance_diagnostics(
            "@if ($user->isAdmin())\n\
             <p>admin</p>\n",
        )
        .await;

        assert_eq!(reported.len(), 1, "one unclosed block: {reported:?}");
        assert_eq!(reported[0].0, "unclosed_blade_directive");
        assert!(
            reported[0].1.contains("@endif"),
            "the report names the directive that is missing: {}",
            reported[0].1
        );
        assert_eq!(reported[0].2, 0, "reported on the opening directive");
    }

    /// A closing directive with nothing open to close is its own mistake.
    #[tokio::test]
    async fn a_closing_directive_with_no_block_is_reported() {
        let reported = balance_diagnostics("<p>hello</p>\n@endforeach\n").await;

        assert_eq!(reported.len(), 1, "one stray closer: {reported:?}");
        assert_eq!(reported[0].0, "unexpected_blade_directive");
        assert_eq!(reported[0].2, 1);
    }

    /// Correctly paired directives, including the ones whose meaning
    /// depends on their argument list, report nothing.
    #[tokio::test]
    async fn paired_directives_are_not_reported() {
        let reported = balance_diagnostics(
            "@extends('layouts.app')\n\
             @section('title', 'Home')\n\
             @section('body')\n\
             @forelse ($rows as $row)\n\
             @if ($row)\n\
             <li>{{ $row }}</li>\n\
             @endif\n\
             @empty\n\
             <li>none</li>\n\
             @endforelse\n\
             @php\n\
             $total = 0;\n\
             @endphp\n\
             {{-- @endsection --}}\n\
             @verbatim\n\
             @endsection\n\
             @endverbatim\n\
             @endsection\n\
             @push('scripts')\n\
             <script></script>\n\
             @endpush\n",
        )
        .await;

        assert!(reported.is_empty(), "nothing is unbalanced: {reported:?}");
    }

    /// The component-tag-balance diagnostics for one template, as
    /// `(code, message, line)` in report order.
    async fn tag_balance_diagnostics(template: &str) -> Vec<(String, String, u32)> {
        let relative = "resources/views/page.blade.php";
        let (backend, dir) = create_psr4_workspace(COMPOSER, &[(relative, template)]);
        backend.initialized(InitializedParams {}).await;

        let path = dir.path().join(relative);
        let uri = Url::from_file_path(&path).unwrap();
        open_document(&backend, &uri, "blade", template).await;

        let effective = backend
            .blade_virtual_php(uri.as_str())
            .unwrap_or_else(|| template.to_string());
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &effective, &mut diags);
        diags
            .into_iter()
            .filter_map(|d| match &d.code {
                Some(NumberOrString::String(code)) if code.ends_with("_blade_component_tag") => {
                    Some((code.clone(), d.message, d.range.start.line))
                }
                _ => None,
            })
            .collect()
    }

    /// A component tag closed by a different component's closing tag is a
    /// mismatched-tag diagnostic.
    #[tokio::test]
    async fn a_component_closed_by_another_components_tag_is_reported() {
        let reported = tag_balance_diagnostics(
            "<x-alert>\n\
             <p>hi</p>\n\
             </x-card>\n",
        )
        .await;

        assert_eq!(reported.len(), 1, "one mismatch, one report: {reported:?}");
        assert_eq!(reported[0].0, "mismatched_blade_component_tag");
        assert!(
            reported[0].1.contains("<x-alert>") && reported[0].1.contains("</x-card>"),
            "the report names both tags: {}",
            reported[0].1
        );
        assert_eq!(reported[0].2, 2, "reported on the closing tag");
    }

    /// A component tag nobody closes renders the rest of the template
    /// inside it.
    #[tokio::test]
    async fn a_component_tag_never_closed_is_reported() {
        let reported = tag_balance_diagnostics(
            "<x-alert>\n\
             <p>hi</p>\n",
        )
        .await;

        assert_eq!(reported.len(), 1, "one unclosed tag: {reported:?}");
        assert_eq!(reported[0].0, "unclosed_blade_component_tag");
        assert!(
            reported[0].1.contains("<x-alert>"),
            "the report names the tag that is missing a close: {}",
            reported[0].1
        );
        assert_eq!(reported[0].2, 0, "reported on the opening tag");
    }

    /// A closing tag with nothing open at all closes nothing.
    #[tokio::test]
    async fn a_component_closing_tag_with_no_open_tag_is_reported() {
        let reported = tag_balance_diagnostics("<p>hi</p>\n</x-alert>\n").await;

        assert_eq!(reported.len(), 1, "one stray closer: {reported:?}");
        assert_eq!(reported[0].0, "unexpected_blade_component_tag");
        assert_eq!(reported[0].2, 1);
    }

    /// Self-closing tags, and tags whose attributes contain a `>`, report
    /// nothing.
    #[tokio::test]
    async fn self_closing_and_bracket_bearing_component_tags_are_not_reported() {
        let reported = tag_balance_diagnostics(
            "<x-alert />\n\
             <x-card :items=\"$a > $b\">\n\
             <p>hi</p>\n\
             </x-card>\n",
        )
        .await;

        assert!(reported.is_empty(), "nothing is unbalanced: {reported:?}");
    }

    /// Properly nested and closed component tags, including a named
    /// inline slot whose closing tag never repeats the slot's own name,
    /// report nothing.
    #[tokio::test]
    async fn balanced_component_tags_are_not_reported() {
        let reported = tag_balance_diagnostics(
            "<x-card>\n\
             <x-slot:title>Title</x-slot>\n\
             <x-alert>\n\
             <p>hi</p>\n\
             </x-alert>\n\
             </x-card>\n\
             <livewire:counter></livewire:counter>\n",
        )
        .await;

        assert!(reported.is_empty(), "nothing is unbalanced: {reported:?}");
    }
}
