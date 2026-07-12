use super::*;

// ── Keywords ────────────────────────────────────────────────────────

#[test]
fn parse_this() {
    assert_eq!(SubjectExpr::parse("$this"), SubjectExpr::This);
}

#[test]
fn parse_self() {
    assert_eq!(SubjectExpr::parse("self"), SubjectExpr::SelfKw);
}

#[test]
fn parse_static() {
    assert_eq!(SubjectExpr::parse("static"), SubjectExpr::StaticKw);
}

#[test]
fn parse_parent() {
    assert_eq!(SubjectExpr::parse("parent"), SubjectExpr::Parent);
}

// ── Bare variable ───────────────────────────────────────────────────

#[test]
fn parse_bare_variable() {
    assert_eq!(
        SubjectExpr::parse("$user"),
        SubjectExpr::Variable("$user".to_string())
    );
}

#[test]
fn parse_bare_variable_underscore() {
    assert_eq!(
        SubjectExpr::parse("$my_var"),
        SubjectExpr::Variable("$my_var".to_string())
    );
}

// ── Bare class name ─────────────────────────────────────────────────

#[test]
fn parse_bare_class_name() {
    assert_eq!(
        SubjectExpr::parse("User"),
        SubjectExpr::ClassName("User".to_string())
    );
}

#[test]
fn parse_fqn_class_name() {
    assert_eq!(
        SubjectExpr::parse("App\\Models\\User"),
        SubjectExpr::ClassName("App\\Models\\User".to_string())
    );
}

// ── Property chains ─────────────────────────────────────────────────

#[test]
fn parse_this_property() {
    assert_eq!(
        SubjectExpr::parse("$this->name"),
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::This),
            property: "name".to_string(),
        }
    );
}

#[test]
fn parse_nullsafe_this_property() {
    assert_eq!(
        SubjectExpr::parse("$this?->name"),
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::This),
            property: "name".to_string(),
        }
    );
}

#[test]
fn parse_var_property() {
    assert_eq!(
        SubjectExpr::parse("$user->address"),
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::Variable("$user".to_string())),
            property: "address".to_string(),
        }
    );
}

#[test]
fn parse_nested_property_chain() {
    assert_eq!(
        SubjectExpr::parse("$user->address->city"),
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::PropertyChain {
                base: Box::new(SubjectExpr::Variable("$user".to_string())),
                property: "address".to_string(),
            }),
            property: "city".to_string(),
        }
    );
}

#[test]
fn parse_nullsafe_var_property() {
    assert_eq!(
        SubjectExpr::parse("$user?->address"),
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::Variable("$user".to_string())),
            property: "address".to_string(),
        }
    );
}

// ── Static access (enum case / constant) ────────────────────────────

#[test]
fn parse_static_access_enum_case() {
    assert_eq!(
        SubjectExpr::parse("Status::Active"),
        SubjectExpr::StaticAccess {
            class: "Status".to_string(),
            member: "Active".to_string(),
        }
    );
}

#[test]
fn parse_static_access_constant() {
    assert_eq!(
        SubjectExpr::parse("MyClass::SOME_CONST"),
        SubjectExpr::StaticAccess {
            class: "MyClass".to_string(),
            member: "SOME_CONST".to_string(),
        }
    );
}

// ── Call expressions ────────────────────────────────────────────────

#[test]
fn parse_function_call_no_args() {
    assert_eq!(
        SubjectExpr::parse("app()"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::FunctionCall("app".to_string())),
            args_text: "".to_string(),
        }
    );
}

#[test]
fn parse_function_call_with_args() {
    assert_eq!(
        SubjectExpr::parse("app(User::class)"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::FunctionCall("app".to_string())),
            args_text: "User::class".to_string(),
        }
    );
}

#[test]
fn parse_method_call() {
    assert_eq!(
        SubjectExpr::parse("$this->getUser()"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::MethodCall {
                base: Box::new(SubjectExpr::This),
                method: "getUser".to_string(),
            }),
            args_text: "".to_string(),
        }
    );
}

#[test]
fn parse_var_method_call() {
    assert_eq!(
        SubjectExpr::parse("$service->process()"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::MethodCall {
                base: Box::new(SubjectExpr::Variable("$service".to_string())),
                method: "process".to_string(),
            }),
            args_text: "".to_string(),
        }
    );
}

#[test]
fn parse_static_method_call() {
    assert_eq!(
        SubjectExpr::parse("User::find(1)"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::StaticMethodCall {
                class: "User".to_string(),
                method: "find".to_string(),
            }),
            args_text: "1".to_string(),
        }
    );
}

#[test]
fn parse_self_static_method_call() {
    assert_eq!(
        SubjectExpr::parse("self::create()"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::StaticMethodCall {
                class: "self".to_string(),
                method: "create".to_string(),
            }),
            args_text: "".to_string(),
        }
    );
}

#[test]
fn parse_parent_method_call() {
    assert_eq!(
        SubjectExpr::parse("parent::build()"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::StaticMethodCall {
                class: "parent".to_string(),
                method: "build".to_string(),
            }),
            args_text: "".to_string(),
        }
    );
}

#[test]
fn parse_chained_method_call() {
    // $this->getFactory()->create()
    let parsed = SubjectExpr::parse("$this->getFactory()->create()");
    assert_eq!(
        parsed,
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::MethodCall {
                base: Box::new(SubjectExpr::CallExpr {
                    callee: Box::new(SubjectExpr::MethodCall {
                        base: Box::new(SubjectExpr::This),
                        method: "getFactory".to_string(),
                    }),
                    args_text: "".to_string(),
                }),
                method: "create".to_string(),
            }),
            args_text: "".to_string(),
        }
    );
}

#[test]
fn parse_chained_method_then_property() {
    // $this->getFactory()->config
    assert_eq!(
        SubjectExpr::parse("$this->getFactory()->config"),
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::CallExpr {
                callee: Box::new(SubjectExpr::MethodCall {
                    base: Box::new(SubjectExpr::This),
                    method: "getFactory".to_string(),
                }),
                args_text: "".to_string(),
            }),
            property: "config".to_string(),
        }
    );
}

#[test]
fn parse_static_call_chain_then_property() {
    // BlogAuthor::whereIn(…)->first()->posts
    let parsed = SubjectExpr::parse("BlogAuthor::whereIn('id', [1])->first()->posts");
    assert_eq!(
        parsed,
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::CallExpr {
                callee: Box::new(SubjectExpr::MethodCall {
                    base: Box::new(SubjectExpr::CallExpr {
                        callee: Box::new(SubjectExpr::StaticMethodCall {
                            class: "BlogAuthor".to_string(),
                            method: "whereIn".to_string(),
                        }),
                        args_text: "'id', [1]".to_string(),
                    }),
                    method: "first".to_string(),
                }),
                args_text: "".to_string(),
            }),
            property: "posts".to_string(),
        }
    );
}

#[test]
fn parse_nested_call_args() {
    // Environment::get(self::country())
    assert_eq!(
        SubjectExpr::parse("Environment::get(self::country())"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::StaticMethodCall {
                class: "Environment".to_string(),
                method: "get".to_string(),
            }),
            args_text: "self::country()".to_string(),
        }
    );
}

// ── new expressions ─────────────────────────────────────────────────

#[test]
fn parse_new_expression_bare() {
    // `new Builder()` is recognised by the `new` handler before the
    // call-expression handler runs, so it collapses to `NewExpr`.
    assert_eq!(
        SubjectExpr::parse("new Builder()"),
        SubjectExpr::NewExpr {
            class_name: "Builder".to_string(),
        }
    );
}

#[test]
fn parse_new_expression_parenthesized() {
    assert_eq!(
        SubjectExpr::parse("(new Builder())"),
        SubjectExpr::NewExpr {
            class_name: "Builder".to_string(),
        }
    );
}

#[test]
fn parse_new_expression_no_parens() {
    assert_eq!(
        SubjectExpr::parse("(new Builder)"),
        SubjectExpr::NewExpr {
            class_name: "Builder".to_string(),
        }
    );
}

#[test]
fn parse_new_expression_fqn() {
    assert_eq!(
        SubjectExpr::parse("(new \\App\\Builder())"),
        SubjectExpr::NewExpr {
            class_name: "App\\Builder".to_string(),
        }
    );
}

// ── Variable callable: `$fn()` ──────────────────────────────────────

#[test]
fn parse_variable_call() {
    assert_eq!(
        SubjectExpr::parse("$fn()"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::Variable("$fn".to_string())),
            args_text: "".to_string(),
        }
    );
}

#[test]
fn parse_variable_call_with_args() {
    assert_eq!(
        SubjectExpr::parse("$fn(42, 'hello')"),
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::Variable("$fn".to_string())),
            args_text: "42, 'hello'".to_string(),
        }
    );
}

// ── Array access ────────────────────────────────────────────────────

#[test]
fn parse_variable_string_key_access() {
    assert_eq!(
        SubjectExpr::parse("$response['items']"),
        SubjectExpr::ArrayAccess {
            base: Box::new(SubjectExpr::Variable("$response".to_string())),
            segments: vec![BracketSegment::StringKey("items".to_string())],
        }
    );
}

#[test]
fn parse_variable_element_access() {
    assert_eq!(
        SubjectExpr::parse("$list[0]"),
        SubjectExpr::ArrayAccess {
            base: Box::new(SubjectExpr::Variable("$list".to_string())),
            segments: vec![BracketSegment::IntKey("0".to_string())],
        }
    );
}

#[test]
fn parse_variable_chained_bracket_access() {
    assert_eq!(
        SubjectExpr::parse("$response['items'][0]"),
        SubjectExpr::ArrayAccess {
            base: Box::new(SubjectExpr::Variable("$response".to_string())),
            segments: vec![
                BracketSegment::StringKey("items".to_string()),
                BracketSegment::IntKey("0".to_string()),
            ],
        }
    );
}

#[test]
fn parse_variable_empty_bracket() {
    assert_eq!(
        SubjectExpr::parse("$arr[]"),
        SubjectExpr::ArrayAccess {
            base: Box::new(SubjectExpr::Variable("$arr".to_string())),
            segments: vec![BracketSegment::ElementAccess],
        }
    );
}

// ── Inline array literal ────────────────────────────────────────────

#[test]
fn parse_inline_array_literal() {
    assert_eq!(
        SubjectExpr::parse("[Customer::first()][0]"),
        SubjectExpr::InlineArray {
            elements: vec!["Customer::first()".to_string()],
            index_segments: vec![BracketSegment::IntKey("0".to_string())],
        }
    );
}

#[test]
fn parse_inline_array_literal_multiple_elements() {
    assert_eq!(
        SubjectExpr::parse("[$a, $b][0]"),
        SubjectExpr::InlineArray {
            elements: vec!["$a".to_string(), "$b".to_string()],
            index_segments: vec![BracketSegment::IntKey("0".to_string())],
        }
    );
}

// ── is_self_like helper ─────────────────────────────────────────────

#[test]
fn is_self_like_keywords() {
    assert!(SubjectExpr::This.is_self_like());
    assert!(SubjectExpr::SelfKw.is_self_like());
    assert!(SubjectExpr::StaticKw.is_self_like());
}

#[test]
fn is_self_like_non_keywords() {
    assert!(!SubjectExpr::Parent.is_self_like());
    assert!(!SubjectExpr::Variable("$x".to_string()).is_self_like());
    assert!(!SubjectExpr::ClassName("User".to_string()).is_self_like());
}

// ── to_subject_text round-trip ──────────────────────────────────────

#[test]
fn round_trip_this() {
    assert_eq!(SubjectExpr::parse("$this").to_subject_text(), "$this");
}

#[test]
fn round_trip_self() {
    assert_eq!(SubjectExpr::parse("self").to_subject_text(), "self");
}

#[test]
fn round_trip_variable() {
    assert_eq!(SubjectExpr::parse("$user").to_subject_text(), "$user");
}

#[test]
fn round_trip_property_chain() {
    assert_eq!(
        SubjectExpr::parse("$this->name").to_subject_text(),
        "$this->name"
    );
}

#[test]
fn round_trip_nested_property_chain() {
    assert_eq!(
        SubjectExpr::parse("$user->address->city").to_subject_text(),
        "$user->address->city"
    );
}

#[test]
fn round_trip_function_call() {
    assert_eq!(SubjectExpr::parse("app()").to_subject_text(), "app()");
}

#[test]
fn round_trip_method_call() {
    assert_eq!(
        SubjectExpr::parse("$this->getUser()").to_subject_text(),
        "$this->getUser()"
    );
}

#[test]
fn round_trip_static_method_call() {
    assert_eq!(
        SubjectExpr::parse("User::find(1)").to_subject_text(),
        "User::find(1)"
    );
}

#[test]
fn round_trip_static_access() {
    assert_eq!(
        SubjectExpr::parse("Status::Active").to_subject_text(),
        "Status::Active"
    );
}

#[test]
fn round_trip_class_name() {
    assert_eq!(SubjectExpr::parse("User").to_subject_text(), "User");
}

#[test]
fn round_trip_chained_call_then_property() {
    assert_eq!(
        SubjectExpr::parse("$this->getFactory()->config").to_subject_text(),
        "$this->getFactory()->config"
    );
}

#[test]
fn round_trip_chained_method_calls() {
    assert_eq!(
        SubjectExpr::parse("$this->getFactory()->create()").to_subject_text(),
        "$this->getFactory()->create()"
    );
}

#[test]
fn round_trip_array_access() {
    // Numeric index `[0]` is parsed as `IntKey` and round-trips
    // preserving the index value so positional shape entries can be
    // addressed downstream.
    assert_eq!(
        SubjectExpr::parse("$response['items'][0]").to_subject_text(),
        "$response['items'][0]"
    );
}

#[test]
fn round_trip_interleaved_bracket_arrow_bracket() {
    assert_eq!(
        SubjectExpr::parse("$results[]->activities[]").to_subject_text(),
        "$results[]->activities[]"
    );
}

#[test]
fn round_trip_triple_interleaved() {
    assert_eq!(
        SubjectExpr::parse("$a[]->b[]->c[]").to_subject_text(),
        "$a[]->b[]->c[]"
    );
}

// ── Whitespace trimming ─────────────────────────────────────────────

#[test]
fn parse_trims_whitespace() {
    assert_eq!(SubjectExpr::parse("  $this  "), SubjectExpr::This);
}

// ── Edge: empty string ──────────────────────────────────────────────

#[test]
fn parse_empty_string() {
    assert_eq!(
        SubjectExpr::parse(""),
        SubjectExpr::ClassName(String::new())
    );
}

// ── Complex chain: static → method → method → property ─────────────

#[test]
fn parse_complex_chain() {
    // ClassName::make()->process()->result
    let parsed = SubjectExpr::parse("ClassName::make()->process()->result");
    assert_eq!(
        parsed,
        SubjectExpr::PropertyChain {
            base: Box::new(SubjectExpr::CallExpr {
                callee: Box::new(SubjectExpr::MethodCall {
                    base: Box::new(SubjectExpr::CallExpr {
                        callee: Box::new(SubjectExpr::StaticMethodCall {
                            class: "ClassName".to_string(),
                            method: "make".to_string(),
                        }),
                        args_text: "".to_string(),
                    }),
                    method: "process".to_string(),
                }),
                args_text: "".to_string(),
            }),
            property: "result".to_string(),
        }
    );
}

// ── Method call with nested parens in args ──────────────────────────

#[test]
fn parse_call_with_nested_parens_in_args() {
    // app(config('key'))
    let parsed = SubjectExpr::parse("app(config('key'))");
    assert_eq!(
        parsed,
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::FunctionCall("app".to_string())),
            args_text: "config('key')".to_string(),
        }
    );
}

// ── Double-quoted string key in array access ────────────────────────

#[test]
fn parse_double_quoted_array_key() {
    assert_eq!(
        SubjectExpr::parse("$data[\"name\"]"),
        SubjectExpr::ArrayAccess {
            base: Box::new(SubjectExpr::Variable("$data".to_string())),
            segments: vec![BracketSegment::StringKey("name".to_string())],
        }
    );
}

// ── Call expression as callee base: `app()->make()` ─────────────────

#[test]
fn parse_function_call_then_method() {
    let parsed = SubjectExpr::parse("app()->make()");
    assert_eq!(
        parsed,
        SubjectExpr::CallExpr {
            callee: Box::new(SubjectExpr::MethodCall {
                base: Box::new(SubjectExpr::CallExpr {
                    callee: Box::new(SubjectExpr::FunctionCall("app".to_string())),
                    args_text: "".to_string(),
                }),
                method: "make".to_string(),
            }),
            args_text: "".to_string(),
        }
    );
}

// ── `$this->prop` call vs property disambiguation ───────────────────

#[test]
fn parse_this_method_vs_property() {
    // `$this->getFactory()` should be a call, not a property chain
    let parsed = SubjectExpr::parse("$this->getFactory()");
    assert!(matches!(parsed, SubjectExpr::CallExpr { .. }));

    // `$this->factory` should be a property chain
    let parsed = SubjectExpr::parse("$this->factory");
    assert!(matches!(parsed, SubjectExpr::PropertyChain { .. }));
}

// ── Static access vs static call disambiguation ─────────────────────

#[test]
fn parse_static_access_vs_call() {
    // `Status::Active` → StaticAccess
    assert!(matches!(
        SubjectExpr::parse("Status::Active"),
        SubjectExpr::StaticAccess { .. }
    ));

    // `User::find(1)` → CallExpr wrapping StaticMethodCall
    assert!(matches!(
        SubjectExpr::parse("User::find(1)"),
        SubjectExpr::CallExpr { .. }
    ));
}

// ── Static call chain with `->` after `::` ─────────────────────────

#[test]
fn parse_static_then_arrow_chain() {
    // `ClassName::make()->config` should be PropertyChain, not StaticAccess
    let parsed = SubjectExpr::parse("ClassName::make()->config");
    assert!(matches!(parsed, SubjectExpr::PropertyChain { .. }));
}

#[test]
fn parse_parenthesized_property_invocation() {
    // `($this->formatter)()->write` — the call target is a parenthesized
    // property access, and the outer `()` invokes it.  The result is then
    // chained via `->write`.
    let parsed = SubjectExpr::parse("($this->formatter)()->write");
    // Top level: PropertyChain { base: CallExpr { … }, property: "write" }
    match &parsed {
        SubjectExpr::PropertyChain { base, property } => {
            assert_eq!(property, "write");
            match base.as_ref() {
                SubjectExpr::CallExpr { callee, args_text } => {
                    assert_eq!(args_text, "");
                    // The callee should be parsed as $this->formatter
                    // (a PropertyChain), NOT as FunctionCall("($this->formatter)")
                    match callee.as_ref() {
                        SubjectExpr::PropertyChain {
                            base: inner_base,
                            property: inner_prop,
                        } => {
                            assert!(matches!(inner_base.as_ref(), SubjectExpr::This));
                            assert_eq!(inner_prop, "formatter");
                        }
                        other => panic!("Expected PropertyChain callee, got: {other:?}"),
                    }
                }
                other => panic!("Expected CallExpr base, got: {other:?}"),
            }
        }
        other => panic!("Expected PropertyChain, got: {other:?}"),
    }
}

#[test]
fn parse_parenthesized_variable_invocation() {
    // `($var)()` — parenthesized variable used as callee.
    let parsed = SubjectExpr::parse("($var)()");
    match &parsed {
        SubjectExpr::CallExpr { callee, args_text } => {
            assert_eq!(args_text, "");
            assert!(
                matches!(callee.as_ref(), SubjectExpr::Variable(v) if v == "$var"),
                "Expected Variable($var), got: {callee:?}"
            );
        }
        other => panic!("Expected CallExpr, got: {other:?}"),
    }
}

// ── Call expression with array access ───────────────────────────────

#[test]
fn parse_instance_method_call_array_access() {
    // `$c->items()[]` — method returning array, indexed inline.
    let parsed = SubjectExpr::parse("$c->items()[]");
    match &parsed {
        SubjectExpr::ArrayAccess { base, segments } => {
            assert!(
                matches!(base.as_ref(), SubjectExpr::CallExpr { .. }),
                "Expected CallExpr base, got: {base:?}"
            );
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0], BracketSegment::ElementAccess);
        }
        other => panic!("Expected ArrayAccess, got: {other:?}"),
    }
}

#[test]
fn parse_static_method_call_array_access() {
    // `Collection::all()[]` — static method returning array, indexed.
    let parsed = SubjectExpr::parse("Collection::all()[]");
    match &parsed {
        SubjectExpr::ArrayAccess { base, segments } => {
            assert!(
                matches!(base.as_ref(), SubjectExpr::CallExpr { .. }),
                "Expected CallExpr base, got: {base:?}"
            );
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0], BracketSegment::ElementAccess);
        }
        other => panic!("Expected ArrayAccess, got: {other:?}"),
    }
}

#[test]
fn parse_function_call_array_access() {
    // `getItems()[]` — function returning array, indexed.
    let parsed = SubjectExpr::parse("getItems()[]");
    match &parsed {
        SubjectExpr::ArrayAccess { base, segments } => {
            assert!(
                matches!(base.as_ref(), SubjectExpr::CallExpr { .. }),
                "Expected CallExpr base, got: {base:?}"
            );
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0], BracketSegment::ElementAccess);
        }
        other => panic!("Expected ArrayAccess, got: {other:?}"),
    }
}

#[test]
fn parse_call_array_access_with_string_key() {
    // `$c->getData()['name']` — method returning array, keyed access.
    let parsed = SubjectExpr::parse("$c->getData()['name']");
    match &parsed {
        SubjectExpr::ArrayAccess { base, segments } => {
            assert!(
                matches!(base.as_ref(), SubjectExpr::CallExpr { .. }),
                "Expected CallExpr base, got: {base:?}"
            );
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0], BracketSegment::StringKey("name".to_string()));
        }
        other => panic!("Expected ArrayAccess, got: {other:?}"),
    }
}

#[test]
fn parse_interleaved_bracket_arrow_bracket() {
    // `$results[]->activities[]` — array element, then property, then array element.
    // Should parse as ArrayAccess { PropertyChain { ArrayAccess { Variable, [EA] }, "activities" }, [EA] }.
    let parsed = SubjectExpr::parse("$results[]->activities[]");
    match &parsed {
        SubjectExpr::ArrayAccess { base, segments } => {
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0], BracketSegment::ElementAccess);
            match base.as_ref() {
                SubjectExpr::PropertyChain {
                    base: inner_base,
                    property,
                } => {
                    assert_eq!(property, "activities");
                    match inner_base.as_ref() {
                        SubjectExpr::ArrayAccess {
                            base: var_base,
                            segments: inner_segs,
                        } => {
                            assert_eq!(
                                *var_base.as_ref(),
                                SubjectExpr::Variable("$results".to_string())
                            );
                            assert_eq!(inner_segs.len(), 1);
                            assert_eq!(inner_segs[0], BracketSegment::ElementAccess);
                        }
                        other => panic!("Expected inner ArrayAccess, got: {other:?}"),
                    }
                }
                other => panic!("Expected PropertyChain, got: {other:?}"),
            }
        }
        other => panic!("Expected ArrayAccess, got: {other:?}"),
    }
}

#[test]
fn parse_interleaved_bracket_arrow_no_trailing_bracket() {
    // `$results[]->activities` — array element, then property (no trailing bracket).
    // Should parse as PropertyChain { ArrayAccess { Variable, [EA] }, "activities" }.
    let parsed = SubjectExpr::parse("$results[]->activities");
    match &parsed {
        SubjectExpr::PropertyChain { base, property } => {
            assert_eq!(property, "activities");
            match base.as_ref() {
                SubjectExpr::ArrayAccess {
                    base: var_base,
                    segments,
                } => {
                    assert_eq!(
                        *var_base.as_ref(),
                        SubjectExpr::Variable("$results".to_string())
                    );
                    assert_eq!(segments.len(), 1);
                    assert_eq!(segments[0], BracketSegment::ElementAccess);
                }
                other => panic!("Expected ArrayAccess base, got: {other:?}"),
            }
        }
        other => panic!("Expected PropertyChain, got: {other:?}"),
    }
}

#[test]
fn parse_interleaved_string_key_arrow_bracket() {
    // `$data['items']->name` — string-key access, then property.
    let parsed = SubjectExpr::parse("$data['items']->name");
    match &parsed {
        SubjectExpr::PropertyChain { base, property } => {
            assert_eq!(property, "name");
            match base.as_ref() {
                SubjectExpr::ArrayAccess {
                    base: var_base,
                    segments,
                } => {
                    assert_eq!(
                        *var_base.as_ref(),
                        SubjectExpr::Variable("$data".to_string())
                    );
                    assert_eq!(segments.len(), 1);
                    assert_eq!(segments[0], BracketSegment::StringKey("items".to_string()));
                }
                other => panic!("Expected ArrayAccess base, got: {other:?}"),
            }
        }
        other => panic!("Expected PropertyChain, got: {other:?}"),
    }
}

#[test]
fn parse_triple_interleaved_bracket_arrow() {
    // `$a[]->b[]->c[]` — three levels of interleaved access.
    let parsed = SubjectExpr::parse("$a[]->b[]->c[]");
    match &parsed {
        SubjectExpr::ArrayAccess { base, segments } => {
            assert_eq!(segments.len(), 1);
            assert_eq!(segments[0], BracketSegment::ElementAccess);
            match base.as_ref() {
                SubjectExpr::PropertyChain {
                    base: mid_base,
                    property,
                } => {
                    assert_eq!(property, "c");
                    match mid_base.as_ref() {
                        SubjectExpr::ArrayAccess {
                            base: inner_base,
                            segments: mid_segs,
                        } => {
                            assert_eq!(mid_segs.len(), 1);
                            assert_eq!(mid_segs[0], BracketSegment::ElementAccess);
                            match inner_base.as_ref() {
                                SubjectExpr::PropertyChain {
                                    base: deepest,
                                    property: prop_b,
                                } => {
                                    assert_eq!(prop_b, "b");
                                    match deepest.as_ref() {
                                        SubjectExpr::ArrayAccess {
                                            base: var_base,
                                            segments: deep_segs,
                                        } => {
                                            assert_eq!(
                                                *var_base.as_ref(),
                                                SubjectExpr::Variable("$a".to_string())
                                            );
                                            assert_eq!(deep_segs.len(), 1);
                                            assert_eq!(deep_segs[0], BracketSegment::ElementAccess);
                                        }
                                        other => {
                                            panic!("Expected deepest ArrayAccess, got: {other:?}")
                                        }
                                    }
                                }
                                other => panic!("Expected inner PropertyChain, got: {other:?}"),
                            }
                        }
                        other => panic!("Expected mid ArrayAccess, got: {other:?}"),
                    }
                }
                other => panic!("Expected outer PropertyChain, got: {other:?}"),
            }
        }
        other => panic!("Expected outer ArrayAccess, got: {other:?}"),
    }
}

#[test]
fn parse_this_prop_bracket_arrow_bracket() {
    // `$this->items[]->name` — property chain, bracket, arrow, property.
    let parsed = SubjectExpr::parse("$this->items[]->name");
    match &parsed {
        SubjectExpr::PropertyChain { base, property } => {
            assert_eq!(property, "name");
            match base.as_ref() {
                SubjectExpr::ArrayAccess {
                    base: prop_base,
                    segments,
                } => {
                    assert_eq!(segments.len(), 1);
                    assert_eq!(segments[0], BracketSegment::ElementAccess);
                    match prop_base.as_ref() {
                        SubjectExpr::PropertyChain {
                            base: this_base,
                            property: items_prop,
                        } => {
                            assert_eq!(items_prop, "items");
                            assert_eq!(*this_base.as_ref(), SubjectExpr::This);
                        }
                        other => panic!("Expected PropertyChain for $this->items, got: {other:?}"),
                    }
                }
                other => panic!("Expected ArrayAccess base, got: {other:?}"),
            }
        }
        other => panic!("Expected PropertyChain, got: {other:?}"),
    }
}
