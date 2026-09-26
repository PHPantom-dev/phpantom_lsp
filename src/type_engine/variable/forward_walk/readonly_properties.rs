//! What the constructor leaves in a class's readonly properties, as the
//! starting point of every other method's scope.

use std::cell::RefCell;

use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::{Method, MethodBody};

use super::*;
use crate::atom::bytes_to_str;
use crate::types::PropertyInfo;

thread_local! {
    /// Classes whose constructor is being walked, as `(source address,
    /// class body offset)` pairs.
    ///
    /// The constructor walk can resolve a `$this->method()` whose body is
    /// walked in turn, and seeding that body would walk the same
    /// constructor again.  Re-entry for the same class leaves the inner
    /// body on the declared property types.
    static WALKING_CONSTRUCTOR: RefCell<Vec<(usize, u32)>> = const { RefCell::new(Vec::new()) };
}

/// RAII entry in [`WALKING_CONSTRUCTOR`].
struct ConstructorWalkGuard {
    key: (usize, u32),
}

impl ConstructorWalkGuard {
    fn enter(key: (usize, u32)) -> Option<Self> {
        WALKING_CONSTRUCTOR.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.contains(&key) {
                return None;
            }
            stack.push(key);
            Some(Self { key })
        })
    }
}

impl Drop for ConstructorWalkGuard {
    fn drop(&mut self) {
        WALKING_CONSTRUCTOR.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(pos) = stack.iter().rposition(|k| *k == self.key) {
                stack.remove(pos);
            }
        });
    }
}

/// Seed the scope a method of `ctx.current_class` starts from with what
/// the constructor assigns to the class's readonly properties.
///
/// A readonly property is written once, so what the constructor leaves in
/// it is what every other method reads.  The constructor's type is taken
/// only where it is narrower than the declared one, as PHPStan does: a
/// `?int` assigned to an `int` property stays `int`.
pub(crate) fn seed_constructor_readonly_properties(
    scope: &mut ScopeState,
    method_name: Option<&str>,
    ctx: &ForwardWalkCtx<'_>,
) {
    if method_name.is_none_or(|name| name.eq_ignore_ascii_case("__construct")) {
        return;
    }
    let class = ctx.current_class;
    let readonly: Vec<&PropertyInfo> = class
        .properties
        .iter()
        .filter(|p| !p.is_static && (p.is_readonly || class.is_readonly) && p.type_hint.is_some())
        .map(|p| &**p)
        .collect();
    if readonly.is_empty() {
        return;
    }
    let Some(_guard) =
        ConstructorWalkGuard::enter((ctx.content.as_ptr() as usize, class.start_offset))
    else {
        return;
    };

    let narrowed = crate::parser::with_parsed_program(
        ctx.content,
        "seed_constructor_readonly_properties",
        |program, _| {
            let Some(ctor) = find_constructor(program.statements.iter(), class.start_offset) else {
                return Vec::new();
            };
            constructor_narrowed_properties(ctor, &readonly, ctx)
        },
    );
    for (key, types) in narrowed {
        scope.seed(&key, types);
    }
}

/// The `$this->prop` keys the constructor leaves narrower than their
/// declared types, with those types.
fn constructor_narrowed_properties(
    ctor: &Method<'_>,
    readonly: &[&PropertyInfo],
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<(String, Vec<ResolvedType>)> {
    let MethodBody::Concrete(body) = &ctor.body else {
        return Vec::new();
    };
    // A promoted property holds the parameter, which has the declared type.
    let is_promoted = |name: &str| {
        ctor.parameter_list.parameters.iter().any(|param| {
            !param.modifiers.is_empty()
                && bytes_to_str(param.variable.name).strip_prefix('$') == Some(name)
        })
    };
    let candidates: Vec<(&PropertyInfo, String)> = readonly
        .iter()
        .filter(|p| !is_promoted(&p.name))
        .map(|p| (*p, format!("$this->{}", p.name)))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let walk_ctx = ForwardWalkCtx {
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: u32::MAX,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
        in_loop: false,
    };
    let mut ctor_scope = ScopeState::new();
    seed_this(&mut ctor_scope, &walk_ctx);
    seed_params(
        &mut ctor_scope,
        ctor.parameter_list.parameters.iter(),
        ctor.span().start.offset,
        Some("__construct"),
        false,
        &walk_ctx,
    );
    // Each property starts out on its declared type, so an assignment
    // only one branch makes joins back to it instead of standing for
    // every path.  Reading the property before the write throws, so the
    // declared type is as much as that state says.
    for (prop, key) in &candidates {
        if let Some(declared) = &prop.type_hint {
            ctor_scope.seed(key, walk_ctx.resolved_types_for(declared.clone()));
        }
    }
    {
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        walk_body_forward(body.statements.iter(), &mut ctor_scope, &walk_ctx);
    }

    candidates
        .into_iter()
        .filter_map(|(prop, key)| {
            let declared = prop.type_hint.as_ref()?;
            let assigned = ctor_scope.get(&key);
            if assigned.is_empty() {
                return None;
            }
            let assigned_type = ResolvedType::types_joined(assigned);
            if assigned_type.equivalent(declared)
                || !crate::class_lookup::is_subtype_of_typed(
                    &assigned_type,
                    declared,
                    ctx.class_loader,
                )
            {
                return None;
            }
            Some((key, assigned.to_vec()))
        })
        .collect()
}

/// The constructor of the class whose body opens at `body_offset`.
fn find_constructor<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
    body_offset: u32,
) -> Option<&'a Method<'a>> {
    for stmt in statements {
        let found = match stmt {
            Statement::Class(class) if class.left_brace.start.offset == body_offset => {
                return class.members.iter().find_map(|member| match member {
                    ClassLikeMember::Method(method)
                        if bytes_to_str(method.name.value).eq_ignore_ascii_case("__construct") =>
                    {
                        Some(method)
                    }
                    _ => None,
                });
            }
            Statement::Namespace(ns) => find_constructor(ns.statements().iter(), body_offset),
            Statement::Block(block) => find_constructor(block.statements.iter(), body_offset),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}
