use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_custom_eloquent_builder_attribute() {
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
                /** @return $this */
                public function orWhereBetween($c, array $v) { return $this; }
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
        ],
    );

    // Ensure all files are indexed
    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/User.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::act";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    // Test static call: User::active()
    let req = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            position: Position::new(2, 9),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items = backend.completion(req).await.unwrap().unwrap();
    let labels: Vec<_> = match items {
        CompletionResponse::Array(arr) => arr.into_iter().map(|i| i.label).collect(),
        _ => panic!("Expected array"),
    };
    assert!(
        labels.iter().any(|l| l.starts_with("active")),
        "Should suggest custom builder method. Labels: {:?}",
        labels
    );

    // Test query() return type
    let uri2 = Url::from_file_path(dir.path().join("test2.php")).unwrap();
    let content2 = "<?php\nuse App\\Models\\User;\nUser::query()->act";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri2.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content2.to_string(),
            },
        })
        .await;

    let req = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(uri2.clone()),
            position: Position::new(2, 18),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items = backend.completion(req).await.unwrap().unwrap();
    let custom_items = match items {
        CompletionResponse::Array(arr) => arr,
        _ => panic!("Expected array"),
    };
    let labels: Vec<_> = custom_items.iter().map(|i| i.label.clone()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("active")),
        "query() should return custom builder. Labels: {:?}",
        labels
    );
    let active = custom_items
        .iter()
        .find(|i| i.label.starts_with("active"))
        .expect("active completion should be present");
    assert_eq!(
        active
            .label_details
            .as_ref()
            .and_then(|d| d.description.as_deref()),
        Some("UserBuilder"),
        "custom builder methods should keep the custom builder label"
    );

    let where_item = custom_items
        .iter()
        .find(|i| i.label.starts_with("where"))
        .expect("where completion should be present");
    assert_eq!(
        where_item
            .label_details
            .as_ref()
            .and_then(|d| d.description.as_deref()),
        Some("Builder"),
        "inherited base builder methods should show their declaring builder, not the custom builder"
    );

    // Test orWhereBetween on model (forwarded from base Builder via custom builder)
    let uri3 = Url::from_file_path(dir.path().join("test3.php")).unwrap();
    let content3 = "<?php\nuse App\\Models\\User;\nUser::orWhereBet";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri3.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content3.to_string(),
            },
        })
        .await;

    let req = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(uri3.clone()),
            position: Position::new(2, 16),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items = backend.completion(req).await.unwrap().unwrap();
    let labels: Vec<_> = match items {
        CompletionResponse::Array(arr) => arr.into_iter().map(|i| i.label).collect(),
        _ => panic!("Expected array"),
    };
    assert!(
        labels.iter().any(|l| l.starts_with("orWhereBetween")),
        "Should suggest forwarded builder method. Labels: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_default_eloquent_builder_completion_label() {
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
                "src/Models/User.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class User extends Model {}
"#,
            ),
        ],
    );

    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/User.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::query()->whe";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let req = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(uri),
            position: Position::new(2, 18),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items = backend.completion(req).await.unwrap().unwrap();
    let items = match items {
        CompletionResponse::Array(arr) => arr,
        _ => panic!("Expected array"),
    };
    let where_item = items
        .iter()
        .find(|i| i.label.starts_with("where"))
        .expect("where completion should be present for default Eloquent Builder");
    assert_eq!(
        where_item
            .label_details
            .as_ref()
            .and_then(|d| d.description.as_deref()),
        Some("Builder")
    );
}

#[tokio::test]
async fn test_goto_definition_forwarded_builder_method() {
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
                public function orWhereBetween($c, array $v) { return $this; }
            }",
            ),
            (
                "src/Models/User.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class User extends Model {}
"#,
            ),
        ],
    );

    // Open Builder.php and User.php
    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/User.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::orWhereBetween('id', [1, 2]);";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    // Test GTD on orWhereBetween
    let req = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(uri),
            position: Position::new(2, 10), // On 'orWhereBetween'
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let resp = backend.goto_definition(req).await.unwrap();
    let locs = match resp {
        Some(GotoDefinitionResponse::Scalar(l)) => vec![l],
        Some(GotoDefinitionResponse::Array(a)) => a,
        None => panic!("Should have found a location for 'orWhereBetween'"),
        _ => panic!("Expected locations"),
    };

    assert!(
        !locs.is_empty(),
        "Should resolve orWhereBetween to Builder method"
    );
    let uri_res = locs[0].uri.to_string();
    assert!(
        uri_res.contains("Builder.php"),
        "Should point to Builder.php, got {}",
        uri_res
    );
    // orWhereBetween is on line 3 (0-indexed 2)
    assert_eq!(locs[0].range.start.line, 2);
}

#[tokio::test]
async fn test_goto_definition_custom_builder_static_call() {
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
        ],
    );

    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/User.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::active();";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let resp = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(2, 8), // On 'active'
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();
    let locs = match resp {
        Some(GotoDefinitionResponse::Scalar(l)) => vec![l],
        Some(GotoDefinitionResponse::Array(a)) => a,
        None => panic!("Should have found a location for 'active'"),
        _ => panic!("Expected locations"),
    };

    assert!(
        !locs.is_empty(),
        "Should resolve active to custom UserBuilder method"
    );
    let uri_res = locs[0].uri.to_string();
    assert!(
        uri_res.contains("UserBuilder.php"),
        "Should point to UserBuilder.php, got {}",
        uri_res
    );
    // active() is on line 5 (0-indexed 4)
    assert_eq!(locs[0].range.start.line, 5);
}

#[tokio::test]
async fn test_goto_definition_custom_builder_same_namespace_no_use() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\Models\\": "src/Models/", "Illuminate\\": "vendor/illuminate/" } } }"#,
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
                "src/Models/MemberBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class MemberBuilder extends Builder {
    /** @return $this */
    public function active() { return $this; }
}
"#,
            ),
            (
                "src/Models/Member.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(MemberBuilder::class)]
class Member extends Model {}
"#,
            ),
            (
                "src/Models/Controller.php",
                r#"<?php
namespace App\Models;
class Controller {
    public function index() {
        Member::active();
    }
}
"#,
            ),
        ],
    );

    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/MemberBuilder.php",
        "src/Models/Member.php",
        "src/Models/Controller.php",
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

    let uri = Url::from_file_path(dir.path().join("src/Models/Controller.php")).unwrap();
    let resp = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(4, 18), // On 'active' in Member::active();
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();
    let locs = match resp {
        Some(GotoDefinitionResponse::Scalar(l)) => vec![l],
        Some(GotoDefinitionResponse::Array(a)) => a,
        None => panic!("Should have found a location for 'active'"),
        _ => panic!("Expected locations"),
    };

    assert!(
        !locs.is_empty(),
        "Should resolve active to custom MemberBuilder method"
    );
    let uri_res = locs[0].uri.to_string();
    assert!(
        uri_res.contains("MemberBuilder.php"),
        "Should point to MemberBuilder.php, got {}",
        uri_res
    );
    assert_eq!(locs[0].range.start.line, 5);
}

#[tokio::test]
async fn test_custom_builder_inherited_override_beats_framework_builder() {
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
                "src/Models/BaseBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class BaseBuilder extends Builder {
    /** @return $this */
    public function where($c, $v = null) { return $this; }
}
"#,
            ),
            (
                "src/Models/UserBuilder.php",
                r#"<?php
namespace App\Models;
class UserBuilder extends BaseBuilder {}
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
        ],
    );

    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/BaseBuilder.php",
        "src/Models/UserBuilder.php",
        "src/Models/User.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::query()->whe";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let items = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(2, 18),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
        .unwrap();
    let items = match items {
        CompletionResponse::Array(arr) => arr,
        _ => panic!("Expected array"),
    };
    let where_item = items
        .iter()
        .find(|i| i.label.starts_with("where"))
        .expect("where completion should be present");
    assert_eq!(
        where_item
            .label_details
            .as_ref()
            .and_then(|d| d.description.as_deref()),
        Some("BaseBuilder"),
        "inherited custom builder override should keep its declaring builder"
    );

    let uri2 = Url::from_file_path(dir.path().join("test2.php")).unwrap();
    let content2 = "<?php\nuse App\\Models\\User;\nUser::query()->where('id', 1);";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri2.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content2.to_string(),
            },
        })
        .await;

    let resp = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri2),
                position: Position::new(2, 17),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();
    let locs = match resp {
        Some(GotoDefinitionResponse::Scalar(l)) => vec![l],
        Some(GotoDefinitionResponse::Array(a)) => a,
        None => panic!("Should have found a location for custom where"),
        _ => panic!("Expected locations"),
    };
    assert!(
        locs[0].uri.to_string().contains("BaseBuilder.php"),
        "Should point to BaseBuilder.php, got {}",
        locs[0].uri
    );
}

#[tokio::test]
async fn test_missing_custom_builder_falls_back_to_eloquent_builder_type() {
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
                "src/Models/User.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
#[UseEloquentBuilder(MissingBuilder::class)]
class User extends Model {}
"#,
            ),
        ],
    );

    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/User.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::query()->whe";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let items = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(2, 18),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
        .unwrap();
    let labels: Vec<_> = match items {
        CompletionResponse::Array(arr) => arr.into_iter().map(|i| i.label).collect(),
        _ => panic!("Expected array"),
    };
    assert!(
        labels.iter().any(|l| l.starts_with("where")),
        "missing custom builder should fall back to Builder<User>. Labels: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_goto_definition_custom_builder_inherited_attribute() {
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
        ],
    );

    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/BaseModel.php",
        "src/Models/Member.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\Member;\nMember::active();";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let resp = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(2, 8), // On 'active'
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();
    let locs = match resp {
        Some(GotoDefinitionResponse::Scalar(l)) => vec![l],
        Some(GotoDefinitionResponse::Array(a)) => a,
        None => panic!("Should have found a location for 'active'"),
        _ => panic!("Expected locations"),
    };

    assert!(
        !locs.is_empty(),
        "Should resolve active to custom UserBuilder method"
    );
    let uri_res = locs[0].uri.to_string();
    assert!(
        uri_res.contains("UserBuilder.php"),
        "Should point to UserBuilder.php, got {}",
        uri_res
    );
}

#[tokio::test]
async fn test_completion_custom_builder_inherited_attribute() {
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
        ],
    );

    for path in [
        "vendor/illuminate/Builder.php",
        "vendor/illuminate/Model.php",
        "src/Models/UserBuilder.php",
        "src/Models/BaseModel.php",
        "src/Models/Member.php",
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

    let uri = Url::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\Member;\nMember::";
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let resp = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(2, 8),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap();
    let items = match resp {
        Some(CompletionResponse::List(l)) => l.items,
        Some(CompletionResponse::Array(a)) => a,
        None => Vec::new(),
    };
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"active()"),
        "Should contain 'active()' from inherited custom builder. Labels: {:?}",
        labels
    );
}
