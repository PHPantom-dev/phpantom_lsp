//! A trait's lexical self keeps the using class's generic arguments.

use crate::common::{
    complete_labels_at_opened, create_psr4_workspace, definitions_at, hover_on, open_php,
    position_after, position_of, workspace_uri,
};

#[tokio::test]
async fn trait_self_preserves_generic_receivers_and_parent_bindings() {
    let declarations = r"<?php
namespace Library;
trait Fluent {
    public function same(): self {}
    /** @return self<int> */
    public function integer(): self {}
    /** @return static */
    public function late(): static {}
    public function accepts(self $other): self {}
}
trait Nested { use Fluent { same as alias; } }
/** @template T */
class Collection {
    use Nested;
    /** @return T */
    public function item() {}
}
class Item { public string $label; }
/** @extends Collection<Item> */
class Items extends Collection {}
";
    let source = r"<?php
namespace Caller;
use Library\Collection;
use Library\Item as ImportedItem;
use Library\Items;
class Item { public string $wrong; }
/** @param Collection<ImportedItem> $collection */
function demo($collection, Items $items): void {
    $collection->same()->item()->label;
    $collection->alias()->same()->item()->label;
    $collection->accepts($collection)->item()->label;
    $items->same()->item()->label;
    $items->alias()->item()->label;
    $items->late()->item()->label;
    $integer = $collection->integer()->item();
}
";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"Library\\":"lib/"}}}"#,
        &[("lib/types.php", declarations), ("caller.php", source)],
    );
    let types_uri = workspace_uri(&backend, "lib/types.php");
    let uri = workspace_uri(&backend, "caller.php");
    open_php(&backend, &types_uri, declarations).await;
    open_php(&backend, &uri, source).await;
    for needle in [
        "$collection->same()->item()->",
        "$collection->alias()->same()->item()->",
        "$collection->accepts($collection)->item()->",
        "$items->same()->item()->",
        "$items->alias()->item()->",
        "$items->late()->item()->",
    ] {
        let at = position_after(source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        assert!(
            labels.contains(&"label".to_string()),
            "{needle}: {labels:?}"
        );
        assert!(
            !labels.contains(&"wrong".to_string()),
            "{needle}: {labels:?}"
        );
    }
    let locations = definitions_at(&backend, &uri, source, "label;").await;
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, types_uri);
    assert_eq!(
        locations[0].range.start.line,
        position_of(declarations, "$label").line
    );
    let hover = hover_on(&backend, &uri, source, "$integer =").await;
    assert!(hover.contains("int"), "{hover}");
}
