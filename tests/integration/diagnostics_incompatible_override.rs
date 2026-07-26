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
        backend.collect_incompatible_override_diagnostics(uri, php, &mut out);
        out
    }

    #[test]
    fn static_return_overridden_with_self() {
        let php = r#"<?php
class Foo {
    public static function foo(): ?static {
        return new static();
    }
}

class Bar extends Foo {
    public static function foo(): ?self {
        return new self();
    }
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("must be compatible with"));
        assert!(diags[0].message.contains("Bar::foo()"));
        assert!(diags[0].message.contains("Foo::foo()"));
        assert!(diags[0].message.contains("'static'"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn static_return_preserved_in_child() {
        let php = r#"<?php
class Foo {
    public static function foo(): ?static {
        return new static();
    }
}

class Bar extends Foo {
    public static function foo(): ?static {
        return new static();
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn no_static_in_parent_no_diagnostic() {
        let php = r#"<?php
class Foo {
    public function bar(): ?self {
        return $this;
    }
}

class Bar extends Foo {
    public function bar(): ?self {
        return $this;
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn non_static_return_no_diagnostic() {
        let php = r#"<?php
class Foo {
    public function bar(): string {
        return 'hello';
    }
}

class Bar extends Foo {
    public function bar(): string {
        return 'world';
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn no_parent_class_no_diagnostic() {
        let php = r#"<?php
class Standalone {
    public function doStuff(): ?self {
        return $this;
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn interface_no_diagnostic() {
        let php = r#"<?php
interface Foo {
    public function bar(): string;
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn diagnostic_code_is_correct() {
        let php = r#"<?php
class Base {
    public function make(): static {
        return new static();
    }
}
class Child extends Base {
    public function make(): self {
        return new self();
    }
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("incompatible_override".to_string()))
        );
    }

    #[test]
    fn no_native_return_type_no_diagnostic() {
        let php = r#"<?php
class Foo {
    public static function foo(): ?static {
        return new static();
    }
}

class Bar extends Foo {
    public static function foo() {
        return new self();
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn parent_no_native_return_type_no_diagnostic() {
        let php = r#"<?php
class Foo {
    public static function foo() {
        return new static();
    }
}

class Bar extends Foo {
    public static function foo(): ?self {
        return new self();
    }
}
"#;
        assert!(collect(php).is_empty());
    }
}
