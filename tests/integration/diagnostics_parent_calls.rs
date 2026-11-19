use crate::common::{collect_diagnostics_with, message_text};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

/// PHPUnit's `TestCase` with its empty hooks, and a base test case that
/// does work in all four, the way Laravel's and Symfony's do.
const BASE: &str = r#"<?php
namespace PHPUnit\Framework {
    abstract class TestCase
    {
        protected function setUp(): void {}
        protected function tearDown(): void {}
        public static function setUpBeforeClass(): void {}
        public static function tearDownAfterClass(): void {}
    }
}

namespace Tests {
    abstract class AppTestCase extends \PHPUnit\Framework\TestCase
    {
        protected function setUp(): void { $this->boot(); }
        protected function tearDown(): void { $this->boot(); }
        public static function setUpBeforeClass(): void {}
        public static function tearDownAfterClass(): void {}
        private function boot(): void {}
    }
}
"#;

fn collect(tests: &str) -> Vec<String> {
    let php = format!("{BASE}\n{tests}");
    collect_diagnostics_with(
        &Backend::new_test(),
        &php,
        Backend::collect_parent_call_diagnostics,
    )
    .iter()
    .map(|d| {
        assert_eq!(
            d.code,
            Some(Code::String("missing_phpunit_parent_call".to_string()))
        );
        assert_eq!(d.severity, Some(DiagnosticSeverity::Warning));
        message_text(&d.message).to_string()
    })
    .collect()
}

#[test]
fn hooks_that_skip_the_parent_are_reported() {
    let tests = r#"
namespace Tests\Feature {
    use Tests\AppTestCase;

    final class OrderTest extends AppTestCase
    {
        protected function setUp(): void
        {
            $this->order = 1;
        }

        protected function tearDown(): void {}

        public static function setUpBeforeClass(): void {}

        public static function tearDownAfterClass(): void {}
    }
}
"#;
    assert_eq!(
        collect(tests),
        vec![
            "This override does not call parent::setUp()",
            "This override does not call parent::tearDown()",
            "This override does not call parent::setUpBeforeClass()",
            "This override does not call parent::tearDownAfterClass()",
        ]
    );
}

#[test]
fn a_parent_call_anywhere_in_the_body_is_enough() {
    let tests = r#"
namespace Tests\Feature {
    final class OrderTest extends \Tests\AppTestCase
    {
        protected function setUp(): void
        {
            $this->order = 1;
            if (true) {
                PARENT::SETUP();
            }
        }

        protected function tearDown(): void
        {
            $this->order = null;
            parent::tearDown();
        }
    }
}
"#;
    assert_eq!(collect(tests), Vec::<String>::new());
}

/// A call to the wrong parent method, or to the parent of an anonymous
/// class, does not run the hook.
#[test]
fn other_parent_calls_do_not_count() {
    let tests = r#"
namespace Tests\Feature {
    final class OrderTest extends \Tests\AppTestCase
    {
        protected function setUp(): void
        {
            parent::tearDown();
            new class extends \Tests\AppTestCase {
                protected function setUp(): void { parent::setUp(); }
            };
        }
    }
}
"#;
    assert_eq!(
        collect(tests),
        vec!["This override does not call parent::setUp()"]
    );
}

/// `TestCase`'s own hooks are empty, an abstract one cannot be called,
/// and a class that is not a test case is not PHPUnit's to run.
#[test]
fn hooks_with_nothing_to_call_are_not_reported() {
    let tests = r#"
namespace Tests\Unit {
    final class PlainTest extends \PHPUnit\Framework\TestCase
    {
        protected function setUp(): void {}
    }

    abstract class AbstractHookTestCase extends \PHPUnit\Framework\TestCase
    {
        abstract protected function setUp(): void;
    }

    final class ConcreteHookTest extends AbstractHookTestCase
    {
        protected function setUp(): void {}
    }

    class Service
    {
        public function setUp(): void {}
    }

    final class ChildService extends Service
    {
        public function setUp(): void {}
    }
}
"#;
    assert_eq!(collect(tests), Vec::<String>::new());
}

/// A hook a base test case gets from a trait is worth calling as well.
#[test]
fn a_hook_inherited_through_a_trait_is_reported() {
    let tests = r#"
namespace Tests\Unit {
    trait RefreshesDatabase
    {
        protected function setUp(): void { $this->refresh(); }
        private function refresh(): void {}
    }

    abstract class DatabaseTestCase extends \PHPUnit\Framework\TestCase
    {
        use RefreshesDatabase;
    }

    final class UserTest extends DatabaseTestCase
    {
        protected function setUp(): void {}
    }
}
"#;
    assert_eq!(
        collect(tests),
        vec!["This override does not call parent::setUp()"]
    );
}
