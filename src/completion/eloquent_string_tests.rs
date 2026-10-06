use super::*;
use tower_lsp::lsp_types::Position;

/// Resolve the lexical position the completion handler would pass in, then
/// detect the Eloquent string context there.
fn eloquent_ctx_at(content: &str, pos: Position) -> Option<EloquentStringContext> {
    let offset = crate::text_position::position_to_offset(content, pos) as usize;
    let code = crate::completion::source::code_context::code_context_at(content, offset)?;
    detect_eloquent_string_context(content, offset, &code)
}

/// The same for the generic string-in-a-call context underneath it.
fn call_ctx_at(content: &str, pos: Position) -> Option<StringCallContext> {
    let offset = crate::text_position::position_to_offset(content, pos) as usize;
    let code = crate::completion::source::code_context::code_context_at(content, offset)?;
    detect_string_call_context(content, offset, &code)
}

#[test]
fn test_detect_relation_method_with() {
    let content = "<?php\nUser::with('";
    let pos = Position {
        line: 1,
        character: 12,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.partial, "");
    assert_eq!(ctx.subject, "User");
    assert!(ctx.is_static);
}

#[test]
fn test_detect_relation_method_with_partial() {
    let content = "<?php\nUser::with('pos";
    let pos = Position {
        line: 1,
        character: 15,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.partial, "pos");
    assert_eq!(ctx.subject, "User");
}

#[test]
fn test_detect_relation_dot_notation() {
    let content = "<?php\nUser::with('posts.com";
    let pos = Position {
        line: 1,
        character: 21,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.partial, "posts.com");
}

#[test]
fn test_detect_column_method_where() {
    let content = "<?php\nUser::where('";
    let pos = Position {
        line: 1,
        character: 13,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Column);
    assert_eq!(ctx.partial, "");
    assert_eq!(ctx.subject, "User");
}

#[test]
fn test_detect_instance_call() {
    let content = "<?php\n$user->load('";
    let pos = Position {
        line: 1,
        character: 13,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.partial, "");
    assert_eq!(ctx.subject, "$user");
    assert!(!ctx.is_static);
}

#[test]
fn test_detect_in_array_second_element() {
    let content = "<?php\nUser::with(['posts', '";
    let pos = Position {
        line: 1,
        character: 22,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.partial, "");
}

#[test]
fn test_no_detection_outside_string() {
    let content = "<?php\nUser::with(";
    let pos = Position {
        line: 1,
        character: 12,
    };
    assert!(eloquent_ctx_at(content, pos).is_none());
}

#[test]
fn test_no_detection_unknown_method() {
    let content = "<?php\nUser::foo('";
    let pos = Position {
        line: 1,
        character: 11,
    };
    assert!(eloquent_ctx_at(content, pos).is_none());
}

#[test]
fn test_detect_nullsafe_operator() {
    let content = "<?php\n$user?->load('";
    let pos = Position {
        line: 1,
        character: 14,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.subject, "$user");
    assert!(!ctx.is_static);
}

#[test]
fn test_detect_orderby_column() {
    let content = "<?php\n$query->orderBy('na";
    let pos = Position {
        line: 1,
        character: 19,
    };
    let ctx = eloquent_ctx_at(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Column);
    assert_eq!(ctx.partial, "na");
}

/// Build `User::with('a', 'a', …, '` with `args` preceding arguments, so the
/// call's opening paren sits `args * 5` bytes before the cursor's quote.
fn with_call_of_length(args: usize) -> (String, Position) {
    let prefix = "User::with(";
    let line = format!("{prefix}{}'", "'a', ".repeat(args));
    let character = line.chars().count() as u32;
    (format!("<?php\n{line}"), Position { line: 1, character })
}

/// The call is found however far back its opening paren sits, since the scan
/// that finds it runs forward from the start of the file either way.
#[test]
fn test_a_long_argument_list_still_finds_the_call() {
    for args in [100, 1000] {
        let (content, pos) = with_call_of_length(args);
        let ctx = eloquent_ctx_at(&content, pos)
            .unwrap_or_else(|| panic!("a with() call with {args} preceding arguments"));
        assert_eq!(ctx.kind, EloquentStringKind::Relation);
        assert_eq!(ctx.subject, "User");
    }
}

/// The cursor's position in a file is `Position { line, character }` of the end
/// of `content`, which every one of these fixtures is typed up to.
fn at_end(content: &str) -> Position {
    let line = content.lines().count() as u32 - 1;
    let character = content.lines().next_back().unwrap_or("").chars().count() as u32;
    Position { line, character }
}

/// A bracket inside a comment cannot unbalance the search for the call's
/// opening paren, because there is no backwards search left to unbalance.
#[test]
fn test_a_paren_in_a_comment_does_not_hide_the_call() {
    let content = "<?php\nUser::with('a' /* ) */, '";
    let ctx = eloquent_ctx_at(content, at_end(content)).expect("the comment is not code");
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.subject, "User");
}

/// Nor can a comma inside one shift the argument index onto the next
/// parameter.
#[test]
fn test_a_comma_in_a_comment_is_not_an_argument_separator() {
    let content = "<?php\n$q->where('a' /* , */, '";
    let ctx = call_ctx_at(content, at_end(content)).expect("a where() call");
    assert_eq!(ctx.method_name, "where");
    assert_eq!(ctx.arg_index, 1);
}

/// A comment between the callee and its argument list is skipped as well.
#[test]
fn test_a_comment_before_the_argument_list_is_skipped() {
    let content = "<?php\n$q->orderBy /* asc */ ('";
    let ctx =
        eloquent_ctx_at(content, at_end(content)).expect("the comment is not part of the callee");
    assert_eq!(ctx.kind, EloquentStringKind::Column);
    assert_eq!(ctx.subject, "$q");
}

/// A comment between the receiver and the operator does not hide it either,
/// since the operator's boundary comes from the scan rather than a raw
/// backwards text walk.
#[test]
fn test_a_comment_before_the_arrow_does_not_hide_the_receiver() {
    let content = "<?php\n$q /* the query */ ->where('";
    let ctx =
        eloquent_ctx_at(content, at_end(content)).expect("the comment does not hide the receiver");
    assert_eq!(ctx.kind, EloquentStringKind::Column);
    assert_eq!(ctx.subject, "$q");
    assert!(!ctx.is_static);
}

#[test]
fn test_a_comment_before_the_double_colon_does_not_hide_the_receiver() {
    let content = "<?php\nUser /* the model */ ::with('";
    let ctx =
        eloquent_ctx_at(content, at_end(content)).expect("the comment does not hide the receiver");
    assert_eq!(ctx.kind, EloquentStringKind::Relation);
    assert_eq!(ctx.subject, "User");
    assert!(ctx.is_static);
}

/// A receiver that is itself a chain is kept whole, so the resolver sees the
/// relationship call or relation property rather than just its last name.
#[test]
fn test_a_chained_receiver_is_kept_whole() {
    let content = "<?php\n$user->posts()->where('";
    let ctx = eloquent_ctx_at(content, at_end(content)).expect("a where() call");
    assert_eq!(ctx.subject, "$user->posts()");

    let content = "<?php\n$user->posts->where('";
    let ctx = eloquent_ctx_at(content, at_end(content)).expect("a where() call");
    assert_eq!(ctx.subject, "$user->posts");
}

#[test]
fn test_a_multi_line_chain_is_kept_whole() {
    let content = "<?php\n$user->posts()\n    ->latest()\n    ->where('";
    let ctx = eloquent_ctx_at(content, at_end(content)).expect("a where() call");
    assert_eq!(ctx.subject, "$user->posts()->latest()");
}

/// A word between the receiver's operator and an unrelated call (`and`/`or`,
/// or any other identifier) must not be mistaken for that operator's callee.
#[test]
fn test_an_unrelated_call_after_a_receiver_chain_has_no_subject() {
    let content = "<?php\nif ($a->foo and bar('";
    let ctx = call_ctx_at(content, at_end(content)).expect("a bar() call");
    assert_eq!(ctx.method_name, "bar");
    assert!(ctx.subject.is_none());
    assert!(!ctx.is_static);
}

/// A comma of a plain call argument list to the operator's left resets the
/// pending operator, so a later bare call in the same list is not credited
/// with an earlier argument's receiver.
#[test]
fn test_a_bare_call_after_a_receiver_argument_has_no_subject() {
    let content = "<?php\nfoo($a->bar, baz('";
    let ctx = call_ctx_at(content, at_end(content)).expect("a baz() call");
    assert_eq!(ctx.method_name, "baz");
    assert!(ctx.subject.is_none());
}

/// A comma of a nested array belongs to that array, so an element of it is
/// still part of the argument the array is.
#[test]
fn test_an_element_of_an_argument_array_keeps_the_array_index() {
    let content = "<?php\n$q->where('a', ['b', '";
    let ctx = call_ctx_at(content, at_end(content)).expect("a where() call");
    assert_eq!(ctx.arg_index, 1);
}
