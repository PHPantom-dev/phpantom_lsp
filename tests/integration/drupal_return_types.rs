//! Return types Drupal core's docblocks leave wider than the call: the
//! query a select's `extend()` hands back is the extender it is given, and
//! a URL is a plain string unless the caller asks for its bubbleable
//! metadata.

use crate::common::{completion_response, message_text, response_labels, slow_diagnostic_messages};
use crate::drupal_services::{position_in, uri_for, workspace_in};
use tower_lsp_server::gen_lsp_types::*;

/// Just enough of core's query builder, where Composer would put core.
const QUERY_STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Database/Query/ExtendableInterface.php",
        "<?php\nnamespace Drupal\\Core\\Database\\Query;\ninterface ExtendableInterface {\n    /**\n     * @param class-string $extender_name\n     * @return \\Drupal\\Core\\Database\\Query\\ExtendableInterface\n     */\n    public function extend($extender_name);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Database/Query/SelectInterface.php",
        "<?php\nnamespace Drupal\\Core\\Database\\Query;\ninterface SelectInterface extends ExtendableInterface {\n    /** @return $this */\n    public function fields($table_alias, array $fields = []);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Database/Query/Select.php",
        "<?php\nnamespace Drupal\\Core\\Database\\Query;\nclass Select implements SelectInterface {\n    /** {@inheritdoc} */\n    public function extend($extender_name) {\n        return new $extender_name($this);\n    }\n    /** {@inheritdoc} */\n    public function fields($table_alias, array $fields = []) {\n        return $this;\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Database/Query/SelectExtender.php",
        "<?php\nnamespace Drupal\\Core\\Database\\Query;\nclass SelectExtender implements SelectInterface {\n    public function __construct(protected SelectInterface $query) {}\n    /** {@inheritdoc} */\n    public function extend($extender_name) {\n        return new $extender_name($this);\n    }\n    /** {@inheritdoc} */\n    public function fields($table_alias, array $fields = []) {\n        return $this;\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Database/Query/PagerSelectExtender.php",
        "<?php\nnamespace Drupal\\Core\\Database\\Query;\nclass PagerSelectExtender extends SelectExtender {\n    /** @return $this */\n    public function limit($limit = 10) {\n        return $this;\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Database/Query/TableSortExtender.php",
        "<?php\nnamespace Drupal\\Core\\Database\\Query;\nclass TableSortExtender extends SelectExtender {\n    /** @return $this */\n    public function orderByHeader(array $header) {\n        return $this;\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Database/Connection.php",
        "<?php\nnamespace Drupal\\Core\\Database;\nabstract class Connection {\n    /** @return \\Drupal\\Core\\Database\\Query\\SelectInterface */\n    public function select($table, $alias = NULL, array $options = []) {}\n}\n",
    ),
];

const LISTING_PATH: &str = "web/modules/custom/acme/src/Listing.php";

/// A module class whose `body` runs with a connection `$db`, a select
/// `$select`, an extender `$extender` and a string `$name` in scope.
fn listing(body: &str) -> String {
    format!(
        r#"<?php
namespace Drupal\acme;

use Drupal\Core\Database\Connection;
use Drupal\Core\Database\Query\PagerSelectExtender;
use Drupal\Core\Database\Query\Select;
use Drupal\Core\Database\Query\SelectExtender;
use Drupal\Core\Database\Query\TableSortExtender;

final class Listing {{
    public function build(Connection $db, Select $select, SelectExtender $extender, string $name): void {{
{body}
    }}
}}
"#
    )
}

/// A Drupal site holding the query stubs, with the listing open.
async fn site(listing: &str) -> (phpantom_lsp::Backend, tempfile::TempDir, Uri) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in QUERY_STUBS {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(dir, &[(LISTING_PATH, listing)]).await;
    backend.ensure_workspace_indexed();
    let uri = uri_for(&dir, LISTING_PATH);
    (backend, dir, uri)
}

/// The messages of the unknown-member warnings in `content`.
fn unknown_members(backend: &phpantom_lsp::Backend, uri: &Uri, content: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_unknown_member_diagnostics(uri.as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn a_select_extended_with_the_pager_has_the_pagers_methods() {
    let content =
        listing("        $db->select('node', 'n')->extend(PagerSelectExtender::class)->limit(10);");
    let (backend, _dir, uri) = site(&content).await;

    assert_eq!(
        unknown_members(&backend, &uri, &content),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn completion_after_an_extended_select_offers_the_extenders_methods() {
    let content = listing(
        "        $db->select('node', 'n')->extend(PagerSelectExtender::class)->\n        ;",
    );
    let (backend, _dir, uri) = site(&content).await;
    let at = position_in(&content, "::class)->", "::class)->".len());

    let labels =
        response_labels(completion_response(&backend, &uri, at.line, at.character, None).await);

    assert!(
        labels.iter().any(|label| label.starts_with("limit")),
        "expected limit among {labels:?}"
    );
}

#[tokio::test]
async fn the_core_select_and_extender_classes_return_the_extender_they_are_given() {
    let content = listing(
        "        $select->extend(PagerSelectExtender::class)->limit(10);\n        $extender->extend(PagerSelectExtender::class)->limit(10);",
    );
    let (backend, _dir, uri) = site(&content).await;

    assert_eq!(
        unknown_members(&backend, &uri, &content),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn a_chain_of_extenders_ends_with_the_last_one() {
    let content = listing(
        "        $chained = $select->extend(PagerSelectExtender::class)->extend(TableSortExtender::class);\n        $chained->orderByHeader([]);\n        $chained->limit(10);",
    );
    let (backend, _dir, uri) = site(&content).await;

    assert_eq!(
        unknown_members(&backend, &uri, &content),
        vec![
            "Method Drupal\\Core\\Database\\Query\\TableSortExtender::limit() not found"
                .to_string()
        ]
    );
}

#[tokio::test]
async fn an_extender_named_by_a_plain_string_leaves_the_interface() {
    let content = listing("        $select->extend($name)->limit(10);");
    let (backend, _dir, uri) = site(&content).await;

    assert_eq!(
        unknown_members(&backend, &uri, &content),
        vec![
            "Method Drupal\\Core\\Database\\Query\\ExtendableInterface::limit() not found"
                .to_string()
        ]
    );
}

/// Core's URL classes and services as 11.x declares them: the wide return
/// type in the docblock only, and `{@inheritdoc}` on the implementations,
/// followed by prose on the assembler's.
const URL_STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Render/BubbleableMetadata.php",
        "<?php\nnamespace Drupal\\Core\\Render;\nclass BubbleableMetadata {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/GeneratedUrl.php",
        "<?php\nnamespace Drupal\\Core;\nuse Drupal\\Core\\Render\\BubbleableMetadata;\nclass GeneratedUrl extends BubbleableMetadata {\n    /** @return string */\n    public function getGeneratedUrl() {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Url.php",
        "<?php\nnamespace Drupal\\Core;\nclass Url {\n    /**\n     * @param bool $collect_bubbleable_metadata\n     * @return string|\\Drupal\\Core\\GeneratedUrl\n     */\n    public function toString($collect_bubbleable_metadata = FALSE) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Routing/UrlGeneratorInterface.php",
        "<?php\nnamespace Drupal\\Core\\Routing;\ninterface UrlGeneratorInterface {\n    /**\n     * @param string $name\n     * @param array $parameters\n     * @param array $options\n     * @param bool $collect_bubbleable_metadata\n     * @return string|\\Drupal\\Core\\GeneratedUrl\n     */\n    public function generateFromRoute(string $name, array $parameters = [], array $options = [], bool $collect_bubbleable_metadata = FALSE);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Routing/UrlGenerator.php",
        "<?php\nnamespace Drupal\\Core\\Routing;\nclass UrlGenerator implements UrlGeneratorInterface {\n    /**\n     * {@inheritdoc}\n     */\n    public function generateFromRoute(string $name, array $parameters = [], array $options = [], bool $collect_bubbleable_metadata = FALSE) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Render/MetadataBubblingUrlGenerator.php",
        "<?php\nnamespace Drupal\\Core\\Render;\nuse Drupal\\Core\\Routing\\UrlGeneratorInterface;\nclass MetadataBubblingUrlGenerator implements UrlGeneratorInterface {\n    /**\n     * {@inheritdoc}\n     */\n    public function generateFromRoute(string $name, array $parameters = [], array $options = [], bool $collect_bubbleable_metadata = FALSE) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Utility/UnroutedUrlAssemblerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Utility;\ninterface UnroutedUrlAssemblerInterface {\n    /**\n     * @param string $uri\n     * @param array $options\n     * @param bool $collect_bubbleable_metadata\n     * @return string|\\Drupal\\Core\\GeneratedUrl\n     */\n    public function assemble($uri, array $options = [], $collect_bubbleable_metadata = FALSE);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Utility/UnroutedUrlAssembler.php",
        "<?php\nnamespace Drupal\\Core\\Utility;\nclass UnroutedUrlAssembler implements UnroutedUrlAssemblerInterface {\n    /**\n     * {@inheritdoc}\n     *\n     * This is a helper function that calls buildExternalUrl() or buildLocalUrl()\n     * based on a check of whether the path is a valid external URL.\n     */\n    public function assemble($uri, array $options = [], $collect_bubbleable_metadata = FALSE) {}\n}\n",
    ),
    (
        "web/core/core.services.yml",
        "services:\n  url_generator.non_bubbling:\n    class: Drupal\\Core\\Routing\\UrlGenerator\n  url_generator:\n    class: Drupal\\Core\\Render\\MetadataBubblingUrlGenerator\n  unrouted_url_assembler:\n    class: Drupal\\Core\\Utility\\UnroutedUrlAssembler\n",
    ),
    // A module class sharing the short name: the corrected type must name
    // core's class, not whatever `GeneratedUrl` means at the call site.
    (
        "web/modules/custom/acme/src/GeneratedUrl.php",
        "<?php\nnamespace Drupal\\acme;\nclass GeneratedUrl {\n    public function getModuleUrl(): string { return ''; }\n}\n",
    ),
];

const LINKS_PATH: &str = "web/modules/custom/acme/src/Links.php";

/// `Drupal\acme\Links` with `body` as the body of a method that has every
/// URL receiver core hands out in scope, and `$this->take()`, which wants a
/// `string`.
fn links(body: &str) -> String {
    format!(
        r#"<?php
namespace Drupal\acme;

use Drupal\Core\Render\MetadataBubblingUrlGenerator;
use Drupal\Core\Routing\UrlGenerator;
use Drupal\Core\Routing\UrlGeneratorInterface;
use Drupal\Core\Url;
use Drupal\Core\Utility\UnroutedUrlAssembler;
use Drupal\Core\Utility\UnroutedUrlAssemblerInterface;

final class Links {{
    public function take(string $value): void {{}}

    public function run(
        Url $url,
        bool $flag,
        UrlGeneratorInterface $generator,
        UrlGenerator $plainGenerator,
        MetadataBubblingUrlGenerator $bubblingGenerator,
        UnroutedUrlAssemblerInterface $assembler,
        UnroutedUrlAssembler $plainAssembler,
    ): void {{
{body}
    }}
}}
"#
    )
}

/// A Drupal project holding [`URL_STUBS`] unopened, as Composer would install
/// it, and `content` open as the consumer.
async fn url_site(content: &str) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in URL_STUBS {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    let (backend, dir) = workspace_in(dir, &[(LINKS_PATH, content)]).await;
    // The container is filled from core's YAML by the workspace index.
    backend.ensure_workspace_indexed();
    (backend, dir)
}

/// The `type_mismatch_argument` messages for a consumer running `body`.
async fn argument_mismatches(body: &str) -> Vec<String> {
    let content = links(body);
    let (backend, dir) = url_site(&content).await;
    let uri = uri_for(&dir, LINKS_PATH);
    slow_diagnostic_messages(&backend, uri.as_str(), &content, "type_mismatch_argument")
}

/// The completion labels right after the first `$generated->` in a consumer
/// running `body`.
async fn members_of_generated(body: &str) -> Vec<String> {
    let content = links(body);
    let (backend, dir) = url_site(&content).await;
    let position = position_in(&content, "$generated->", "$generated->".len());
    response_labels(
        completion_response(
            &backend,
            &uri_for(&dir, LINKS_PATH),
            position.line,
            position.character,
            None,
        )
        .await,
    )
}

/// Assert that `found` is the one report of a `GeneratedUrl`, and nothing
/// else, passed where a `string` is expected.
fn assert_exactly_generated_url(found: &[String], context: &str) {
    assert_eq!(found.len(), 1, "{context}: {found:?}");
    assert!(
        found[0].ends_with("got Drupal\\Core\\GeneratedUrl"),
        "{context}: {found:?}"
    );
}

fn assert_generated_url(labels: &[String]) {
    assert!(
        labels.iter().any(|l| l.starts_with("getGeneratedUrl")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("getModuleUrl")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn to_string_without_the_flag_is_a_string() {
    let found = argument_mismatches("        $this->take($url->toString());").await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn to_string_with_the_flag_off_is_a_string() {
    let found = argument_mismatches(
        "        $this->take($url->toString(FALSE));\n        $this->take($url->toString(false));",
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn to_string_with_the_flag_on_is_a_generated_url() {
    for flag in ["TRUE", "true"] {
        let found =
            argument_mismatches(&format!("        $this->take($url->toString({flag}));")).await;
        assert_exactly_generated_url(&found, flag);

        let labels = members_of_generated(&format!(
            "        $generated = $url->toString({flag});\n        $generated->"
        ))
        .await;
        assert_generated_url(&labels);
    }
}

#[tokio::test]
async fn to_string_with_a_named_flag_is_a_generated_url() {
    let found = argument_mismatches(
        "        $this->take($url->toString(collect_bubbleable_metadata: TRUE));",
    )
    .await;
    assert_exactly_generated_url(&found, "named");

    let labels = members_of_generated(
        "        $generated = $url->toString(collect_bubbleable_metadata: TRUE);\n        $generated->",
    )
    .await;
    assert_generated_url(&labels);
}

#[tokio::test]
async fn to_string_with_an_unknown_flag_may_be_either() {
    let found = argument_mismatches("        $this->take($url->toString($flag));").await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("got Drupal\\Core\\GeneratedUrl|string"),
        "{found:?}"
    );
}

/// Every receiver core hands out for the generator and the assembler, with
/// the call each one makes.
const RECEIVER_CALLS: &[&str] = &[
    "$generator->generateFromRoute('acme.page', [], []",
    "$plainGenerator->generateFromRoute('acme.page', [], []",
    "$bubblingGenerator->generateFromRoute('acme.page', [], []",
    "\\Drupal::service('url_generator')->generateFromRoute('acme.page', [], []",
    "\\Drupal::service('url_generator.non_bubbling')->generateFromRoute('acme.page', [], []",
    "$assembler->assemble('base:acme', []",
    "$plainAssembler->assemble('base:acme', []",
    "\\Drupal::service('unrouted_url_assembler')->assemble('base:acme', []",
];

#[tokio::test]
async fn every_generator_and_assembler_returns_a_string_without_the_flag() {
    for call in RECEIVER_CALLS {
        let found = argument_mismatches(&format!("        $this->take({call}));")).await;
        assert!(found.is_empty(), "{call}: {found:?}");
        // The receiver resolves: the same call with the flag on is reported.
        let found = argument_mismatches(&format!("        $this->take({call}, TRUE));")).await;
        assert_eq!(found.len(), 1, "{call}: {found:?}");
    }
}

#[tokio::test]
async fn every_generator_and_assembler_returns_a_generated_url_with_the_flag() {
    for call in RECEIVER_CALLS {
        let found = argument_mismatches(&format!("        $this->take({call}, TRUE));")).await;
        assert_exactly_generated_url(&found, call);
        let labels = members_of_generated(&format!(
            "        $generated = {call}, TRUE);\n        $generated->"
        ))
        .await;
        assert!(
            labels.iter().any(|l| l.starts_with("getGeneratedUrl")),
            "{call}: {labels:?}"
        );
    }
}
