use super::super::catalog::{RuleClass, RuleDefinition};
use super::*;
use std::sync::Arc;

fn catalog() -> RuleCatalog {
    let mut catalog = RuleCatalog::default();
    catalog.complete = true;
    catalog.rules = ["required", "same", "required_if", "unique"]
        .into_iter()
        .map(|name| RuleDefinition {
            name: name.to_string(),
            description: String::new(),
            takes_parameters: false,
            location: None,
        })
        .collect();
    catalog.dependent_rules = ["same", "required_if", "unique"]
        .into_iter()
        .map(str::to_string)
        .collect();
    catalog
}

fn field(name: &str) -> Candidate {
    Candidate {
        name: name.to_string(),
        detail: String::new(),
        location: None,
        kind: CompletionItemKind::Field,
    }
}

fn selection(source: &str) -> Option<Selection> {
    let cursor = source.find('§').unwrap();
    let content = source.replace('§', "");
    let site = super::super::collect(&content)
        .into_iter()
        .find(|site| site.start <= cursor && cursor <= site.end)?;
    selection_at(&site, &content, cursor, &catalog())
}

#[test]
fn csv_parameter_ranges_preserve_quotes_commas_and_escaped_enclosures() {
    for (source, values) in [
        ("one,two,", vec!["one", "two", ""]),
        (
            r#"one,"alpha,beta",tail"#,
            vec!["one", "alpha,beta", "tail"],
        ),
        (r#" "a""b",c"#, vec!["a\"b", "c"]),
        (r#""a\"b",c"#, vec![r#"a\"b"#, "c"]),
        (r#""part,done"#, vec!["part,done"]),
        (r#""quoted"suffix,tail"#, vec!["quotedsuffix", "tail"]),
        ("", vec![""]),
    ] {
        let actual: Vec<_> = csv_ranges(source)
            .into_iter()
            .map(|(start, end)| csv_value(&source[start..end]).into_owned())
            .collect();
        assert_eq!(actual, values, "{source}");
    }
}

#[test]
fn csv_parameter_encoding_keeps_representable_enum_values() {
    for value in [
        "alpha,beta",
        "one,\"two",
        "\"quoted\"",
        "",
        "a\nb",
        "a\rb",
        "ordinary",
        r"a\b",
        r#"a\"b"#,
    ] {
        let encoded = csv_parameter(value).expect(value);
        assert_eq!(csv_ranges(&encoded).len(), 1);
        assert_eq!(csv_value(&encoded), value);
    }
    assert!(matches!(
        csv_parameter("ordinary"),
        Some(Cow::Borrowed("ordinary"))
    ));
    assert_eq!(csv_parameter("alpha,beta").unwrap(), "\"alpha,beta\"");
    assert!(csv_parameter("a,b\\").is_none());
    assert!(csv_parameter("a,b\\\"c").is_none());
}

#[test]
fn escaped_csv_values_share_completion_hover_and_definition_ranges() {
    for source in [
        r#"<?php $r->validate(['value' => 'in:other,"alpha,§beta",tail']);"#,
        r#"<?php $r->validate(['value' => "in:other,\"alpha,§beta\",tail"]);"#,
        r#"<?php $r->validate(['value' => "in:other,\x22alpha,§beta\x22,tail"]);"#,
    ] {
        let cursor = source.find('§').unwrap();
        let content = source.replace('§', "");
        let site = super::super::collect(&content).remove(0);
        let selected = selection_at(&site, &content, cursor, &catalog()).unwrap();
        assert!(matches!(selected.kind, SelectionKind::Enum));
        assert_eq!(
            selection_value(&site, &selected, &content, cursor).unwrap(),
            "alpha,"
        );
        assert_eq!(
            selection_value(&site, &selected, &content, selected.end).unwrap(),
            "alpha,beta"
        );
        assert_eq!(&content[selected.end..selected.end + 5], ",tail");
    }
}

#[test]
fn wildcard_field_patterns_match_segments_without_crossing_dots() {
    for (left, right) in [
        ("email", "email"),
        ("*", "email"),
        ("user.*", "user.email"),
        ("items.*.name", "items.3.name"),
        ("items.3.name", "items.*.name"),
        ("user_*_id", "user_primary_id"),
        ("a*b*c", "a*bc"),
        ("prefix*suffix", "prefix*mid*suffix"),
        ("*a*b*", "*b*a*"),
        ("pre*mid*end", "premiddleend"),
    ] {
        assert!(field_matches(left, right), "{left}, {right}");
    }
    for (left, right) in [
        ("email", "name"),
        ("user.*", "user.email.address"),
        ("items.*.name", "items.name"),
        ("user_*_id", "member_primary_id"),
        ("pre*mid*end", "premissingend"),
        ("a*b", "a*c"),
        ("a*", "b*"),
        ("*suffix", "wrong"),
    ] {
        assert!(!field_matches(left, right), "{left}, {right}");
    }
}

#[test]
fn message_and_attribute_keys_accept_wildcards_and_rule_names() {
    let catalog = catalog();
    let fields = [field("email"), field("items.*.name")];
    for key in [
        "email",
        "email.required",
        "*.required",
        "required",
        "items.0.name.required",
        "items.*.name.req*",
    ] {
        assert!(
            message_key_known(key, SiteKind::MessageKey, &fields, &catalog),
            "{key}"
        );
    }
    for key in ["email", "*", "items.0.name", "items.*.name"] {
        assert!(
            message_key_known(key, SiteKind::AttributeKey, &fields, &catalog),
            "{key}"
        );
    }
    for key in ["emial.required", "email.requried", "items.*.nam.required"] {
        assert!(
            !message_key_known(key, SiteKind::MessageKey, &fields, &catalog),
            "{key}"
        );
    }
    assert!(!message_key_known(
        "email.required",
        SiteKind::AttributeKey,
        &fields,
        &catalog
    ));
}

#[test]
fn custom_rule_messages_and_open_catalogs_are_recognized() {
    let mut catalog = catalog();
    catalog.classes.push(RuleClass {
        fqn: "App\\Rules\\UppercaseWord".to_string(),
        description: String::new(),
    });
    let fields = [field("name")];
    assert!(message_key_known(
        "name.uppercase_word",
        SiteKind::MessageKey,
        &fields,
        &catalog
    ));
    assert!(message_key_known(
        "name.App\\Rules\\UppercaseWord",
        SiteKind::MessageKey,
        &fields,
        &catalog
    ));
    catalog.complete = false;
    assert!(message_key_known(
        "registered_at_runtime",
        SiteKind::MessageKey,
        &fields,
        &catalog
    ));
    assert!(message_key_known(
        "name.registered_at_runtime",
        SiteKind::MessageKey,
        &fields,
        &catalog
    ));
    assert!(!message_key_known(
        "missing.required",
        SiteKind::MessageKey,
        &fields,
        &catalog
    ));
}

#[test]
fn concrete_field_prefixes_keep_declared_wildcards_navigable() {
    assert_eq!(
        concrete_field_name("items.*.name", "items.0.na"),
        "items.0.name"
    );
    assert_eq!(
        concrete_field_name("items.*.name", "items."),
        "items.*.name"
    );
    assert_eq!(concrete_field_name("email", "em"), "email");
    assert_eq!(
        message_field_name("items.*.name", "items.0.name.req"),
        "items.0.name"
    );
    assert_eq!(message_field_name("email", "*.req"), "*");
    assert_eq!(message_field_name("email", "em"), "email");
    assert_eq!(message_field_name("email", "other.req"), "email");
}

#[test]
fn parameter_roles_use_laravel_rule_name_normalization() {
    assert!(matches!(
        selection("<?php $r->validate(['email' => 'Exists:u§sers']);")
            .unwrap()
            .kind,
        SelectionKind::Table
    ));
    assert!(
        matches!(selection("<?php $r->validate(['email' => 'UNIQUE:users,em§ail']);").unwrap().kind, SelectionKind::Column(table) if table == "users")
    );
    assert!(matches!(
        selection("<?php $r->validate(['email' => 'NotIn:pe§nding']);")
            .unwrap()
            .kind,
        SelectionKind::Enum
    ));
    assert!(matches!(
        selection("<?php $r->validate(['email' => 'Same:pass§word']);")
            .unwrap()
            .kind,
        SelectionKind::Field
    ));
    assert!(matches!(
        selection("<?php $r->validate(['email' => 'RequiredIf:sta§tus,active']);")
            .unwrap()
            .kind,
        SelectionKind::Field
    ));
    for source in [
        "<?php $r->validate(['email' => 'unique:users,email,1§23,id']);",
        "<?php $r->validate(['email' => 'unique:users,email,123,i§d']);",
        "<?php $r->validate(['email' => 'RequiredIf:status,ac§tive']);",
        "<?php $r->validate(['email' => 'min:1§0']);",
    ] {
        assert!(selection(source).is_none(), "{source}");
    }
}

#[test]
fn selections_cover_only_the_edited_part() {
    for source in [
        "<?php class R { function messages() { return ['email.req§uired' => 'Message']; } }",
        "<?php class R { function attributes() { return ['em§ail' => 'Name']; } }",
        "<?php $r->validate(['status' => [Rule::in(['pen§ding'])]]);",
        "<?php $r->validate(['email' => 'req§uired|email']);",
    ] {
        let cursor = source.find('§').unwrap();
        let selection = selection(source).unwrap();
        assert!(selection.start <= cursor && selection.end > cursor);
        assert_eq!(selection.prefix_end, cursor);
    }
}

fn rules_source(source: &str) -> Option<String> {
    let cursor = source.find('§').unwrap();
    let content = source.replace('§', "");
    let end = content[cursor..]
        .find("->sometimes")
        .map(|offset| cursor + offset)
        .unwrap();
    let mut context = FileContext::default();
    context.use_map.insert(
        "Validator".to_string(),
        "Illuminate\\Support\\Facades\\Validator".to_string(),
    );
    context.use_map.insert(
        "Checks".to_string(),
        "Illuminate\\Support\\Facades\\Validator".to_string(),
    );
    validator_rules_range(&content, cursor, end, &context)
        .map(|(start, end)| content[start..end].to_string())
}

#[test]
fn sometimes_reads_the_selected_validator_assignment() {
    let source = "<?php function check() { $first = Validator::make([], ['password' => 'required']); $second = Validator::make([], ['other' => 'string']); §$first->sometimes('name', 'same:password', fn () => true); }";
    assert_eq!(
        rules_source(source).as_deref(),
        Some("['password' => 'required']")
    );
    let source = "<?php $v = Checks::make(rules: ['email' => 'email'], data: []); §$v->sometimes('name', 'required', fn () => true);";
    assert_eq!(
        rules_source(source).as_deref(),
        Some("['email' => 'email']")
    );
    let source = "<?php §Validator::make([], ['email' => 'email'])->sometimes('name', 'required', fn () => true);";
    assert_eq!(
        rules_source(source).as_deref(),
        Some("['email' => 'email']")
    );
}

#[test]
fn validator_source_respects_reassignment_and_variable_scopes() {
    for source in [
        "<?php $v = Validator::make([], ['old' => 'string']); $v = custom_validator(); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = Validator::make([], ['old' => 'string']); if ($condition) { $v = Validator::make([], ['maybe' => 'string']); } §$v->sometimes('name', 'required', fn () => true);",
        "<?php function first() { $v = Validator::make([], ['other' => 'string']); } function second($v) { §$v->sometimes('name', 'required', fn () => true); }",
        "<?php $v = Validator::make([], ['outer' => 'string']); $closure = function ($v) { §$v->sometimes('name', 'required', fn () => true); };",
        "<?php $v = Other::make([], ['other' => 'string']); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = Validator::other([], ['other' => 'string']); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = $factory->make([], ['other' => 'string']); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = Validator::$factory([], ['other' => 'string']); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = $factory::make([], ['other' => 'string']); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = Validator::make(data: []); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = Validator::make([], ...$rules); §$v->sometimes('name', 'required', fn () => true);",
        "<?php $v = Validator::make([], ['outer' => 'string']); $closure = fn ($v) => §$v->sometimes('name', 'required', fn () => true);",
    ] {
        assert!(rules_source(source).is_none(), "{source}");
    }
    let source = "<?php $v = Validator::make([], ['outer' => 'string']); $closure = function () { $v = Validator::make([], ['inner' => 'string']); }; §$v->sometimes('name', 'required', fn () => true);";
    assert_eq!(
        rules_source(source).as_deref(),
        Some("['outer' => 'string']")
    );
    let source = "<?php $v = Validator::make([], ['old' => 'string']); $v = Validator::make([], ['current' => 'string']); §$v->sometimes('name', 'required', fn () => true);";
    assert_eq!(
        rules_source(source).as_deref(),
        Some("['current' => 'string']")
    );
}

#[test]
fn models_match_the_requested_connection_and_explicit_class_name() {
    let mut model = crate::test_fixtures::make_class("App\\Models\\Customer");
    model.laravel_mut().table_name = Some("customers".to_string());
    assert!(matches_table(&model, "customers", "mysql"));
    assert!(matches_table(&model, "mysql.customers", "mysql"));
    assert!(!matches_table(&model, "analytics.customers", "mysql"));
    model.laravel_mut().connection_name = Some("analytics".to_string());
    assert!(!matches_table(&model, "customers", "mysql"));
    assert!(matches_table(&model, "analytics.customers", "mysql"));
    assert!(matches_table(&model, "App\\Models\\Customer", "mysql"));
    assert!(matches_table(
        &model,
        "mysql.\\App\\Models\\Customer",
        "mysql"
    ));
    model.laravel_mut().connection_name = None;
    model.laravel_mut().has_get_connection_name_method = true;
    assert!(!matches_table(&model, "customers", "mysql"));
    assert!(model_connection(&ClassInfo::default(), "mysql").is_none());
}

#[test]
fn column_candidates_exclude_relationship_properties_and_guarded_wildcards() {
    let mut model = crate::test_fixtures::make_class("Customer");
    let relation = PropertySource::Relationship {
        method: "orders".to_string(),
        kind: "HasMany".to_string(),
        pivot_accessor: None,
        pivot_using: None,
        pivot_columns: Vec::new(),
    };
    for (name, source) in [
        ("orders", relation),
        (
            "orders_count",
            PropertySource::RelationshipCount {
                relationship: "orders".to_string(),
            },
        ),
        ("pivot", PropertySource::Pivot),
    ] {
        let mut property = crate::types::PropertyInfo::virtual_property(name, None);
        property.source = Some(source);
        model.properties.push(Arc::new(property));
    }
    assert!(!is_model_column(&model, "*"));
    assert!(!is_model_column(&model, "orders"));
    assert!(!is_model_column(&model, "orders_count"));
    assert!(!is_model_column(&model, "pivot"));
    assert!(is_model_column(&model, "email"));
    model
        .laravel_mut()
        .casts_definitions
        .push(("orders_count".to_string(), "integer".to_string()));
    assert!(is_model_column(&model, "orders_count"));
    model.laravel_mut().attributes_definitions.push((
        "pivot".to_string(),
        crate::php_type::PhpType::parse("string"),
    ));
    assert!(is_model_column(&model, "pivot"));
}

#[test]
fn message_candidates_only_expand_fields_matching_the_selected_prefix() {
    let backend = Backend::new_test();
    let uri = "file:///validation-message-fields.php";
    let content = "<?php class ProfileRequest { function rules() { return ['email' => 'required', 'name' => 'string']; } function messages() { return ['email.req' => 'Required']; } }";
    backend.update_ast(uri, content);
    backend.laravel.string_key_cache.write().validation_rules = Some(Arc::new(catalog()));
    let ctx = backend.file_context(uri);
    let site = super::super::collect(content)
        .into_iter()
        .find(|site| site.kind == SiteKind::MessageKey)
        .unwrap();
    let selected = selection_at(&site, content, site.end, &catalog()).unwrap();
    let candidates = candidates_at(&backend, uri, content, &ctx, &site, &selected);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["email", "email.required", "email.required_if"]
    );
}

#[test]
fn message_keys_do_not_become_fields_when_rules_are_unavailable() {
    let backend = Backend::new_test();
    for method in ["messages", "attributes"] {
        let content = format!(
            "<?php class Request {{ function {method}() {{ return ['emial' => 'Email']; }} }}"
        );
        let site = super::super::collect(&content).remove(0);
        let ctx = FileContext {
            classes: backend
                .parse_php_resolved(&content)
                .into_iter()
                .map(Arc::new)
                .collect(),
            ..FileContext::default()
        };
        let (fields, complete) = field_candidates(
            &backend,
            "file:///validation-no-rules.php",
            &content,
            &ctx,
            &site,
        );
        assert!(fields.is_empty());
        assert!(!complete);
    }
}

#[test]
fn model_table_candidates_do_not_invent_unknown_connections_or_source_locations() {
    let backend = Backend::new_test();
    let mut model = crate::test_fixtures::make_class("Customer");
    model.laravel_mut().table_name = Some("customers".to_string());
    let mut catalog = catalog();
    catalog.models.push(Arc::new(model));
    let mut tenant_model = crate::test_fixtures::make_class("TenantCustomer");
    tenant_model.laravel_mut().has_get_connection_name_method = true;
    catalog.models.push(Arc::new(tenant_model));
    let mut dynamic_table = crate::test_fixtures::make_class("DynamicCustomer");
    dynamic_table.laravel_mut().has_get_table_method = true;
    catalog.models.push(Arc::new(dynamic_table));
    let candidates = table_candidates(&backend, &catalog);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["customers"]
    );
    backend.laravel.string_key_cache.write().validation_rules = Some(Arc::new(catalog));
    for kind in [
        SelectionKind::Table,
        SelectionKind::Column("customers".to_string()),
        SelectionKind::Rule,
    ] {
        let selection = Selection {
            start: 0,
            end: 0,
            prefix_end: 0,
            kind,
        };
        assert!(database_definition(&backend, &selection, "customers").is_none());
    }
}

#[test]
fn enum_candidates_require_a_known_field_and_rules_array() {
    let backend = Backend::new_test();
    for content in [
        "<?php $r->validate([$field => 'in:']);",
        "<?php $validator->sometimes('status', 'in:', fn () => true);",
    ] {
        let site = super::super::collect(content).remove(0);
        assert!(
            enum_candidates(
                &backend,
                content,
                &FileContext::default(),
                &site,
                &catalog()
            )
            .is_none()
        );
    }
}

#[test]
fn incomplete_enum_value_candidates_use_the_repaired_rules_metadata() {
    let backend = Backend::new_test();
    let definitions = "<?php use Illuminate\\Validation\\Rule; enum Status: string { case Pending = 'pending'; } ";
    let ctx = FileContext {
        classes: backend
            .parse_php_resolved(definitions)
            .into_iter()
            .map(Arc::new)
            .collect(),
        ..FileContext::default()
    };
    for rule in ["'in:pen", "Rule::in(['pen"] {
        let content =
            format!("{definitions} $r->validate(['status' => [Rule::enum(Status::class), {rule}");
        let cursor = content.len();
        let code =
            crate::completion::source::code_context::code_context_at(&content, cursor).unwrap();
        let site = super::super::at_cursor(&content, cursor, &code).unwrap();
        let candidates = enum_candidates(&backend, &content, &ctx, &site, &catalog()).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.name.as_str())
                .collect::<Vec<_>>(),
            ["pending"]
        );
        assert_eq!(candidates[0].detail, "Status::Pending");
    }
}

#[test]
fn enum_candidates_only_offer_known_string_and_integer_backing_values() {
    let backend = Backend::new_test();
    let content = r#"<?php
        use Illuminate\Validation\Rule;
        enum Status: string {
            case Text = 'pending';
            case Number = 42;
            case InProgress;
            case Computed = 'pen' . 'ding';
            case Missing = UNKNOWN_VALUE;
            case Fraction = 1.25;
        }
        $r->validate(['status' => ['required', Rule::enum(Status::class), 'in:']]);
    "#;
    let ctx = FileContext {
        classes: backend
            .parse_php_resolved(content)
            .into_iter()
            .map(Arc::new)
            .collect(),
        ..FileContext::default()
    };
    let site = super::super::collect(content)
        .into_iter()
        .find(|site| &content[site.start..site.end] == "in:")
        .unwrap();
    let candidates = enum_candidates(&backend, content, &ctx, &site, &catalog()).unwrap();
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["pending", "42"]
    );
}
