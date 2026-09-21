use super::*;
use crate::atom::atom;
use crate::test_fixtures::{make_class, make_method};
use crate::types::Visibility;

/// Build a facade whose `getFacadeAccessor()` names `concrete`.
fn make_facade(name: &str, concrete: &str) -> ClassInfo {
    let mut facade = make_class(name);
    facade.parent_class = Some(atom(FACADE_FQN));
    facade.laravel_mut().facade_accessor = Some(FacadeAccessor::Class(atom(concrete)));
    facade
}

fn loader_for(classes: Vec<ClassInfo>) -> impl Fn(&str) -> Option<Arc<ClassInfo>> {
    let classes: Vec<Arc<ClassInfo>> = classes.into_iter().map(Arc::new).collect();
    move |name: &str| {
        classes
            .iter()
            .find(|c| c.fqn().as_str() == name.trim_start_matches('\\'))
            .cloned()
    }
}

fn forwarded_names(methods: &[Arc<MethodInfo>]) -> Vec<String> {
    methods.iter().map(|m| m.name.to_string()).collect()
}

#[test]
fn concrete_public_instance_methods_are_forwarded_as_static() {
    let mut container = make_class("Container");
    container
        .methods
        .push(Arc::new(make_method("resolveThing", Some("object"))));

    let facade = make_facade("MyFacade", "Container");
    let loader = loader_for(vec![container]);

    assert!(LaravelFacadeProvider.applies_to(&facade, &loader));
    let members = LaravelFacadeProvider.provide(&facade, &loader, None);
    assert_eq!(forwarded_names(&members.methods), vec!["resolveThing"]);
    assert!(members.methods[0].is_static);
    assert_eq!(
        members.methods[0]
            .return_type
            .as_ref()
            .map(|t| t.to_string()),
        Some("object".to_string())
    );
}

#[test]
fn non_public_static_and_magic_methods_are_not_forwarded() {
    let mut container = make_class("Container");
    let mut hidden = make_method("secret", None);
    hidden.visibility = Visibility::Protected;
    let mut already_static = make_method("boot", None);
    already_static.is_static = true;
    container.methods.push(Arc::new(hidden));
    container.methods.push(Arc::new(already_static));
    container
        .methods
        .push(Arc::new(make_method("__call", None)));
    container
        .methods
        .push(Arc::new(make_method("visible", None)));

    let facade = make_facade("MyFacade", "Container");
    let loader = loader_for(vec![container]);

    let members = LaravelFacadeProvider.provide(&facade, &loader, None);
    assert_eq!(forwarded_names(&members.methods), vec!["visible"]);
}

#[test]
fn facade_own_static_method_wins_over_the_forwarded_one() {
    let mut container = make_class("Container");
    container
        .methods
        .push(Arc::new(make_method("resolveThing", Some("object"))));

    let mut facade = make_facade("MyFacade", "Container");
    let declared = MethodInfo {
        is_static: true,
        ..make_method("resolveThing", Some("string"))
    };
    facade.methods.push(Arc::new(declared));

    let loader = loader_for(vec![container]);
    let members = LaravelFacadeProvider.provide(&facade, &loader, None);
    assert!(members.methods.is_empty());
}

#[test]
fn fluent_return_types_resolve_to_the_concrete_class() {
    let mut container = make_class("Container");
    container
        .methods
        .push(Arc::new(make_method("configure", Some("static"))));

    let facade = make_facade("MyFacade", "Container");
    let loader = loader_for(vec![container]);

    let members = LaravelFacadeProvider.provide(&facade, &loader, None);
    assert_eq!(
        members.methods[0]
            .return_type
            .as_ref()
            .map(|t| t.to_string()),
        Some("Container".to_string())
    );
}

#[test]
fn a_class_that_does_not_extend_facade_is_skipped() {
    let mut class = make_class("NotAFacade");
    class.laravel_mut().facade_accessor = Some(FacadeAccessor::Class(atom("Container")));
    let loader = loader_for(vec![make_class("Container")]);

    assert!(!LaravelFacadeProvider.applies_to(&class, &loader));
}

/// A facade whose `getFacadeAccessor()` returns the container binding
/// key `key`.
fn make_binding_facade(name: &str, key: &str) -> ClassInfo {
    let mut facade = make_class(name);
    facade.parent_class = Some(atom(FACADE_FQN));
    facade.laravel_mut().facade_accessor = Some(FacadeAccessor::Alias(atom(key)));
    facade
}

/// A resolved-class cache whose alias table binds `key` to `concrete`,
/// standing in for what the `Backend` parses out of the installed
/// framework and the project's service providers.
fn cache_binding(key: &str, concrete: &str) -> crate::virtual_members::ResolvedClassCache {
    let mut aliases = crate::virtual_members::laravel::aliases::LaravelAliases::default();
    aliases
        .container
        .insert(key.to_string(), concrete.to_string());
    let slot = crate::virtual_members::laravel::new_alias_slot();
    slot.publish_for_test(Arc::new(aliases));

    let cache = crate::virtual_members::new_resolved_class_cache();
    cache.write().set_laravel_aliases(slot);
    cache
}

#[test]
fn a_container_binding_accessor_forwards_the_bound_class() {
    let mut factory = make_class("ViewFactory");
    factory
        .methods
        .push(Arc::new(make_method("render", Some("string"))));

    let facade = make_binding_facade("ViewFacade", "view");
    let loader = loader_for(vec![factory]);
    let cache = cache_binding("view", "ViewFactory");

    assert!(LaravelFacadeProvider.applies_to(&facade, &loader));
    let members = LaravelFacadeProvider.provide(&facade, &loader, Some(&cache));
    assert_eq!(forwarded_names(&members.methods), vec!["render"]);
    assert!(members.methods[0].is_static);
}

#[test]
fn a_container_key_bound_only_at_runtime_forwards_nothing() {
    let mut factory = make_class("ViewFactory");
    factory
        .methods
        .push(Arc::new(make_method("render", Some("string"))));

    let facade = make_binding_facade("SentryFacade", "sentry");
    let loader = loader_for(vec![factory]);
    let cache = cache_binding("view", "ViewFactory");

    let members = LaravelFacadeProvider.provide(&facade, &loader, Some(&cache));
    assert!(members.methods.is_empty());
}

#[test]
fn a_container_binding_accessor_forwards_nothing_without_the_alias_table() {
    let mut factory = make_class("ViewFactory");
    factory
        .methods
        .push(Arc::new(make_method("render", Some("string"))));

    let facade = make_binding_facade("ViewFacade", "view");
    let loader = loader_for(vec![factory]);

    let members = LaravelFacadeProvider.provide(&facade, &loader, None);
    assert!(members.methods.is_empty());
}

#[test]
fn a_facade_naming_itself_forwards_nothing() {
    let facade = make_facade("MyFacade", "MyFacade");
    let loader = loader_for(vec![make_facade("MyFacade", "MyFacade")]);

    let members = LaravelFacadeProvider.provide(&facade, &loader, None);
    assert!(members.methods.is_empty());
}

// ── Parse-time accessor extraction ──────────────────────────────────

fn parse_accessor(src: &str, class_name: &str) -> Option<FacadeAccessor> {
    let classes = crate::Backend::parse_php_versioned_with_namespaces(src, None);
    classes
        .iter()
        .find(|(c, _)| c.name == atom(class_name))
        .and_then(|(c, _)| c.laravel())
        .and_then(|l| l.facade_accessor)
}

#[test]
fn class_reference_accessor_is_recorded() {
    let src = r#"<?php
class MyFacade extends \Illuminate\Support\Facades\Facade {
    protected static function getFacadeAccessor(): string { return Container::class; }
}
"#;
    assert_eq!(
        parse_accessor(src, "MyFacade"),
        Some(FacadeAccessor::Class(atom("Container")))
    );
}

#[test]
fn container_binding_string_accessor_is_recorded() {
    let src = r#"<?php
class ViewFacade extends \Illuminate\Support\Facades\Facade {
    protected static function getFacadeAccessor() { return 'view'; }
}
"#;
    assert_eq!(
        parse_accessor(src, "ViewFacade"),
        Some(FacadeAccessor::Alias(atom("view")))
    );
}

#[test]
fn a_computed_accessor_is_not_recorded() {
    let src = r#"<?php
class DynamicFacade extends \Illuminate\Support\Facades\Facade {
    protected static function getFacadeAccessor() { return static::$binding; }
}
"#;
    assert_eq!(parse_accessor(src, "DynamicFacade"), None);
}

#[test]
fn a_class_without_the_accessor_records_nothing() {
    let src = "<?php class Plain { public function go(): void {} }";
    assert_eq!(parse_accessor(src, "Plain"), None);
}

#[test]
fn each_facade_in_a_file_records_its_own_accessor() {
    let src = r#"<?php
class FirstFacade extends \Illuminate\Support\Facades\Facade {
    protected static function getFacadeAccessor() { return FirstThing::class; }
}
class SecondFacade extends \Illuminate\Support\Facades\Facade {
    protected static function getFacadeAccessor() { return SecondThing::class; }
}
"#;
    assert_eq!(
        parse_accessor(src, "FirstFacade"),
        Some(FacadeAccessor::Class(atom("FirstThing")))
    );
    assert_eq!(
        parse_accessor(src, "SecondFacade"),
        Some(FacadeAccessor::Class(atom("SecondThing")))
    );
}
