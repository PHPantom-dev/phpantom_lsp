use super::*;
use tower_lsp::lsp_types::Position;

#[test]
fn test_detect_relation_method_with() {
    let content = "<?php\nUser::with('";
    let pos = Position {
        line: 1,
        character: 12,
    };
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
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
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
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
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
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
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
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
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
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
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
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
    assert!(detect_eloquent_string_context(content, pos).is_none());
}

#[test]
fn test_no_detection_unknown_method() {
    let content = "<?php\nUser::foo('";
    let pos = Position {
        line: 1,
        character: 11,
    };
    assert!(detect_eloquent_string_context(content, pos).is_none());
}

#[test]
fn test_detect_nullsafe_operator() {
    let content = "<?php\n$user?->load('";
    let pos = Position {
        line: 1,
        character: 14,
    };
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
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
    let ctx = detect_eloquent_string_context(content, pos).unwrap();
    assert_eq!(ctx.kind, EloquentStringKind::Column);
    assert_eq!(ctx.partial, "na");
}
