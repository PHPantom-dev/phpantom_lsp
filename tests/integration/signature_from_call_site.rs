//! The parts of a callable's signature that its own declaration does not
//! spell out.
//!
//! A closure's parameters come from the parameter it is passed to, its
//! return type is narrowed by what its body hands back, and a doc comment
//! written above the statement it is assigned in still types it. On the
//! other side of the same subsystem, a `@template` argument may be
//! reachable only through another template's declared bound, through the
//! class a `class-string` argument names, or through the constructor
//! argument a `@mixin` is bound from.
//!
//! Every case here reported a member access on an unresolved subject, or a
//! type mismatch against a supertype, before the signature was completed.
//!
//! Where the shape is "this subject now resolves", the body reads a member
//! the subject does not have and the test asserts on the *type* the
//! resulting message names: an unresolved subject reports nothing at all,
//! so asserting the absence of diagnostics would pass either way.

use crate::common::create_test_backend_with_full_stubs;

/// Every diagnostic the slow pipeline reports for `php`, as messages.
fn diagnostics(php: &str) -> Vec<String> {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
    out.iter().map(|d| d.message.clone()).collect()
}

fn assert_clean(php: &str) {
    let messages = diagnostics(php);
    assert!(
        messages.is_empty(),
        "expected the signature to be completed, got: {messages:?}"
    );
}

/// The one diagnostic `php` must report, checked for the type it names.
fn assert_only(php: &str, needle: &str) {
    let messages = diagnostics(php);
    assert_eq!(messages.len(), 1, "got: {messages:?}");
    assert!(
        messages[0].contains(needle),
        "expected a message containing {needle:?}, got: {:?}",
        messages[0]
    );
}

// ─── Callback parameters from a templated callable parameter ────────────────

/// `usort($errors, fn ($a, $b) => …)` types both callback parameters from
/// the array's element type. Nothing at the call site names that type: it
/// reaches the callback through the array parameter's own `@template`.
#[test]
fn usort_callback_parameters_come_from_the_array_element() {
    assert_clean(
        r#"<?php
class Err {
    public function getLine(): int { return 0; }
}
class Repro {
    /** @return array{list<Err>, list<Err>} */
    private function gather(): array { return [[], []]; }

    public function run(): void {
        [$errors, $delayed] = $this->gather();
        usort($errors, static function ($a, $b) {
            return $a->getLine() <=> $b->getLine();
        });
        echo count($delayed);
    }
}
"#,
    );
    assert_only(
        r#"<?php
class Err {
    public function getLine(): int { return 0; }
}
class Repro {
    /** @param list<Err> $errors */
    public function run(array $errors): void {
        usort($errors, static function ($a, $b) {
            $a->nope();
            return 0;
        });
    }
}
"#,
        "Method 'nope' not found on class 'Err'",
    );
}

/// `uasort` compares values and `uksort` compares keys, so the same array
/// hands their callbacks different types.
#[test]
fn the_sort_family_hands_its_callback_keys_or_values() {
    let messages = diagnostics(
        r#"<?php
class Repro {
    /** @param array<int, string> $rows */
    public function run(array $rows): void {
        uksort($rows, static function ($a, $b) {
            $a->nope();
            return 0;
        });
        uasort($rows, static function ($c, $d) {
            $c->nope();
            return 0;
        });
    }
}
"#,
    );
    assert_eq!(messages.len(), 2, "got: {messages:?}");
    assert!(messages[0].contains("'int'"), "uksort: {:?}", messages[0]);
    assert!(
        messages[1].contains("'string'"),
        "uasort: {:?}",
        messages[1]
    );
}

/// A `@template` that no argument names directly, only the declared bound
/// of a template that an argument did bind. Written the way PHPStan's own
/// `usort` stub is, so the recovery is not specific to a patched builtin.
#[test]
fn a_template_is_recovered_from_another_templates_bound() {
    assert_only(
        r#"<?php
class Err {
    public function getLine(): int { return 0; }
}
/**
 * @template T
 * @template TArray of array<T>
 *
 * @param TArray $array
 * @param callable(T, T): int $callback
 */
function mysort(array &$array, callable $callback): bool { return true; }

class Repro {
    /** @param list<Err> $errors */
    public function run(array $errors): void {
        mysort($errors, static fn ($a, $b) => $a->nope() <=> $b->getLine());
    }
}
"#,
        "Method 'nope' not found on class 'Err'",
    );
}

/// A by-reference out type still written in the callee's own `@template`
/// params says nothing about the caller's variable, so the precise
/// argument type survives the call rather than being replaced by an array
/// of a name nothing can resolve.
#[test]
fn a_by_ref_out_type_naming_a_template_leaves_the_argument_alone() {
    assert_only(
        r#"<?php
class Item {
    public string $name = '';
}
class Repro {
    /** @param list<Item> $items */
    public function run(array $items): void {
        usort($items, static fn ($a, $b) => strcmp($a->name, $b->name));
        foreach ($items as $item) {
            $item->nope();
        }
    }
}
"#,
        "Method 'nope' not found on class 'Item'",
    );
}

// ─── Closure return types narrowed by the body ──────────────────────────────

/// PHPStan computes a closure's return type as the intersection of what it
/// declares and what its body produces, so a callback that declares a
/// supertype of what it returns still satisfies the narrower parameter.
#[test]
fn a_callback_return_type_is_narrowed_by_its_body() {
    assert_clean(
        r#"<?php
interface MethodReflection {}
interface ExtendedMethodReflection extends MethodReflection {}
interface Prototype {
    public function getTransformedMethod(): ExtendedMethodReflection;
}
class Sink {
    /** @param ExtendedMethodReflection[] $methods */
    public function __construct(array $methods) {}
}
class Repro {
    /** @param Prototype[] $prototypes */
    public function run(array $prototypes): Sink
    {
        return new Sink(array_map(
            static fn (Prototype $p): MethodReflection => $p->getTransformedMethod(),
            $prototypes
        ));
    }
}
"#,
    );
}

/// The narrowing only goes one way: a body that returns something *wider*
/// than the annotation leaves the annotation standing.
#[test]
fn a_callback_keeps_its_declared_return_when_the_body_is_wider() {
    assert_only(
        r#"<?php
interface MethodReflection {}
interface ExtendedMethodReflection extends MethodReflection {}
interface Prototype {
    public function getMethod(): MethodReflection;
}
class Repro {
    /** @param Prototype[] $prototypes */
    public function run(array $prototypes): void
    {
        $methods = array_map(
            static fn (Prototype $p): ExtendedMethodReflection => $p->getMethod(),
            $prototypes
        );
        strlen($methods);
    }
}
"#,
        "ExtendedMethodReflection",
    );
}

/// An untyped callback over a union of two instantiations of one generic
/// class resolves a `@return T` on each alternative rather than handing
/// back the receiver.
#[test]
fn a_callback_over_a_union_of_generic_instantiations_resolves_its_template() {
    assert_clean(
        r#"<?php
interface RuleError {}
interface IdentifierRuleError extends RuleError {}
interface LineRuleError extends RuleError {}

/** @template-covariant T of RuleError */
final class Builder
{
    /** @return self<RuleError> */
    public static function message(string $m): self { return new self(); }
    /** @return self<T&IdentifierRuleError> */
    public function identifier(string $i): self { return $this; }
    /** @return self<T&LineRuleError> */
    public function line(int $l): self { return $this; }
    /** @return T */
    public function build(): RuleError { throw new \Exception(); }
}

class Repro
{
    /** @return list<IdentifierRuleError> */
    public function run(bool $flag): array
    {
        $builders = [];
        if ($flag) {
            $builders[] = Builder::message('x')->identifier('a');
        } else {
            $builders[] = Builder::message('y')->identifier('b')->line(1);
        }

        return array_map(static fn ($builder) => $builder->build(), $builders);
    }
}
"#,
    );
}

// ─── A doc comment above the statement a closure is assigned in ─────────────

/// PHP attaches the comment to the expression statement, not to the
/// closure, but its `@param` tags are still the closure's.
#[test]
fn a_doc_comment_above_a_closure_assignment_types_its_parameters() {
    assert_only(
        r#"<?php
class Arg {
    public mixed $value = null;
}
class Repro {
    public function run(): void {
        /**
         * @param Arg[] $callArgs
         */
        $collect = static function (array $callArgs): void {
            foreach ($callArgs as $callArg) {
                $callArg->nope();
            }
        };
        $collect([]);
    }
}
"#,
        "Method 'nope' not found on class 'Arg'",
    );
}

/// The same annotation, written inline between the assignment and the
/// closure it documents rather than on its own line above the
/// statement, still types the closure's parameters.
#[test]
fn a_doc_comment_inline_with_a_closure_assignment_types_its_parameters() {
    assert_only(
        r#"<?php
class Arg {
    public mixed $value = null;
}
class Repro {
    public function run(): void {
        $collect = /** @param Arg[] $callArgs */ static function (array $callArgs): void {
            foreach ($callArgs as $callArg) {
                $callArg->nope();
            }
        };
        $collect([]);
    }
}
"#,
        "Method 'nope' not found on class 'Arg'",
    );
}

/// The relaxation only steps over an assignment target. A docblock
/// separated from the closure by a statement of its own belongs to that
/// statement, or two closures sharing a parameter name would borrow each
/// other's annotations.
#[test]
fn a_doc_comment_separated_by_a_statement_does_not_type_the_closure() {
    let messages = diagnostics(
        r#"<?php
class Arg {
    public mixed $value = null;
}
class Repro {
    public function run(): void {
        /** @param Arg[] $callArgs */
        $unrelated = 1;
        $collect = static function (array $callArgs): void {
            strlen($callArgs);
        };
        $collect([]);
        echo $unrelated;
    }
}
"#,
    );
    assert_eq!(messages.len(), 1, "got: {messages:?}");
    assert!(
        !messages[0].contains("array<Arg>"),
        "the annotation belongs to the statement above it, got: {:?}",
        messages[0]
    );
}

// ─── Template arguments recovered from the class an argument names ──────────

/// `@template TCollector of Collector<Node, TValue>` with
/// `@param class-string<TCollector>`: `TValue` is nowhere in the call, it
/// is on the named class's own `@implements`, reached through an
/// intermediate generic interface.
#[test]
fn a_template_is_recovered_through_the_named_classs_implements_chain() {
    assert_only(
        r#"<?php
class Node {}
/**
 * @template-covariant TNodeType of Node
 * @template-covariant TValue
 */
interface Collector {}
/**
 * @template-covariant TNodeType of Node
 * @template TValue
 * @extends Collector<TNodeType, TValue>
 */
interface CollectorWithPaths extends Collector {}

class Message {
    public function text(): string { return ''; }
}
/** @implements CollectorWithPaths<Node, Message> */
final class MessageCollector implements CollectorWithPaths {}

final class DataNode {
    /**
     * @template TCollector of Collector<Node, TValue>
     * @template TValue
     * @param class-string<TCollector> $collectorType
     * @return array<string, list<TValue>>
     */
    public function get(string $collectorType): array { return []; }
}

class Repro {
    public function run(DataNode $node): void {
        foreach ($node->get(MessageCollector::class) as $perFile) {
            foreach ($perFile as $message) {
                $message->nope();
            }
        }
    }
}
"#,
        "Method 'nope' not found on class 'Message'",
    );
}

/// The directory-walk idiom: `RecursiveIteratorIterator` binds its
/// `@template` from the iterator it is constructed with and reaches that
/// iterator's element type through `@mixin`.
#[test]
fn recursive_iterator_iterator_iterates_the_wrapped_iterator() {
    assert_clean(
        r#"<?php
class Repro {
    public function run(string $dir): void {
        $iterator = new RecursiveIteratorIterator(
            new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS)
        );
        foreach ($iterator as $file) {
            if ($file->getExtension() !== 'php') {
                continue;
            }
            echo str_replace(DIRECTORY_SEPARATOR, '/', $file->getPathname());
        }
    }
}
"#,
    );
    assert_only(
        r#"<?php
class Repro {
    public function run(string $dir): void {
        foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir)) as $file) {
            $file->nope();
        }
    }
}
"#,
        "'nope'",
    );
}

// ─── A trait method's prototype ─────────────────────────────────────────────

/// A trait has no parent and no interface list, so the `@param` its method
/// implements has to be found on the classes that use it.
#[test]
fn a_trait_method_inherits_the_param_docblock_of_the_interface_it_implements() {
    assert_clean(
        r#"<?php
interface Ty {
    /** @param class-string $ancestorClassName */
    public function getTemplateType(string $ancestorClassName, string $templateTypeName): Ty;
}

trait LateResolvable
{
    abstract public function resolve(): Ty;

    public function getTemplateType(string $ancestorClassName, string $templateTypeName): Ty
    {
        return $this->resolve()->getTemplateType($ancestorClassName, $templateTypeName);
    }
}

final class UserA implements Ty {
    use LateResolvable;
    public function resolve(): Ty { throw new \Exception(); }
}
final class UserB implements Ty {
    use LateResolvable;
    public function resolve(): Ty { throw new \Exception(); }
}
"#,
    );
}
