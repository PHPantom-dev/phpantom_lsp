use super::text_args::class_const_condition_parts;
use super::*;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::types::ClassInfo;
use crate::types::ClassLikeKind;
use std::sync::Arc;

/// A single required parameter with the given name (including `$`).
fn param(name: &str) -> ParameterInfo {
    ParameterInfo {
        name: crate::atom::atom(name),
        is_required: true,
        type_hint: None,
        native_type_hint: None,
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
        param_out_type: None,
        callable_invocation: None,
    }
}

/// An optional parameter with the given declared default value.
fn param_with_default(name: &str, default: &str) -> ParameterInfo {
    ParameterInfo {
        is_required: false,
        default_value: Some(default.to_string()),
        ..param(name)
    }
}

/// Resolve a `str_word_count`-style conditional keyed on the value of a
/// `$format` parameter that defaults to `0`.
fn resolve_word_count(text_args: &str) -> Option<String> {
    let cond = PhpType::parse(
        "($format is 0 ? int : ($format is 1 ? list<string> : ($format is 2 ? array<int, string> : list<string>|int)))",
    );
    let params = [param("$string"), param_with_default("$format", "0")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let tpl = TemplateContext::with_params(&[]);
    resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        text_args,
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string())
}

/// An omitted argument takes the parameter's declared default, so the
/// condition on `$format` is decided by that `0` rather than left open.
#[test]
fn omitted_argument_is_decided_by_its_declared_default() {
    assert_eq!(resolve_word_count("$text").as_deref(), Some("int"));
}

/// A value condition compares literal values, so an explicit int argument
/// picks its own branch.
#[test]
fn int_literal_argument_matches_value_condition() {
    assert_eq!(
        resolve_word_count("$text, 1").as_deref(),
        Some("list<string>")
    );
    assert_eq!(
        resolve_word_count("$text, 2").as_deref(),
        Some("array<int, string>")
    );
    // Comparison is by value, not by spelling.
    assert_eq!(
        resolve_word_count("$text, 0x1").as_deref(),
        Some("list<string>")
    );
}

/// A named argument reaches the parameter it targets rather than the slot
/// it sits in.
#[test]
fn named_argument_matches_value_condition() {
    assert_eq!(
        resolve_word_count("$text, format: 1").as_deref(),
        Some("list<string>")
    );
}

/// An argument that is not a literal decides nothing, and with no resolver
/// to fall back on the conditional collapses to its final else branch —
/// every value the call could return.
#[test]
fn non_literal_argument_leaves_value_condition_undecided() {
    assert_eq!(
        resolve_word_count("$text, $format").as_deref(),
        Some("list<string>|int")
    );
}

/// Resolve a `pathinfo`-style value conditional with a resolver that
/// answers for named constants, returning the resolved type's display
/// string.
fn resolve_pathinfo(text_args: &str) -> Option<String> {
    let cond = PhpType::parse("($flags is 15 ? Shape : string)");
    let params = [param("$path"), param_with_default("$flags", "PATHINFO_ALL")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let resolver = |t: &str| match t.trim() {
        "PATHINFO_ALL" => Some(PhpType::literal_int("15")),
        "PATHINFO_FILENAME" => Some(PhpType::literal_int("8")),
        _ => None,
    };
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        text_args,
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string())
}

/// A named constant is not a syntactic literal, but its resolved type is
/// one, so it settles a value condition both ways.
#[test]
fn value_condition_is_decided_by_a_resolved_literal() {
    assert_eq!(resolve_pathinfo("$path").as_deref(), Some("Shape"));
    assert_eq!(
        resolve_pathinfo("$path, PATHINFO_ALL").as_deref(),
        Some("Shape")
    );
    assert_eq!(
        resolve_pathinfo("$path, PATHINFO_FILENAME").as_deref(),
        Some("string")
    );
}

/// An argument whose value cannot be pinned down really may take either
/// branch, so both are returned rather than committing to the else.
#[test]
fn undecidable_value_condition_unions_branches() {
    assert_eq!(
        resolve_pathinfo("$path, $flags").as_deref(),
        Some("Shape|string")
    );
}

/// A parameter default written `self::SOME_CONST` (mirroring
/// `ContainerInterface::get()`'s `$invalidBehavior`) is written inside
/// the method that declares it, so its `self` must resolve against that
/// declaring class, not whichever unrelated class the call happens to
/// sit in — otherwise the constant is never found and the condition is
/// left undecided.
#[test]
fn omitted_default_self_const_resolves_against_declaring_class() {
    let cond = PhpType::parse("($mode is 1 ? Found : Found|null)");
    let params = [
        param("$id"),
        param_with_default("$mode", "self::EXCEPTION_ON_INVALID_REFERENCE"),
    ];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    // Fully qualified, so a call site inside a namespace does not read
    // the declaring class relative to its own.
    let resolver = |t: &str| match t.trim() {
        "\\Vendor\\Di\\ContainerInterface::EXCEPTION_ON_INVALID_REFERENCE" => {
            Some(PhpType::literal_int("1"))
        }
        _ => None,
    };
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    // The call sits in an unrelated class; only the declaring class
    // resolves the default's `self` to the constant the resolver knows.
    let resolved = resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        "Service::class",
        None,
        ConditionalClassContext {
            calling: Some("App\\Controller"),
            declaring: Some("Vendor\\Di\\ContainerInterface"),
            written_class: None,
        },
        loader,
        &tpl,
    )
    .map(|t| t.to_string());
    assert_eq!(resolved.as_deref(), Some("Found"));
}

/// `is true` and `is false` name one boolean value each, so the two
/// literals pick different branches instead of both reading as `is bool`.
#[test]
fn boolean_literal_arguments_decide_is_true_conditions() {
    let params = [param("$value"), param_with_default("$flag", "false")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let tpl = TemplateContext::with_params(&[]);
    let resolve = |cond: &str, text_args: &str| {
        resolve_conditional_with_text_args_and_defaults(
            &PhpType::parse(cond),
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    };

    let is_true = "($flag is true ? Then : Else)";
    assert_eq!(resolve(is_true, "$value, true").as_deref(), Some("Then"));
    assert_eq!(resolve(is_true, "$value, false").as_deref(), Some("Else"));
    // The declared default answers for the omitted argument.
    assert_eq!(resolve(is_true, "$value").as_deref(), Some("Else"));

    let is_false = "($flag is false ? Then : Else)";
    assert_eq!(resolve(is_false, "$value, false").as_deref(), Some("Then"));
    assert_eq!(resolve(is_false, "$value, true").as_deref(), Some("Else"));

    // A plain `is bool` is satisfied by either literal.
    let is_bool = "($flag is bool ? Then : Else)";
    assert_eq!(resolve(is_bool, "$value, true").as_deref(), Some("Then"));
    assert_eq!(resolve(is_bool, "$value, false").as_deref(), Some("Then"));
}

/// A resolved `bool` cannot settle `is true`: the argument really may be
/// either value. A resolved `true`/`false` can.
#[test]
fn is_true_condition_needs_a_resolved_boolean_value() {
    let params = [param("$flag")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let resolve = |resolved: PhpType| {
        let resolver = |_: &str| Some(resolved.clone());
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        resolve_conditional_with_text_args_and_defaults(
            &PhpType::parse("($flag is true ? Then : Else)"),
            &params,
            "$flag",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    };

    assert_eq!(resolve(PhpType::parse("true")).as_deref(), Some("Then"));
    assert_eq!(resolve(PhpType::parse("false")).as_deref(), Some("Else"));
    assert_eq!(
        resolve(PhpType::bool()).as_deref(),
        Some("Then|Else"),
        "a plain bool leaves both branches open"
    );
}

/// With no arguments at all, the no-args path decides the condition from
/// the declared default the same way the text path does.
#[test]
fn no_args_path_is_decided_by_declared_default() {
    let cond = PhpType::parse("($format is 0 ? int : list<string>)");
    let params = [param_with_default("$format", "0")];
    assert_eq!(
        resolve_conditional_without_args(&cond, &params).map(|t| t.to_string()),
        Some("int".to_string())
    );

    // A non-null default no longer reads as null just because the
    // parameter is optional.
    let nullable = PhpType::parse("($format is null ? int : list<string>)");
    assert_eq!(
        resolve_conditional_without_args(&nullable, &params).map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
}

/// Resolve a `PDOStatement::fetch`-style conditional keyed on the fetch
/// mode class constant, returning the resolved type's display string.
fn resolve_fetch(text_args: &str) -> Option<String> {
    // ($mode is PDO::FETCH_OBJ ? \stdClass|false
    //   : ($mode is PDO::FETCH_ASSOC ? array<string, mixed>|false : mixed))
    let cond = PhpType::parse(
        "($mode is PDO::FETCH_OBJ ? \\stdClass|false : ($mode is PDO::FETCH_ASSOC ? array<string, mixed>|false : mixed))",
    );
    let params = [param("$mode")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let tpl = TemplateContext::with_params(&[]);
    resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        text_args,
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string())
}

#[test]
fn class_const_condition_selects_matching_branch() {
    assert!(
        resolve_fetch("\\PDO::FETCH_OBJ")
            .unwrap()
            .contains("stdClass")
    );
    assert!(
        resolve_fetch("\\PDO::FETCH_ASSOC")
            .unwrap()
            .contains("array")
    );
}

#[test]
fn class_const_condition_ignores_leading_backslash() {
    // Argument without a leading backslash still matches the condition.
    assert!(
        resolve_fetch("PDO::FETCH_OBJ")
            .unwrap()
            .contains("stdClass")
    );
}

#[test]
fn class_const_condition_unlisted_mode_falls_through() {
    // A mode with no dedicated branch reaches the `mixed` else branch.
    // `mixed` is informative (a value of unknown type), so it flows
    // through rather than yielding no resolved type.
    assert_eq!(
        resolve_fetch("\\PDO::FETCH_COLUMN").as_deref(),
        Some("mixed")
    );
}

#[test]
fn class_const_condition_requires_matching_class() {
    // The same constant name on a different class must not match the
    // `PDO::FETCH_OBJ` branch — it falls through past every PDO branch to
    // the `mixed` else, which flows through as `mixed`.
    assert_eq!(resolve_fetch("Other::FETCH_OBJ").as_deref(), Some("mixed"));
    // A different class whose constant matches an inner branch also fails.
    assert_eq!(
        resolve_fetch("Other::FETCH_ASSOC").as_deref(),
        Some("mixed")
    );
}

#[test]
fn class_const_condition_parts_rejects_non_constants() {
    assert!(class_const_condition_parts(&PhpType::named(atom("string"))).is_none());
    assert!(class_const_condition_parts(&PhpType::raw("Foo::*")).is_none());
    assert!(class_const_condition_parts(&PhpType::raw("Foo::class")).is_none());
    assert_eq!(
        class_const_condition_parts(&PhpType::raw("PDO::FETCH_OBJ")),
        Some(("PDO", "FETCH_OBJ"))
    );
}

/// A `$x is (array|string)` condition with a string-literal argument
/// takes the then-branch, mirroring Laravel's `keyBy`/`groupBy` key type.
#[test]
fn union_scalar_condition_with_string_literal_takes_then() {
    let cond = PhpType::parse("($key is (array|string) ? array-key : ObjectKey)");
    let params = [param("$key")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let tpl = TemplateContext::with_params(&[]);
    let resolved = resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        "'field'",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string());
    assert_eq!(resolved.as_deref(), Some("array-key"));
}

/// An `is string` condition whose argument is a non-literal expression
/// (a method-call chain) is decided from the argument's resolved type
/// via the resolver, taking the then-branch when it is a string.
#[test]
fn is_string_condition_uses_resolved_arg_type() {
    let cond = PhpType::parse("($subject is string ? string : list<string>)");
    let params = [param("$subject")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let resolver = |t: &str| {
        if t == "$obj->toHtml()" {
            Some(PhpType::named(atom("string")))
        } else {
            None
        }
    };
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    let resolved = resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        "$obj->toHtml()",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string());
    assert_eq!(resolved.as_deref(), Some("string"));
}

#[test]
fn scalar_conditions_classify_resolved_int_and_float_literals() {
    let params = [param("$value")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;

    for (condition_type, literal, expected) in [
        ("int", PhpType::literal_int("1"), "IntBranch"),
        ("float", PhpType::literal_float("1.5"), "FloatBranch"),
    ] {
        let cond = PhpType::parse(&format!(
            "($value is {condition_type} ? {expected} : ElseBranch)"
        ));
        let resolver = |_: &str| Some(literal.clone());
        let tpl = TemplateContext {
            defaults: None,
            params: &[],
            bindings: &[],
            arg_type_resolver: Some(&resolver),
            this_type: None,
        };
        let resolved = resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            "$value",
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        );
        assert_eq!(
            resolved.as_ref().map(ToString::to_string).as_deref(),
            Some(expected)
        );
    }
}

/// When the argument's type cannot be resolved (e.g. a magic property
/// chain that resolves to `mixed`) an `is string` condition is genuinely
/// undecidable, so the result is the union of both branches rather than a
/// commitment to the else branch.
#[test]
fn undecidable_is_string_condition_unions_branches() {
    let cond = PhpType::parse("($subject is string ? string : list<string>)");
    let params = [param("$subject")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    // Resolver that cannot resolve the argument (returns None).
    let resolver = |_: &str| None;
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    let resolved = resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        "$obj->magic->toHtml()",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string());
    assert_eq!(resolved.as_deref(), Some("string|list<string>"));
}

/// An argument whose resolved type is mutually exclusive with the
/// condition (a closure against `array|string`) takes the else branch,
/// not the union — this keeps `Collection::groupBy(fn …)` resolving to
/// its non-array key type.
#[test]
fn closure_arg_refutes_array_or_string_condition() {
    let cond = PhpType::parse("($groupBy is (array|string) ? array-key : Value)");
    let params = [param("$groupBy")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let resolver = |_: &str| Some(PhpType::named(atom("Closure")));
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    let resolved = resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        "fn($x) => $x->value",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string());
    assert_eq!(resolved.as_deref(), Some("Value"));
}

/// Without a resolver an unclassifiable `is string` argument falls
/// through to the else branch (unchanged conservative default).
#[test]
fn is_string_condition_without_resolver_falls_to_else() {
    let cond = PhpType::parse("($subject is string ? string : list<string>)");
    let params = [param("$subject")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let tpl = TemplateContext::with_params(&[]);
    let resolved = resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        "$obj->toHtml()",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string());
    assert_eq!(resolved.as_deref(), Some("list<string>"));
}

/// A conditional nested inside a generic wrapper is collapsed against
/// the call arguments rather than surviving raw.
#[test]
fn nested_conditional_in_generic_is_evaluated() {
    let ty = PhpType::parse("Collection<($key is (array|string) ? array-key : ObjectKey), Value>");
    assert!(ty.contains_conditional());
    let params = [param("$key")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let tpl = TemplateContext::with_params(&[]);
    let evaluated = evaluate_nested_conditionals_text(
        &ty,
        &params,
        "'field'",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    );
    assert!(!evaluated.contains_conditional());
    assert_eq!(evaluated.to_string(), "Collection<array-key, Value>");
}

/// A multi-level conditional whose branches are resolved via a template
/// default (as in Laravel's `groupBy`) collapses fully — the template
/// default resolver returns a branch that is itself a conditional, and
/// the evaluator must keep collapsing rather than leaving a raw residue.
#[test]
fn nested_template_default_conditional_collapses_fully() {
    // static<($g is array|string ? array-key
    //   : (T is \UnitEnum ? array-key : (T is \Stringable ? string : T))), V>
    let ty = PhpType::parse(
        "Collection<($g is (array|string) ? array-key : (T is \\UnitEnum ? array-key : (T is \\Stringable ? string : T))), V>",
    );
    let params = [param("$g")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    // The closure return type binds T to a nullable string key. As in the
    // real call path, template substitution runs before the evaluator, so
    // every `T` in a branch position is already concrete; only the `T is
    // …` condition subjects (carried in the conditional's `param` field)
    // remain and are evaluated via the template defaults.
    let mut defaults = HashMap::new();
    defaults.insert(
        "T".to_string(),
        PhpType::union(vec![
            PhpType::named(atom("string")),
            PhpType::named(atom("null")),
        ]),
    );
    let ty = ty.substitute(&defaults);
    assert!(ty.contains_conditional());
    let resolver = |_: &str| Some(PhpType::named(atom("Closure")));
    let tpl = TemplateContext {
        defaults: Some(&defaults),
        params: &[crate::atom::atom("T")],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    let evaluated = evaluate_nested_conditionals_text(
        &ty,
        &params,
        "fn($x) => $x->value",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    );
    assert!(
        !evaluated.contains_conditional(),
        "residual conditional left in {evaluated}"
    );
    assert_eq!(evaluated.to_string(), "Collection<string|null, V>");
}

/// A loader answering for an Eloquent-shaped hierarchy: `Collection`
/// implements `Arrayable`, `Order` extends `Model`, and nothing else
/// relates to anything.
fn eloquent_loader(name: Fqn<'_>) -> Option<Arc<ClassInfo>> {
    let name = name.as_str();
    let class = |name: &str, parent: Option<&str>, interfaces: &[&str], kind| ClassInfo {
        name: atom(name),
        parent_class: parent.map(atom),
        interfaces: interfaces.iter().map(|i| atom(i)).collect(),
        kind,
        ..ClassInfo::default()
    };
    let info = match name.trim_start_matches('\\') {
        "Collection" => class("Collection", None, &["Arrayable"], ClassLikeKind::Class),
        "Arrayable" => class("Arrayable", None, &[], ClassLikeKind::Interface),
        "Model" => class("Model", None, &[], ClassLikeKind::Class),
        "Order" => class("Order", Some("Model"), &[], ClassLikeKind::Class),
        _ => return None,
    };
    Some(Arc::new(info))
}

/// Resolve an Eloquent `find`-shaped conditional against an argument of
/// the given resolved type.
fn resolve_find(arg_type: Option<PhpType>) -> Option<String> {
    let cond = PhpType::parse("($id is array<mixed>|Arrayable ? Collection : Order|null)");
    let params = [param("$id")];
    let loader: &ResolvedClassLoader<'_> = &eloquent_loader;
    let resolver = |_: &str| arg_type.clone();
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        "$id",
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string())
}

/// A condition naming a class is decided by the class hierarchy: a scalar
/// can never satisfy it, and a class that implements the named interface
/// always does.
#[test]
fn class_condition_is_decided_by_the_resolved_argument_type() {
    assert_eq!(
        resolve_find(Some(PhpType::int())).as_deref(),
        Some("Order|null")
    );
    assert_eq!(
        resolve_find(Some(PhpType::named(atom("Collection")))).as_deref(),
        Some("Collection")
    );
    // A class that does not implement the interface still leaves both
    // branches open: a subclass of it may implement it.
    assert_eq!(
        resolve_find(Some(PhpType::named(atom("Order")))).as_deref(),
        Some("Collection|Order|null")
    );
    // An argument whose type says nothing leaves both branches open.
    assert_eq!(
        resolve_find(Some(PhpType::mixed())).as_deref(),
        Some("Collection|Order|null")
    );
}

/// Resolve a `tap`-shaped `is null` conditional against an argument of the
/// given resolved type.
fn resolve_is_null(text_args: &str, arg_type: Option<PhpType>) -> Option<String> {
    let cond = PhpType::parse("($callback is null ? Proxy : Value)");
    let params = [param_with_default("$callback", "null")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let resolver = |_: &str| arg_type.clone();
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    resolve_conditional_with_text_args_and_defaults(
        &cond,
        &params,
        text_args,
        None,
        ConditionalClassContext::default(),
        loader,
        &tpl,
    )
    .map(|t| t.to_string())
}

/// An `is null` condition follows the argument's type rather than its mere
/// presence, so an argument that may be null leaves both branches open.
#[test]
fn is_null_condition_follows_the_resolved_argument_type() {
    assert_eq!(resolve_is_null("", None).as_deref(), Some("Proxy"));
    assert_eq!(resolve_is_null("null", None).as_deref(), Some("Proxy"));
    assert_eq!(
        resolve_is_null("$callback", Some(PhpType::named(atom("Closure")))).as_deref(),
        Some("Value")
    );
    assert_eq!(
        resolve_is_null("$name", Some(PhpType::nullable(PhpType::string()))).as_deref(),
        Some("Proxy|Value")
    );
}

/// `is not null` takes the branch its negation names, rather than reading
/// as the un-negated condition.
#[test]
fn negated_null_condition_swaps_its_branches() {
    let cond = PhpType::parse("($callback is not null ? Value : Proxy)");
    let params = [param_with_default("$callback", "null")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let tpl = TemplateContext::with_params(&[]);
    let resolve = |text_args: &str| {
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    };
    assert_eq!(resolve("null").as_deref(), Some("Proxy"));
    assert_eq!(resolve("$fn").as_deref(), Some("Value"));
}

/// A value condition is refuted by an argument of another kind entirely:
/// `null` is not the string `'array'`, so spatie's `Data::collect()` picks
/// its collection branch rather than unioning every branch it has.
#[test]
fn value_condition_is_refuted_by_another_kind_of_argument() {
    let cond = PhpType::parse("($into is 'array' ? array<Data> : Collection)");
    let params = [param("$items"), param_with_default("$into", "null")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let resolver = |_: &str| Some(PhpType::null());
    let tpl = TemplateContext {
        defaults: None,
        params: &[],
        bindings: &[],
        arg_type_resolver: Some(&resolver),
        this_type: None,
    };
    let resolve = |text_args: &str| {
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    };
    assert_eq!(resolve("$rows").as_deref(), Some("Collection"));
    assert_eq!(resolve("$rows, 'array'").as_deref(), Some("array<Data>"));
}

/// A condition keyed on a `@template` parameter is decided against the
/// argument that binds it, not against the first parameter.
#[test]
fn template_subject_is_decided_through_its_parameter_binding() {
    let cond = PhpType::parse("(B is true ? Sync : Async)");
    let params = [param("$id"), param_with_default("$wait", "true")];
    let loader: &ResolvedClassLoader<'_> = &|_| None;
    let bindings = [(atom("B"), atom("$wait"))];
    let tpl = TemplateContext {
        defaults: None,
        params: &[atom("B")],
        bindings: &bindings,
        arg_type_resolver: None,
        this_type: None,
    };
    let resolve = |text_args: &str| {
        resolve_conditional_with_text_args_and_defaults(
            &cond,
            &params,
            text_args,
            None,
            ConditionalClassContext::default(),
            loader,
            &tpl,
        )
        .map(|t| t.to_string())
    };
    assert_eq!(resolve("$id").as_deref(), Some("Sync"));
    assert_eq!(resolve("$id, false").as_deref(), Some("Async"));
}
