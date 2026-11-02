use super::*;

fn ty(text: &str) -> PhpType {
    PhpType::parse(text)
}

fn url(text: &str) -> Option<String> {
    let parts = url_parts(text)?;
    Some(
        parts
            .into_iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[test]
fn url_parts_of_a_full_url() {
    assert_eq!(
        url("https://example.com:8080/a/b?x=1&y=2#top").as_deref(),
        Some(
            "scheme: 'https', host: 'example.com', port: 8080, path: '/a/b', query: 'x=1&y=2', fragment: 'top'"
        ),
    );
    assert_eq!(
        url("http://example.com?a/b").as_deref(),
        Some("scheme: 'http', host: 'example.com', query: 'a/b'"),
    );
}

#[test]
fn url_parts_decline_what_is_not_reproduced() {
    for text in [
        "//example.com/path",
        "/relative/path",
        "mailto:someone@example.com",
        "http://user:pass@example.com",
        "http://[::1]/",
        "http://example.com:99999",
        "http://example.com:",
        "http://example.com/?",
        "http://example.com/#",
        "http://example.com/a b",
        "http:///path",
        "1http://example.com",
    ] {
        assert_eq!(url(text), None, "{text}");
    }
}

#[test]
fn string_folds() {
    assert_eq!(addslashes("a'b\"c\\d\0"), "a\\'b\\\"c\\\\d\\0");
    assert_eq!(stripslashes("a\\'b\\\\c\\0d\\"), "a'b\\c\0d");
    assert_eq!(backslashit("9a-"), "\\\\9\\a-");
    assert_eq!(normalize_whitespace(" \ta \t b\r\n\n c \n"), "a b\n c");
}

#[test]
fn shortcode_attributes_of_an_untyped_callback_are_strings() {
    let pairs = ty("array{foo: '', bar: 19}");
    let expected = "array{foo: string, bar: 19|string}";
    assert_eq!(
        shortcode_atts(&pairs, &ty("mixed")).map(|t| t.to_string()),
        Some(expected.to_string()),
    );
    assert_eq!(
        shortcode_atts(&pairs, &ty("array|string")).map(|t| t.to_string()),
        Some(expected.to_string()),
    );
    assert_eq!(shortcode_atts(&pairs, &ty("int")), None);
}

#[test]
fn slashing_drops_a_class_name() {
    assert_eq!(slash(&ty("class-string")).to_string(), "string");
    assert_eq!(
        slash(&ty("list<non-empty-string>")).to_string(),
        "list<non-empty-string>"
    );
}

#[test]
fn escaping_maps_generic_arrays() {
    assert_eq!(
        escape_sql(&ty("array<string, int|null>")).map(|t| t.to_string()),
        Some("array<string, string>".to_string()),
    );
    assert_eq!(escape_sql(&ty("mixed")), None);
}
