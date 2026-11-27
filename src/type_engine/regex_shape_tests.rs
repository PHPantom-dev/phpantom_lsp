use super::{
    PREG_OFFSET_CAPTURE, PREG_SET_ORDER, PREG_UNMATCHED_AS_NULL, matches_type_of_any,
    opaque_matches_type, split_delimiters,
};

fn matches_type(pattern: &str, flags: i64, matches_all: bool) -> Option<crate::php_type::PhpType> {
    matches_type_of_any(&[pattern], flags, matches_all)
}

fn shape(pattern: &str) -> String {
    matches_type(pattern, 0, false)
        .map(|t| t.to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

fn shape_all(pattern: &str, flags: i64) -> String {
    matches_type(pattern, flags, true)
        .map(|t| t.to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

#[test]
fn splits_symmetric_delimiters() {
    assert_eq!(split_delimiters("/foo/i"), Some(("foo", "i")));
    assert_eq!(split_delimiters("#foo#"), Some(("foo", "")));
    assert_eq!(split_delimiters("~a/b~u"), Some(("a/b", "u")));
    // Leading whitespace and spaced-out modifiers are both allowed.
    assert_eq!(split_delimiters("  /foo/ i u"), Some(("foo", " i u")));
    // The body ends at the first *unescaped* delimiter.
    assert_eq!(split_delimiters(r"_foo(.)\_i_i"), Some((r"foo(.)\_i", "i")));
}

#[test]
fn splits_bracket_delimiters() {
    assert_eq!(split_delimiters("(foo)i"), Some(("foo", "i")));
    assert_eq!(split_delimiters("(a(b)c)"), Some(("a(b)c", "")));
    assert_eq!(split_delimiters("{a}"), Some(("a", "")));
    assert_eq!(split_delimiters("<a>"), Some(("a", "")));
}

#[test]
fn rejects_malformed_patterns() {
    assert_eq!(split_delimiters(""), None);
    assert_eq!(split_delimiters("/unterminated"), None);
    // An alphanumeric or backslash delimiter is not allowed.
    assert_eq!(split_delimiters("afooa"), None);
    assert_eq!(split_delimiters(r"\foo\"), None);
}

#[test]
fn counts_plain_groups() {
    assert_eq!(shape("/(a)(b)/"), "array{0: string, 1: 'a', 2: 'b'}");
    // Non-capturing, lookahead, lookbehind and atomic groups do not count.
    assert_eq!(shape("/(?:a)(?=b)(?<=c)(?>d)/"), "array{0: string}");
    assert_eq!(
        shape("/Price: (?:£|€)(\\d+)/"),
        "array{0: string, 1: string}"
    );
}

#[test]
fn ignores_parens_that_are_not_groups() {
    for pattern in [
        r"/\(a\)/",
        "/[(]/",
        r"/[\](]/",
        "/[])(]/",
        "/[[:alpha:](]/",
        "/(?#a comment with a ( in it)/",
        r"/\Q(a)\E/",
    ] {
        assert_eq!(shape(pattern), "array{0: string}", "{pattern}");
    }
}

#[test]
fn reads_named_groups() {
    let expected = "array{0: string, num: string, 1: string}";
    assert_eq!(shape("/(?<num>\\d+)/"), expected);
    assert_eq!(shape("/(?P<num>\\d+)/"), expected);
    assert_eq!(shape("/(?'num'\\d+)/"), expected);
}

#[test]
fn a_group_of_fixed_text_holds_its_literals() {
    assert_eq!(shape("/Price: (£|€)\\d+/"), "array{0: string, 1: '£'|'€'}");
    assert_eq!(shape("/([157])/"), "array{0: string, 1: '1'|'5'|'7'}");
    assert_eq!(shape("/(ba[rz])/"), "array{0: string, 1: 'bar'|'baz'}");
    assert_eq!(shape(r"/(\.)/"), "array{0: string, 1: '.'}");
    assert_eq!(shape("/()/"), "array{0: string, 1: ''}");
    // A newline is spelled the way PHP would write it.
    assert_eq!(shape("/(\\n)/"), "array{0: string, 1: \"\\n\"}");
}

#[test]
fn a_group_that_can_match_varied_text_is_a_string() {
    for pattern in [
        "/(.)/",
        "/(\\d)/",
        "/([a-z])/",
        "/([^a])/",
        "/(a+)/",
        // A letter under case-insensitive matching is not one text.
        "/(a)/i",
        "/(?i)(a)/",
        "/([ab])/i",
    ] {
        assert_eq!(shape(pattern), "array{0: string, 1: string}", "{pattern}");
    }
    // Case-insensitivity cannot change a group without letters.
    assert_eq!(shape("/(1)/i"), "array{0: string, 1: '1'}");
    // A repeated group holds one repetition, but the literals are dropped as
    // PHPStan drops them.
    assert_eq!(shape("/(a)+/"), "array{0: string, 1: string}");
}

#[test]
fn only_trailing_optional_groups_get_optional_keys() {
    // PHP reports an unmatched group followed by a matched one as `''`, and
    // drops only the trailing ones.
    assert_eq!(
        shape("/(a)(b)*(c)(d)*/"),
        "list{0: string, 1: 'a', 2: string, 3: 'c', 4?: string}"
    );
    assert_eq!(
        shape("/(a)(b)*(c)(?<name>d)*/"),
        "array{0: string, 1: 'a', 2: string, 3: 'c', name?: string, 4?: string}"
    );
    // An optional group before a required one may be `''`.
    assert_eq!(shape("/(a)?(b)/"), "array{0: string, 1: 'a'|'', 2: 'b'}");
}

/// A shape with and without the group, which fold into one with the group's
/// key optional, as PHPStan folds them.
#[test]
fn a_single_optional_group_may_be_left_out() {
    assert_eq!(shape("/x(a)?/"), "list{0: string, 1?: 'a'}");
    // Two optional groups take the general rule: each may be missing.
    assert_eq!(
        shape("/x(?:(a)(b))?/"),
        "list{0: string, 1?: 'a'|'', 2?: 'b'}"
    );
}

#[test]
fn a_top_level_alternation_gives_a_shape_per_branch() {
    // An earlier branch's groups read `''`; a later one's are left out.
    assert_eq!(
        shape(r"/^(?:(\d+)x(\d+)|(\d+)|x(\d+))$/"),
        "array{0: string, 1: string, 2: string}|array{0: string, 1: '', 2: '', 3: string}|array{0: string, 1: '', 2: '', 3: '', 4: string}"
    );
    // A branch with no group adds the whole match on its own, which folds
    // into the other branch's shape.
    assert_eq!(shape("/foo|(bar)/"), "list{0: string, 1?: 'bar'}");
}

#[test]
fn a_negative_lookaround_never_fills_its_groups() {
    assert_eq!(shape("/x(?!(a))/"), "list{0: string, 1?: 'a'}");
}

#[test]
fn branch_reset_numbers_each_branch_from_the_same_group() {
    assert_eq!(
        shape("/(?|(a)|(b)(c)|(d)(e)(f))/"),
        "list{0: string, 1: string, 2?: string, 3?: string}"
    );
    assert_eq!(
        shape("/(?|(a)|(b))(c)/"),
        "array{0: string, 1: string, 2: 'c'}"
    );
}

#[test]
fn extended_mode_skips_whitespace_and_comments() {
    assert_eq!(shape("/( a ) # (comment)/x"), "array{0: string, 1: 'a'}");
    assert_eq!(
        shape("/(?x) ( a ) # (comment)/"),
        "array{0: string, 1: 'a'}"
    );
    assert_eq!(shape("/^ ([\\#.]) $/x"), "array{0: string, 1: '#'|'.'}");
    // An escaped `#` is a literal, not a comment.
    assert_eq!(shape("/(\\#) (b)/x"), "array{0: string, 1: '#', 2: 'b'}");
}

#[test]
fn a_mark_verb_adds_its_key() {
    assert_eq!(
        shape("/a(*MARK:x)(b)/"),
        "array{0: string, 1: 'b', MARK?: 'x'}"
    );
}

#[test]
fn refuses_constructs_it_cannot_model() {
    for pattern in [
        // Conditional groups, recursion and subroutine calls.
        "/(a)(?(1)b|c)/",
        "/(a(?R))/",
        "/(a)(?1)/",
        "/(a)(?&name)/",
        // `(*ACCEPT)` ends the match with groups still open.
        "/(a(*ACCEPT))/",
        // Unbalanced parens.
        "/(a/",
        "/a)/",
        // An unknown modifier means the pattern is not one PHP would accept.
        "/(a)/z",
        // Two groups sharing a name without `J`.
        "/(?<n>a)|(?<n>b)/",
    ] {
        assert_eq!(shape(pattern), "<none>", "{pattern}");
    }
}

#[test]
fn a_shared_name_is_allowed_under_j() {
    assert_eq!(
        shape("/(?<n>a)|(?<n>b)/J"),
        "array{0: string, n: 'a', 1: 'a'}|array{0: string, n: 'b', 1: '', 2: 'b'}"
    );
}

#[test]
fn no_auto_capture_leaves_only_named_groups() {
    assert_eq!(
        shape("/(a)(?<name>b)/n"),
        "array{0: string, name: 'b', 1: 'b'}"
    );
    assert_eq!(shape("/(?n)(a)/"), "array{0: string}");
}

#[test]
fn shapes_from_several_patterns_join() {
    assert_eq!(
        matches_type_of_any(&["/(a)/", "/(b)/"], 0, false)
            .unwrap()
            .to_string(),
        "array{0: string, 1: 'a'|'b'}"
    );
}

#[test]
fn shapes_the_whole_match_and_every_group() {
    assert_eq!(shape("/Price: /i"), "array{0: string}");
    assert_eq!(shape("/(a)(b)/"), "array{0: string, 1: 'a', 2: 'b'}");
    assert_eq!(
        shape("/\\w-(?P<num>\\d+)-(\\w)/"),
        "array{0: string, num: string, 1: string, 2: string}"
    );
}

#[test]
fn unmatched_as_null_reports_every_group() {
    assert_eq!(
        matches_type("/(a)(b)?/", PREG_UNMATCHED_AS_NULL, false)
            .unwrap()
            .to_string(),
        "array{0: string, 1: 'a', 2: 'b'|null}"
    );
}

#[test]
fn offset_capture_pairs_each_entry_with_its_offset() {
    assert_eq!(
        matches_type("/(a)/", PREG_OFFSET_CAPTURE, false)
            .unwrap()
            .to_string(),
        "array{0: array{string, int<-1, max>}, 1: array{'a', int<-1, max>}}"
    );
}

#[test]
fn match_all_collects_each_group_in_pattern_order() {
    assert_eq!(
        shape_all("/(a)(b)?/", 0),
        "array{0: list<string>, 1: list<'a'>, 2: list<'b'|''>}"
    );
}

#[test]
fn match_all_in_set_order_is_a_list_of_match_shapes() {
    assert_eq!(
        shape_all("/(a)(b)?/", PREG_SET_ORDER),
        "list<array{0: string, 1: 'a', 2?: 'b'}>"
    );
}

#[test]
fn unmodelled_flags_abandon_the_analysis() {
    assert_eq!(matches_type("/(a)/", 4096, false), None);
    assert_eq!(opaque_matches_type(4096, false), None);
}

#[test]
fn an_unanalysable_pattern_still_types_the_entries() {
    assert_eq!(
        opaque_matches_type(0, false).unwrap().to_string(),
        "array<array-key, string>"
    );
    assert_eq!(
        opaque_matches_type(0, true).unwrap().to_string(),
        "array<array-key, list<string>>"
    );
    assert_eq!(
        opaque_matches_type(PREG_SET_ORDER, true)
            .unwrap()
            .to_string(),
        "list<array<array-key, string>>"
    );
}
