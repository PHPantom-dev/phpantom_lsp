//! Integration tests for the `fix` CLI module.
//!
//! Tests exercise the unused-import fixer with a real `Backend` to
//! verify end-to-end correctness: parse → detect unused → build edits
//! → apply edits → verify output.

use crate::common::create_test_backend;
use phpantom_lsp::Backend;

/// Parse a PHP file into the backend and run the unused-import fixer,
/// returning the fixed content.
fn fix_unused_imports(backend: &Backend, uri: &str, content: &str) -> String {
    backend.update_ast(uri, content);
    phpantom_lsp::fix::fix_unused_imports(backend, uri, content).0
}

// ── Single unused import ────────────────────────────────────────────────────

#[test]
fn removes_single_unused_import() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;

class Foo {}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        !result.contains("use App\\Models\\User"),
        "Unused import should be removed. Got:\n{result}"
    );
    assert!(
        result.contains("class Foo {}"),
        "Class declaration should remain"
    );
}

// ── Multiple unused imports ─────────────────────────────────────────────────

#[test]
fn removes_multiple_unused_imports() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;
use App\Models\Post;
use App\Models\Comment;

class Foo {}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        !result.contains("use App\\Models\\User"),
        "User import should be removed"
    );
    assert!(
        !result.contains("use App\\Models\\Post"),
        "Post import should be removed"
    );
    assert!(
        !result.contains("use App\\Models\\Comment"),
        "Comment import should be removed"
    );
    assert!(
        result.contains("class Foo {}"),
        "Class declaration should remain"
    );
}

// ── Used import is preserved ────────────────────────────────────────────────

#[test]
fn preserves_used_import() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;

class Foo {
    public function bar(): User {
        return new User();
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("use App\\Models\\User"),
        "Used import should be preserved. Got:\n{result}"
    );
}

// ── Mix of used and unused imports ──────────────────────────────────────────

#[test]
fn removes_only_unused_from_mixed_imports() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;
use App\Models\Post;
use App\Models\Comment;

class Foo {
    public function bar(): User {
        return new User();
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("use App\\Models\\User"),
        "Used import (User) should be preserved"
    );
    assert!(
        !result.contains("use App\\Models\\Post"),
        "Unused import (Post) should be removed"
    );
    assert!(
        !result.contains("use App\\Models\\Comment"),
        "Unused import (Comment) should be removed"
    );
}

// ── No imports at all ───────────────────────────────────────────────────────

#[test]
fn no_imports_returns_unchanged() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

class Foo {
    public function bar(): void {}
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);
    assert_eq!(result, content);
}

// ── All imports used ────────────────────────────────────────────────────────

#[test]
fn all_imports_used_returns_unchanged() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;
use App\Models\Post;

class Foo {
    public function bar(): User {
        return new User();
    }
    public function baz(): Post {
        return new Post();
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);
    assert_eq!(result, content);
}

// ── Group import with one unused member ─────────────────────────────────────

#[test]
fn removes_unused_member_from_group_import() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\{User, Post};

class Foo {
    public function bar(): User {
        return new User();
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("User"),
        "Used member (User) should be preserved"
    );
    assert!(
        !result.contains("Post"),
        "Unused member (Post) should be removed from group"
    );
}

// ── Group import with all members unused ────────────────────────────────────

#[test]
fn removes_entire_group_import_when_all_unused() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\{User, Post};

class Foo {}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        !result.contains("use App\\Models"),
        "Entire group import should be removed. Got:\n{result}"
    );
    assert!(
        result.contains("class Foo {}"),
        "Class declaration should remain"
    );
}

// ── Blank line collapsing ───────────────────────────────────────────────────

#[test]
fn collapses_blank_lines_after_removing_all_imports() {
    let backend = create_test_backend();
    let content = "<?php\n\nnamespace App;\n\nuse App\\Models\\User;\n\nclass Foo {}\n";

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    // Should not have double blank lines where the import was.
    assert!(
        !result.contains("\n\n\n"),
        "Should not leave triple newlines. Got:\n{result}"
    );
}

// ── Static method reference keeps import ────────────────────────────────────

#[test]
fn preserves_import_used_in_static_call() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Utils\Helper;

class Foo {
    public function bar(): void {
        Helper::doSomething();
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("use App\\Utils\\Helper"),
        "Import used in static call should be preserved"
    );
}

// ── Import used in type hint ────────────────────────────────────────────────

#[test]
fn preserves_import_used_in_parameter_type_hint() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;

class Foo {
    public function bar(User $user): void {}
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("use App\\Models\\User"),
        "Import used as parameter type hint should be preserved"
    );
}

// ── Import used in docblock ─────────────────────────────────────────────────

#[test]
fn preserves_import_referenced_in_phpdoc_return_tag() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;

class Foo {
    /** @return User */
    public function bar() {}
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("use App\\Models\\User"),
        "Import referenced in @return should be preserved"
    );
}

// ── Braced namespace ────────────────────────────────────────────────────────

#[test]
fn removes_unused_import_in_braced_namespace() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App {
    use App\Models\User;
    use App\Models\Post;

    class Foo {
        public function bar(): User {
            return new User();
        }
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("use App\\Models\\User"),
        "Used import should be preserved in braced namespace"
    );
    assert!(
        !result.contains("use App\\Models\\Post"),
        "Unused import should be removed from braced namespace"
    );
}

// ── Trait use statement is not removed ───────────────────────────────────────

#[test]
fn does_not_remove_trait_use_statements() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Traits\HasName;

class Foo {
    use HasName;
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("use App\\Traits\\HasName"),
        "Namespace-level import for trait should be preserved (used by trait-use inside class)"
    );
}

// ── Idempotency ─────────────────────────────────────────────────────────────

#[test]
fn fix_is_idempotent() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\User;
use App\Models\Post;

class Foo {
    public function bar(): User {
        return new User();
    }
}
"#;

    let first_pass = fix_unused_imports(&backend, "file:///test.php", content);

    // Re-parse with the fixed content and fix again.
    let second_pass = fix_unused_imports(&backend, "file:///test.php", &first_pass);

    assert_eq!(
        first_pass, second_pass,
        "Running fix twice should produce the same result"
    );
}

// ── Blank lines around a removed import ─────────────────────────────────────

/// Removing an import from the middle of a contiguous block must not leave
/// a blank line between the survivors.
#[test]
fn removes_middle_import_without_blank_line() {
    let backend = create_test_backend();
    let content = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;
use PHPMD\\Rule\\Design\\CouplingBetweenObjects;

class Foo extends AbstractCallableNode {
    public function bar(MethodNode $m, CouplingBetweenObjects $c): void {}
}
";
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let (result, fixes) = phpantom_lsp::fix::fix_unused_imports(&backend, uri, content);

    assert_eq!(fixes.len(), 1, "should fix exactly one unused import");
    assert!(
        fixes[0].description.contains("Rule"),
        "should fix the Rule import"
    );

    let expected = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule\\Design\\CouplingBetweenObjects;

class Foo extends AbstractCallableNode {
    public function bar(MethodNode $m, CouplingBetweenObjects $c): void {}
}
";
    assert_eq!(
        result, expected,
        "Removing a middle import should not leave a blank line"
    );
}

#[test]
fn removes_first_import_without_blank_line() {
    let backend = create_test_backend();
    let content = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;

class Foo {
    public function bar(MethodNode $m, Rule $r): void {}
}
";
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let (result, fixes) = phpantom_lsp::fix::fix_unused_imports(&backend, uri, content);

    assert_eq!(fixes.len(), 1);
    assert!(fixes[0].description.contains("AbstractCallableNode"));

    let expected = "\
<?php
namespace Test;

use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;

class Foo {
    public function bar(MethodNode $m, Rule $r): void {}
}
";
    assert_eq!(
        result, expected,
        "Removing the first import should not leave a blank line"
    );
}

#[test]
fn removes_last_import_without_blank_line() {
    let backend = create_test_backend();
    let content = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;

class Foo {
    public function bar(AbstractCallableNode $a, MethodNode $m): void {}
}
";
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let (result, fixes) = phpantom_lsp::fix::fix_unused_imports(&backend, uri, content);

    assert_eq!(fixes.len(), 1);
    assert!(fixes[0].description.contains("Rule"));

    let expected = "\
<?php
namespace Test;

use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;

class Foo {
    public function bar(AbstractCallableNode $a, MethodNode $m): void {}
}
";
    assert_eq!(
        result, expected,
        "Removing the last import should not leave a blank line"
    );
}

// ── Multi-line group imports ────────────────────────────────────────────────

#[test]
fn removes_unused_member_from_multiline_group_import() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\{
    User,
    Post,
};

class Foo {
    public function bar(): User {
        return new User();
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        result.contains("User"),
        "Used member (User) should be preserved. Got:\n{result}"
    );
    assert!(
        !result.contains("Post"),
        "Unused member (Post) should be removed from group. Got:\n{result}"
    );
    assert!(
        !result.contains("\n\n    \n") && !result.contains("{\n\n"),
        "Removing a member should not leave a blank line in the group. Got:\n{result}"
    );
}

#[test]
fn removes_entire_multiline_group_import_when_all_unused() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\{
    User,
    Post,
};

class Foo {}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        !result.contains("use App\\Models"),
        "Entire group import should be removed. Got:\n{result}"
    );
    assert!(
        result.contains("class Foo {}"),
        "Class declaration should remain. Got:\n{result}"
    );
}

#[test]
fn removes_sole_unused_member_of_multiline_group_import() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\{
    Post,
};

class Foo {}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        !result.contains("use App\\Models"),
        "A one-member group should be removed outright. Got:\n{result}"
    );
}

#[test]
fn preserves_used_members_of_multiline_group_import() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\{
    User,
    Post,
};

class Foo {
    public function bar(User $u, Post $p): void {}
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);
    assert_eq!(result, content);
}

#[test]
fn removes_unused_aliased_member_from_group_import() {
    let backend = create_test_backend();
    let content = r#"<?php

namespace App;

use App\Models\{User, Post as BlogPost};

class Foo {
    public function bar(): User {
        return new User();
    }
}
"#;

    let result = fix_unused_imports(&backend, "file:///test.php", content);

    assert!(
        !result.contains("BlogPost"),
        "Unused aliased member should be removed. Got:\n{result}"
    );
    assert!(
        result.contains("use App\\Models\\{User}"),
        "Used member should be preserved. Got:\n{result}"
    );
}
