//! Shared "cursor context" AST walker for code actions.
//!
//! Many code actions need to answer the question "which class-like
//! declaration and which member is the cursor on?"  This module
//! provides a single AST walk that answers that question, so that
//! individual code actions don't each need their own copy of the
//! namespace → class-like → member traversal.  [`find_enclosing_function`]
//! answers the narrower "which function is this offset in?" for the
//! actions that edit a function wherever it is declared, and
//! [`innermost_at`] finds the construct under the cursor for the actions
//! that rewrite one.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::constant::ClassLikeConstant;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::{Method, MethodBody};
use mago_syntax::cst::class_like::property::Property;
use mago_syntax::cst::function_like::function::Function;
use mago_syntax::cst::*;

// ── Public types ────────────────────────────────────────────────────────────

/// Which kind of class-like declaration the cursor is inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassLikeContextKind {
    /// A `class` declaration (concrete or abstract).
    Class,
    Interface,
    Trait,
    Enum,
}

/// Which member the cursor is on, together with its byte span.
#[derive(Debug)]
pub(crate) enum MemberContext<'a> {
    /// A method declaration.  The boolean is `true` when the cursor is
    /// inside the method body (as opposed to on the signature).
    Method(&'a Method<'a>, bool),
    Property(&'a Property<'a>),
    Constant(&'a ClassLikeConstant<'a>),
    TraitUse,
    /// An enum case (backed or unit).
    EnumCase,
    /// The cursor is inside the class body but not on any specific member
    /// (e.g. on blank lines between members, or on the closing brace).
    None,
}

/// The result of the cursor-context walk.
#[derive(Debug)]
pub(crate) enum CursorContext<'a> {
    InClassLike {
        kind: ClassLikeContextKind,
        /// Whether the class-like declaration has the `readonly` modifier
        /// (PHP 8.2+ `readonly class`).  Always `false` for interfaces,
        /// traits, and enums.
        class_readonly: bool,
        member: MemberContext<'a>,
        /// All members of the class-like, for actions that need to
        /// inspect siblings (e.g. "does a constructor already exist?").
        all_members: &'a Sequence<'a, ClassLikeMember<'a>>,
    },
    /// The cursor is on a top-level (or namespace-level) function.
    /// The boolean is `true` when the cursor is inside the function body.
    InFunction(&'a Function<'a>, bool),
    None,
}

/// Which declarations [`find_enclosing_function`] stops at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionKinds {
    /// Functions and methods.  A closure belongs to the one it is written
    /// in, the way PHPStan charges an exception thrown inside it to that
    /// function.
    Named,
    /// Closures and arrow functions as well.
    All,
}

/// A function-like declaration: a function, a method, a closure or an
/// arrow function.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FunctionLike<'ast, 'arena> {
    Function(&'ast Function<'arena>),
    Method(&'ast Method<'arena>),
    Closure(&'ast Closure<'arena>),
    ArrowFunction(&'ast ArrowFunction<'arena>),
}

impl<'ast, 'arena> FunctionLike<'ast, 'arena> {
    /// The declaration `node` is, if it is one.
    pub(crate) fn of(node: Node<'ast, 'arena>) -> Option<Self> {
        match node {
            Node::Function(function) => Some(Self::Function(function)),
            Node::Method(method) => Some(Self::Method(method)),
            Node::Closure(closure) => Some(Self::Closure(closure)),
            Node::ArrowFunction(arrow) => Some(Self::ArrowFunction(arrow)),
            _ => None,
        }
    }

    /// From its first attribute, modifier or keyword to its end.
    pub(crate) fn span(self) -> mago_span::Span {
        match self {
            Self::Function(function) => function.span(),
            Self::Method(method) => method.span(),
            Self::Closure(closure) => closure.span(),
            Self::ArrowFunction(arrow) => arrow.span(),
        }
    }

    /// Whether it is a closure or an arrow function.
    pub(crate) fn is_anonymous(self) -> bool {
        matches!(self, Self::Closure(_) | Self::ArrowFunction(_))
    }

    /// The parentheses and the parameters between them.
    pub(crate) fn parameter_list(self) -> &'ast FunctionLikeParameterList<'arena> {
        match self {
            Self::Function(function) => &function.parameter_list,
            Self::Method(method) => &method.parameter_list,
            Self::Closure(closure) => &closure.parameter_list,
            Self::ArrowFunction(arrow) => &arrow.parameter_list,
        }
    }

    /// The `:` and the native return type after it, when it declares one.
    pub(crate) fn return_type_hint(self) -> Option<&'ast FunctionLikeReturnTypeHint<'arena>> {
        match self {
            Self::Function(function) => function.return_type_hint.as_ref(),
            Self::Method(method) => method.return_type_hint.as_ref(),
            Self::Closure(closure) => closure.return_type_hint.as_ref(),
            Self::ArrowFunction(arrow) => arrow.return_type_hint.as_ref(),
        }
    }

    /// The block of its body.  An abstract method has none, and neither
    /// does an arrow function, whose body is the expression it returns.
    pub(crate) fn body(self) -> Option<&'ast Block<'arena>> {
        match self {
            Self::Function(function) => Some(&function.body),
            Self::Method(method) => match &method.body {
                MethodBody::Concrete(block) => Some(block),
                MethodBody::Abstract(_) => None,
            },
            Self::Closure(closure) => Some(&closure.body),
            Self::ArrowFunction(_) => None,
        }
    }

    /// The offsets of the braces around its body, when it has one.
    pub(crate) fn body_braces(self) -> Option<(u32, u32)> {
        self.body().map(|block| {
            (
                block.left_brace.start.offset,
                block.right_brace.start.offset,
            )
        })
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Walk the AST to find the cursor context at the given byte offset.
///
/// This is the single entry point that all code actions should use to
/// determine which class-like and member the cursor is on.
pub(crate) fn find_cursor_context<'a>(
    statements: &'a Sequence<'a, Statement<'a>>,
    cursor: u32,
) -> CursorContext<'a> {
    for stmt in statements.iter() {
        let ctx = find_in_statement(stmt, cursor);
        if !matches!(ctx, CursorContext::None) {
            return ctx;
        }
    }
    CursorContext::None
}

/// The innermost function-like declaration of `kinds` whose span contains
/// `offset`, wherever it is declared: at the top level, in a namespace, in
/// a class-like or anonymous class, or nested in another body.
pub(crate) fn find_enclosing_function<'ast, 'arena>(
    program: &'ast Program<'arena>,
    offset: u32,
    kinds: FunctionKinds,
) -> Option<FunctionLike<'ast, 'arena>> {
    innermost_at(program, offset, |node| {
        FunctionLike::of(node)
            .filter(|function| kinds == FunctionKinds::All || !function.is_anonymous())
    })
}

/// What `pick` makes of the innermost node holding `offset` that it
/// accepts, for an action that rewrites the construct under the cursor.
///
/// The walk is generic over [`Node`], so it reaches every construct the
/// parser produces (`elseif` and `else` branches, `match` arms, closure
/// bodies, anonymous classes), and it only descends into the subtree
/// holding `offset`, so it costs the depth of the cursor rather than the
/// size of the file.  Spans are inclusive at both ends: a cursor just past
/// a construct is still on it.
pub(crate) fn innermost_at<'ast, 'arena, T>(
    program: &'ast Program<'arena>,
    offset: u32,
    pick: impl FnMut(Node<'ast, 'arena>) -> Option<T>,
) -> Option<T> {
    crate::parser::innermost_node_at(Node::Program(program), offset, pick)
}

/// A `$var = value;` statement: a plain `=` to a variable other than
/// `$this`.
pub(crate) struct PlainAssignment<'ast, 'arena> {
    /// The whole statement, `;` included.
    pub(crate) statement: &'ast ExpressionStatement<'arena>,
    pub(crate) variable: &'ast DirectVariable<'arena>,
    pub(crate) value: &'ast Expression<'arena>,
}

/// The innermost `$var = value;` statement under `offset`; see
/// [`PlainAssignment`].
pub(crate) fn plain_assignment_at<'ast, 'arena>(
    program: &'ast Program<'arena>,
    offset: u32,
) -> Option<PlainAssignment<'ast, 'arena>> {
    innermost_at(program, offset, |node| {
        let Node::ExpressionStatement(statement) = node else {
            return None;
        };
        let Expression::Assignment(assignment) = statement.expression else {
            return None;
        };
        let Expression::Variable(Variable::Direct(variable)) = assignment.lhs else {
            return None;
        };
        (assignment.operator.is_assign() && variable.name != b"$this").then_some(PlainAssignment {
            statement,
            variable,
            value: assignment.rhs,
        })
    })
}

// ── AST walk (private) ─────────────────────────────────────────────────────

fn find_in_statement<'a>(stmt: &'a Statement<'a>, cursor: u32) -> CursorContext<'a> {
    match stmt {
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                let ctx = find_in_statement(s, cursor);
                if !matches!(ctx, CursorContext::None) {
                    return ctx;
                }
            }
            CursorContext::None
        }
        Statement::Function(func) => {
            let span = func.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                let body_start = func.body.left_brace.start.offset;
                let in_body = cursor >= body_start;
                CursorContext::InFunction(func, in_body)
            } else {
                CursorContext::None
            }
        }
        Statement::Class(class) => {
            let span = class.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                let member = find_member_at_cursor(class.members.iter(), cursor);
                CursorContext::InClassLike {
                    kind: ClassLikeContextKind::Class,
                    class_readonly: class.modifiers.contains_readonly(),
                    member,
                    all_members: &class.members,
                }
            } else {
                CursorContext::None
            }
        }
        Statement::Interface(iface) => {
            let span = iface.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                let member = find_member_at_cursor(iface.members.iter(), cursor);
                CursorContext::InClassLike {
                    kind: ClassLikeContextKind::Interface,
                    class_readonly: false,
                    member,
                    all_members: &iface.members,
                }
            } else {
                CursorContext::None
            }
        }
        Statement::Trait(tr) => {
            let span = tr.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                let member = find_member_at_cursor(tr.members.iter(), cursor);
                CursorContext::InClassLike {
                    kind: ClassLikeContextKind::Trait,
                    class_readonly: false,
                    member,
                    all_members: &tr.members,
                }
            } else {
                CursorContext::None
            }
        }
        Statement::Enum(en) => {
            let span = en.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                let member = find_member_at_cursor(en.members.iter(), cursor);
                CursorContext::InClassLike {
                    kind: ClassLikeContextKind::Enum,
                    class_readonly: false,
                    member,
                    all_members: &en.members,
                }
            } else {
                CursorContext::None
            }
        }
        _ => CursorContext::None,
    }
}

fn find_member_at_cursor<'a>(
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    cursor: u32,
) -> MemberContext<'a> {
    for member in members {
        let member_span = member.span();
        if cursor < member_span.start.offset || cursor > member_span.end.offset {
            continue;
        }
        return match member {
            ClassLikeMember::Method(method) => {
                let body_start = method.body.span().start.offset;
                let in_body = cursor >= body_start;
                MemberContext::Method(method, in_body)
            }
            ClassLikeMember::Property(property) => MemberContext::Property(property),
            ClassLikeMember::Constant(constant) => MemberContext::Constant(constant),
            ClassLikeMember::TraitUse(_) => MemberContext::TraitUse,
            ClassLikeMember::EnumCase(_) => MemberContext::EnumCase,
        };
    }
    MemberContext::None
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mago_allocator::LocalArena;

    fn ctx_at(php: &str, offset: u32) -> CursorContext<'_> {
        // SAFETY: We leak the arena so the returned CursorContext (which
        // borrows from the Program) lives long enough for the test
        // assertions.  This is fine in tests — the memory is reclaimed
        // when the process exits.
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());
        find_cursor_context(&program.statements, offset)
    }

    // ── Class-like detection ────────────────────────────────────────────

    #[test]
    fn finds_class() {
        let php = "<?php\nclass Foo {\n    public function bar() {}\n}";
        let pos = php.find("public function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InClassLike { .. }));
    }

    #[test]
    fn finds_interface() {
        let php = "<?php\ninterface Foo {\n    public function bar(): void;\n}";
        let pos = php.find("public function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InClassLike { .. }));
    }

    #[test]
    fn finds_trait() {
        let php = "<?php\ntrait Foo {\n    protected function bar() {}\n}";
        let pos = php.find("protected function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InClassLike { .. }));
    }

    #[test]
    fn finds_enum() {
        let php = "<?php\nenum Foo {\n    public function bar(): void {}\n}";
        let pos = php.find("public function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InClassLike { .. }));
    }

    // ── Member detection ────────────────────────────────────────────────

    #[test]
    fn finds_method_on_signature() {
        let php = "<?php\nclass Foo {\n    public function bar() {}\n}";
        let pos = php.find("bar").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        match &ctx {
            CursorContext::InClassLike {
                member: MemberContext::Method(method, false),
                ..
            } => assert_eq!(method.name.value, b"bar"),
            _ => panic!("should find method on signature"),
        }
    }

    #[test]
    fn detects_cursor_in_method_body() {
        let php = "<?php\nclass Foo {\n    public function bar() {\n        $x = 1;\n    }\n}";
        let pos = php.find("$x = 1").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        match &ctx {
            CursorContext::InClassLike {
                member: MemberContext::Method(method, true),
                ..
            } => assert_eq!(method.name.value, b"bar"),
            _ => panic!("should find method with in_body=true"),
        }
    }

    #[test]
    fn finds_property() {
        let php = "<?php\nclass Foo {\n    protected string $bar;\n}";
        let pos = php.find("protected string").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(
            ctx,
            CursorContext::InClassLike {
                member: MemberContext::Property(_),
                ..
            }
        ));
    }

    #[test]
    fn finds_constant() {
        let php = "<?php\nclass Foo {\n    private const BAR = 1;\n}";
        let pos = php.find("private const").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(
            ctx,
            CursorContext::InClassLike {
                member: MemberContext::Constant(_),
                ..
            }
        ));
    }

    // ── Namespace handling ──────────────────────────────────────────────

    #[test]
    fn finds_class_in_namespace() {
        let php = "<?php\nnamespace App;\nclass Foo {\n    public function bar() {}\n}";
        let pos = php.find("public function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InClassLike { .. }));
    }

    #[test]
    fn finds_class_in_braced_namespace() {
        let php = "<?php\nnamespace App {\nclass Foo {\n    private function bar() {}\n}\n}";
        let pos = php.find("private function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InClassLike { .. }));
    }

    // ── Standalone function ─────────────────────────────────────────────

    #[test]
    fn finds_standalone_function() {
        let php = "<?php\nfunction foo() { return 1; }";
        let pos = php.find("function foo").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InFunction(_, false)));
    }

    #[test]
    fn detects_cursor_in_function_body() {
        let php = "<?php\nfunction foo() { $x = 1; }";
        let pos = php.find("$x = 1").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InFunction(_, true)));
    }

    // ── No context ──────────────────────────────────────────────────────

    #[test]
    fn no_context_outside_class() {
        let php = "<?php\n$x = 1;\n";
        let ctx = ctx_at(php, 7);
        assert!(matches!(ctx, CursorContext::None));
    }

    #[test]
    fn no_context_on_class_keyword() {
        let php = "<?php\nclass Foo {\n    public function bar() {}\n}";
        // Cursor on "class" keyword itself — before the body starts
        let pos = php.find("class Foo").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        // Should be in the class-like but with MemberContext::None
        assert!(matches!(
            ctx,
            CursorContext::InClassLike {
                member: MemberContext::None,
                ..
            }
        ));
    }

    // ── all_members access ──────────────────────────────────────────────

    #[test]
    fn all_members_returns_members() {
        let php = "<?php\nclass Foo {\n    public string $a;\n    public function bar() {}\n    private const C = 1;\n}";
        let pos = php.find("public function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        match &ctx {
            CursorContext::InClassLike { all_members, .. } => {
                assert_eq!(all_members.len(), 3);
            }
            _ => panic!("should have members"),
        }
    }

    // ── Body offset tracking ────────────────────────────────────────────

    #[test]
    fn cursor_inside_class_body_finds_class_like() {
        let php = "<?php\nclass Foo {\n    public function bar() {}\n}";
        let pos = php.find("public function").unwrap() as u32;
        let ctx = ctx_at(php, pos);
        assert!(matches!(ctx, CursorContext::InClassLike { .. }));
    }
}
