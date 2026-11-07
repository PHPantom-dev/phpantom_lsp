//! Completion of the values a parameter's `#[ExpectedValues]` attribute
//! declares, at the start of the argument, after a `|` in a flag
//! argument, and inside a string argument.

use crate::common::{
    complete_at, create_test_backend, create_test_backend_with_full_stubs, item_labels,
};
use tower_lsp_server::gen_lsp_types::*;

const DECLARATIONS: &str = concat!(
    "<?php\n",
    "use JetBrains\\PhpStorm\\ExpectedValues;\n",
    "const MODE_A = 1;\n",
    "const MODE_B = 2;\n",
    "const FLAG_A = 1;\n",
    "const FLAG_B = 2;\n",
    "const FLAG_C = 4;\n",
    "function mode(string $label, #[ExpectedValues([MODE_A, MODE_B])] int $mode = MODE_A) {}\n",
    "function flagged(#[ExpectedValues(flags: [FLAG_A, FLAG_B, FLAG_C])] int $flags) {}\n",
    "function act(#[ExpectedValues(values: ['read', \"write\"])] string $how) {}\n",
    "class Level {\n",
    "    const LOW = 1;\n",
    "    const HIGH = 2;\n",
    "    private const SECRET = 3;\n",
    "}\n",
    "function level(#[ExpectedValues(valuesFromClass: Level::class)] int $level) {}\n",
);

/// Complete at the end of `call`, written on the line after the
/// declarations.
async fn complete_call(call: &str) -> Vec<CompletionItem> {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///expected_values.php").unwrap();
    let text = format!("{DECLARATIONS}{call}\n");
    let line = DECLARATIONS.lines().count() as u32;
    complete_at(&backend, &uri, &text, line, call.len() as u32).await
}

fn expected(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|i| {
            i.detail
                .as_deref()
                .is_some_and(|d| d.starts_with("Expected"))
        })
        .map(|i| i.label.as_str())
        .collect()
}

#[tokio::test]
async fn offers_listed_values_for_positional_argument() {
    let items = complete_call("mode('x', ").await;
    assert_eq!(expected(&items), ["MODE_A", "MODE_B"]);

    let first = items.iter().find(|i| i.label == "MODE_A").unwrap();
    assert_eq!(first.kind, Some(CompletionItemKind::Constant));
    assert_eq!(first.insert_text.as_deref(), Some("MODE_A"));
    let named_arg = items.iter().find(|i| i.label.starts_with("mode:")).unwrap();
    assert!(
        first.sort_text < named_arg.sort_text,
        "expected values rank ahead of named arguments"
    );
}

#[tokio::test]
async fn offers_values_while_typing_a_prefix() {
    let items = complete_call("mode('x', MODE").await;
    assert_eq!(expected(&items), ["MODE_A", "MODE_B"]);
    // Listed once, as an expected value, not again as a plain constant.
    assert_eq!(
        item_labels(items)
            .iter()
            .filter(|l| l.as_str() == "MODE_A")
            .count(),
        1
    );
}

#[tokio::test]
async fn nothing_for_another_parameter() {
    let items = complete_call("mode(").await;
    assert!(expected(&items).is_empty(), "{:?}", expected(&items));
}

#[tokio::test]
async fn offers_values_for_named_argument() {
    let items = complete_call("mode(mode: ").await;
    assert_eq!(expected(&items), ["MODE_A", "MODE_B"]);
}

#[tokio::test]
async fn value_argument_is_not_combined() {
    let items = complete_call("mode('x', MODE_A | ").await;
    assert!(expected(&items).is_empty(), "{:?}", expected(&items));
}

#[tokio::test]
async fn flags_continue_after_bitwise_or() {
    let items = complete_call("flagged(").await;
    assert_eq!(expected(&items), ["FLAG_A", "FLAG_B", "FLAG_C"]);

    let items = complete_call("flagged(FLAG_A | FLAG_C | ").await;
    assert_eq!(expected(&items), ["FLAG_B"]);
    assert_eq!(items[0].detail.as_deref(), Some("Expected flag"));
}

#[tokio::test]
async fn literal_values_at_the_argument() {
    let items = complete_call("act(").await;
    assert_eq!(expected(&items), ["'read'", "\"write\""]);
    let first = items.iter().find(|i| i.label == "'read'").unwrap();
    assert_eq!(first.kind, Some(CompletionItemKind::Value));
}

#[tokio::test]
async fn string_values_inside_the_quotes() {
    let items = complete_call("act('w").await;
    assert_eq!(item_labels(items), ["write"]);
}

#[tokio::test]
async fn values_from_class_lists_its_public_constants() {
    let items = complete_call("level(").await;
    assert_eq!(expected(&items), ["Level::LOW", "Level::HIGH"]);
}

/// A class constant is written the way the calling file can name it:
/// by its short name when imported, fully qualified otherwise.
#[tokio::test]
async fn class_constants_are_spelled_for_the_caller() {
    let backend = create_test_backend();
    let lib = Uri::parse("file:///lib.php").unwrap();
    let lib_text = concat!(
        "<?php\n",
        "namespace Lib;\n",
        "use JetBrains\\PhpStorm\\ExpectedValues as EV;\n",
        "class Mode { const ON = 1; const OFF = 0; }\n",
        "function toggle(#[EV([Mode::ON, Mode::OFF])] int $mode) {}\n",
    );
    crate::common::open_php(&backend, &lib, lib_text).await;

    let uri = Uri::parse("file:///caller.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "\\Lib\\toggle(\n",);
    let items = complete_at(&backend, &uri, text, 2, 12).await;
    assert_eq!(expected(&items), ["\\Lib\\Mode::ON", "\\Lib\\Mode::OFF"]);
    let on = items.iter().find(|i| i.label == "\\Lib\\Mode::ON").unwrap();
    assert_eq!(on.filter_text.as_deref(), Some("Lib\\Mode::ON"));

    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use Lib\\Mode;\n",
        "\\Lib\\toggle(\n",
    );
    let items = complete_at(&backend, &uri, text, 3, 12).await;
    assert_eq!(expected(&items), ["Mode::ON", "Mode::OFF"]);
}

#[tokio::test]
async fn phpstorm_stub_flags() {
    let backend = create_test_backend_with_full_stubs();
    let uri = Uri::parse("file:///stub_flags.php").unwrap();
    let text = "<?php\npathinfo('a.txt', PATHINFO_FILENAME | \n";
    let items = complete_at(&backend, &uri, text, 1, 38).await;
    let labels = expected(&items);
    assert!(labels.contains(&"PATHINFO_DIRNAME"), "{labels:?}");
    assert!(!labels.contains(&"PATHINFO_FILENAME"), "{labels:?}");

    let text = "<?php\nnew Attribute(\n";
    let items = complete_at(&backend, &uri, text, 1, 14).await;
    let labels = expected(&items);
    assert!(labels.contains(&"Attribute::TARGET_CLASS"), "{labels:?}");

    let text = "<?php\nsettype($value, 'bo\n";
    let items = complete_at(&backend, &uri, text, 1, 19).await;
    assert_eq!(item_labels(items), ["bool", "boolean"]);
}
