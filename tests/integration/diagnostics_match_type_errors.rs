#[cfg(test)]
mod tests {
    use crate::common::collect_diagnostics_with;
    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    fn collect(php: &str) -> Vec<Diagnostic> {
        collect_diagnostics_with(
            &Backend::new_test(),
            php,
            Backend::collect_match_type_diagnostics,
        )
    }

    #[test]
    fn int_literal_against_string_subject() {
        let php = r#"<?php
function foo(string $str) {
    return match ($str) {
        'foo' => 'bar',
        321 => 'x',
    };
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("int"));
        assert!(diags[0].message.contains("string"));
        assert!(diags[0].message.contains("==="));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn matching_types_no_diagnostic() {
        let php = r#"<?php
function foo(string $str) {
    return match ($str) {
        'foo' => 'bar',
        'baz' => 'qux',
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn int_subject_with_string_arm() {
        let php = r#"<?php
function foo(int $val) {
    return match ($val) {
        1 => 'one',
        'two' => 'nope',
    };
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("string"));
        assert!(diags[0].message.contains("int"));
    }

    #[test]
    fn bool_against_string() {
        let php = r#"<?php
function foo(string $s) {
    return match ($s) {
        'a' => 1,
        true => 2,
    };
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("bool"));
    }

    #[test]
    fn match_true_no_diagnostic() {
        let php = r#"<?php
function foo(string $s) {
    return match (true) {
        $s === 'a' => 1,
        $s === 'b' => 2,
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn no_type_info_no_diagnostic() {
        let php = r#"<?php
function foo($val) {
    return match ($val) {
        'foo' => 'bar',
        321 => 'x',
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn float_against_int() {
        let php = r#"<?php
function foo(int $n) {
    return match ($n) {
        1 => 'one',
        2.5 => 'nope',
    };
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("float"));
        assert!(diags[0].message.contains("int"));
    }

    #[test]
    fn diagnostic_code_is_correct() {
        let php = r#"<?php
function foo(string $s) {
    return match ($s) {
        123 => 'x',
    };
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("unreachable_match_arm".to_string()))
        );
    }

    #[test]
    fn multiple_incompatible_arms() {
        let php = r#"<?php
function foo(string $s) {
    return match ($s) {
        'ok' => 1,
        42 => 2,
        true => 3,
        3.14 => 4,
    };
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn null_arm_against_nullable_subject() {
        let php = r#"<?php
function foo(?int $n) {
    return match ($n) {
        1 => 'one',
        null => 'none',
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn null_arm_against_union_with_null() {
        let php = r#"<?php
/** @param int|null $n */
function foo($n) {
    return match ($n) {
        1 => 'one',
        null => 'none',
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn bool_arms_against_bool_subject() {
        let php = r#"<?php
function foo(bool $b) {
    return match ($b) {
        true => 1,
        false => 2,
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn arms_covering_each_union_member() {
        let php = r#"<?php
/** @param int|string $v */
function foo($v) {
    return match ($v) {
        1 => 'i',
        'a' => 's',
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    /// A union member we cannot reduce to a scalar means the subject could
    /// hold a value outside the set we recognise, so no arm is provably
    /// unreachable.
    #[test]
    fn union_with_non_scalar_member_no_diagnostic() {
        let php = r#"<?php
class Thing {}
/** @param int|Thing $v */
function foo($v) {
    return match ($v) {
        'nope' => 1,
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn mixed_subject_no_diagnostic() {
        let php = r#"<?php
/** @param mixed $v */
function foo($v) {
    return match ($v) {
        1 => 'i',
        'a' => 's',
        null => 'n',
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn enum_subject_no_diagnostic() {
        let php = r#"<?php
enum Suit: string {
    case Hearts = 'H';
    case Spades = 'S';
}
function foo(Suit $s) {
    return match ($s) {
        Suit::Hearts => 1,
        Suit::Spades => 2,
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn signed_int_literals_against_int_subject() {
        let php = r#"<?php
function foo(int $n) {
    return match ($n) {
        -1 => 'neg',
        +2 => 'pos',
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    /// Indexing a union-typed array resolves to nothing today, so the
    /// ternary keeps only its else branch and the subject comes out as the
    /// literal `'exception'` rather than `mixed` (see B68 in
    /// `docs/todo/bugs.md`). A literal subject must not be read as the
    /// complete set of values the subject can hold.
    #[test]
    fn literal_subject_no_diagnostic() {
        let php = r#"<?php
/** @param array|string|null $service */
function foo($service) {
    $mode = \array_key_exists('mode', $service) ? $service['mode'] : 'exception';

    return match ($mode) {
        'exception' => 1,
        null => 2,
    };
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn default_arm_no_diagnostic() {
        let php = r#"<?php
function foo(string $s) {
    return match ($s) {
        'a' => 1,
        default => 2,
    };
}
"#;
        assert!(collect(php).is_empty());
    }
}
