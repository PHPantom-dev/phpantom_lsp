//! "Create test" code action.
//!
//! On a class declaration, or on the signature of one of its public
//! methods, offer to write the class' test where its test lens looks for
//! it (see [`crate::composer::new_test_file`]): a PHPUnit test class in the
//! test root's namespace, extending the project's base test case, covering
//! the class, with a test method for each public method (or only the one
//! the cursor is on).  A project whose test root has a `Pest.php` gets a
//! Pest file instead.  Offered only while the class has no test the lens
//! would find.

use std::path::{Path, PathBuf};

use tower_lsp_server::gen_lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::signature_render::detect_class_indent;
use crate::types::{ClassLikeKind, MethodInfo, Visibility};

const PHPUNIT_TEST_CASE: &str = "PHPUnit\\Framework\\TestCase";
const COVERS_CLASS: &str = "PHPUnit\\Framework\\Attributes\\CoversClass";

/// What a new test file is made from.
struct TestPlan<'a> {
    subject_fqn: String,
    /// The tested methods' names, in declaration order.
    methods: Vec<&'a str>,
    /// The test's namespace, `None` for the global one.
    namespace: Option<String>,
    class_name: String,
    /// `None` for a Pest file.
    style: Option<PhpUnitStyle>,
    strict_types: bool,
    indent: String,
}

struct PhpUnitStyle {
    base_class: String,
    /// Whether the PHPUnit in use reads `#[CoversClass]` (PHPUnit 10 on)
    /// rather than `@covers`.
    attributes: bool,
}

impl Backend {
    /// Collect the "Create test" action for the class at the cursor.
    pub(crate) fn collect_create_test_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        // The action creates a file, which only a client that accepts the
        // `create` resource operation can apply.
        if !self.caps().file_create {
            return;
        }

        let cursor = crate::text_position::position_to_offset(content, params.range.start);
        let file_ctx = self.file_context(uri);
        let Some(class) = find_class_at_offset(&file_ctx.classes, cursor) else {
            return;
        };
        if class.keyword_offset == 0
            || !matches!(class.kind, ClassLikeKind::Class | ClassLikeKind::Enum)
        {
            return;
        }

        // On the declaration, between members, or on a method's signature,
        // but not inside a method body, where the cursor spends most of its
        // time and a test is not what is being worked on.
        let Some(method_at_cursor) =
            crate::parser::with_parsed_program(content, "create_test", |program, _| {
                match find_cursor_context(&program.statements, cursor) {
                    CursorContext::InClassLike {
                        member: MemberContext::Method(method, in_body),
                        ..
                    } => (!in_body).then_some(Some(method.name.span.start.offset)),
                    CursorContext::InClassLike { .. } => Some(None),
                    _ => None,
                }
            })
        else {
            return;
        };

        let methods: Vec<&str> = match method_at_cursor
            .and_then(|offset| class.methods.iter().find(|m| m.name_offset == offset))
        {
            Some(method) if is_testable(method) => vec![method.name.as_str()],
            _ => class
                .methods
                .iter()
                .filter(|m| is_testable(m))
                .map(|m| m.name.as_str())
                .collect(),
        };
        let only_method = (method_at_cursor.is_some() && methods.len() == 1).then(|| methods[0]);

        let subject_fqn = class.fqn().to_string();
        let Some((test_root, test_path, namespace, class_name)) =
            self.new_test_location(uri, &subject_fqn)
        else {
            return;
        };

        let style = if test_root.join("Pest.php").is_file() {
            None
        } else {
            let base_class = base_test_case(self, &test_root, &test_path)
                .unwrap_or_else(|| PHPUNIT_TEST_CASE.to_string());
            Some(PhpUnitStyle {
                base_class,
                attributes: self.phpunit_major().is_none_or(|major| major >= 10),
            })
        };

        let plan = TestPlan {
            subject_fqn,
            methods,
            namespace,
            class_name,
            style,
            strict_types: has_strict_types(content),
            indent: detect_class_indent(content, class),
        };
        let source = render_test(&plan);
        let Some(new_file_uri) = crate::util::path_to_url(&test_path) else {
            return;
        };
        let open_at = first_test_position(&source);

        let short = crate::util::short_name(&plan.subject_fqn);
        let title = match only_method {
            Some(method) => format!("Create test for {short}::{method}()"),
            None => format!("Create test for {short}"),
        };
        out.push(CodeActionResponse::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::Refactor),
            diagnostics: None,
            edit: Some(super::create_file_edit(
                new_file_uri.clone(),
                source,
                Vec::new(),
            )),
            command: Some(self.build_code_lens_command(
                "Open new test".to_string(),
                new_file_uri,
                open_at,
            )),
            is_preferred: None,
            disabled: None,
            data: None,
            tags: None,
        }));
    }

    /// Where a new test for the class `subject_fqn`, declared in `uri`,
    /// goes, as `(test root, test file, test namespace, test class name)`.
    /// `None` when the project has no test root for it, or the class
    /// already has a test there.
    fn new_test_location(
        &self,
        uri: &str,
        subject_fqn: &str,
    ) -> Option<(PathBuf, PathBuf, Option<String>, String)> {
        let mappings = self.psr4_mappings().read();
        if !mappings.iter().any(|mapping| mapping.is_dev) {
            return None;
        }
        let root = self.workspace_root().read().clone()?;
        let path = crate::util::uri_to_path(uri)?;
        if !crate::composer::conventional_test_files(&mappings, &root, subject_fqn, &path)
            .is_empty()
        {
            return None;
        }
        let (test_root, test_path) =
            crate::composer::new_test_file(&mappings, &root, subject_fqn, &path)?;
        if test_path.exists() {
            return None;
        }
        let (namespace, class_name) =
            crate::composer::resolve_namespace_from_path(&mappings, &root, &test_path)?;
        Some((test_root, test_path, namespace, class_name))
    }
}

/// Whether a test is written for `method`: a public method of the class'
/// own, other than the constructor and the other magic methods.
fn is_testable(method: &MethodInfo) -> bool {
    method.visibility == Visibility::Public
        && !method.is_virtual
        && !method.name.as_str().starts_with("__")
}

/// The project's base test case for a test written at `test_path`: the
/// `TestCase` class of the nearest directory from the test's own up to the
/// test root that declares one, as Laravel's `tests/TestCase.php` does.
fn base_test_case(backend: &Backend, test_root: &Path, test_path: &Path) -> Option<String> {
    let mappings = backend.psr4_mappings().read();
    let root = backend.workspace_root().read().clone()?;
    let mut dir = test_path.parent()?;
    loop {
        let candidate = dir.join("TestCase.php");
        if candidate.is_file()
            && let Some((namespace, name)) =
                crate::composer::resolve_namespace_from_path(&mappings, &root, &candidate)
        {
            return Some(match namespace {
                Some(namespace) => format!("{namespace}\\{name}"),
                None => name,
            });
        }
        if dir == test_root {
            return None;
        }
        dir = dir
            .parent()
            .filter(|parent| parent.starts_with(test_root))?;
    }
}

/// Whether `content` declares `strict_types=1`, which the test then does too.
fn has_strict_types(content: &str) -> bool {
    content.split("declare").skip(1).take(3).any(|rest| {
        let compact: String = rest
            .chars()
            .take(40)
            .filter(|c| !c.is_whitespace())
            .collect();
        compact.starts_with("(strict_types=1)")
    })
}

/// Where the editor opens the new test: on the first test body, or the
/// class line when there is none.
fn first_test_position(source: &str) -> Position {
    let lines: Vec<&str> = source.lines().collect();
    let line = lines
        .iter()
        .position(|line| line.contains("markTestIncomplete") || line.contains("->todo()"))
        .or_else(|| lines.iter().position(|line| line.starts_with("class ")))
        .unwrap_or(0);
    let character = lines
        .get(line)
        .map_or(0, |text| text.len() - text.trim_start().len());
    Position::new(line as u32, character as u32)
}

/// The names the test file imports, and how its code spells each class.
struct Imports {
    namespace: Option<String>,
    used: Vec<String>,
}

impl Imports {
    /// The name to write `fqn` as, importing it unless it is in the test's
    /// own namespace or its short name is already taken.
    fn name_for(&mut self, fqn: &str, own_class: &str) -> String {
        let short = crate::util::short_name(fqn);
        let fqn_namespace = fqn.rsplit_once('\\').map(|(namespace, _)| namespace);
        if fqn_namespace == self.namespace.as_deref() {
            return short.to_string();
        }
        let taken = short.eq_ignore_ascii_case(own_class)
            || self.used.iter().any(|used| {
                crate::util::short_name(used).eq_ignore_ascii_case(short) && used != fqn
            });
        if taken {
            return format!("\\{fqn}");
        }
        if !self.used.iter().any(|used| used == fqn) {
            self.used.push(fqn.to_string());
        }
        short.to_string()
    }
}

/// The source of the new test file.
fn render_test(plan: &TestPlan<'_>) -> String {
    let indent = &plan.indent;
    let mut imports = Imports {
        namespace: plan.namespace.clone(),
        used: Vec::new(),
    };
    let mut body = String::new();
    match &plan.style {
        None => {
            let subject = imports.name_for(&plan.subject_fqn, &plan.class_name);
            body.push_str(&format!("covers({subject}::class);\n"));
            for method in &plan.methods {
                body.push_str(&format!("\ntest('{method}')->todo();\n"));
            }
        }
        Some(style) => {
            // The subject claims its short name first: it is the name the
            // test is about.
            let subject = style
                .attributes
                .then(|| imports.name_for(&plan.subject_fqn, &plan.class_name));
            let base = imports.name_for(&style.base_class, &plan.class_name);
            if let Some(subject) = subject {
                let covers = imports.name_for(COVERS_CLASS, &plan.class_name);
                body.push_str(&format!("#[{covers}({subject}::class)]\n"));
            } else {
                // PHPUnit reads the tag's name as written, without the imports.
                body.push_str(&format!("/**\n * @covers \\{}\n */\n", plan.subject_fqn));
            }
            body.push_str(&format!("class {} extends {base}\n{{\n", plan.class_name));
            for (i, method) in plan.methods.iter().enumerate() {
                if i > 0 {
                    body.push('\n');
                }
                body.push_str(&format!(
                    "{indent}public function test{}(): void\n{indent}{{\n{indent}{indent}self::markTestIncomplete();\n{indent}}}\n",
                    upper_first(method)
                ));
            }
            body.push_str("}\n");
        }
    }

    let mut src = String::from("<?php\n\n");
    if plan.strict_types {
        src.push_str("declare(strict_types=1);\n\n");
    }
    if let Some(namespace) = &plan.namespace {
        src.push_str(&format!("namespace {namespace};\n\n"));
    }
    if !imports.used.is_empty() {
        imports.used.sort_by_key(|fqn| fqn.to_ascii_lowercase());
        for fqn in &imports.used {
            src.push_str(&format!("use {fqn};\n"));
        }
        src.push('\n');
    }
    src.push_str(&body);
    src
}

fn upper_first(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(style: Option<PhpUnitStyle>) -> TestPlan<'static> {
        TestPlan {
            subject_fqn: "App\\Billing\\Invoice".to_string(),
            methods: vec!["total", "addLine"],
            namespace: Some("Tests\\Unit\\Billing".to_string()),
            class_name: "InvoiceTest".to_string(),
            style,
            strict_types: true,
            indent: "    ".to_string(),
        }
    }

    #[test]
    fn renders_a_phpunit_test() {
        let src = render_test(&plan(Some(PhpUnitStyle {
            base_class: "Tests\\TestCase".to_string(),
            attributes: true,
        })));
        assert_eq!(
            src,
            "<?php

declare(strict_types=1);

namespace Tests\\Unit\\Billing;

use App\\Billing\\Invoice;
use PHPUnit\\Framework\\Attributes\\CoversClass;
use Tests\\TestCase;

#[CoversClass(Invoice::class)]
class InvoiceTest extends TestCase
{
    public function testTotal(): void
    {
        self::markTestIncomplete();
    }

    public function testAddLine(): void
    {
        self::markTestIncomplete();
    }
}
"
        );
        assert_eq!(first_test_position(&src), Position::new(15, 8));
    }

    #[test]
    fn older_phpunit_covers_through_the_docblock() {
        let src = render_test(&plan(Some(PhpUnitStyle {
            base_class: PHPUNIT_TEST_CASE.to_string(),
            attributes: false,
        })));
        assert!(src.contains(
            "/**\n * @covers \\App\\Billing\\Invoice\n */\nclass InvoiceTest extends TestCase\n"
        ));
        assert!(src.contains("use PHPUnit\\Framework\\TestCase;\n"));
        assert!(!src.contains("CoversClass"));
        assert!(!src.contains("use App\\Billing\\Invoice;"));
    }

    #[test]
    fn renders_a_pest_file() {
        let src = render_test(&plan(None));
        assert_eq!(
            src,
            "<?php

declare(strict_types=1);

namespace Tests\\Unit\\Billing;

use App\\Billing\\Invoice;

covers(Invoice::class);

test('total')->todo();

test('addLine')->todo();
"
        );
    }

    #[test]
    fn a_base_class_sharing_the_subjects_name_is_written_in_full() {
        let mut plan = plan(Some(PhpUnitStyle {
            base_class: "Tests\\TestCase".to_string(),
            attributes: true,
        }));
        plan.subject_fqn = "App\\Testing\\TestCase".to_string();
        plan.class_name = "TestCaseTest".to_string();
        let src = render_test(&plan);
        assert!(src.contains("use App\\Testing\\TestCase;\n"));
        assert!(src.contains("class TestCaseTest extends \\Tests\\TestCase\n"));
    }

    #[test]
    fn a_class_in_the_tests_namespace_is_not_imported() {
        let mut plan = plan(Some(PhpUnitStyle {
            base_class: "Tests\\Unit\\Billing\\TestCase".to_string(),
            attributes: true,
        }));
        plan.strict_types = false;
        let src = render_test(&plan);
        assert!(!src.contains("use Tests\\"));
        assert!(!src.contains("declare"));
        assert!(src.contains("extends TestCase\n"));
    }

    #[test]
    fn strict_types_is_detected_however_it_is_spaced() {
        assert!(has_strict_types("<?php\ndeclare( strict_types = 1 );\n"));
        assert!(has_strict_types("<?php declare(strict_types=1);"));
        assert!(!has_strict_types("<?php\ndeclare(strict_types=0);\n"));
        assert!(!has_strict_types("<?php\nclass A {}\n"));
    }
}
