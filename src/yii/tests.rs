use super::aliases::Aliases;
use super::views::{controller_name, normalize_view_path, view_key};

#[test]
fn controller_names_follow_the_renderer() {
    for (class, expected) in [
        (
            "App\\Controller\\FooBar\\BazController",
            Some("foo-bar/baz"),
        ),
        (
            "App\\Controllers\\FooBar\\BazController",
            Some("foo-bar/baz"),
        ),
        (
            "App\\AllControllers\\MyController\\FooBar\\BazController",
            Some("foo-bar/baz"),
        ),
        (
            "App\\AllControllers\\MyController\\BazController",
            Some("baz"),
        ),
        ("Path\\To\\File\\BlogController", Some("blog")),
        ("App\\Web\\Blog\\PostController", Some("post")),
        ("App\\Web\\Blog\\Action", None),
    ] {
        assert_eq!(controller_name(class).as_deref(), expected, "for {class}");
    }
}

#[test]
fn view_names_join_the_path_they_are_relative_to() {
    let base = Some("@views/blog");
    let dir = Some("/app/resources/views/blog");
    assert_eq!(
        view_key("index", base, None).as_deref(),
        Some("@views/blog/index")
    );
    assert_eq!(
        view_key("../shared/menu", base, dir).as_deref(),
        Some("/app/resources/views/shared/menu")
    );
    assert_eq!(
        view_key("./_card", base, dir).as_deref(),
        Some("/app/resources/views/blog/_card")
    );
    assert_eq!(
        view_key("/abs/view", None, None).as_deref(),
        Some("/abs/view")
    );
    // Under the view's base path, or relative to a template the call is
    // not in, a name names nothing that can be told.
    assert_eq!(view_key("//layout", base, dir), None);
    assert_eq!(view_key("./_card", base, None), None);
    assert_eq!(view_key("index", None, None), None);
    assert_eq!(normalize_view_path("@views/../x"), "@views/x");
}

#[test]
fn aliases_resolve_through_each_other() {
    let aliases = Aliases::from_entries(&[
        ("@root", "/app"),
        ("@views", "@root/resources/views"),
        ("@views/admin", "/elsewhere/admin"),
        ("@loop", "@loop/again"),
    ]);
    assert_eq!(
        aliases.resolve("@views/blog/index").as_deref(),
        Some("/app/resources/views/blog/index")
    );
    assert_eq!(
        aliases.resolve("@views/admin/edit").as_deref(),
        Some("/elsewhere/admin/edit")
    );
    assert_eq!(aliases.resolve("@viewsx/index"), None);
    assert_eq!(aliases.resolve("@unknown/x"), None);
    assert_eq!(aliases.resolve("@loop/x"), None);
    assert_eq!(aliases.resolve("/plain").as_deref(), Some("/plain"));
}
