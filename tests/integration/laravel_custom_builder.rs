use crate::common::create_psr4_workspace;
use crate::common::{completion_response, goto_definition_at, open_php};
use tower_lsp_server::gen_lsp_types::*;

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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::act";
    open_php(&backend, &uri, content).await;

    // Test static call: User::active()
    let items = completion_response(&backend, &uri, 2, 9, None)
        .await
        .unwrap();
    let labels: Vec<_> = match items {
        CompletionResponse::CompletionItemList(arr) => arr.into_iter().map(|i| i.label).collect(),
        _ => panic!("Expected array"),
    };
    assert!(
        labels.iter().any(|l| l.starts_with("active")),
        "Should suggest custom builder method. Labels: {:?}",
        labels
    );

    // Test query() return type
    let uri2 = Uri::from_file_path(dir.path().join("test2.php")).unwrap();
    let content2 = "<?php\nuse App\\Models\\User;\nUser::query()->act";
    open_php(&backend, &uri2, content2).await;

    let items = completion_response(&backend, &uri2, 2, 18, None)
        .await
        .unwrap();
    let custom_items = match items {
        CompletionResponse::CompletionItemList(arr) => arr,
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
    let uri3 = Uri::from_file_path(dir.path().join("test3.php")).unwrap();
    let content3 = "<?php\nuse App\\Models\\User;\nUser::orWhereBet";
    open_php(&backend, &uri3, content3).await;

    let items = completion_response(&backend, &uri3, 2, 16, None)
        .await
        .unwrap();
    let labels: Vec<_> = match items {
        CompletionResponse::CompletionItemList(arr) => arr.into_iter().map(|i| i.label).collect(),
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::query()->whe";
    open_php(&backend, &uri, content).await;

    let items = completion_response(&backend, &uri, 2, 18, None)
        .await
        .unwrap();
    let items = match items {
        CompletionResponse::CompletionItemList(arr) => arr,
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::orWhereBetween('id', [1, 2]);";
    open_php(&backend, &uri, content).await;

    // Test GTD on orWhereBetween
    // On 'orWhereBetween'
    let resp = goto_definition_at(&backend, &uri, 2, 10).await;
    let locs = match resp {
        Some(DefinitionResponse::Definition(Definition::Location(l))) => vec![l],
        Some(DefinitionResponse::Definition(Definition::LocationList(a))) => a,
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::active();";
    open_php(&backend, &uri, content).await;

    // On 'active'
    let resp = goto_definition_at(&backend, &uri, 2, 8).await;
    let locs = match resp {
        Some(DefinitionResponse::Definition(Definition::Location(l))) => vec![l],
        Some(DefinitionResponse::Definition(Definition::LocationList(a))) => a,
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("src/Models/Controller.php")).unwrap();
    // On 'active' in Member::active();
    let resp = goto_definition_at(&backend, &uri, 4, 18).await;
    let locs = match resp {
        Some(DefinitionResponse::Definition(Definition::Location(l))) => vec![l],
        Some(DefinitionResponse::Definition(Definition::LocationList(a))) => a,
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::query()->whe";
    open_php(&backend, &uri, content).await;

    let items = completion_response(&backend, &uri, 2, 18, None)
        .await
        .unwrap();
    let items = match items {
        CompletionResponse::CompletionItemList(arr) => arr,
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

    let uri2 = Uri::from_file_path(dir.path().join("test2.php")).unwrap();
    let content2 = "<?php\nuse App\\Models\\User;\nUser::query()->where('id', 1);";
    open_php(&backend, &uri2, content2).await;

    let resp = goto_definition_at(&backend, &uri2, 2, 17).await;
    let locs = match resp {
        Some(DefinitionResponse::Definition(Definition::Location(l))) => vec![l],
        Some(DefinitionResponse::Definition(Definition::LocationList(a))) => a,
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
async fn test_custom_builder_two_levels_below_builder_binds_its_model() {
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
                "<?php namespace Illuminate\\Database\\Eloquent;
            /** @template TModel of Model */
            class Builder {
                /** @return TModel|null */
                public function first() {}
            }",
            ),
            (
                "src/Models/BaseBuilder.php",
                r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
class BaseBuilder extends Builder {}
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
class User extends Model {
    public function fullName(): string { return ''; }
}
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::query()->first()->full";
    open_php(&backend, &uri, content).await;

    let items = completion_response(&backend, &uri, 2, 27, None)
        .await
        .unwrap();
    let labels: Vec<_> = match items {
        CompletionResponse::CompletionItemList(arr) => arr.into_iter().map(|i| i.label).collect(),
        _ => panic!("Expected array"),
    };
    assert!(
        labels.iter().any(|l| l.starts_with("fullName")),
        "a builder two levels below Builder should still bind TModel to the model, \
         not degrade to the `TModel of Model` bound. Labels: {:?}",
        labels
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\User;\nUser::query()->whe";
    open_php(&backend, &uri, content).await;

    let items = completion_response(&backend, &uri, 2, 18, None)
        .await
        .unwrap();
    let labels: Vec<_> = match items {
        CompletionResponse::CompletionItemList(arr) => arr.into_iter().map(|i| i.label).collect(),
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\Member;\nMember::active();";
    open_php(&backend, &uri, content).await;

    // On 'active'
    let resp = goto_definition_at(&backend, &uri, 2, 8).await;
    let locs = match resp {
        Some(DefinitionResponse::Definition(Definition::Location(l))) => vec![l],
        Some(DefinitionResponse::Definition(Definition::LocationList(a))) => a,
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
        let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
        open_php(
            &backend,
            &uri,
            &std::fs::read_to_string(dir.path().join(path)).unwrap(),
        )
        .await;
    }

    let uri = Uri::from_file_path(dir.path().join("test.php")).unwrap();
    let content = "<?php\nuse App\\Models\\Member;\nMember::";
    open_php(&backend, &uri, content).await;

    let resp = completion_response(&backend, &uri, 2, 8, None).await;
    let items = match resp {
        Some(CompletionResponse::CompletionList(l)) => l.items,
        Some(CompletionResponse::CompletionItemList(a)) => a,
        None => Vec::new(),
    };
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"active()"),
        "Should contain 'active()' from inherited custom builder. Labels: {:?}",
        labels
    );
}
