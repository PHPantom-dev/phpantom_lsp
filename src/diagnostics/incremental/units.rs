//! The functions and methods of a file, and which of them an edit touched.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::function_like::function::Function;
use mago_syntax::cst::{Program, Statement};

use crate::symbol_map::{SymbolKind, SymbolMap};

/// A function or method with a body: what a partial pass re-diagnoses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiagnosticUnit {
    /// The whole declaration, from its first attribute or modifier to just
    /// past its closing brace.
    pub(crate) span: (u32, u32),
    /// The body, from its opening brace to just past its closing one.
    pub(crate) body: (u32, u32),
    /// The name token.
    name: (u32, u32),
    /// Whether the body declares a function of its own.  Such a function
    /// is global the moment the body runs, so other code in the file can
    /// call it.
    pub(crate) declares_function: bool,
    /// Whether inference read this body while a slow pass over the file
    /// diagnosed something, since the last pass that diagnosed all of it.
    /// See [`crate::type_engine::body_reads`].
    pub(crate) observed: bool,
}

impl DiagnosticUnit {
    /// The function or method name, as written.
    pub(crate) fn name<'a>(&self, content: &'a str) -> &'a str {
        content
            .get(self.name.0 as usize..self.name.1 as usize)
            .unwrap_or("")
    }
}

/// Every function and method body in `program`, in source order.
///
/// Mirrors the bodies the diagnostic walk visits on its own: the methods
/// of each class-like and the functions at the top of the file, of a
/// `namespace` block, or of the body of a top-level `if` (the
/// `if (!function_exists(...))` guard).  Anything else (top-level code,
/// property hooks, a function in an `else` branch) is left to the text
/// around the units, where an edit forces a full pass.
pub(crate) fn collect(program: &Program<'_>) -> Vec<DiagnosticUnit> {
    let mut units = Vec::new();
    collect_from_statements(program.statements.iter(), &mut units);
    units
}

fn collect_from_statements<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
    units: &mut Vec<DiagnosticUnit>,
) {
    for statement in statements {
        match statement {
            Statement::Namespace(namespace) => {
                collect_from_statements(namespace.statements().iter(), units)
            }
            _ if let Some(body) = crate::parser::class_like_body(statement) => {
                collect_from_members(body.members.iter(), units)
            }
            Statement::Function(function) => units.push(function_unit(function)),
            Statement::If(_) => {
                crate::parser::for_each_nested_function(statement, &mut |function| {
                    units.push(function_unit(function))
                })
            }
            _ => {}
        }
    }
}

fn collect_from_members<'a>(
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    units: &mut Vec<DiagnosticUnit>,
) {
    for member in members {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let MethodBody::Concrete(block) = &method.body else {
            continue;
        };
        let span = method.span();
        units.push(DiagnosticUnit {
            span: (span.start.offset, span.end.offset),
            body: (block.left_brace.start.offset, block.right_brace.end.offset),
            name: (method.name.span.start.offset, method.name.span.end.offset),
            declares_function: false,
            observed: false,
        });
    }
}

fn function_unit(function: &Function<'_>) -> DiagnosticUnit {
    let span = function.span();
    DiagnosticUnit {
        span: (span.start.offset, span.end.offset),
        body: (
            function.body.left_brace.start.offset,
            function.body.right_brace.end.offset,
        ),
        name: (
            function.name.span.start.offset,
            function.name.span.end.offset,
        ),
        declares_function: false,
        observed: false,
    }
}

/// Flag the units whose body declares a function, from the function
/// definitions `symbol_map` recorded.
pub(crate) fn mark_function_declarations(units: &mut [DiagnosticUnit], symbol_map: &SymbolMap) {
    for span in &symbol_map.spans {
        if !matches!(
            span.kind,
            SymbolKind::FunctionCall {
                is_definition: true,
                ..
            }
        ) {
            continue;
        }
        // Units are sorted and do not overlap, so only the last one
        // starting at or before the definition can hold it.
        let candidate = units.partition_point(|unit| unit.body.0 <= span.start);
        if let Some(unit) = candidate.checked_sub(1).map(|i| &mut units[i])
            && span.start < unit.body.1
        {
            unit.declares_function = true;
        }
    }
}

/// Flag the units whose name token starts at one of `name_offsets`: the
/// bodies inference read during a pass.
pub(crate) fn mark_observed(units: &mut [DiagnosticUnit], name_offsets: &[u32]) {
    for unit in units.iter_mut() {
        if name_offsets.contains(&unit.name.0) {
            unit.observed = true;
        }
    }
}

/// The indices of the units whose bodies differ between the two texts, or
/// `None` when anything outside the bodies differs too.
///
/// The units are paired up in order: with every byte between the bodies
/// equal (signatures, docblocks, class headers, properties, top-level
/// code), the two lists name the same declarations in the same places.
pub(crate) fn changed_units(
    old: &str,
    old_units: &[DiagnosticUnit],
    new: &str,
    new_units: &[DiagnosticUnit],
) -> Option<Vec<usize>> {
    if old_units.len() != new_units.len() {
        return None;
    }
    let mut old_at = 0usize;
    let mut new_at = 0usize;
    let mut changed = Vec::new();
    for (index, (old_unit, new_unit)) in old_units.iter().zip(new_units).enumerate() {
        let (old_start, old_end) = (old_unit.body.0 as usize, old_unit.body.1 as usize);
        let (new_start, new_end) = (new_unit.body.0 as usize, new_unit.body.1 as usize);
        if old.get(old_at..old_start)? != new.get(new_at..new_start)? {
            return None;
        }
        if old.get(old_start..old_end)? != new.get(new_start..new_end)? {
            changed.push(index);
        }
        old_at = old_end;
        new_at = new_end;
    }
    (old.get(old_at..)? == new.get(new_at..)?).then_some(changed)
}
