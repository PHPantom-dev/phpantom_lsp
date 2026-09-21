//! What a callee leaves in a by-reference parameter.
//!
//! A parameter declared `?string &$key` describes what the *caller* may
//! hand over, and a callee that unconditionally assigns a `string` to it
//! has said something the declaration cannot: after the call the caller's
//! variable holds a `string`, null included nowhere. PHPStan reads the
//! same thing out of the body; without it every out-parameter reads back
//! at its widest declared form and the reads below the call are checked
//! against a type the callee ruled out.
//!
//! The reading is a *refinement only*: a body whose result the declaration
//! does not already admit is discarded, so an imprecise walk can lose
//! sharpness but never contradict what the signature promises. When the
//! parameter carries no type at all, there is nothing to contradict and
//! the reading stands on its own.
//!
//! The walk goes through [`resolve_variable_php_type`] — the same forward
//! walker every other consumer asks — so branch merging, early returns
//! and narrowing are accounted for by the shared pipeline rather than by
//! a second one written here.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_syntax::cst::Program;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::statement::Statement;

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::type_engine::resolver::Loaders;
use crate::types::{ClassInfo, FunctionInfo, ParameterInfo};

// ─── Callee identity ────────────────────────────────────────────────────────

/// The callee a by-reference argument was handed to.
///
/// Carries enough to find the declaration's source, and doubles as the
/// owner whose `@template` parameters an out type may name.
pub(crate) enum OutParamCallee {
    /// A global function, by the signature the function loader answered
    /// with.
    Function(Box<FunctionInfo>),
    /// A method, by the (already fully resolved) receiver class and the
    /// method name the call selected.
    Method(Arc<ClassInfo>, Atom),
}

impl OutParamCallee {
    /// The `@template` parameters the callee declares.
    ///
    /// An out type written in one of them (`usort`'s
    /// `array<TKey, TValue> &$array`) describes the caller's variable only
    /// once the call site has bound them, so the by-reference write-back
    /// paths leave such a parameter alone.
    pub(crate) fn template_params(&self) -> &[Atom] {
        match self {
            Self::Function(fi) => &fi.template_params,
            Self::Method(cls, name) => match cls.get_method(name) {
                Some(m) => &m.template_params,
                None => &[],
            },
        }
    }

    /// The memo key identifying one parameter of this callee.
    ///
    /// Built from interned names the symbol tables already hold, so it
    /// adds nothing to the global interner. `param_index` disambiguates
    /// two parameters of the same callee; the empty owner marks a plain
    /// function.
    fn key(&self, param_index: usize) -> OutTypeKey {
        match self {
            Self::Function(fi) => (Atom::default(), fi.name, param_index as u16),
            Self::Method(cls, name) => (atom(cls.fqn().as_ref()), *name, param_index as u16),
        }
    }

    /// The file the callee's body lives in, and the byte offset of its
    /// name token there.
    ///
    /// A method inherited from a trait or a parent is read out of the file
    /// that *declares* it: `name_offset` is relative to that file, so
    /// reading the receiver's own file at the same offset would land
    /// somewhere unrelated.
    fn declaration_site(&self, backend: &Backend) -> Option<(String, u32)> {
        match self {
            Self::Function(fi) => {
                let fqn = match fi.namespace {
                    Some(ref ns) => format!("{ns}\\{}", fi.name),
                    None => fi.name.to_string(),
                };
                let uri = backend
                    .symbols
                    .global_functions
                    .read()
                    .get(fqn.as_str())
                    .map(|(uri, _)| uri.clone())?;
                // An embedded stub has a synthetic URI and no body to read.
                if uri.starts_with("phpantom-stub-fn://") {
                    return None;
                }
                (fi.name_offset != 0).then_some((uri, fi.name_offset))
            }
            Self::Method(cls, name) => {
                let loader = |n: &str| backend.find_or_load_class(n);
                let declaring = crate::hover::find_declaring_class(
                    cls,
                    name,
                    &crate::hover::MemberKindForOrigin::Method,
                    &loader,
                );
                let method = declaring.get_method(name)?;
                if method.is_virtual || method.name_offset == 0 {
                    return None;
                }
                let uri = backend
                    .symbols
                    .fqn_uri_index
                    .read()
                    .get(&declaring.fqn())
                    .cloned()?;
                Some((uri, method.name_offset))
            }
        }
    }
}

// ─── Request-scoped memo ────────────────────────────────────────────────────

/// `(declaring class FQN or empty, function/method name, parameter index)`.
type OutTypeKey = (Atom, Atom, u16);

thread_local! {
    /// When `Some`, memoizes completed out-parameter readings for the
    /// current request. Every call site that passes a variable to the
    /// same out-parameter would otherwise re-walk the whole callee body,
    /// and a by-reference helper called from fifty places is common.
    static OUT_TYPE_MEMO: RefCell<Option<HashMap<OutTypeKey, Option<PhpType>>>> =
        const { RefCell::new(None) };

    /// The out-parameters currently being read. A callee that reaches
    /// itself (directly or through a cycle) re-enters on the same key and
    /// falls back to its declaration rather than recursing.
    static OUT_TYPE_VISITED: RefCell<HashSet<OutTypeKey>> = RefCell::new(HashSet::new());

    /// Nesting depth of out-parameter readings, so a chain of by-reference
    /// helpers cannot queue up an unbounded run of body walks.
    static OUT_TYPE_DEPTH: Cell<u8> = const { Cell::new(0) };
}

/// Activate the out-parameter memo for the current thread.
/// The guard [`with_out_type_memo`] hands back.
pub(crate) type OutTypeMemoGuard =
    crate::type_engine::MemoGuard<HashMap<OutTypeKey, Option<PhpType>>>;

pub(super) fn with_out_type_memo() -> OutTypeMemoGuard {
    crate::type_engine::activate_memo(&OUT_TYPE_MEMO)
}

/// How deep a chain of out-parameter readings may go.
///
/// Cycles are broken by [`OUT_TYPE_VISITED`]; this bounds the *cost* of
/// an acyclic chain, since each level runs a full forward walk of a body
/// that may in turn drive return-type inference of its own.
const MAX_OUT_TYPE_DEPTH: u8 = 2;

// ─── Entry point ────────────────────────────────────────────────────────────

/// The type the caller's argument holds after the call returns.
///
/// Starts from [`ParameterInfo::out_type`] — the declaration, with the
/// null an out-parameter's `= null` default implies removed — and
/// sharpens it with what the callee's body actually leaves behind.
pub(crate) fn effective_out_type(
    param: &ParameterInfo,
    param_index: usize,
    callee: &OutParamCallee,
    backend: Option<&Backend>,
) -> Option<PhpType> {
    let declared = param.out_type();
    let Some(backend) = backend else {
        return declared;
    };
    let Some(inferred) = infer_out_type(backend, callee, param, param_index) else {
        return declared;
    };
    match declared {
        // The declaration is the contract; a reading of the body may only
        // narrow it. A body the walk mistyped would otherwise hand the
        // caller a type the signature never promised.
        Some(declared) if inferred.is_subtype_of(&declared) => Some(inferred),
        Some(declared) => Some(declared),
        // Nothing declared, nothing to contradict.
        None => Some(inferred),
    }
}

/// Read the callee's body for the type it leaves in `param`, memoized and
/// bounded.
fn infer_out_type(
    backend: &Backend,
    callee: &OutParamCallee,
    param: &ParameterInfo,
    param_index: usize,
) -> Option<PhpType> {
    let key = callee.key(param_index);

    crate::type_engine::memoized_bounded_inference(
        &OUT_TYPE_MEMO,
        &OUT_TYPE_VISITED,
        &OUT_TYPE_DEPTH,
        MAX_OUT_TYPE_DEPTH,
        key,
        key,
        || {
            callee
                .declaration_site(backend)
                .and_then(|(uri, name_offset)| {
                    read_out_type(backend, &uri, name_offset, &param.name)
                })
                .filter(|ty| !ty.is_untyped() && !ty.is_mixed())
        },
    )
}

/// Resolve `param_name` at the closing brace of the body declared at
/// `name_offset` in `uri`.
///
/// The closing brace is where the fall-through paths have all merged, so
/// a parameter every path assigns reads back as what they assigned and one
/// only some paths touch reads back as the join with its declared type.
fn read_out_type(
    backend: &Backend,
    uri: &str,
    name_offset: u32,
    param_name: &str,
) -> Option<PhpType> {
    let content = backend.get_file_content(uri)?;
    let body_end = with_parsed_program(&content, "out_param_body", |program, _| {
        body_close_offset(program, name_offset)
    })?;

    let local_classes: Vec<Arc<ClassInfo>> = backend
        .symbols
        .uri_classes_index
        .read()
        .get(uri)
        .cloned()
        .unwrap_or_default();
    let file_use_map = backend.file_use_map(uri);
    let file_namespace = backend.first_file_namespace(uri);
    let class_loader = backend.class_loader_with(&local_classes, &file_use_map, &file_namespace);
    let function_loader = backend.function_loader_with(None, &file_use_map, &file_namespace);

    let enclosing_class = local_classes.iter().find(|c| {
        !c.name.starts_with("__anonymous@")
            && body_end >= c.start_offset
            && body_end <= c.end_offset
    });

    // The scope cache and the chain cache both key on offsets alone, and
    // the offsets below belong to another file. Neither may answer, nor
    // record, anything while this walk runs.
    let _isolated = (
        crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
        crate::type_engine::resolver::with_isolated_chain_cache(),
    );

    crate::type_engine::variable::resolution::resolve_variable_php_type(
        param_name,
        &content,
        body_end,
        enclosing_class.map(Arc::as_ref),
        &local_classes,
        &class_loader,
        Some(backend),
        Loaders::with_function(Some(&function_loader)),
    )
}

/// The offset of the closing brace of the function or method whose name
/// token starts at `name_offset`.
///
/// `None` for a declaration with no body (abstract, interface) and for an
/// offset that names nothing in this file, which is what a stale index
/// entry looks like.
fn body_close_offset(program: &Program<'_>, name_offset: u32) -> Option<u32> {
    fn in_statements<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        name_offset: u32,
    ) -> Option<u32> {
        for stmt in statements {
            let found = match stmt {
                Statement::Function(func) if func.name.span.start.offset == name_offset => {
                    Some(func.body.right_brace.start.offset)
                }
                Statement::Class(class) => in_members(class.members.iter(), name_offset),
                Statement::Trait(tr) => in_members(tr.members.iter(), name_offset),
                Statement::Enum(en) => in_members(en.members.iter(), name_offset),
                Statement::Interface(itf) => in_members(itf.members.iter(), name_offset),
                Statement::Namespace(ns) => in_statements(ns.statements().iter(), name_offset),
                Statement::Block(block) => in_statements(block.statements.iter(), name_offset),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    fn in_members<'a>(
        members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
        name_offset: u32,
    ) -> Option<u32> {
        for member in members {
            if let ClassLikeMember::Method(method) = member
                && method.name.span.start.offset == name_offset
                && let MethodBody::Concrete(block) = &method.body
            {
                return Some(block.right_brace.start.offset);
            }
        }
        None
    }

    in_statements(program.statements.iter(), name_offset)
}

#[cfg(test)]
#[path = "out_param_tests.rs"]
mod tests;
