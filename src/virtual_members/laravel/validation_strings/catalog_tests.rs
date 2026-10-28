use super::*;

const URI: &str = "file:///app/Rules.php";

fn registrations(uri: &str, content: &str) -> Registrations {
    super::registrations(uri, content, |_| false, |_| None)
}

#[test]
fn validation_catalog_custom_methods_use_positional_parameters() {
    let source = r#"<?php
class CustomValidator {
    public function validatePostalCode($field, $value, $options) {}
    public function validateCountry($field, $value) {}
    public function VALIDATEregion($field, $value, $arguments) {}
    public function validateFlexible(...$arguments) {}
    public function validateWithBag($bag) {}
    public function validated($key = null, $default = null) {}
    public function validate() {}
    protected function validatePrivate($field, $value, $options) {}
    public function unrelatedMethod($field, $value, $options) {}
}
"#;
    let classes = Backend::parse_php_versioned(source, None);
    let rules: Vec<_> = classes[0]
        .methods
        .iter()
        .filter_map(|method| method_rule(method, None))
        .collect();
    assert_eq!(
        rules
            .iter()
            .map(|rule| (rule.name.as_str(), rule.takes_parameters))
            .collect::<Vec<_>>(),
        [
            ("postal_code", true),
            ("country", false),
            ("region", true),
            ("flexible", true)
        ]
    );
}

#[test]
fn validation_catalog_methods_follow_the_installed_signature() {
    let source = r#"<?php
trait ValidatesAttributes {
    /** Validate a postal code. */
    public function validatePostalCode($attribute, $value, $parameters) {}
    public function validateULID($attribute, $value) {}
    public function validateBail() {}
    protected function validateAttribute($attribute, $rule) {}
    public function validateWithBag($errorBag) {}
    public function validate() {}
    public function validated() {}
    public function unrelated() {}
}
"#;
    let classes = Backend::parse_php_versioned(source, None);
    let rules: Vec<_> = classes[0]
        .methods
        .iter()
        .filter_map(|method| method_rule(method, Some((URI, source))))
        .collect();
    assert_eq!(
        rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        ["postal_code", "ulid", "bail"]
    );
    assert!(rules[0].takes_parameters);
    assert!(!rules[1].takes_parameters);
    assert_eq!(rules[0].description, "Validate a postal code.");
    assert_eq!(rules[0].location.as_ref().unwrap().range.start.line, 3);
    assert!(
        method_rule(&classes[0].methods[0], None)
            .unwrap()
            .location
            .is_none()
    );
    assert!(declaration_location("not a URI", source, 0).is_none());
}

#[test]
fn validation_catalog_extensions_resolve_aliases_and_namespaces() {
    let source = r#"<?php
namespace App {
    use Illuminate\Support\Facades\{Validator as Rules, Route};
    Rules::extend('postal_code', fn ($attribute, $value, $parameters) => true);
    Rules::extendImplicit(extension: fn () => true, rule: 'remote_id');
    Rules::extendDependent('matches_sibling', fn () => true);
    Rules::EXTEND('upper_call', fn () => true);
    Rules::extend('', fn () => true);
    Rules::make([], []);
    Rules::$method('unknown', fn () => true);
    $target::extend('unknown', fn () => true);
    Validator::extend('unimported', fn () => true);
}
namespace Other {
    use App\Validator;
    Validator::extend('unrelated', fn () => true);
    \Illuminate\Support\Facades\Validator::extend('global_rule', fn () => true);
}
"#;
    let found = registrations(URI, source);
    assert_eq!(
        found
            .rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        [
            "postal_code",
            "remote_id",
            "matches_sibling",
            "upper_call",
            "global_rule"
        ]
    );
    assert_eq!(found.dependent, ["matches_sibling"]);
    assert!(!found.dynamic);
    assert!(found.rules.iter().all(|rule| rule.takes_parameters));
    let location = found.rules[0].location.as_ref().unwrap();
    assert_eq!(location.range.start.line, 3);
    assert_eq!(location.range.start.character, 19);
}

#[test]
fn validation_catalog_dynamic_extensions_keep_the_set_open() {
    for call in [
        "extend($name, fn () => true)",
        "extend()",
        "extend(extension: fn () => true)",
        "extendImplicit($name, fn () => true)",
        "extendDependent($name, fn () => true)",
    ] {
        let source =
            format!("<?php use Illuminate\\Support\\Facades\\Validator; Validator::{call};");
        let found = registrations(URI, &source);
        assert!(found.dynamic, "{call}");
        assert!(found.rules.is_empty());
    }
    assert!(!registrations(URI, "<?php $nothing = 1;").dynamic);
    assert!(
        !registrations(
            URI,
            "<?php use App\\Validator; Validator::extend($name, fn () => true);"
        )
        .dynamic
    );
}

#[test]
fn validation_catalog_aliases_come_from_the_rule_parser() {
    let source = r#"<?php
class ValidationRuleParser {
    protected static function normalizeRule($rule) {
        return match ($rule) {
            'Int' => 'Integer',
            'Bool', 'BoolValue' => 'Boolean',
            $dynamic => 'String',
            'Computed' => someRule(),
            default => $rule,
        };
    }
    public static function unrelated($rule) {
        return match ($rule) { 'Wrong' => 'Integer' };
    }
}
"#;
    assert_eq!(
        rule_aliases(source),
        [
            ("int".into(), "integer".into()),
            ("bool".into(), "boolean".into()),
            ("bool_value".into(), "boolean".into())
        ]
    );
    assert!(
        rule_aliases(
            "<?php abstract class Parser { abstract public function normalizeRule($rule); }"
        )
        .is_empty()
    );
}

#[test]
fn validation_catalog_dependent_rules_follow_the_framework_property() {
    let source = r#"<?php
class Validator {
    protected $unrelated = ['Other'];
    protected $dependentRules = ['Same', 'RequiredWith', 'BeforeOrEqual'];
}
"#;
    let classes = Backend::parse_php_versioned(source, None);
    assert_eq!(
        dependent_rules(&classes[0]),
        ["same", "required_with", "before_or_equal"]
    );
    assert!(dependent_rules(&ClassInfo::default()).is_empty());
    let source = "<?php class Validator { protected $dependentRules; }";
    assert!(dependent_rules(&Backend::parse_php_versioned(source, None)[0]).is_empty());
    let source = "<?php class Validator { protected $dependentRules = dynamicRules(); }";
    assert!(dependent_rules(&Backend::parse_php_versioned(source, None)[0]).is_empty());
}

#[test]
fn validation_catalog_tracks_source_dependencies() {
    let mut catalog = RuleCatalog::default();
    assert!(!catalog.reads(URI));
    catalog.sources.insert(URI.to_string());
    assert!(catalog.reads(URI));
}

#[test]
fn validation_catalog_instance_calls_use_resolved_factory_identity() {
    let source = r#"<?php
$factory->extend('ordinary', fn () => true);
$factory?->extendImplicit(rule: 'implicit', extension: fn () => true);
$factory->EXTENDDEPENDENT('dependent', fn () => true);
$other->extend('unrelated', fn () => true);
$factory->extend($dynamic, fn () => true);
$factory->$method('unknown', fn () => true);
"#;
    let found = super::registrations(
        URI,
        source,
        |receiver| matches!(receiver, Expression::Variable(Variable::Direct(variable)) if variable.name == b"$factory"),
        |_| None,
    );
    assert_eq!(
        found
            .rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        ["ordinary", "implicit", "dependent"]
    );
    assert_eq!(found.dependent, ["dependent"]);
    assert!(found.dynamic);
}

#[test]
fn validation_catalog_resolvers_record_the_returned_validator() {
    let source = r#"<?php
use Illuminate\Support\Facades\Validator;
Validator::resolver(fn () => new AppValidator);
$factory->resolver(resolver: fn () => new AppValidator);
Unrelated::resolver(fn () => new OtherValidator);
"#;
    let validator = Arc::new(ClassInfo::default());
    let found = super::registrations(URI, source, |_| true, |_| Some(Arc::clone(&validator)));
    assert_eq!(found.validators.len(), 2);
    assert!(
        found
            .validators
            .iter()
            .all(|class| Arc::ptr_eq(class, &validator))
    );
    assert!(!found.dynamic);
    assert!(found.rules.is_empty());

    for call in [
        "resolver($callback)",
        "resolver()",
        "resolver(fn () => dynamicValidator())",
    ] {
        let source =
            format!("<?php use Illuminate\\Support\\Facades\\Validator; Validator::{call};");
        let found = registrations(URI, &source);
        assert!(found.dynamic, "{call}");
        assert!(found.validators.is_empty());
    }
}

#[test]
fn validation_catalog_base_resolver_annotations_do_not_close_unknown_subclasses() {
    let class = ClassInfo {
        name: crate::atom::atom("Validator"),
        file_namespace: Some(crate::atom::atom("Illuminate\\Validation")),
        ..ClassInfo::default()
    };
    assert_eq!(class.fqn().as_str(), VALIDATOR);
    for (callback, expected) in [
        ("fn (): Validator => dynamicValidator()", false),
        (
            "function (): Validator { return dynamicValidator(); }",
            false,
        ),
        ("fn () => new Validator", true),
        ("function () { return (new Validator); }", true),
        ("function () { return; }", false),
        ("function () { throw new Exception; }", false),
        ("function () {}", false),
        ("$callback", false),
    ] {
        let source = format!("<?php return {callback};");
        let actual =
            crate::parser::with_parsed_program(&source, "validation_resolver", |program, _| {
                program
                    .statements
                    .iter()
                    .find_map(|statement| match statement {
                        Statement::Return(returned) => returned.value,
                        _ => None,
                    })
                    .map(|returned| resolver_return_is_concrete(returned, &class))
            });
        assert_eq!(actual, Some(expected), "{callback}");
    }
    for declaration in [
        "interface Validator {}",
        "abstract class Validator {}",
        "class CustomValidator {}",
    ] {
        let classes = Backend::parse_php_versioned(&format!("<?php {declaration}"), None);
        let actual = crate::parser::with_parsed_program(
            "<?php return $callback;",
            "validation_resolver",
            |program, _| {
                program
                    .statements
                    .iter()
                    .find_map(|statement| match statement {
                        Statement::Return(returned) => returned.value,
                        _ => None,
                    })
                    .map(|returned| resolver_return_is_concrete(returned, &classes[0]))
            },
        );
        assert_eq!(
            actual,
            Some(declaration == "class CustomValidator {}"),
            "{declaration}"
        );
    }
}

#[test]
fn validation_catalog_resolver_constructions_override_broad_return_annotations() {
    for (callback, expected) in [
        (
            "fn (): BaseValidator => new ActualValidator",
            Some("Domain\\CustomValidator"),
        ),
        (
            "function (): BaseValidator { return (new ActualValidator); }",
            Some("Domain\\CustomValidator"),
        ),
        ("fn (): BaseValidator => new BaseValidator", Some(VALIDATOR)),
        ("fn () => new Validator", None),
        ("fn (): BaseValidator => dynamicValidator()", None),
    ] {
        let source = format!(
            r#"<?php
namespace Illuminate\Validation {{ class Validator {{}} }}
namespace Domain {{ class CustomValidator extends \Illuminate\Validation\Validator {{}} }}
namespace App {{
    use Illuminate\Validation\Validator as BaseValidator;
    use Domain\CustomValidator as ActualValidator;
    class Validator {{}}
    $resolver = {callback};
}}
"#
        );
        let backend = Backend::new_test();
        backend.update_ast(URI, &source);
        let file = backend.file_context(URI);
        let loaders = backend.file_loaders(&file);
        let actual = crate::parser::with_parsed_program(
            &source,
            "validation_resolver_construction",
            |program, content| {
                let mut found = None;
                walk_program_expressions(program, &mut |expression| {
                    if matches!(
                        expression,
                        Expression::Closure(_) | Expression::ArrowFunction(_)
                    ) {
                        let ctx = loaders.var_ctx_at(content, expression.span().start.offset);
                        found = Some(
                            registered_validator(expression, &ctx)
                                .map(|class| class.fqn().to_string()),
                        );
                        return ControlFlow::Break(());
                    }
                    ControlFlow::Continue(())
                });
                found
            },
        );
        assert_eq!(actual, Some(expected.map(str::to_string)), "{callback}");
    }
}

#[test]
fn validation_catalog_provider_resolution_ignores_the_callers_scope_snapshots() {
    use crate::type_engine::variable::forward_walk::{
        ScopeState, lookup_diagnostic_scope, record_scope_snapshot, with_diagnostic_scope_cache,
    };

    let source = r#"<?php
namespace Illuminate\Contracts\Validation {
    interface Factory { public function extend($rule, $extension); }
}
namespace App\Providers {
    use Illuminate\Contracts\Validation\Factory as RuleFactory;
    class RuleProvider {
        public function boot(RuleFactory $factory): void {
            $factory->extend('contract_rule', fn () => true);
        }
    }
}
"#;
    let backend = Backend::new_test();
    backend.update_ast(URI, source);
    let offset = source.find("$factory->").unwrap() as u32;
    let _caller_scope = with_diagnostic_scope_cache();
    record_scope_snapshot(0, &ScopeState::new());
    assert!(lookup_diagnostic_scope("$factory", offset).is_some_and(|types| types.is_empty()));

    let found = source_registrations(&backend, URI, source);
    assert_eq!(
        found
            .rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        ["contract_rule"]
    );
    assert!(!found.dynamic);
    assert!(lookup_diagnostic_scope("$factory", offset).is_some_and(|types| types.is_empty()));
}
