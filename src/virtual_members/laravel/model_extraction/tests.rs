use std::collections::HashMap;

use crate::Backend;
use crate::atom::atom;

#[test]
fn casts_method_returning_a_single_line_array_keeps_its_last_entry() {
    let src = r#"<?php
class User {
protected function casts(): array {
    return ['nickname' => 'string', 'is_admin' => 'boolean'];
}
}
"#;
    let classes = Backend::parse_php_versioned_with_namespaces(src, None);
    let laravel = classes[0].0.laravel().unwrap();
    assert_eq!(
        laravel.casts_definitions,
        [
            ("nickname".to_string(), "string".to_string()),
            ("is_admin".to_string(), "boolean".to_string()),
        ]
    );
}

#[test]
fn class_based_cast_calls_keep_their_method_and_arguments() {
    let src = r#"<?php
class User {
protected function casts(): array {
    return [
        'statuses' => AsEnumCollection::of(OrderStatus::class),
        'flavors' => AsEnumArrayObject::of(JamFlavor::class),
        'options' => AsArrayObject::class,
        'tags' => AsCollection::class,
        'toppings' => AsCollection::of(Frosting::class),
        'notes' => AsCollection::using(PostCollection::class),
        'mapped' => AsCollection::using(PostCollection::class, Frosting::class),
        'secrets' => AsEncryptedCollection::of(Frosting::class),
        'directory' => AsStringable::class,
        'uuid' => AsBinary::uuid(),
        'ulid' => AsBinary::ulid(),
        'binary' => AsBinary::of('uuid'),
        'address' => Address::class.':nullable',
        'joined' => AsEnumCollection::class.':'.OrderStatus::class,
    ];
}
}
"#;
    let classes = Backend::parse_php_versioned_with_namespaces(src, None);
    let laravel = classes[0].0.laravel().unwrap();
    assert_eq!(
        laravel.casts_definitions,
        [
            (
                "statuses".to_string(),
                "AsEnumCollection::of:OrderStatus".to_string()
            ),
            (
                "flavors".to_string(),
                "AsEnumArrayObject::of:JamFlavor".to_string()
            ),
            ("options".to_string(), "AsArrayObject".to_string()),
            ("tags".to_string(), "AsCollection".to_string()),
            (
                "toppings".to_string(),
                "AsCollection::of:Frosting".to_string()
            ),
            (
                "notes".to_string(),
                "AsCollection::using:PostCollection".to_string()
            ),
            (
                "mapped".to_string(),
                "AsCollection::using:PostCollection,Frosting".to_string()
            ),
            (
                "secrets".to_string(),
                "AsEncryptedCollection::of:Frosting".to_string()
            ),
            ("directory".to_string(), "AsStringable".to_string()),
            ("uuid".to_string(), "AsBinary::uuid".to_string()),
            ("ulid".to_string(), "AsBinary::ulid".to_string()),
            ("binary".to_string(), "AsBinary::of:uuid".to_string()),
            ("address".to_string(), "Address:nullable".to_string()),
            (
                "joined".to_string(),
                "AsEnumCollection:OrderStatus".to_string()
            ),
        ]
    );
}

#[test]
fn laravel_model_table_and_connection_attributes_are_extracted() {
    let src = r#"<?php
use Illuminate\Database\Eloquent\Attributes\Connection;
use Illuminate\Database\Eloquent\Attributes\Table;

#[Connection('analytics')]
#[Table('event_records')]
class EventRecord {}
"#;
    let classes = Backend::parse_php_versioned_with_namespaces(src, None);
    let class = classes
        .iter()
        .find(|(c, _)| c.name == atom("EventRecord"))
        .map(|(c, _)| c)
        .unwrap();
    let laravel = class.laravel().unwrap();
    assert_eq!(laravel.connection_name.as_deref(), Some("analytics"));
    assert_eq!(laravel.table_name.as_deref(), Some("event_records"));
}

#[test]
fn local_connection_and_table_attributes_are_ignored() {
    let src = r#"<?php
namespace App\Models;

#[Connection('analytics')]
#[Table('event_records')]
class EventRecord {}
"#;
    let classes = Backend::parse_php_versioned_with_namespaces(src, None);
    let class = classes
        .iter()
        .find(|(c, _)| c.name == atom("EventRecord"))
        .map(|(c, _)| c)
        .unwrap();
    let laravel = class.laravel().unwrap();
    assert_eq!(laravel.connection_name, None);
    assert_eq!(laravel.table_name, None);
}

#[test]
fn laravel_model_get_table_override_is_detected() {
    let src = r#"<?php
class ReportRow {
public function getTable(): string { return 'dynamic_' . date('Y'); }
public function getConnectionName(): string { return tenant_connection(); }
}
"#;
    let classes = Backend::parse_php_versioned_with_namespaces(src, None);
    let class = classes
        .iter()
        .find(|(c, _)| c.name == atom("ReportRow"))
        .map(|(c, _)| c)
        .unwrap();
    let laravel = class.laravel().unwrap();
    assert!(laravel.has_get_table_method);
    assert!(laravel.has_get_connection_name_method);
}

#[test]
fn laravel_model_primary_key_config_is_extracted() {
    let src = r#"<?php
class Passport {
protected $primaryKey = 'passport_number';
protected $keyType = 'string';
public function getKeyName(): string { return $this->primaryKey; }
}
"#;
    let classes = Backend::parse_php_versioned_with_namespaces(src, None);
    let class = classes
        .iter()
        .find(|(c, _)| c.name == atom("Passport"))
        .map(|(c, _)| c)
        .unwrap();
    let laravel = class.laravel().unwrap();
    assert_eq!(laravel.primary_key.as_deref(), Some("passport_number"));
    assert_eq!(laravel.key_type.as_deref(), Some("string"));
    assert!(laravel.has_get_key_name_method);
}

#[test]
fn laravel_factory_model_property_is_extracted_and_resolved() {
    let src = r#"<?php
namespace Database\Factories;

use App\Models\Draft as DraftModel;

class ClassConstantFactory {
protected $model = DraftModel::class;
}

class StringFactory {
protected $model = 'Domain\\Models\\PublishedDraft';
}

class DoubleQuotedStringFactory {
protected $model = "Domain\\Models\\PreviewDraft";
}

class DynamicFactory {
protected $model = model_name();
}

class InterpolatedFactory {
protected $model = "App\\Models\\$model";
}
"#;
    let mut classes: Vec<_> = Backend::parse_php_versioned_with_namespaces(src, None)
        .into_iter()
        .map(|(class, _)| class)
        .collect();
    let use_map = HashMap::from([("DraftModel".to_string(), "App\\Models\\Draft".to_string())]);
    Backend::resolve_parent_class_names(
        &mut classes,
        &use_map,
        &HashMap::new(),
        &Some("Database\\Factories".to_string()),
    );

    let model_type = |class_name: &str| {
        classes
            .iter()
            .find(|class| class.name == atom(class_name))
            .and_then(|class| class.laravel())
            .and_then(|laravel| laravel.factory_model.as_ref())
            .map(ToString::to_string)
    };

    assert_eq!(
        model_type("ClassConstantFactory").as_deref(),
        Some("App\\Models\\Draft")
    );
    assert_eq!(
        model_type("StringFactory").as_deref(),
        Some("Domain\\Models\\PublishedDraft")
    );
    assert_eq!(
        model_type("DoubleQuotedStringFactory").as_deref(),
        Some("Domain\\Models\\PreviewDraft")
    );
    assert_eq!(model_type("DynamicFactory"), None);
    assert_eq!(model_type("InterpolatedFactory"), None);
}

#[test]
fn laravel_factory_model_property_resolves_supported_class_name_forms() {
    let src = r#"<?php
namespace Database\Factories;

use App\Models as Models;

class LocalDraft {}

class NamespaceRelativeFactory {
public function definition(): array { return []; }

protected $unused = null, $model = LocalDraft::CLASS;
}

class QualifiedImportFactory {
protected $model = Models\QualifiedDraft::class;
}

class FullyQualifiedFactory {
protected $model = \App\Models\AbsoluteDraft::class;
}

class AbsoluteStringFactory {
protected $model = '\\Domain\\Models\\AbsoluteDraft';
}

class BinaryStringFactory {
protected $model = b'Domain\\Models\\BinaryDraft';
}

class EscapedDoubleQuotedStringFactory {
protected $model = "\x44omain\\Models\\EscapedDraft";
}
"#;
    let mut classes: Vec<_> = Backend::parse_php_versioned_with_namespaces(src, None)
        .into_iter()
        .map(|(class, _)| class)
        .collect();
    let use_map = HashMap::from([("Models".to_string(), "App\\Models".to_string())]);
    Backend::resolve_parent_class_names(
        &mut classes,
        &use_map,
        &HashMap::new(),
        &Some("Database\\Factories".to_string()),
    );

    let model_type = |class_name: &str| {
        classes
            .iter()
            .find(|class| class.name == atom(class_name))
            .and_then(|class| class.laravel())
            .and_then(|laravel| laravel.factory_model.as_ref())
            .map(ToString::to_string)
    };

    assert_eq!(
        model_type("NamespaceRelativeFactory").as_deref(),
        Some("Database\\Factories\\LocalDraft")
    );
    assert_eq!(
        model_type("QualifiedImportFactory").as_deref(),
        Some("App\\Models\\QualifiedDraft")
    );
    assert_eq!(
        model_type("FullyQualifiedFactory").as_deref(),
        Some("App\\Models\\AbsoluteDraft")
    );
    assert_eq!(
        model_type("AbsoluteStringFactory").as_deref(),
        Some("Domain\\Models\\AbsoluteDraft")
    );
    assert_eq!(
        model_type("BinaryStringFactory").as_deref(),
        Some("Domain\\Models\\BinaryDraft")
    );
    assert_eq!(
        model_type("EscapedDoubleQuotedStringFactory").as_deref(),
        Some("Domain\\Models\\EscapedDraft")
    );
}

#[test]
fn laravel_factory_model_property_ignores_non_model_initializers() {
    let src = r#"<?php
namespace Database\Factories;

class FactoryParent {}

class UninitializedFactory {
protected $model;
}

class EmptyStringFactory {
protected $model = '   ';
}

class SelfFactory {
protected $model = self::class;
}

class StaticFactory {
protected $model = static::class;
}

class ParentFactory extends FactoryParent {
protected $model = parent::class;
}

class OtherConstantFactory {
private const MODEL_CLASS = 'App\\Models\\Draft';
protected $model = self::MODEL_CLASS;
}

class UnrelatedPropertyFactory {
protected $notModel = \App\Models\Draft::class;
}

class StaticPropertyFactory {
protected static $model = \App\Models\Draft::class;
}

class NonUtf8StringFactory {
protected $model = "\xff";
}

class InvalidEscapeFactory {
protected $model = "\u{}";
}
"#;
    let mut classes: Vec<_> = Backend::parse_php_versioned_with_namespaces(src, None)
        .into_iter()
        .map(|(class, _)| class)
        .collect();
    Backend::resolve_parent_class_names(
        &mut classes,
        &HashMap::new(),
        &HashMap::new(),
        &Some("Database\\Factories".to_string()),
    );

    for class_name in [
        "UninitializedFactory",
        "EmptyStringFactory",
        "SelfFactory",
        "StaticFactory",
        "ParentFactory",
        "OtherConstantFactory",
        "UnrelatedPropertyFactory",
        "StaticPropertyFactory",
        "NonUtf8StringFactory",
        "InvalidEscapeFactory",
    ] {
        let class = classes
            .iter()
            .find(|class| class.name == atom(class_name))
            .expect("factory model fixture should parse");
        let model = class
            .laravel()
            .and_then(|laravel| laravel.factory_model.as_ref());
        assert!(model.is_none(), "{class_name} must not declare a model");
    }
}
