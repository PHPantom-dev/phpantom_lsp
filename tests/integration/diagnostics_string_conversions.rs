use crate::common::message_text;
use crate::common::{collect_diagnostics_with, create_test_backend};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let mut out = collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_slow_diagnostics,
    );
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, Code::String(s) if s == "invalid_string_conversion"))
    });
    out
}

/// The lines (0-based) of every string conversion diagnostic.
fn lines(php: &str) -> Vec<u32> {
    collect(php).iter().map(|d| d.range.start.line).collect()
}

// ─── Conversions ────────────────────────────────────────────────────────────

#[test]
fn echoing_a_final_object_without_to_string_is_an_error() {
    let php = r#"<?php
final class Invoice {}

function show(Invoice $invoice): void {
    echo $invoice;
}
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::Error));
    assert_eq!(d.range.start, Position::new(4, 9));
    assert_eq!(d.range.end, Position::new(4, 17));
    assert_eq!(
        message_text(&d.message),
        "Object of class Invoice cannot be converted to string (no __toString() method)"
    );
}

#[test]
fn every_string_conversion_is_flagged() {
    let php = r#"<?php
final class Invoice {}

function show(Invoice $invoice, string $s): void {
    echo 'No. ', $invoice;
    print $invoice;
    $a = 'No. ' . $invoice;
    $b = $invoice . 'x';
    $c = "Invoice $invoice";
    $d = "Invoice {$invoice}";
    $e = (string) $invoice;
    $s .= $invoice;
    $f = <<<TXT
    Invoice $invoice
    TXT;
    $g = 'x' . ($invoice);
    ?><?= $invoice ?><?php
}
"#;
    assert_eq!(lines(php), vec![4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 16]);
}

#[test]
fn a_concat_assignment_to_an_object_is_flagged() {
    let php = r#"<?php
final class Invoice {}

function show(Invoice $invoice): void {
    $invoice .= 'x';
}
"#;
    assert_eq!(lines(php), vec![4]);
}

#[test]
fn a_property_and_a_call_result_are_flagged() {
    let php = r#"<?php
final class Invoice {}
final class Order {
    public Invoice $invoice;
    public function invoice(): Invoice { return $this->invoice; }
}

function show(Order $order): void {
    echo "Invoice {$order->invoice}";
    echo $order->invoice();
    echo "Invoice $order->invoice";
}
"#;
    assert_eq!(lines(php), vec![8, 9, 10]);
}

// ─── Severity ───────────────────────────────────────────────────────────────

#[test]
fn a_class_a_subclass_could_extend_is_a_warning() {
    let php = r#"<?php
class Invoice {}
interface Document {}
abstract class Base {}

function show(Invoice $invoice, Document $document, Base $base): void {
    echo $invoice;
    echo $document;
    echo $base;
}
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 3, "got {diags:?}");
    assert!(
        diags
            .iter()
            .all(|d| d.severity == Some(DiagnosticSeverity::Warning)),
        "got {diags:?}"
    );
}

#[test]
fn an_enum_is_an_error() {
    let php = r#"<?php
enum Status { case Open; }

echo Status::Open;
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Error));
    assert_eq!(
        message_text(&diags[0].message),
        "Object of class Status cannot be converted to string (no __toString() method)"
    );
}

#[test]
fn a_union_of_classes_without_to_string_is_flagged() {
    let php = r#"<?php
final class Invoice {}
class Receipt {}

function show(Invoice|Receipt|null $doc): void {
    echo $doc;
}
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::Warning));
    assert_eq!(
        message_text(&diags[0].message),
        "Object of class Invoice|Receipt cannot be converted to string (no __toString() method)"
    );
}

// ─── Convertible values ─────────────────────────────────────────────────────

#[test]
fn a_class_with_to_string_is_not_flagged() {
    let php = r#"<?php
final class Own { public function __toString(): string { return ''; } }
trait Printable { public function __toString(): string { return ''; } }
final class ViaTrait { use Printable; }
class Parent_ { public function __toString(): string { return ''; } }
final class ViaParent extends Parent_ {}
interface Label extends Stringable {}
final class Plain {}

function show(Own $a, ViaTrait $b, ViaParent $c, Label $d, Stringable $e, Plain $control): void {
    echo $a, $b, $c, $d, $e;
    echo $control;
}
"#;
    assert_eq!(lines(php), vec![11]);
}

#[test]
fn a_value_that_may_be_a_string_is_not_flagged() {
    let php = r#"<?php
final class Invoice {}

function show(Invoice|string $a, mixed $b, object $c, ?Invoice $d, Invoice $control): void {
    echo $a;
    echo $b;
    echo $c;
    echo $d ?? 'none';
    echo $control;
}
"#;
    assert_eq!(lines(php), vec![8]);
}

#[test]
fn a_narrowed_value_is_not_flagged() {
    let php = r#"<?php
final class Invoice {}

function show(Invoice|string $a, Invoice $control): void {
    if (is_string($a)) {
        echo $a;
    }
    if (!$a instanceof Invoice) {
        echo $a;
    }
    echo $control;
}
"#;
    assert_eq!(lines(php), vec![10]);
}

#[test]
fn an_unknown_class_is_not_flagged() {
    let php = r#"<?php
function show(Missing $missing): void {
    echo $missing;
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_global_and_a_namespaced_class_with_the_same_name_are_told_apart() {
    let php = r#"<?php
namespace {
    final class Invoice { public function __toString(): string { return ''; } }
}

namespace App {
    final class Invoice {}

    function own(Invoice $invoice): void {
        echo $invoice;
    }

    function global_(\Invoice $invoice): void {
        echo $invoice;
    }
}

namespace Other {
    use App\Invoice;

    function imported(Invoice $invoice): void {
        echo $invoice;
    }

    function unimported(\Invoice $invoice): void {
        echo $invoice;
    }
}
"#;
    let diags = collect(php);
    assert_eq!(
        diags.iter().map(|d| d.range.start.line).collect::<Vec<_>>(),
        vec![9, 21],
        "got {diags:?}"
    );
    assert_eq!(
        message_text(&diags[0].message),
        "Object of class App\\Invoice cannot be converted to string (no __toString() method)"
    );
}
