#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, &Arc::new(php.to_string()));
        let mut out = Vec::new();
        backend.collect_match_type_diagnostics(uri, php, &mut out);
        out
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
