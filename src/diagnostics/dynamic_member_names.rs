//! Dynamic member name diagnostics.
//!
//! A member name written as an expression (`$obj->{$name}`,
//! `$obj->$name()`, `Cls::{$name}()`) has to be usable as a name at
//! runtime, and properties and methods disagree on what that means:
//!
//! ```php
//! function f(\Stringable $s, \stdClass $o, Target $t): void {
//!     $t->{$s};      // fine: property names are string-cast
//!     $t->{$o};      // Error: stdClass cannot be converted to string
//!     $t->{$s}();    // Error: method name must be a string
//! }
//! ```
//!
//! A property name goes through string conversion, so scalars, `null`
//! and `Stringable` objects are fine, while an array or any other object
//! is not.  A method name must already be a string: PHP does not call
//! `__toString()` for it.
//!
//! Only a selector whose every possible type breaks the rule is reported.
//! A type the check cannot classify (unresolved, `mixed`, a template
//! parameter, a class that does not load) keeps the selector silent.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMemberSelector;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::variable::iterable_elements::resolve_expression_type;
use crate::types::ClassInfo;
use crate::virtual_members::resolve_class_fully_cached;

use super::helpers::{FileDiagnosticContext, TypeCheckCtx, collect_type_check};

/// Diagnostic code used for a dynamic member name of the wrong type.
pub(crate) const INVALID_MEMBER_NAME_CODE: &str = "invalid_member_name";

/// A dynamic member name whose type can never name the member.
struct InvalidMemberName {
    /// The resolved type of the selector expression.
    name_type: PhpType,
    /// Whether the name selects a method rather than a property.
    is_method: bool,
    /// Byte range of the selector expression.
    start: usize,
    end: usize,
}

struct SelectorCheck<'a> {
    backend: &'a Backend,
    content: &'a str,
    type_ctx: &'a TypeCheckCtx<'a>,
    out: Vec<InvalidMemberName>,
}

impl SelectorCheck<'_> {
    fn check(&mut self, selector: &ClassLikeMemberSelector<'_>, is_method: bool) {
        if matches!(
            selector,
            ClassLikeMemberSelector::Identifier(_) | ClassLikeMemberSelector::Missing(_)
        ) {
            return;
        }
        let span = selector.span();
        let site = span.start.offset;
        if !super::incremental::reports_site(site) {
            return;
        }
        let var_ctx = self.type_ctx.loaders.var_ctx_at(self.content, site);
        let name_type = match selector {
            ClassLikeMemberSelector::Expression(selector) => {
                resolve_expression_type(selector.expression, &var_ctx)
            }
            ClassLikeMemberSelector::Variable(variable) => {
                resolve_expression_type(&Expression::Variable(variable.clone()), &var_ctx)
            }
            ClassLikeMemberSelector::Identifier(_) | ClassLikeMemberSelector::Missing(_) => return,
        };
        let Some(name_type) = name_type else {
            return;
        };
        let class_loader = self.type_ctx.loaders.resolved_class_loader();
        let name_type = name_type.resolve_names(&|name: &str| match class_loader(Fqn::new(name)) {
            Some(class) => class.fqn().to_string(),
            None => name.trim_start_matches('\\').to_string(),
        });
        let names = NameRule {
            is_method,
            class_loader,
            backend: self.backend,
        };
        if names.rejects(&name_type) {
            self.out.push(InvalidMemberName {
                name_type,
                is_method,
                start: span.start.offset as usize,
                end: span.end.offset as usize,
            });
        }
    }
}

/// Hands every member selector in the file to [`SelectorCheck::check`].
struct SelectorWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, SelectorCheck<'_>> for SelectorWalker {
    fn walk_in_property_access(
        &self,
        node: &'ast PropertyAccess<'arena>,
        check: &mut SelectorCheck<'_>,
    ) {
        check.check(&node.property, false);
    }

    fn walk_in_null_safe_property_access(
        &self,
        node: &'ast NullSafePropertyAccess<'arena>,
        check: &mut SelectorCheck<'_>,
    ) {
        check.check(&node.property, false);
    }

    fn walk_in_method_call(&self, node: &'ast MethodCall<'arena>, check: &mut SelectorCheck<'_>) {
        check.check(&node.method, true);
    }

    fn walk_in_null_safe_method_call(
        &self,
        node: &'ast NullSafeMethodCall<'arena>,
        check: &mut SelectorCheck<'_>,
    ) {
        check.check(&node.method, true);
    }

    fn walk_in_static_method_call(
        &self,
        node: &'ast StaticMethodCall<'arena>,
        check: &mut SelectorCheck<'_>,
    ) {
        check.check(&node.method, true);
    }

    fn walk_in_method_partial_application(
        &self,
        node: &'ast MethodPartialApplication<'arena>,
        check: &mut SelectorCheck<'_>,
    ) {
        check.check(&node.method, true);
    }

    fn walk_in_static_method_partial_application(
        &self,
        node: &'ast StaticMethodPartialApplication<'arena>,
        check: &mut SelectorCheck<'_>,
    ) {
        check.check(&node.method, true);
    }
}

/// What a dynamic property or method name may be.
struct NameRule<'a> {
    is_method: bool,
    class_loader: &'a ResolvedClassLoader<'a>,
    backend: &'a Backend,
}

impl NameRule<'_> {
    /// Whether no value of type `ty` can name the member.
    fn rejects(&self, ty: &PhpType) -> bool {
        match ty.kind() {
            TypeKind::Union(members) => members.iter().all(|m| self.rejects(m)),
            TypeKind::Nullable(inner) => self.is_method && self.rejects(inner),
            // A value of `A&B` is one object: no string at all for a
            // method, and a valid property name when either side converts.
            TypeKind::Intersection(members) if self.is_method => {
                members.iter().any(|m| self.rejects(m))
            }
            TypeKind::Intersection(members) => members.iter().all(|m| self.rejects(m)),
            _ if self.is_method => self.rejects_method_name(ty),
            _ => self.rejects_property_name(ty),
        }
    }

    fn rejects_method_name(&self, ty: &PhpType) -> bool {
        if ty.is_int_subtype()
            || ty.is_float_subtype()
            || ty.is_bool()
            || ty.is_true()
            || ty.is_false()
            || ty.is_null()
            || ty.is_resource()
            || ty.is_array_like()
            || ty.is_closure()
            || ty.is_object()
            || ty.is_object_shape()
        {
            return true;
        }
        self.object_class(ty).is_some()
    }

    fn rejects_property_name(&self, ty: &PhpType) -> bool {
        // `iterable` may hold a `Traversable` that also converts.
        if (ty.is_array_like() && !ty.is_named_ci("iterable")) || ty.is_closure() {
            return true;
        }
        self.object_class(ty)
            .is_some_and(|class| !self.is_stringable(&class))
    }

    /// The class of an object type, when it names one that loads.
    fn object_class(&self, ty: &PhpType) -> Option<Arc<ClassInfo>> {
        let name = match ty.kind() {
            TypeKind::Named(name) if !ty.is_scalar() => name,
            TypeKind::Generic(generic) if !ty.is_scalar() => &generic.name,
            TypeKind::StaticType(name) | TypeKind::ThisType(name) => name,
            _ => return None,
        };
        (self.class_loader)(Fqn::new(name))
    }

    fn is_stringable(&self, class: &ClassInfo) -> bool {
        is_subtype_of(class, "Stringable", self.class_loader)
            || resolve_class_fully_cached(
                class,
                self.class_loader,
                &self.backend.resolved_class_cache,
            )
            .get_method_ci("__toString")
            .is_some()
    }
}

impl Backend {
    /// Collect dynamic member name diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_dynamic_member_name_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_dynamic_member_name_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_dynamic_member_name_diagnostics`] over an
    /// already-gathered [`FileDiagnosticContext`].
    pub(crate) fn collect_dynamic_member_name_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // Every dynamic selector follows `->`, `?->` or `::` with `{` or
        // `$`; a file without one has nothing to check or parse.
        if !["->{", "->$", "::{", "::$"]
            .iter()
            .any(|marker| content.contains(marker))
        {
            return;
        }
        out.extend(collect_type_check(
            self,
            &ctx.file,
            uri,
            content,
            INVALID_MEMBER_NAME_CODE,
            |type_ctx| {
                with_parsed_program(content, "dynamic_member_name_diagnostics", |program, _| {
                    let mut check = SelectorCheck {
                        backend: self,
                        content,
                        type_ctx,
                        out: Vec::new(),
                    };
                    SelectorWalker.walk_program(program, &mut check);
                    check.out
                })
            },
            |site, _| {
                let kind = if site.is_method { "Method" } else { "Property" };
                Some((
                    (site.start, site.end),
                    format!(
                        "{kind} name must be a string, but {} was given",
                        site.name_type
                    ),
                ))
            },
        ));
    }
}
