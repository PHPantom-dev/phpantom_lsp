use super::*;

#[test]
fn literal_decorator_bodies_require_the_same_route_and_a_known_prefix() {
    let file = FileContext::default();
    for (body, expected) in [
        (
            r"return new \Tempest\Router\Prefix('/admin')->decorate($route);",
            Some(("/admin", true)),
        ),
        (
            r"return (new \Tempest\Router\Prefix(prefix: ''))->decorate(route: ($route));",
            Some(("", true)),
        ),
        (
            r"$route->uri = ('/admin/' . $route->uri); return $route;",
            Some(("/admin/", false)),
        ),
        (r"return;", None),
        (r"return $route;", None),
        (
            r"return new \Tempest\Router\Prefix('/admin')->decorate($other);",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix('/admin')->other($route);",
            None,
        ),
        (r"return $prefix->decorate($route);", None),
        (r"return new $prefix('/admin')->decorate($route);", None),
        (r"return new Prefix('/admin')->decorate($route);", None),
        (
            r"return new \Tempest\Router\Prefix->decorate($route);",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix()->decorate($route);",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix(other: '/admin')->decorate($route);",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix(...$parts)->decorate($route);",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix('/admin', '/extra')->decorate($route);",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix('/admin')->decorate();",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix('/admin')->decorate(other: $route);",
            None,
        ),
        (
            r"return new \Tempest\Router\Prefix($prefix)->decorate($route);",
            None,
        ),
        (
            "return new \\Tempest\\Router\\Prefix(\"\\xFF\")->decorate($route);",
            None,
        ),
        (
            r"$other->uri = '/admin' . $route->uri; return $route;",
            None,
        ),
        (
            r"$route->other = '/admin' . $route->uri; return $route;",
            None,
        ),
        (r"$route->uri .= '/admin'; return $route;", None),
        (r"$route->uri = '/admin'; return $route;", None),
        (r"$route->uri = 1 + $route->uri; return $route;", None),
        (
            r"$route->uri = '/admin' . $other->uri; return $route;",
            None,
        ),
        (r"$route->uri = $prefix . $route->uri; return $route;", None),
        (
            r"$route->uri = '/admin' . $route->uri; return $other;",
            None,
        ),
        (r"$route->uri = '/admin' . $route->uri; return;", None),
        (r"echo 'ignored'; return $route;", None),
        (r"log(); return $route;", None),
        (
            r"if ($condition) { $route->uri = '/admin' . $route->uri; } return $route;",
            None,
        ),
        (
            r"$route->uri = '/admin' . $route->uri; $route->uri = '/other'; return $route;",
            None,
        ),
    ] {
        let content = format!("<?php class Admin {{ function decorate($route) {{ {body} }} }}");
        let actual =
            crate::parser::with_parsed_program(&content, "decorator_test", |program, _| {
                let mut found = (content.find("decorate($route)").unwrap() as u32, None);
                MethodPrefix { file: &file }.walk_program(program, &mut found);
                found.1
            });
        assert_eq!(
            actual.map(|prefix| (prefix.value, prefix.join)),
            expected.map(|(value, join)| (atom(value), join)),
            "{body}"
        );
    }
}

#[test]
fn abstract_parameterless_and_missing_decorate_methods_have_no_prefix() {
    for content in [
        "<?php abstract class Admin { abstract function decorate($route); }",
        "<?php class Admin { function decorate() { return new \\Tempest\\Router\\Prefix('/admin')->decorate($route); } }",
        "<?php class Admin { function other($route) { return $route; } }",
    ] {
        let prefix = crate::parser::with_parsed_program(content, "decorator_test", |program, _| {
            let mut found = (content.find("decorate").unwrap_or(0) as u32, None);
            MethodPrefix {
                file: &FileContext::default(),
            }
            .walk_program(program, &mut found);
            found.1
        });
        assert!(prefix.is_none(), "{content}");
    }
}

#[test]
fn ordinary_routes_borrow_the_precomputed_path_and_share_negative_decorator_lookups() {
    let backend = crate::test_fixtures::make_backend();
    let declaration = RouteAttribute {
        attribute: atom("Tempest\\Router\\Get"),
        class: atom("Books"),
        method: atom("index"),
        method_offset: 0,
        path: "/api/books".into(),
        original_path: Some("/books".into()),
        decorators: vec![
            RouteDecorator::Prefix(atom("/api")),
            RouteDecorator::Custom(atom("Missing")),
        ]
        .into_boxed_slice(),
        start: 0,
        placeholders: Vec::new(),
        parameters: Box::default(),
    };
    let mut decorators = Decorators::default();
    for _ in 0..2 {
        let resolved = decorators.resolve(&backend, &declaration);
        assert!(matches!(resolved.path, Cow::Borrowed("/api/books")));
        assert_eq!(decorators.prefixes.len(), 1);
    }
}
