use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action_titled, get_code_actions_at,
    line_char_of,
};

/// Apply the action titled `title` offered at `needle`, or `None` when it
/// is not offered there.
fn fix(content: &str, needle: &str, title: &str) -> Option<String> {
    let backend = create_test_backend();
    let uri = "file:///Invoice.php";
    backend.update_ast(uri, content);
    let (line, character) = line_char_of(content, needle);
    let actions = get_code_actions_at(&backend, uri, content, line, character);
    let action = find_action_titled(&actions, title)?;
    Some(apply_edits(content, &extract_edits(action)))
}

#[test]
fn removes_an_unused_method_with_its_docblock() {
    let php = r#"<?php
final class Invoice
{
    public function total(): int
    {
        return 1;
    }

    /**
     * Old helper.
     */
    #[\Deprecated]
    private function recalculate(): void
    {
    }
}
"#;
    assert_eq!(
        fix(
            php,
            "recalculate",
            "Remove unused method Invoice::recalculate()"
        )
        .unwrap(),
        r#"<?php
final class Invoice
{
    public function total(): int
    {
        return 1;
    }
}
"#
    );
}

#[test]
fn removes_an_unused_property_and_constant() {
    let php = r#"<?php
final class Invoice
{
    private const LEGACY = 'x';
    private $legacyNumber;
    public int $total = 0;
}
"#;
    let fixed = fix(php, "LEGACY", "Remove unused constant Invoice::LEGACY").unwrap();
    assert_eq!(
        fixed,
        "<?php\nfinal class Invoice\n{\n    private $legacyNumber;\n    public int $total = 0;\n}\n"
    );
    let fixed = fix(
        php,
        "$legacyNumber",
        "Remove unused property Invoice::$legacyNumber",
    )
    .unwrap();
    assert_eq!(
        fixed,
        "<?php\nfinal class Invoice\n{\n    private const LEGACY = 'x';\n    public int $total = 0;\n}\n"
    );
}

#[test]
fn removes_one_item_of_a_grouped_declaration() {
    let php = "<?php\nfinal class A\n{\n    private const B = 1, C = 2;\n    public function b(): int { return self::B; }\n}\n";
    assert_eq!(
        fix(php, "C = 2", "Remove unused constant A::C").unwrap(),
        "<?php\nfinal class A\n{\n    private const B = 1;\n    public function b(): int { return self::B; }\n}\n"
    );
}

#[test]
fn a_property_still_written_is_not_offered_for_removal() {
    let php = r#"<?php
final class Invoice
{
    private ?string $note = null;
    public function pay(): void { $this->note = 'paid'; }
}
"#;
    assert_eq!(
        fix(php, "$note", "Remove unused property Invoice::$note"),
        None
    );
}
