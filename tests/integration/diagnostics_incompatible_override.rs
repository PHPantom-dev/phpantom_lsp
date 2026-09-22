#[cfg(test)]
mod tests {
    use crate::common::collect_diagnostics_with;
    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    fn collect(php: &str) -> Vec<Diagnostic> {
        collect_diagnostics_with(
            &Backend::new_test(),
            php,
            Backend::collect_incompatible_override_diagnostics,
        )
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

    /// A class method displaces the concrete trait method it collides
    /// with and PHP runs no compatibility check on that pair, so the
    /// narrower return type is legal here.
    #[test]
    fn concrete_trait_method_override_no_diagnostic() {
        let php = r#"<?php
trait Fluent {
    public function fluent(): static {
        return $this;
    }
}

class Uses {
    use Fluent;

    public function fluent(): self {
        return $this;
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn abstract_trait_method_override_is_flagged() {
        let php = r#"<?php
trait RequiresFluent {
    abstract public function fluent(): static;
}

class Uses {
    use RequiresFluent;

    public function fluent(): self {
        return $this;
    }
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("RequiresFluent::fluent()"));
    }

    /// `never` is the bottom type, so it is a legal narrowing of any
    /// declared return type. Symfony's `ButtonBuilder` overrides a whole
    /// interface this way.
    #[test]
    fn never_return_type_no_diagnostic() {
        let php = r#"<?php
interface Buildable {
    public function add(): static;
}

class Button implements Buildable {
    public function add(): never {
        throw new \BadMethodCallException('Buttons cannot have children.');
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn interface_method_override_is_flagged() {
        let php = r#"<?php
interface Chainable {
    public function chain(): static;
}

class Impl implements Chainable {
    public function chain(): self {
        return $this;
    }
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Chainable::chain()"));
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
