use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_laravel_custom_builder_references_from_builder() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "vendor/illuminate/" } } }"#,
        &[
            (
                "vendor/illuminate/Model.php",
                "<?php namespace Illuminate\\Database\\Eloquent; abstract class Model {
                public static function query() {}
            }",
            ),
            (
                "vendor/illuminate/Builder.php",
                "<?php namespace Illuminate\\Database\\Eloquent; class Builder {
                /** @return $this */
                public function where($c, $v = null) { return $this; }
            }",
            ),
            (
                "src/Models/UserBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class UserBuilder extends Builder {
    /** @return $this */
    public function active() { return $this; }
}
"#,
            ),
            (
                "src/Models/User.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(UserBuilder::class)]
class User extends Model {}
"#,
            ),
            (
                "src/Models/PostBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class PostBuilder extends Builder {
    /** @return $this */
    public function active() { return $this; }
}
"#,
            ),
            (
                "src/Models/Post.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(PostBuilder::class)]
class Post extends Model {}
"#,
            ),
            (
                "usage.php",
                r#"<?php
use App\Models\User;
use App\Models\Post;

User::active();
Post::active();
User::query()->active();
"#,
            ),
        ],
    );

    // Open all files to index them
    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/User.php",
        "src/Models/PostBuilder.php",
        "src/Models/Post.php",
        "usage.php",
    ] {
        let uri = Url::from_file_path(dir.path().join(path)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: std::fs::read_to_string(dir.path().join(path)).unwrap(),
                },
            })
            .await;
    }

    let builder_uri = Url::from_file_path(dir.path().join("src/Models/UserBuilder.php")).unwrap();

    // Find references for UserBuilder::active() declaration (line 5, col 21)
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(builder_uri),
            position: Position::new(5, 21),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let locations = backend
        .references(params)
        .await
        .unwrap()
        .unwrap_or_default();

    let usage_uri = Url::from_file_path(dir.path().join("usage.php")).unwrap();
    let usage_locs: Vec<_> = locations.iter().filter(|l| l.uri == usage_uri).collect();

    let lines: Vec<_> = usage_locs.iter().map(|l| l.range.start.line).collect();

    assert!(
        lines.contains(&4),
        "Should find User::active(). Found at lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&6),
        "Should find User::query()->active(). Found at lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&5),
        "Should NOT find Post::active(). Found at lines: {:?}",
        lines
    );
    assert_eq!(
        usage_locs.len(),
        2,
        "Should find exactly 2 references in usage.php, but found: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_laravel_custom_builder_references_from_model() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "vendor/illuminate/" } } }"#,
        &[
            (
                "vendor/illuminate/Model.php",
                "<?php namespace Illuminate\\Database\\Eloquent; abstract class Model {
                public static function query() {}
            }",
            ),
            (
                "vendor/illuminate/Builder.php",
                "<?php namespace Illuminate\\Database\\Eloquent; class Builder {
                /** @return $this */
                public function where($c, $v = null) { return $this; }
            }",
            ),
            (
                "src/Models/UserBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class UserBuilder extends Builder {
    /** @return $this */
    public function active() { return $this; }
}
"#,
            ),
            (
                "src/Models/User.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(UserBuilder::class)]
class User extends Model {}
"#,
            ),
            (
                "src/Models/PostBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class PostBuilder extends Builder {
    /** @return $this */
    public function active() { return $this; }
}
"#,
            ),
            (
                "src/Models/Post.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(PostBuilder::class)]
class Post extends Model {}
"#,
            ),
            (
                "usage.php",
                r#"<?php
use App\Models\User;
use App\Models\Post;

User::active();
Post::active();
User::query()->active();
"#,
            ),
        ],
    );

    // Open all files to index them
    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/User.php",
        "src/Models/PostBuilder.php",
        "src/Models/Post.php",
        "usage.php",
    ] {
        let uri = Url::from_file_path(dir.path().join(path)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: std::fs::read_to_string(dir.path().join(path)).unwrap(),
                },
            })
            .await;
    }

    let usage_uri = Url::from_file_path(dir.path().join("usage.php")).unwrap();

    // Find references for User::active() usage (line 4, col 6)
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(usage_uri.clone()),
            position: Position::new(4, 6),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let locations = backend
        .references(params)
        .await
        .unwrap()
        .unwrap_or_default();

    let usage_locs: Vec<_> = locations.iter().filter(|l| l.uri == usage_uri).collect();
    let lines: Vec<_> = usage_locs.iter().map(|l| l.range.start.line).collect();

    assert!(
        lines.contains(&4),
        "Should find User::active(). Found at lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&6),
        "Should find User::query()->active(). Found at lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&5),
        "Should NOT find Post::active(). Found at lines: {:?}",
        lines
    );
    assert_eq!(
        usage_locs.len(),
        2,
        "Should find exactly 2 references in usage.php, but found: {:?}",
        lines
    );

    // Also should find the declaration in UserBuilder.php
    let builder_locs: Vec<_> = locations
        .iter()
        .filter(|l| l.uri.to_string().contains("UserBuilder.php"))
        .collect();
    assert_eq!(
        builder_locs.len(),
        1,
        "Should find declaration in UserBuilder.php"
    );
}

#[tokio::test]
async fn test_laravel_custom_builder_references_inherited() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "vendor/illuminate/" } } }"#,
        &[
            (
                "vendor/illuminate/Model.php",
                "<?php namespace Illuminate\\Database\\Eloquent; abstract class Model {
                public static function query() {}
            }",
            ),
            (
                "vendor/illuminate/Builder.php",
                "<?php namespace Illuminate\\Database\\Eloquent; class Builder {
                /** @return $this */
                public function where($c, $v = null) { return $this; }
            }",
            ),
            (
                "src/Models/UserBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class UserBuilder extends Builder {
    /** @return $this */
    public function active() { return $this; }
}
"#,
            ),
            (
                "src/Models/BaseModel.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(UserBuilder::class)]
abstract class BaseModel extends Model {}
"#,
            ),
            (
                "src/Models/Member.php",
                r#"<?php
namespace App\Models;
class Member extends BaseModel {}
"#,
            ),
            (
                "usage.php",
                r#"<?php
use App\Models\Member;

Member::active();
Member::query()->active();
"#,
            ),
        ],
    );

    // Open all files to index them
    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/BaseModel.php",
        "src/Models/Member.php",
        "usage.php",
    ] {
        let uri = Url::from_file_path(dir.path().join(path)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: std::fs::read_to_string(dir.path().join(path)).unwrap(),
                },
            })
            .await;
    }

    let builder_uri = Url::from_file_path(dir.path().join("src/Models/UserBuilder.php")).unwrap();

    // Find references for UserBuilder::active() declaration (line 5, col 21)
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(builder_uri),
            position: Position::new(5, 21),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let locations = backend
        .references(params)
        .await
        .unwrap()
        .unwrap_or_default();

    let usage_uri = Url::from_file_path(dir.path().join("usage.php")).unwrap();
    let usage_locs: Vec<_> = locations.iter().filter(|l| l.uri == usage_uri).collect();
    let lines: Vec<_> = usage_locs.iter().map(|l| l.range.start.line).collect();

    assert!(
        lines.contains(&3),
        "Should find Member::active(). Found at lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&4),
        "Should find Member::query()->active(). Found at lines: {:?}",
        lines
    );
    assert_eq!(
        usage_locs.len(),
        2,
        "Should find exactly 2 references in usage.php"
    );
}
