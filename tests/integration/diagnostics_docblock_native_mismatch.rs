#[cfg(test)]
mod tests {
    use crate::common::collect_diagnostics_with;
    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    fn collect(php: &str) -> Vec<Diagnostic> {
        collect_diagnostics_with(
            &Backend::new_test(),
            php,
            Backend::collect_docblock_native_mismatch_diagnostics,
        )
    }

    #[test]
    fn scalar_param_wider_than_native_hint_is_flagged() {
        let php = r#"<?php
/**
 * @param ?string $name
 */
function greet(string $name): void {}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String(
                "docblock_native_mismatch".to_string()
            ))
        );
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
        assert!(diags[0].message.contains("'?string'"));
        assert!(diags[0].message.contains("$name"));
        assert!(diags[0].message.contains("'string'"));
        // The range covers `string $name` on the parameter line.
        assert_eq!(diags[0].range.start.line, 4);
        assert_eq!(diags[0].range.end.line, 4);
    }

    #[test]
    fn array_param_wider_than_native_hint_is_flagged() {
        let php = r#"<?php
/**
 * @param list<int>|null $items
 */
function takesItems(array $items): void {}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(diags[0].message.contains("'list<int>|null'"));
        assert!(diags[0].message.contains("'array'"));
    }

    #[test]
    fn bare_null_docblock_type_is_flagged() {
        let php = r#"<?php
/**
 * @param null $name
 */
function greet(string $name): void {}
"#;
        assert_eq!(collect(php).len(), 1);
    }

    #[test]
    fn narrower_docblock_on_nullable_hint_is_clean() {
        // The native `?string` carries its null over to the documented type,
        // so omitting `|null` from the docblock is not a contradiction.
        let php = r#"<?php
/**
 * @param non-empty-string $name
 * @param list<int> $items
 * @param class-string $type
 */
function greet(?string $name, ?array $items, ?string $type = null): void {}
"#;
        assert!(collect(php).is_empty(), "got: {:?}", collect(php));
    }

    #[test]
    fn union_spelling_of_native_null_is_clean_too() {
        let php = r#"<?php
/**
 * @param non-empty-string $name
 */
function greet(string|null $name): void {}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn matching_nullable_docblock_is_clean() {
        let php = r#"<?php
/**
 * @param string|null $name
 * @param list<int>|null $items
 * @param ?int $count
 */
function greet(?string $name, ?array $items, ?int $count): void {}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn non_nullable_docblock_type_is_clean() {
        let php = r#"<?php
/**
 * @param list<int> $items
 */
function takesItems(array $items): void {}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn implicit_nullable_default_is_clean() {
        // `string $name = null` is PHP's pre-8.4 implicit-nullable form, so
        // the signature does accept the null the docblock documents.
        let php = r#"<?php
/**
 * @param ?non-empty-string $name
 */
function greet(string $name = null): void {}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn a_non_null_default_does_not_excuse_the_docblock() {
        let php = r#"<?php
/**
 * @param ?string $name
 */
function greet(string $name = 'anon'): void {}
"#;
        assert_eq!(collect(php).len(), 1);
    }

    #[test]
    fn widening_to_mixed_is_left_alone() {
        // `mixed` admits null without spelling it.  Widening a native hint all
        // the way to `mixed` is a different mistake than contradicting its
        // nullability, and not this diagnostic's business.
        let php = r#"<?php
/**
 * @param mixed $items
 */
function takesItems(array $items): void {}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn narrowing_native_mixed_is_clean() {
        let php = r#"<?php
/**
 * @param ?list<int> $items
 */
function takesItems(mixed $items): void {}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn class_like_docblock_name_is_left_alone() {
        // `Foo` could be a `@template` parameter or an imported
        // `@psalm-type` alias standing in for a nullable type, so nothing
        // in the annotation settles whether it admits null.
        let php = r#"<?php
/**
 * @template T
 * @param T $value
 * @param Wrapper $wrapper
 */
function unwrap(object $value, object $wrapper): void {}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn return_type_wider_than_native_hint_is_flagged() {
        let php = r#"<?php
class Entry {
    /**
     * @return string|null
     */
    public function getPath(): string
    {
        return $this->path;
    }
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(
            diags[0]
                .message
                .contains("Documented return type 'string|null'")
        );
        assert!(diags[0].message.contains("'string'"));
    }

    #[test]
    fn narrower_return_docblock_on_nullable_hint_is_clean() {
        let php = r#"<?php
class Entry {
    /**
     * @return string
     */
    public function getPath(): ?string
    {
        return $this->path;
    }
}
"#;
        assert!(collect(php).is_empty());
    }

    #[test]
    fn promoted_constructor_property_is_flagged() {
        let php = r#"<?php
final class GenerateFactory
{
    /**
     * @param ?class-string $resultType
     */
    public function __construct(public string $resultType = 'stdClass') {}
}
"#;
        assert_eq!(collect(php).len(), 1);
    }

    #[test]
    fn a_callable_with_a_nullable_return_is_not_itself_nullable() {
        // The `?` belongs to the closure's return type, not to the parameter,
        // so the docblock does not admit null and the native `Closure` hint is
        // not contradicted.
        let php = r#"<?php
class IssetabilityDescriptor
{
    /**
     * @param Closure(Scope): ?PropertyReflection $reflectionResolver
     */
    public function __construct(private Closure $reflectionResolver) {}
}
"#;
        assert!(collect(php).is_empty(), "got: {:?}", collect(php));
    }

    #[test]
    fn vendor_prefixed_tag_is_read() {
        let php = r#"<?php
class Helper {
    /** @phpstan-return array<int, string>|null */
    public function placeholders(string $format): array
    {
        return [];
    }
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(diags[0].message.contains("array<int, string>|null"));
    }

    #[test]
    fn interface_and_trait_methods_are_checked() {
        let php = r#"<?php
namespace App;

interface Reader {
    /**
     * @param ?string $key
     */
    public function read(string $key): void;
}

trait Writer {
    /**
     * @param ?string $key
     */
    public function write(string $key): void {}
}
"#;
        assert_eq!(collect(php).len(), 2);
    }

    #[test]
    fn no_docblock_is_clean() {
        let php = r#"<?php
function greet(?string $name): void {}
"#;
        assert!(collect(php).is_empty());
    }
}
