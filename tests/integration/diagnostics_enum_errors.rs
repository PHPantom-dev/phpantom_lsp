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
        backend.collect_enum_error_diagnostics(uri, php, &mut out);
        out
    }

    #[test]
    fn valid_unit_enum() {
        let php = r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn valid_string_backed_enum() {
        let php = r#"<?php
enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn valid_int_backed_enum() {
        let php = r#"<?php
enum Priority: int {
    case Low = 1;
    case Medium = 2;
    case High = 3;
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn backed_value_on_unit_enum() {
        let php = r#"<?php
enum MyType {
    case Any = 0;
    case Array = 2;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 2);
        assert!(diags[0].message.contains("must not have a value"));
        assert!(diags[0].message.contains("MyType::Any"));
        assert!(diags[1].message.contains("MyType::Array"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn missing_backed_value() {
        let php = r#"<?php
enum MyType: int {
    case Any = 0;
    case Array;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("must have a value"));
        assert!(diags[0].message.contains("MyType::Array"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn invalid_backing_type_bool() {
        let php = r#"<?php
enum TypeDescriptor: bool {
    case Any = false;
    case Array = true;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("must be 'int' or 'string'"));
        assert!(diags[0].message.contains("bool"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn invalid_backing_type_float() {
        let php = r#"<?php
enum Weight: float {
    case Light = 1.0;
    case Heavy = 9.9;
}
"#;
        let diags = collect(php);
        assert!(diags.iter().any(
            |d| d.message.contains("must be 'int' or 'string'") && d.message.contains("float")
        ),);
    }

    #[test]
    fn duplicate_backed_values() {
        let php = r#"<?php
enum Test: string {
    case FOO = 'baz';
    case BAR = 'baz';
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Duplicate value"));
        assert!(diags[0].message.contains("BAR"));
        assert!(diags[0].message.contains("FOO"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn duplicate_int_backed_values() {
        let php = r#"<?php
enum Numbers: int {
    case A = 1;
    case B = 2;
    case C = 1;
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Duplicate value"));
        assert!(diags[0].message.contains("C"));
        assert!(diags[0].message.contains("A"));
    }

    #[test]
    fn no_diagnostic_for_regular_class() {
        let php = r#"<?php
class Foo {
    const A = 1;
    const B = 2;
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn enum_with_implements() {
        let php = r#"<?php
interface HasValue {}
enum Status: string implements HasValue {
    case Active = 'active';
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn diagnostic_code_is_correct() {
        let php = r#"<?php
enum Test: string {
    case A = 'x';
    case B = 'x';
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("invalid_enum_case".to_string()))
        );
    }

    #[test]
    fn invalid_backing_type_code() {
        let php = r#"<?php
enum Flags: bool {
    case On = true;
}
"#;
        let diags = collect(php);
        assert!(diags.iter().any(|d| d.code
            == Some(NumberOrString::String(
                "invalid_enum_backing_type".to_string()
            ))),);
    }
}
