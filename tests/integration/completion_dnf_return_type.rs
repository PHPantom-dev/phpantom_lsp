//! Regression test for parenthesized (DNF) `@return` types resolving
//! through an inheritance + `@mixin` generic chain.
//!
//! A method annotated `@return (TRelated&object{pivot: TPivot})|null`
//! must parse as a normal type (a leading `(` here opens a type group,
//! not a conditional).  Before the fix the type was dropped, so the
//! method inherited its ancestor mixin's raw template parameter
//! (`TModel`) instead, which never resolved to the concrete related
//! model.  This mirrors Laravel's `BelongsToMany::first()`.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn dnf_return_type_resolves_through_mixin_generic_chain() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "vendor/illuminate/" } } }"#,
        &[
            (
                "vendor/illuminate/Database/Concerns/BuildsQueries.php",
                r#"<?php
namespace Illuminate\Database\Concerns;
/** @template TValue */
trait BuildsQueries {
    /** @return TValue|null */
    public function first() {}
}
"#,
            ),
            (
                "vendor/illuminate/Database/Eloquent/Builder.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;
use Illuminate\Database\Concerns\BuildsQueries;
/** @template TModel of \Illuminate\Database\Eloquent\Model */
class Builder {
    /** @use \Illuminate\Database\Concerns\BuildsQueries<TModel> */
    use BuildsQueries;
}
"#,
            ),
            (
                "vendor/illuminate/Database/Eloquent/Relations/Relation.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @template TResult
 * @mixin \Illuminate\Database\Eloquent\Builder<TRelatedModel>
 */
abstract class Relation {}
"#,
            ),
            (
                "vendor/illuminate/Database/Eloquent/Relations/BelongsToMany.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @template TPivotModel of \Illuminate\Database\Eloquent\Model = \Illuminate\Database\Eloquent\Model
 * @extends \Illuminate\Database\Eloquent\Relations\Relation<TRelatedModel, TDeclaringModel, \Illuminate\Support\Collection<int, TRelatedModel&object{pivot: TPivotModel}>>
 */
class BelongsToMany extends Relation {
    /** @return (TRelatedModel&object{pivot: TPivotModel})|null */
    public function first($columns = ['*']) {}
}
"#,
            ),
            (
                "vendor/illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;
abstract class Model {}
"#,
            ),
            (
                "src/Models/Subcategory.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Subcategory extends Model {
    public int $vat_group_id = 0;
}
"#,
            ),
            (
                "src/Models/Product.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;
class Product extends Model {
    /** @return BelongsToMany<Subcategory, $this> */
    public function primarySubcategory(): BelongsToMany {}

    public function test(): void {
        $x = $this->primarySubcategory()->first();
        $x;
    }
}
"#,
            ),
        ],
    );

    for path in [
        "vendor/illuminate/Database/Concerns/BuildsQueries.php",
        "vendor/illuminate/Database/Eloquent/Builder.php",
        "vendor/illuminate/Database/Eloquent/Relations/Relation.php",
        "vendor/illuminate/Database/Eloquent/Relations/BelongsToMany.php",
        "vendor/illuminate/Database/Eloquent/Model.php",
        "src/Models/Subcategory.php",
        "src/Models/Product.php",
    ] {
        let full = dir.path().join(path);
        let uri = Url::from_file_path(&full).unwrap();
        let text = std::fs::read_to_string(&full).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text,
                },
            })
            .await;
    }

    let product_path = dir.path().join("src/Models/Product.php");
    let product_uri = Url::from_file_path(&product_path).unwrap();
    let content = std::fs::read_to_string(&product_path).unwrap();
    let line = content.lines().position(|l| l.trim() == "$x;").unwrap() as u32;

    let hover = backend
        .handle_hover(
            product_uri.as_str(),
            &content,
            Position { line, character: 8 },
        )
        .expect("hover should resolve the chained call");
    let text = match hover.contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("expected markup hover, got {other:?}"),
    };

    // The related model must flow through the chain; the raw mixin
    // template parameter `TModel` must never leak.
    assert!(
        text.contains("Subcategory"),
        "hover should resolve the related model, got:\n{text}"
    );
    assert!(
        !text.contains("TModel"),
        "the mixin's raw template parameter must not leak, got:\n{text}"
    );
}
