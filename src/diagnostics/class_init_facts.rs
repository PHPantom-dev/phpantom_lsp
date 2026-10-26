//! What a class body guarantees about initializing its own properties.
//!
//! The constructor is where PHP expects a class to initialize its
//! properties, and `__clone`/`__unserialize` are the other two methods
//! that may legitimately (re)initialize a `readonly` one.  This module
//! reads those methods once per class body and records the facts the
//! property checks build on.

use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::class_like::member::{ClassLikeMember, ClassLikeMemberSelector};
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::variable::Variable;

use crate::atom::bytes_to_str;

/// What a class body says about its own property initialization.
#[derive(Default)]
pub(crate) struct ClassInitFacts {
    /// Body ranges of `__construct`, `__clone`, and `__unserialize`,
    /// where a readonly property may legitimately be initialized.
    pub(crate) initializer_ranges: Vec<(u32, u32)>,
    /// Properties the constructor is guaranteed to have initialized by
    /// the time any other method runs: promoted parameters and
    /// unconditional `$this->name = …` statements in its body.
    pub(crate) initialized: HashSet<String>,
    /// Body range of `__construct`.
    constructor_range: Option<(u32, u32)>,
    /// For each property the constructor body assigns unconditionally,
    /// where the first such statement ends.  Any later write in the
    /// constructor finds the property already initialized.
    first_assignment_end: HashMap<String, u32>,
}

impl ClassInitFacts {
    /// Whether `offset` lies inside one of the initializer bodies.
    pub(crate) fn covers_initializer(&self, offset: u32) -> bool {
        self.initializer_ranges
            .iter()
            .any(|(start, end)| offset >= *start && offset <= *end)
    }

    /// Whether a write to `property` at `offset` lands in the constructor
    /// after an unconditional assignment has already initialized it.
    pub(crate) fn reassigns_in_constructor(&self, property: &str, offset: u32) -> bool {
        self.constructor_range
            .is_some_and(|(start, end)| offset >= start && offset <= end)
            && self
                .first_assignment_end
                .get(property)
                .is_some_and(|first_end| offset > *first_end)
    }
}

/// Read the initialization facts of the class body made of `members`.
pub(crate) fn class_init_facts(members: &Sequence<'_, ClassLikeMember<'_>>) -> ClassInitFacts {
    let mut facts = ClassInitFacts::default();

    for member in members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let name = bytes_to_str(method.name.value);
        let is_constructor = name.eq_ignore_ascii_case("__construct");
        if !is_constructor
            && !name.eq_ignore_ascii_case("__clone")
            && !name.eq_ignore_ascii_case("__unserialize")
        {
            continue;
        }
        let MethodBody::Concrete(block) = &method.body else {
            continue;
        };
        let span = block.span();
        facts
            .initializer_ranges
            .push((span.start.offset, span.end.offset));

        if !is_constructor {
            continue;
        }
        facts.constructor_range = Some((span.start.offset, span.end.offset));

        for param in method.parameter_list.parameters.iter() {
            if param.is_promoted_property() {
                let raw = bytes_to_str(param.variable.name);
                facts
                    .initialized
                    .insert(raw.strip_prefix('$').unwrap_or(raw).to_string());
            }
        }

        // Only statements at the top level of the constructor body count:
        // an assignment inside a branch or a loop may not run, which
        // leaves the property free to be initialized elsewhere.
        for statement in block.statements.iter() {
            if let Statement::Expression(stmt) = statement
                && let Expression::Assignment(assign) = stmt.expression
                && assign.operator.is_assign()
                && let Expression::Access(Access::Property(access)) = assign.lhs
                && is_this(access.object)
                && let ClassLikeMemberSelector::Identifier(ident) = &access.property
            {
                let name = bytes_to_str(ident.value);
                facts.initialized.insert(name.to_string());
                facts
                    .first_assignment_end
                    .entry(name.to_string())
                    .or_insert(stmt.span().end.offset);
            }
        }
    }

    facts
}

/// Whether `expr` is the bare `$this` variable.
pub(crate) fn is_this(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Variable(Variable::Direct(var)) if var.name == b"$this")
}
