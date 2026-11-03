use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action_titled, get_code_actions_at,
    line_char_of,
};

const BASE: &str = r#"<?php
namespace PHPUnit\Framework {
    abstract class TestCase
    {
        protected function setUp(): void {}
        protected function tearDown(): void {}
    }
}

namespace Tests {
    abstract class AppTestCase extends \PHPUnit\Framework\TestCase
    {
        protected function setUp(): void { $this->boot(); }
        protected function tearDown(): void { $this->boot(); }
        private function boot(): void {}
    }
}
"#;

/// Apply the quick fix offered on the hook named `hook`, and return the
/// test class's part of the file.
fn fix(tests: &str, hook: &str) -> String {
    let content = format!("{BASE}{tests}");
    let backend = create_test_backend();
    let uri = "file:///OrderTest.php";
    backend.update_ast(uri, &content);
    let needle = format!("function {hook}(");
    let (line, character) = line_char_of(tests, &needle);
    let line = line + BASE.lines().count() as u32;
    let actions = get_code_actions_at(
        &backend,
        uri,
        &content,
        line,
        character + "function ".len() as u32,
    );
    let title = format!("Call parent::{hook}()");
    let action = find_action_titled(&actions, &title)
        .unwrap_or_else(|| panic!("expected `{title}` on `{hook}`"));
    let fixed = apply_edits(&content, &extract_edits(action));
    fixed[BASE.len()..].to_string()
}

#[test]
fn set_up_calls_the_parent_first() {
    let tests = r#"
final class OrderTest extends \Tests\AppTestCase
{
    protected function setUp(): void
    {
        $this->order = 1;
    }
}
"#;
    assert_eq!(
        fix(tests, "setUp"),
        r#"
final class OrderTest extends \Tests\AppTestCase
{
    protected function setUp(): void
    {
        parent::setUp();
        $this->order = 1;
    }
}
"#
    );
}

#[test]
fn tear_down_calls_the_parent_last() {
    let tests = r#"
final class OrderTest extends \Tests\AppTestCase
{
    protected function tearDown(): void
    {
        $this->order = null;
    }
}
"#;
    assert_eq!(
        fix(tests, "tearDown"),
        r#"
final class OrderTest extends \Tests\AppTestCase
{
    protected function tearDown(): void
    {
        $this->order = null;
        parent::tearDown();
    }
}
"#
    );
}

#[test]
fn an_empty_body_gets_the_call_on_its_own_line() {
    let tests = r#"
final class OrderTest extends \Tests\AppTestCase
{
    protected function setUp(): void {}
}
"#;
    assert_eq!(
        fix(tests, "setUp"),
        r#"
final class OrderTest extends \Tests\AppTestCase
{
    protected function setUp(): void {
        parent::setUp();
    }
}
"#
    );
}
