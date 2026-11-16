//! Return types Drupal core's docblocks leave wider than the call: the
//! query a select's `extend()` hands back is the extender it is given.

use crate::common::{completion_response, message_text, response_labels};
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
