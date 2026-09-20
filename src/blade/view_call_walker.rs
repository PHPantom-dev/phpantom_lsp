//! The AST walk behind call-site inference: finding the `view()` /
//! `View::make()` / `@include`-family call sites that name a template and
//! collecting the data entries each one passes, before
//! [`super::call_site_inference`] resolves their types against the caller's
//! scope.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::literal::{Literal, LiteralString};
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ClassInfo;

use super::call_site_inference::ByteRange;

/// Collects every `blade_bound_attr_directive(EXPR)` call in a Blade file's
/// virtual PHP, in document order. The preprocessor emits exactly one such
/// call per bound HTML attribute that is not consumed as a component call's
/// argument (see `super::preprocessor`), so this order matches the order
/// `super::component_tags::scan_component_tag_calls` counts bound attributes
/// in. That marker is exclusive to bound attributes — unlike the generic
/// `blade_directive` shared by `@class`, `@json`, and other directives —
/// so a directive appearing between two tags cannot shift this sequence out
/// of sync with that count.
pub(super) struct BladeDirectiveCollectCtx<'ast, 'arena> {
    pub(super) calls: Vec<&'ast Expression<'arena>>,
}

pub(super) struct BladeDirectiveWalker;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, BladeDirectiveCollectCtx<'ast, 'arena>>
    for BladeDirectiveWalker
{
    fn walk_in_function_call(
        &self,
        node: &'ast FunctionCall<'arena>,
        ctx: &mut BladeDirectiveCollectCtx<'ast, 'arena>,
    ) {
        let Expression::Identifier(ident) = node.function else {
            return;
        };
        if bytes_to_str(ident.value()) != "blade_bound_attr_directive" {
            return;
        }
        if let Some(arg) = node.argument_list.arguments.iter().next() {
            ctx.calls.push(arg.value());
        }
    }
}

/// Which of the two variables an `@each` binds an entry describes.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum IterationPart {
    Item,
    Key,
}

/// One variable passed at a call site: the value expression (array entry
/// / `->with()` value), the same-named variable to resolve at the
/// call-site offset (for `compact('name')`), one of the two variables
/// `@each` derives from the collection it iterates, or a whole data
/// argument whose entries only its type names.
#[derive(Clone)]
pub(super) enum SiteEntry<'ast, 'arena> {
    Expr {
        name: String,
        key_range: ByteRange,
        expr: &'ast Expression<'arena>,
    },
    Variable {
        name: String,
        key_range: ByteRange,
    },
    Iteration {
        name: String,
        key_range: ByteRange,
        collection: &'ast Expression<'arena>,
        part: IterationPart,
    },
    /// A data argument written as anything but an array literal or a
    /// `compact()` call — `view('page', $data)`, `->with($extra)`,
    /// `array_merge($a, $b)`, a method call returning an array. It names
    /// its variables only in its type, so it stands for however many
    /// entries [`data_shape_entries`] reads off it.
    Shape {
        expr: &'ast Expression<'arena>,
    },
}

impl SiteEntry<'_, '_> {
    /// Whether Blade names the variable itself.  Only `@each`'s `$key`
    /// does: its name comes from the directive rather than from anything
    /// the call site writes.
    pub(super) fn framework_bound(&self) -> bool {
        matches!(
            self,
            SiteEntry::Iteration {
                part: IterationPart::Key,
                ..
            }
        )
    }
}

/// One call site as the walker finds it, before its entries are resolved.
pub(super) struct SiteDraft<'ast, 'arena> {
    /// The offset of the `view()` call itself, which every chained
    /// `->with(…)` is folded into and which types resolve at.
    pub(super) offset: u32,
    pub(super) name_range: ByteRange,
    pub(super) entries: Vec<SiteEntry<'ast, 'arena>>,
    pub(super) complete: bool,
    pub(super) forwards_scope: bool,
}

pub(super) struct CollectCtx<'w, 'ast, 'arena> {
    pub(super) sites: &'w mut Vec<SiteDraft<'ast, 'arena>>,
}

impl<'ast, 'arena> CollectCtx<'_, 'ast, 'arena> {
    /// The draft for the view call at `offset` that names the template at
    /// `name_range`, created if the walker has not reached it yet.
    ///
    /// A chained `->with(…)` is walked *before* the `view()` call it hangs
    /// off (the method call is the outer node), so either end of the chain
    /// may be the first to open the site.
    ///
    /// The name is part of the key, not just the offset: an
    /// `@includeFirst(['custom.header', 'partials.header'])` renders whichever
    /// of its candidates exists, so one call is a site of each template it
    /// names, and each is judged against its own contract.
    fn site(&mut self, offset: u32, name_range: ByteRange) -> &mut SiteDraft<'ast, 'arena> {
        if let Some(index) = self
            .sites
            .iter()
            .position(|site| site.offset == offset && site.name_range == name_range)
        {
            return &mut self.sites[index];
        }
        self.sites.push(SiteDraft {
            offset,
            name_range,
            entries: Vec::new(),
            complete: true,
            forwards_scope: true,
        });
        self.sites.last_mut().expect("just pushed")
    }

    /// Record the same data against every template one call names.
    fn record(
        &mut self,
        offset: u32,
        name_ranges: &[ByteRange],
        entries: Vec<SiteEntry<'ast, 'arena>>,
        complete: bool,
        forwards_scope: bool,
    ) {
        for name_range in name_ranges {
            let site = self.site(offset, *name_range);
            site.entries.extend(entries.iter().cloned());
            site.complete &= complete;
            site.forwards_scope &= forwards_scope;
        }
    }
}

/// Walker that finds `view('name', …)` / `View::make('name', …)` calls
/// whose view-name string contents sit at one of the requested offsets,
/// and collects the data entries they pass: the array literal /
/// `compact()` call that follows the name, plus any `->with(…)` chained
/// onto the call.
pub(super) struct ViewCallWalker<'a> {
    pub(super) offsets: &'a [u32],
}

impl ViewCallWalker<'_> {
    /// The ranges of the view-name strings in an argument list, along with
    /// the index of the argument holding them, when the list names one of
    /// the views asked about.
    ///
    /// The name is looked for at any position rather than only the first:
    /// `Route::view('/about', 'pages.about', …)` puts the URI first, and
    /// the data argument is always the one after the name whatever the
    /// helper's shape.
    ///
    /// One argument can name several templates: a `…First` directive and the
    /// factory's `first()` take a list of candidates and render whichever
    /// exists, so every entry of the array is one the call reaches.
    fn matches(&self, argument_list: &ArgumentList<'_>) -> Option<(usize, Vec<ByteRange>)> {
        argument_list
            .arguments
            .iter()
            .enumerate()
            .find_map(|(index, argument)| {
                let ranges = self.matching_ranges(argument.value());
                (!ranges.is_empty()).then_some((index, ranges))
            })
    }

    /// The view-name ranges one argument holds: the string itself, or the
    /// entries of the candidate array a `…First` directive names.
    fn matching_ranges(&self, expr: &Expression<'_>) -> Vec<ByteRange> {
        let mut ranges = Vec::new();
        match expr {
            Expression::Literal(Literal::String(s)) => {
                let inner = (s.span.start.offset + 1, s.span.end.offset - 1);
                if self.offsets.contains(&inner.0) {
                    ranges.push(inner);
                }
            }
            Expression::Array(array) => {
                for element in array.elements.iter() {
                    if let ArrayElement::Value(value) = element {
                        ranges.extend(self.matching_ranges(value.value));
                    }
                }
            }
            Expression::LegacyArray(array) => {
                for element in array.elements.iter() {
                    if let ArrayElement::Value(value) = element {
                        ranges.extend(self.matching_ranges(value.value));
                    }
                }
            }
            _ => {}
        }
        ranges
    }
}

impl<'ast, 'arena, 'w> mago_syntax::walker::Walker<'ast, 'arena, CollectCtx<'w, 'ast, 'arena>>
    for ViewCallWalker<'_>
{
    fn walk_in_function_call(
        &self,
        node: &'ast FunctionCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let Expression::Identifier(ident) = node.function else {
            return;
        };
        let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
        if !is_view_render_function(name) {
            return;
        }
        let Some((index, name_ranges)) = self.matches(&node.argument_list) else {
            return;
        };
        let mut entries = Vec::new();
        // `@each` names its partial first and then a collection and an item
        // name, so the argument after the view name is not a data array and
        // the two variables the partial receives are derived rather than
        // listed.  A match on any later argument (the optional empty-view
        // name) says nothing about what the partial is handed.
        let complete = if is_each_render_function(name) {
            index == 0 && collect_each_arguments(&node.argument_list, &mut entries)
        } else {
            collect_data_argument(&node.argument_list, index + 1, &mut entries)
        };
        ctx.record(
            node.span().start.offset,
            &name_ranges,
            entries,
            complete,
            !is_each_render_function(name),
        );
    }

    fn walk_in_static_method_call(
        &self,
        node: &'ast StaticMethodCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        let method_name = bytes_to_str(method.value);
        if is_view_facade(node.class) && is_render_each_method(method_name) {
            collect_render_each_sites(node.span().start.offset, &node.argument_list, self, ctx);
            return;
        }
        if !is_view_render_static_call(node.class, method_name) {
            return;
        }
        let Some((index, name_ranges)) = self.matches(&node.argument_list) else {
            return;
        };
        let mut entries = Vec::new();
        let complete = collect_data_argument(&node.argument_list, index + 1, &mut entries);
        ctx.record(
            node.span().start.offset,
            &name_ranges,
            entries,
            complete,
            true,
        );
    }

    /// `new Content(view: 'emails.orders.shipped', with: […])`, the value a
    /// mailable's `content()` returns.
    fn walk_in_instantiation(
        &self,
        node: &'ast Instantiation<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let Some(argument_list) = &node.argument_list else {
            return;
        };
        let Some((_, name_ranges)) = self.matches(argument_list) else {
            return;
        };
        let mut entries = Vec::new();
        let complete = collect_content_data_argument(argument_list, &mut entries);
        ctx.record(
            node.span().start.offset,
            &name_ranges,
            entries,
            complete,
            true,
        );
    }

    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        let method_name = bytes_to_str(method.value);

        // `renderEach()` is the PHP spelling of `@each`, down to the
        // argument order, so both templates it names are read the way the
        // directive's are.
        if is_render_each_method(method_name) {
            collect_render_each_sites(node.span().start.offset, &node.argument_list, self, ctx);
            return;
        }

        // A render the receiver's type decides: a mailable's
        // `$this->view('name', […])` and the view factory's own `make()` /
        // `first()` / `renderWhen()`.  Which of those the receiver actually
        // is was settled when the name was indexed, so a matching name at
        // the position the method takes one is the whole test here.
        if let Some(name_index) = view_render_method_name_index(method_name)
            && let Some((index, name_ranges)) = self.matches(&node.argument_list)
            && index == name_index
        {
            let mut entries = Vec::new();
            let complete = collect_data_argument(&node.argument_list, index + 1, &mut entries);
            ctx.record(
                node.span().start.offset,
                &name_ranges,
                entries,
                complete,
                true,
            );
            return;
        }

        // `->with('key', $value)` / `->with(['key' => $value])` /
        // `->withKey($value)` chained onto a matching `view()` call.  The
        // receiver chain may pass through other builder methods
        // (`->layout(…)`), so scan the whole spine for the matching view
        // call.
        if !method_name.starts_with("with") && !method_name.starts_with("With") {
            return;
        }
        let Some((offset, name_ranges)) = matching_view_call_in_chain(node.object, self) else {
            return;
        };

        let mut entries = Vec::new();
        let mut complete = true;
        // `->withUser($user)` is Laravel's magic setter for `$user`; the
        // name is the method's own tail, so the method identifier is what
        // a diagnostic points at.
        if let Some(magic) = magic_with_name(method_name) {
            match node.argument_list.arguments.iter().next() {
                Some(value) => entries.push(SiteEntry::Expr {
                    name: magic,
                    key_range: (method.span.start.offset, method.span.end.offset),
                    expr: value.value(),
                }),
                None => complete = false,
            }
        } else {
            let mut args = node.argument_list.arguments.iter();
            match (args.next(), args.next()) {
                (Some(key_arg), Some(value_arg)) => {
                    // ->with('key', $value)
                    match key_arg.value() {
                        Expression::Literal(Literal::String(s)) => {
                            match string_literal_contents(s) {
                                Some(name) => entries.push(SiteEntry::Expr {
                                    name,
                                    key_range: (s.span.start.offset + 1, s.span.end.offset - 1),
                                    expr: value_arg.value(),
                                }),
                                None => complete = false,
                            }
                        }
                        _ => complete = false,
                    }
                }
                (Some(single), None) => {
                    // ->with(['key' => $value, …]) or ->with(compact('key'))
                    complete = collect_from_data_expr(single.value(), &mut entries);
                }
                _ => complete = false,
            }
        }

        ctx.record(offset, &name_ranges, entries, complete, true);
    }
}

/// The type one of `@each`'s two variables has, derived from the
/// collection the directive iterates.
///
/// The derivation is the same one `foreach ($collection as $key => $item)`
/// uses, so a `Collection<int, User>`, an `array<string, Row>`, and a
/// class whose `@implements IteratorAggregate` says what it holds all
/// answer the way they do in a loop.  A collection that says nothing about
/// its entries leaves the item `mixed` and the key `int|string`, matching
/// what PHP guarantees about iterating anything at all.
pub(super) fn each_variable_type(
    collection: &Expression<'_>,
    part: IterationPart,
    var_ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    use crate::type_engine::variable::foreach_resolution as iter;

    let derived = iter::resolve_expression_type(collection, var_ctx).and_then(|collection_ty| {
        let iter_ctx = iter::IterableCtx::from_var_ctx(var_ctx);
        match part {
            IterationPart::Item => iter::iteration_value_type(&collection_ty, &iter_ctx),
            IterationPart::Key => iter::iteration_key_type(&collection_ty, &iter_ctx),
        }
    });
    derived.unwrap_or_else(|| match part {
        IterationPart::Item => PhpType::mixed(),
        IterationPart::Key => PhpType::union(vec![PhpType::int(), PhpType::string()]),
    })
}

/// Render the class names a resolved type mentions as FQNs.
///
/// A call site's types are resolved in the caller's import context, while
/// the template that receives them has neither those imports nor a
/// namespace: an injected `@var User $user` in a template's prologue means
/// the global `\User`, and a template's contract is qualified the same way
/// before a call site is judged against it.
pub(super) fn qualify_class_names(
    ty: PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    ty.resolve_names(&|name: &str| match class_loader(name) {
        Some(cls) => format!("\\{}", cls.fqn()),
        None => name.to_string(),
    })
}

/// The interface Laravel's view factory accepts in place of a data array,
/// converting it with `toArray()` before rendering.
const ARRAYABLE: &str = "Illuminate\\Contracts\\Support\\Arrayable";

/// The variables a data argument hands the template when only its type
/// names them.
///
/// The argument is resolved through the shared pipeline and read as a
/// single constant array shape, which is what Bladestan reads off
/// `$scope->getType()`. Optional keys (`array{user?: User}`) are dropped:
/// the caller may or may not pass them, so they stay reportable as missing
/// while the guaranteed keys are still type-checked.
///
/// `None` when the type is not one shape, which leaves the call site
/// incomplete and stands the missing and unknown checks down as before.
pub(super) fn data_shape_entries(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<(String, PhpType)>> {
    let ty = crate::type_engine::variable::foreach_resolution::resolve_expression_type(expr, ctx)?;
    array_shape_entries(&ty).or_else(|| arrayable_shape_entries(&ty, ctx))
}

/// The guaranteed entries of an array shape, under the names Blade's
/// `extract()` would bind them to.
///
/// A positional entry, or one keyed by anything that is not a variable
/// name, is dropped rather than counted against the shape: `extract()`
/// skips it too, so it hides nothing the caller passes.
fn array_shape_entries(ty: &PhpType) -> Option<Vec<(String, PhpType)>> {
    let TypeKind::ArrayShape(entries) = ty.kind() else {
        return None;
    };
    Some(
        entries
            .iter()
            .filter(|entry| !entry.optional)
            .filter_map(|entry| {
                let key = entry.key.as_deref().filter(|key| is_variable_name(key))?;
                Some((key.to_string(), entry.value_type.clone()))
            })
            .collect(),
    )
}

/// The entries an `Arrayable` hands over: the factory calls `toArray()` on
/// one before rendering, so its return type describes the data exactly as
/// an array argument's own type does.
fn arrayable_shape_entries(
    ty: &PhpType,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<(String, PhpType)>> {
    let class = (ctx.class_loader)(ty.base_name()?)?;
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &class,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    if !crate::class_lookup::is_subtype_of(&merged, ARRAYABLE, ctx.class_loader) {
        return None;
    }
    array_shape_entries(merged.get_method_ci("toArray")?.return_type.as_ref()?)
}

/// Whether a helper function renders a view named by one of its string
/// arguments.
///
/// `blade_view_directive` is what the preprocessor compiles Blade's own
/// `@include` family, `@extends`, and `@component` into, so a template
/// rendering another template is judged by the same rules a controller
/// is.  `@each` compiles to [`is_each_render_function`]'s marker instead,
/// because its arguments do not describe a data array.
fn is_view_render_function(name: &str) -> bool {
    name.eq_ignore_ascii_case("view")
        || name.eq_ignore_ascii_case("blade_view_directive")
        || is_each_render_function(name)
}

/// Whether the call is the preprocessor's compilation of `@each`.
fn is_each_render_function(name: &str) -> bool {
    name.eq_ignore_ascii_case("blade_each_directive")
}

/// Whether a static call renders a view: the `View` facade's factory
/// methods, the `Route::view()` shorthand that binds a URI straight to a
/// template, or `Response::view()`.
///
/// `View::exists()` is left out on purpose. It names a template without
/// rendering it, so it hands it nothing, and reading it as a render would
/// report every variable the template declares as missing.
fn is_view_render_static_call(class: &Expression<'_>, method: &str) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let subject = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
    let is_facade = |short: &str, fqn: &str| {
        subject.eq_ignore_ascii_case(short) || subject.eq_ignore_ascii_case(fqn)
    };
    (is_view_facade(class) && view_render_method_name_index(method).is_some())
        || (is_facade("Route", "Illuminate\\Support\\Facades\\Route")
            && method.eq_ignore_ascii_case("view"))
        || (is_facade("Response", "Illuminate\\Support\\Facades\\Response")
            && method.eq_ignore_ascii_case("view"))
}

/// Whether a static call's subject is the `View` facade, which proxies the
/// view factory.
fn is_view_facade(class: &Expression<'_>) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let subject = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
    subject.eq_ignore_ascii_case("View")
        || subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\View")
}

/// Whether a method is the view factory's `renderEach()`.
fn is_render_each_method(method: &str) -> bool {
    method.eq_ignore_ascii_case("renderEach")
}

/// Record the sites a `renderEach($view, $data, $iterator, $empty)` call is.
///
/// It renders `$view` once per entry of `$data`, with the entry under the
/// name `$iterator` spells and the key beside it, and `$empty` once with
/// nothing at all when the collection is empty. Neither partial sees the
/// scope the call is written in, so neither forwards it.
fn collect_render_each_sites<'ast, 'arena>(
    offset: u32,
    argument_list: &'ast ArgumentList<'arena>,
    walker: &ViewCallWalker<'_>,
    ctx: &mut CollectCtx<'_, 'ast, 'arena>,
) {
    let mut arguments = argument_list.arguments.iter();
    if let Some(view) = arguments.next() {
        let ranges = walker.matching_ranges(view.value());
        if !ranges.is_empty() {
            let mut entries = Vec::new();
            let complete = collect_each_arguments(argument_list, &mut entries);
            ctx.record(offset, &ranges, entries, complete, false);
        }
    }
    if let Some(empty) = arguments.nth(2) {
        let ranges = walker.matching_ranges(empty.value());
        if !ranges.is_empty() {
            ctx.record(offset, &ranges, Vec::new(), true, false);
        }
    }
}

/// The argument index at which a view-rendering *method* names its
/// template, or `None` when the method names none.
///
/// Covers both families the symbol map indexes by receiver type: a
/// mailable's `view()` / `text()` / `markdown()`, and the view factory's
/// `make()` / `first()` / `renderWhen()` / `renderUnless()`. Which of the
/// two a receiver is was decided when the name was indexed, so the two sets
/// need not be told apart again here.
fn view_render_method_name_index(method: &str) -> Option<usize> {
    match method.to_ascii_lowercase().as_str() {
        "view" | "text" | "markdown" | "make" | "first" => Some(0),
        "renderwhen" | "renderunless" => Some(1),
        _ => None,
    }
}

/// The variable a `->withSomething()` magic setter names, following
/// Laravel's `View::__call()` (`with` plus the camel-cased tail).
///
/// Plain `->with(…)` is not a magic setter, and neither is a method whose
/// tail does not start a new word (`->within(…)`).
fn magic_with_name(method: &str) -> Option<String> {
    let rest = method
        .strip_prefix("with")
        .or_else(|| method.strip_prefix("With"))?;
    let mut chars = rest.chars();
    let first = chars.next().filter(|ch| ch.is_uppercase())?;
    Some(first.to_lowercase().chain(chars).collect())
}

/// The offset and view-name ranges of the `view()` / `View::make()` /
/// `$this->view()` call a method call's receiver spine ends in, when it
/// names one of the views asked about.
///
/// Walks through chained method calls (`view(…)->with(…)->with(…)`) but
/// not through variables — a `$view = view(…); $view->with(…)` split is
/// out of scope.
fn matching_view_call_in_chain(
    mut expr: &Expression<'_>,
    walker: &ViewCallWalker<'_>,
) -> Option<(u32, Vec<ByteRange>)> {
    loop {
        match expr {
            Expression::Call(Call::Function(fc)) => {
                let Expression::Identifier(ident) = fc.function else {
                    return None;
                };
                let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
                if !is_view_render_function(name) {
                    return None;
                }
                let (_, ranges) = walker.matches(&fc.argument_list)?;
                return Some((fc.span().start.offset, ranges));
            }
            Expression::Call(Call::StaticMethod(sc)) => {
                let ClassLikeMemberSelector::Identifier(method) = &sc.method else {
                    return None;
                };
                if !is_view_render_static_call(sc.class, bytes_to_str(method.value)) {
                    return None;
                }
                let (_, ranges) = walker.matches(&sc.argument_list)?;
                return Some((sc.span().start.offset, ranges));
            }
            Expression::Call(Call::Method(mc)) => {
                // `$this->view('emails.shipped')->with([…])` in a mailable
                // renders from the link the spine passes through, not from
                // the one it ends at.
                if let ClassLikeMemberSelector::Identifier(method) = &mc.method
                    && let Some(name_index) =
                        view_render_method_name_index(bytes_to_str(method.value))
                    && let Some((index, ranges)) = walker.matches(&mc.argument_list)
                    && index == name_index
                {
                    return Some((mc.span().start.offset, ranges));
                }
                expr = mc.object;
            }
            Expression::Parenthesized(p) => {
                expr = p.expression;
            }
            _ => return None,
        }
    }
}

/// Collect variable entries from the data argument at `index` of a
/// `view()` / `View::make()` argument list.
///
/// Returns whether the argument was readable in full: an absent one
/// passes nothing (readable), while one built from a variable or a
/// non-literal key hides names the caller does pass.
fn collect_data_argument<'ast, 'arena>(
    argument_list: &'ast ArgumentList<'arena>,
    index: usize,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    match argument_list.arguments.iter().nth(index) {
        Some(arg) => collect_from_data_expr(arg.value(), entries),
        None => true,
    }
}

/// The position `Illuminate\Mail\Mailables\Content` takes its data at when
/// its constructor is called positionally, after `view`, `html`, `text`,
/// and `markdown`.
const CONTENT_WITH_INDEX: usize = 4;

/// Collect variable entries from the data a `new Content(…)` passes.
///
/// A `Content` names its data `with:` rather than putting it after the view
/// name, so the pairing is by argument name; the constructor still accepts
/// it positionally, at [`CONTENT_WITH_INDEX`].
///
/// A mailable also hands its view every public property it declares, which
/// no argument here names — see `Backend::component_render_scope_names`.
fn collect_content_data_argument<'ast, 'arena>(
    argument_list: &'ast ArgumentList<'arena>,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    for argument in argument_list.arguments.iter() {
        if let Argument::Named(named) = argument
            && bytes_to_str(named.name.value).eq_ignore_ascii_case("with")
        {
            return collect_from_data_expr(named.value, entries);
        }
    }
    match argument_list.arguments.iter().nth(CONTENT_WITH_INDEX) {
        Some(Argument::Positional(positional)) => collect_from_data_expr(positional.value, entries),
        _ => true,
    }
}

/// Collect the two variables an `@each` binds: the entry, under the name
/// the third argument spells, and `$key`.
///
/// `@each('partials.row', $rows, 'row')` renders `partials.row` once per
/// entry of `$rows` with `$row` and `$key` in scope and nothing else, so
/// both are derived from the collection rather than read off a data array.
///
/// Returns whether the pair was readable: an `@each` short of arguments,
/// or one whose item name is not a plain string literal, binds a name that
/// cannot be known.
fn collect_each_arguments<'ast, 'arena>(
    argument_list: &'ast ArgumentList<'arena>,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    let mut args = argument_list.arguments.iter().skip(1);
    let (Some(collection), Some(item)) = (args.next(), args.next()) else {
        return false;
    };
    let Expression::Literal(Literal::String(s)) = item.value() else {
        return false;
    };
    let Some(name) = string_literal_contents(s) else {
        return false;
    };
    // The item name is the only text at the call site that names either
    // variable, so it is where a diagnostic about the pair points.
    let key_range = (s.span.start.offset + 1, s.span.end.offset - 1);
    let collection = collection.value();
    entries.push(SiteEntry::Iteration {
        name,
        key_range,
        collection,
        part: IterationPart::Item,
    });
    entries.push(SiteEntry::Iteration {
        name: "key".to_string(),
        key_range,
        collection,
        part: IterationPart::Key,
    });
    true
}

/// Collect entries from a data expression: an array literal with
/// string keys, a `compact('a', 'b')` call (whose values are the
/// same-named variables at the call site), or anything else, whose
/// entries are read off its resolved type instead (see
/// [`data_shape_entries`]).
///
/// Returns whether every entry the expression writes down was readable.
/// An expression that writes none is readable here and answered for when
/// its type is resolved.
fn collect_from_data_expr<'ast, 'arena>(
    expr: &'ast Expression<'arena>,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) -> bool {
    let mut collect_array_elements =
        |elements: &'ast TokenSeparatedSequence<'arena, ArrayElement<'arena>>| {
            let mut complete = true;
            for element in elements.iter() {
                let ArrayElement::KeyValue(kv) = element else {
                    // A spread, or a positional entry Blade's `extract()`
                    // would drop: either way the key set is not the one
                    // written here.
                    complete = false;
                    continue;
                };
                let Expression::Literal(Literal::String(s)) = kv.key else {
                    complete = false;
                    continue;
                };
                match string_literal_contents(s) {
                    Some(name) => entries.push(SiteEntry::Expr {
                        name,
                        key_range: (s.span.start.offset + 1, s.span.end.offset - 1),
                        expr: kv.value,
                    }),
                    None => complete = false,
                }
            }
            complete
        };
    match expr {
        Expression::Array(array) => collect_array_elements(&array.elements),
        Expression::LegacyArray(array) => collect_array_elements(&array.elements),
        Expression::Call(Call::Function(fc)) if is_compact_call(fc) => {
            let mut complete = true;
            for arg in fc.argument_list.arguments.iter() {
                match arg.value() {
                    Expression::Literal(Literal::String(s)) => match string_literal_contents(s) {
                        Some(name) => entries.push(SiteEntry::Variable {
                            name,
                            key_range: (s.span.start.offset + 1, s.span.end.offset - 1),
                        }),
                        None => complete = false,
                    },
                    _ => complete = false,
                }
            }
            complete
        }
        _ => {
            entries.push(SiteEntry::Shape { expr });
            true
        }
    }
}

/// Whether a function call is `compact(…)`, whose arguments name the
/// variables it copies out of the calling scope.
fn is_compact_call(call: &FunctionCall<'_>) -> bool {
    let Expression::Identifier(ident) = call.function else {
        return false;
    };
    crate::util::strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("compact")
}

/// The contents of a single- or double-quoted string literal, when it
/// is a plain identifier-safe name.
pub(crate) fn string_literal_contents(s: &LiteralString<'_>) -> Option<String> {
    let value = s.value.and_then(literal_bytes_to_str)?;
    is_variable_name(value).then(|| value.to_string())
}

/// Whether a key can name a PHP variable, and so survive the `extract()`
/// Blade hands a template's data through.
fn is_variable_name(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !value.starts_with(|c: char| c.is_ascii_digit())
}
