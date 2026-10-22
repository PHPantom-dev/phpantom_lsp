//! Laravel validation-rule extraction.
//!
//! The keys of a validation rules array are a static contract: they name
//! exactly the input fields a validated request may carry.  This module
//! recovers that contract from source so that request-input completion
//! and go-to-definition can use it.
//!
//! Two sources are supported:
//!
//! - The `rules()` method of a `FormRequest` subclass, walking the parent
//!   chain and through `array_merge()` so `parent::rules()` composition
//!   still yields the locally declared keys.
//! - An inline `$request->validate([…])`, `$this->validate($request, […])`,
//!   `$request->validateWithBag('bag', […])`, or `Validator::make($data, […])`
//!   call earlier in the same function body.
//!
//! Only literal string keys are recovered.  A computed key contributes
//! nothing, which degrades to "fewer suggestions" rather than to wrong
//! ones.

use std::collections::HashMap;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH};
use crate::util::short_name;

use super::expr_walker::{beats_best, enclosing_body, walk_before_cursor};
use super::helpers::extract_string_literal;

/// The FQN of Laravel's base form-request class.
pub(crate) const FORM_REQUEST_FQN: &str = "Illuminate\\Foundation\\Http\\FormRequest";

/// The FQN of the object `Request::safe()` returns.
pub(crate) const VALIDATED_INPUT_FQN: &str = "Illuminate\\Support\\ValidatedInput";

/// One field entry of a validation rules array.
#[derive(Debug, Clone)]
pub(crate) struct ValidationRule {
    /// The field name exactly as written, e.g. `"name"` or `"items.*.id"`.
    pub key: String,
    /// The rule specification rendered for display, e.g.
    /// `"required|string|max:255"`.  Empty when the value is not literal.
    pub rules: String,
    /// Byte offset of the key literal's content (just inside the quotes),
    /// in the file named by [`Self::origin`], or by the owning
    /// [`RulesSource`] when that is `None`.
    pub key_start: usize,
    /// The file this entry was declared in, when it was merged in from a
    /// `parent::rules()` call rather than written in the array the owning
    /// [`ResolvedRules`] names.
    pub origin: Option<Arc<RulesFile>>,
    /// The enum class named by an enum rule (`new Enum(Role::class)`,
    /// `Rule::enum(Role::class)`), or `None` when the entry has no such rule.
    ///
    /// Stored exactly as written until [`resolve_enum_class_names`] resolves
    /// it against the imports of the file that declares the rules array.
    pub enum_class: Option<String>,
}

/// The entries of one validation rules array, plus whether its key set is
/// known to be complete.
#[derive(Debug, Clone)]
pub(crate) struct RulesArray {
    /// The field entries, in declaration order.
    pub entries: Vec<ValidationRule>,
    /// `false` when a key was not a string literal (`$field => 'required'`),
    /// which means entries are missing from [`Self::entries`].
    ///
    /// Callers that turn the rules into an array shape must bail on this: a
    /// shape that omits keys the request really accepts would report valid
    /// input as unknown.
    pub keys_complete: bool,
    /// Where a `parent::rules()` call appeared among [`Self::entries`], so the
    /// ancestor's keys can be merged in at the position PHP's `array_merge`
    /// would put them.  `None` when the array does not compose the parent's.
    parent_rules_at: Option<usize>,
}

impl RulesArray {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            keys_complete: true,
            parent_rules_at: None,
        }
    }

    /// Whether no entries were recovered at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Give up on a recorded `parent::rules()` composition.
    ///
    /// Only [`form_request_rules`] can read the ancestor's array; every other
    /// caller has no class chain to follow, so the keys the call contributes
    /// stay unknown and the set is no longer complete.
    fn without_parent_rules(&mut self) {
        if self.parent_rules_at.take().is_some() {
            self.keys_complete = false;
        }
    }

    /// Add `entry` under PHP's `array_merge` semantics for string keys: a
    /// repeated key keeps the position of its first occurrence and takes the
    /// value of its last.
    fn merge_entry(&mut self, entry: ValidationRule) {
        match self.entries.iter_mut().find(|e| e.key == entry.key) {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
    }
}

/// A file a rules array was read from, shared by every entry recovered from it.
#[derive(Debug)]
pub(crate) struct RulesFile {
    /// URI of the file.
    pub uri: String,
    /// Content of the file; [`ValidationRule::key_start`] indexes it.
    pub content: Arc<String>,
}

/// Which file a resolved rules array was parsed from.
#[derive(Debug, Clone)]
pub(crate) enum RulesSource {
    /// The rules live in the file the cursor is in — the caller already
    /// holds its content, so nothing is cloned.
    CurrentFile,
    /// The rules live in another file (a `FormRequest` class).
    OtherFile(Arc<RulesFile>),
}

/// A validation rules array located for a cursor position.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedRules {
    /// Where the rules were parsed from.
    pub source: RulesSource,
    /// The recovered entries.
    pub rules: RulesArray,
}

// ─── Array parsing ──────────────────────────────────────────────────────────

/// Collect the top-level entries of a validation rules array literal.
///
/// Unlike config files, rules arrays are flat: nesting is expressed in the
/// key itself (`items.*.id`), so only the outermost level is walked.
/// `array_merge(parent::rules(), [...])` is followed into each argument.
fn collect_rules_from_expr(expr: &Expression<'_>, content: &str, out: &mut RulesArray) {
    match expr {
        Expression::Array(arr) => collect_rules_from_elements(arr.elements.iter(), content, out),
        Expression::LegacyArray(arr) => {
            collect_rules_from_elements(arr.elements.iter(), content, out)
        }
        Expression::Parenthesized(p) => collect_rules_from_expr(p.expression, content, out),
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"array_merge")
            {
                for arg in fc.argument_list.arguments.iter() {
                    collect_rules_from_expr(arg.value(), content, out);
                }
            } else {
                // `rules()` returning some other call's result — the keys it
                // produces are invisible here.
                out.keys_complete = false;
            }
        }
        // `parent::rules()` composes the ancestor's array. The keys are
        // recoverable, but only by reading the ancestor, so record where they
        // belong and leave the merge to `form_request_rules`.
        Expression::Call(Call::StaticMethod(smc)) if is_parent_rules_call(smc) => {
            if out.parent_rules_at.is_none() {
                out.parent_rules_at = Some(out.entries.len());
            }
        }
        // A spread, a variable, a match — whatever keys it contributes are
        // not recoverable, so the set is no longer known to be complete.
        _ => out.keys_complete = false,
    }
}

/// Whether a static call is `parent::rules()` (no arguments).
fn is_parent_rules_call(call: &StaticMethodCall<'_>) -> bool {
    matches!(call.class, Expression::Parent(_))
        && matches!(&call.method, ClassLikeMemberSelector::Identifier(ident)
            if bytes_to_str(ident.value).eq_ignore_ascii_case("rules"))
        && call.argument_list.arguments.is_empty()
}

fn collect_rules_from_elements<'a>(
    elements: impl Iterator<Item = &'a ArrayElement<'a>>,
    content: &str,
    out: &mut RulesArray,
) {
    for element in elements {
        let ArrayElement::KeyValue(kv) = element else {
            // A spread (`...$base`) or a positional entry adds keys we
            // cannot name.
            out.keys_complete = false;
            continue;
        };
        let Some((key, key_start, _)) = extract_string_literal(kv.key, content) else {
            out.keys_complete = false;
            continue;
        };
        if key.is_empty() {
            out.keys_complete = false;
            continue;
        }
        out.merge_entry(ValidationRule {
            key: key.to_string(),
            rules: render_rule_value(kv.value, content),
            key_start,
            origin: None,
            enum_class: enum_rule_class(kv.value),
        });
    }
}

/// Render a rule value as the pipe-separated string Laravel documents,
/// regardless of whether it was written as a string or an array.
///
/// Non-literal entries (`new Enum(Role::class)`, `Rule::unique(...)`) are
/// rendered from their source text so hover/completion detail still says
/// something useful.
fn render_rule_value(expr: &Expression<'_>, content: &str) -> String {
    match expr {
        Expression::Literal(literal::Literal::String(_)) => extract_string_literal(expr, content)
            .map_or_else(String::new, |(v, _, _)| v.to_string()),
        Expression::Array(arr) => render_rule_list(arr.elements.iter(), content),
        Expression::LegacyArray(arr) => render_rule_list(arr.elements.iter(), content),
        other => condense(source_text(other, content)),
    }
}

fn render_rule_list<'a>(
    elements: impl Iterator<Item = &'a ArrayElement<'a>>,
    content: &str,
) -> String {
    let parts: Vec<String> = elements
        .filter_map(|element| match element {
            ArrayElement::Value(v) => Some(render_rule_value(v.value, content)),
            _ => None,
        })
        .filter(|p| !p.is_empty())
        .collect();
    parts.join("|")
}

// ─── Enum rules ─────────────────────────────────────────────────────────────

/// The enum class an enum rule names, exactly as written in the source.
///
/// Laravel writes an enum rule as an object — `new Enum(Role::class)` or its
/// `Rule::enum(Role::class)` shorthand — so the class is only reachable
/// through the expression, not through the rule string.  The rule may sit
/// alone or as one element of an array-form rule list, and the shorthand may
/// carry a fluent chain (`Rule::enum(Role::class)->only([…])`), whose head is
/// still the call that names the enum.
///
/// Returns `None` for every other rule value, and for a class expression
/// that names no class of its own (`self::class`, a variable).
fn enum_rule_class(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Array(arr) => arr.elements.iter().find_map(element_enum_class),
        Expression::LegacyArray(arr) => arr.elements.iter().find_map(element_enum_class),
        Expression::Parenthesized(p) => enum_rule_class(p.expression),
        Expression::Instantiation(inst) => {
            let Expression::Identifier(class) = inst.class else {
                return None;
            };
            if !short_name(bytes_to_str(class.value())).eq_ignore_ascii_case("Enum") {
                return None;
            }
            class_const_name(argument_at(inst.argument_list.as_ref()?, 0)?)
        }
        Expression::Call(Call::StaticMethod(call)) => {
            let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                return None;
            };
            if !bytes_to_str(method.value).eq_ignore_ascii_case("enum") {
                return None;
            }
            let Expression::Identifier(class) = call.class else {
                return None;
            };
            if !short_name(bytes_to_str(class.value())).eq_ignore_ascii_case("Rule") {
                return None;
            }
            class_const_name(argument_at(&call.argument_list, 0)?)
        }
        Expression::Call(Call::Method(mc)) => enum_rule_class(mc.object),
        Expression::Call(Call::NullSafeMethod(mc)) => enum_rule_class(mc.object),
        _ => None,
    }
}

fn element_enum_class(element: &ArrayElement<'_>) -> Option<String> {
    match element {
        ArrayElement::Value(v) => enum_rule_class(v.value),
        _ => None,
    }
}

/// The class name written in a `Something::class` expression.
///
/// `self::class` and friends are relative to a declaration site the rules
/// array does not carry, so they name nothing here.
fn class_const_name(expr: &Expression<'_>) -> Option<String> {
    crate::parser::class_fetch_identifier(expr).map(|class| bytes_to_str(class.value()).to_string())
}

/// Resolve the enum class names in `rules` against the imports of the file
/// `program` was parsed from.
///
/// `use App\Enums\Role; … new Enum(Role::class)` names `App\Enums\Role`, and
/// only the declaring file's `use` statements and namespace say so — the
/// class index cannot, since a bare short name matches nothing there.
///
/// Building the import table walks the file's top-level statements, so it is
/// paid only when an enum rule was actually found.
fn resolve_enum_names_in_program(rules: &mut RulesArray, program: &Program<'_>) {
    if !rules.entries.iter().any(|e| e.enum_class.is_some()) {
        return;
    }

    let mut use_map = HashMap::new();
    Backend::extract_use_statements_from_statements(program.statements.iter(), &mut use_map, None);
    let namespace = Backend::extract_namespace_from_statements(program.statements.iter());

    for entry in &mut rules.entries {
        if let Some(name) = &entry.enum_class {
            entry.enum_class = Some(crate::names::resolve_to_fqn(name, &use_map, &namespace));
        }
    }
}

/// Resolve the enum class names in `rules` against the imports of `content`.
///
/// Rules recovered from a file are resolved during the parse that recovered
/// them; this is for the one array that is parsed on its own — the argument
/// text of a `validate([…])` call, whose imports are the calling file's.
/// Nothing is parsed unless an enum rule was found, and the parse then goes
/// through the shared cache, which the calling file is normally already in.
pub(crate) fn resolve_enum_class_names(rules: &mut RulesArray, content: &str) {
    if !rules.entries.iter().any(|e| e.enum_class.is_some()) {
        return;
    }
    crate::parser::with_parsed_program(content, "resolve_enum_class_names", |program, _| {
        resolve_enum_names_in_program(rules, program);
    });
}

fn source_text<'c>(expr: &Expression<'_>, content: &'c str) -> &'c str {
    let span = expr.span();
    content
        .get(span.start.offset as usize..span.end.offset as usize)
        .unwrap_or_default()
}

/// Collapse runs of whitespace so a multi-line rule expression fits on the
/// single line that completion detail and hover display it on.
fn condense(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.truncate(out.trim_end().len());
    out
}

// ─── Inline `validate()` / `Validator::make()` ──────────────────────────────

/// Cheap pre-filter for the shapes [`inline_validate_rules`] recognises.
///
/// `validate()`, `validateWithBag()` and `Validator::make()` all contain
/// "validat" as written, so a file without it is not worth parsing.  PHP
/// method names are case-insensitive, so an unconventional `VALIDATE(` is
/// missed here; that costs suggestions rather than producing wrong ones.
fn mentions_validation(content: &str) -> bool {
    // Matching on the case-invariant tail covers `validate`, `Validator` and
    // `validateWithBag` in one pass.
    memchr::memmem::find(content.as_bytes(), b"alidat").is_some()
}

/// The rules in force at `offset` from the `validate()` /
/// `Validator::make()` calls that complete before it inside the same function
/// body.
///
/// A call the cursor cannot have skipped replaces whatever preceded it: two
/// `validate()` calls in a row leave only the second in force, the way
/// `validated()` returns only the last validation's data. A call inside a
/// branch or a loop may or may not have run, so it is merged with what was in
/// force instead — with one `validate()` per arm of an `if`/`else`, the keys of
/// both describe the request the cursor sees. Merging can only over-report the
/// key set, which is the safe direction: a key wrongly claimed costs a
/// suggestion, while a key wrongly omitted makes valid input look unknown.
///
/// Returns `None` when the cursor is not inside a function body, or when no
/// such call precedes it.
pub(crate) fn inline_validate_rules(content: &str, offset: usize) -> Option<RulesArray> {
    if !mentions_validation(content) {
        return None;
    }

    crate::parser::with_parsed_program(content, "inline_validate_rules", |program, content| {
        inline_validate_rules_in(program, content, offset as u32)
    })
}

fn inline_validate_rules_in(
    program: &Program<'_>,
    content: &str,
    cursor: u32,
) -> Option<RulesArray> {
    let body = enclosing_body(Node::Program(program), cursor)?;

    let mut in_force: Option<RulesArray> = None;
    walk_candidate_calls(body, cursor, false, &mut |node, conditional| {
        let rules_arg = match node {
            Node::MethodCall(mc) => method_rules_argument(&mc.method, &mc.argument_list),
            Node::NullSafeMethodCall(mc) => method_rules_argument(&mc.method, &mc.argument_list),
            Node::StaticMethodCall(smc) => static_rules_argument(smc),
            _ => None,
        };
        let Some(arg) = rules_arg else {
            return;
        };
        if node.span().end.offset > cursor {
            return;
        }
        let mut rules = RulesArray::new();
        collect_rules_from_expr(arg, content, &mut rules);
        rules.without_parent_rules();
        // An array whose keys could not all be read still counts, even with
        // nothing recovered from it: it says the request carries keys this pass
        // cannot name, which a caller building an array shape has to know.
        if rules.is_empty() && rules.keys_complete {
            return;
        }
        match (&mut in_force, conditional) {
            (Some(all), true) => {
                all.keys_complete &= rules.keys_complete;
                for entry in rules.entries {
                    all.merge_entry(entry);
                }
            }
            (slot, _) => *slot = Some(rules),
        }
    });
    let mut rules = in_force?;
    resolve_enum_names_in_program(&mut rules, program);
    Some(rules)
}

/// Hand every node that starts before `cursor` to `visit`, together with
/// whether it sits inside a construct that may not have executed.
///
/// Walk order is source order, so a `visit` that overwrites its state ends up
/// holding the last node.
fn walk_candidate_calls<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    cursor: u32,
    conditional: bool,
    visit: &mut impl FnMut(Node<'ast, 'arena>, bool),
) {
    visit(node, conditional);
    let conditional = conditional || introduces_control_flow_choice(node);
    node.visit_children(|child| {
        if child.span().start.offset < cursor {
            walk_candidate_calls(child, cursor, conditional, visit);
        }
    });
}

/// Whether reaching a node's children depends on a runtime choice, so a call
/// inside it may not have run by the time the cursor is reached.
///
/// A closure counts: its body runs whenever (and however often) something
/// calls it, which is not knowable here.
fn introduces_control_flow_choice(node: Node<'_, '_>) -> bool {
    matches!(
        node,
        Node::If(_)
            | Node::Switch(_)
            | Node::Match(_)
            | Node::Conditional(_)
            | Node::While(_)
            | Node::DoWhile(_)
            | Node::For(_)
            | Node::Foreach(_)
            | Node::Try(_)
            | Node::Closure(_)
            | Node::ArrowFunction(_)
    )
}

/// Parse a rules array written directly at a call site, e.g. the argument
/// text of `$request->validate([…])`.
///
/// The text is parsed standalone, so [`ValidationRule::key_start`] offsets
/// index `array_text` rather than any file; callers that need navigable
/// offsets must use [`inline_validate_rules`] instead.  For the same reason
/// there are no imports to read here, so an enum rule keeps its name as
/// written and the caller must pass the rules through
/// [`resolve_enum_class_names`] with the surrounding file's content.
pub(crate) fn rules_from_array_text(array_text: &str) -> Option<RulesArray> {
    let source = format!("<?php return {};", array_text.trim());
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, source.as_bytes());

    let mut out = RulesArray::new();
    collect_returned_rules(Node::Program(program), &source, &mut out);
    out.without_parent_rules();
    (!out.is_empty()).then_some(out)
}

/// The rules argument of `->validate([...])`, `->validate($request, [...])`,
/// or `->validateWithBag('bag', [...])`.
fn method_rules_argument<'ast, 'arena>(
    selector: &ClassLikeMemberSelector<'arena>,
    arguments: &'ast ArgumentList<'arena>,
) -> Option<&'ast Expression<'arena>> {
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return None;
    };
    let name = bytes_to_str(ident.value);
    if name.eq_ignore_ascii_case("validate") {
        // `$request->validate($rules)` and the `ValidatesRequests` trait's
        // `$this->validate($request, $rules)` differ only in arity.
        argument_at(arguments, 0)
            .filter(|e| is_array_literal(e))
            .or_else(|| argument_at(arguments, 1).filter(|e| is_array_literal(e)))
    } else if name.eq_ignore_ascii_case("validateWithBag") {
        argument_at(arguments, 1).filter(|e| is_array_literal(e))
    } else {
        None
    }
}

/// The rules argument of `Validator::make($data, [...])`.
fn static_rules_argument<'ast, 'arena>(
    call: &'ast StaticMethodCall<'arena>,
) -> Option<&'ast Expression<'arena>> {
    let ClassLikeMemberSelector::Identifier(ident) = &call.method else {
        return None;
    };
    if !bytes_to_str(ident.value).eq_ignore_ascii_case("make") {
        return None;
    }
    let Expression::Identifier(class) = call.class else {
        return None;
    };
    if !short_name(bytes_to_str(class.value())).eq_ignore_ascii_case("Validator") {
        return None;
    }
    argument_at(&call.argument_list, 1).filter(|e| is_array_literal(e))
}

fn argument_at<'ast, 'arena>(
    arguments: &'ast ArgumentList<'arena>,
    index: usize,
) -> Option<&'ast Expression<'arena>> {
    arguments.arguments.iter().nth(index).map(|a| a.value())
}

fn is_array_literal(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Array(_) | Expression::LegacyArray(_) => true,
        Expression::Parenthesized(p) => is_array_literal(p.expression),
        _ => false,
    }
}

// ─── `safe()` provenance ────────────────────────────────────────────────────

/// The request variable a `ValidatedInput` was narrowed from, e.g.
/// `"$request"` for `$safe = $request->safe();`.
///
/// `safe()` hands back the same rules array under a different type, so a
/// validated-input variable completes against the request that produced it.
/// Only the last assignment to `variable` before `offset` in the enclosing
/// body counts, so a reassignment wins.
///
/// Returns `None` when no such assignment precedes the cursor, or when the
/// object `safe()` was called on is not itself a plain variable.
pub(crate) fn safe_source_variable(content: &str, offset: usize, variable: &str) -> Option<String> {
    // Cheap pre-filter: without a `safe(` anywhere there is nothing to trace,
    // so the file is not worth parsing.
    memchr::memmem::find(content.as_bytes(), b"safe(")?;

    crate::parser::with_parsed_program(content, "safe_source_variable", |program, _| {
        safe_source_variable_in(program, offset as u32, variable)
    })
}

fn safe_source_variable_in(program: &Program<'_>, cursor: u32, variable: &str) -> Option<String> {
    let body = enclosing_body(Node::Program(program), cursor)?;

    let mut best: Option<(u32, String)> = None;
    walk_before_cursor(body, cursor, &mut |node| {
        let Node::Assignment(assignment) = node else {
            return;
        };
        let Expression::Variable(Variable::Direct(target)) = assignment.lhs else {
            return;
        };
        if bytes_to_str(target.name) != variable {
            return;
        }
        let end = node.span().end.offset;
        if !beats_best(&best, end, cursor) {
            return;
        }
        if let Some(source) = safe_call_receiver_variable(assignment.rhs) {
            best = Some((end, source));
        }
    });
    best.map(|(_, source)| source)
}

/// The receiver of a no-argument `->safe()` call, when it is a plain
/// variable.
pub(crate) fn safe_call_receiver_variable(expr: &Expression<'_>) -> Option<String> {
    let (object, method) = match expr {
        Expression::Call(Call::Method(mc)) => (mc.object, &mc.method),
        Expression::Call(Call::NullSafeMethod(mc)) => (mc.object, &mc.method),
        _ => return None,
    };
    let ClassLikeMemberSelector::Identifier(ident) = method else {
        return None;
    };
    if !bytes_to_str(ident.value).eq_ignore_ascii_case("safe") {
        return None;
    }
    let Expression::Variable(Variable::Direct(var)) = object else {
        return None;
    };
    Some(bytes_to_str(var.name).to_string())
}

// ─── `FormRequest::rules()` ─────────────────────────────────────────────────

fn is_form_request_fqn(name: &str) -> bool {
    name == FORM_REQUEST_FQN
}

fn is_request_fqn(name: &str) -> bool {
    name == super::REQUEST_FQN || name == VALIDATED_INPUT_FQN
}

/// Whether `class` is a `FormRequest` subclass (or `FormRequest` itself).
pub(crate) fn is_form_request(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    is_form_request_fqn(&class.fqn())
        || super::helpers::walks_parent_chain(class, class_loader, is_form_request_fqn)
}

/// Whether `class` holds request input — an `Illuminate\Http\Request`
/// (which `FormRequest` extends) or the `ValidatedInput` wrapper that
/// `Request::safe()` returns.
///
/// `walks_parent_chain` matches parent names, which post-processing has
/// already resolved to FQNs; the class's own `name` is the short name, so
/// it is checked separately against its `fqn()`.
pub(crate) fn is_request_like(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    is_request_fqn(&class.fqn())
        || super::helpers::walks_parent_chain(class, class_loader, is_request_fqn)
}

/// Whether `class` is the `ValidatedInput` wrapper `Request::safe()` returns,
/// which carries the request's rules but not its `rules()` method.
pub(crate) fn is_validated_input(class: &ClassInfo) -> bool {
    class.fqn() == VALIDATED_INPUT_FQN
}

/// The rules declared by `class`'s own `rules()` method, or the nearest
/// ancestor that declares one, with the keys of a composed `parent::rules()`
/// merged in from the ancestor that declares them.
///
/// Returns `None` when no ancestor declares a `rules()` method with a literal
/// array return, or when the declaring class's source cannot be located.
pub(crate) fn form_request_rules(
    backend: &Backend,
    class: &ClassInfo,
    current_uri: &str,
    current_content: &str,
) -> Option<ResolvedRules> {
    let fqn = class.fqn();
    let key = (fqn.to_string(), current_uri.to_string());
    if let Some(hit) = RULES_MEMO.with(|memo| memo.borrow().as_ref()?.get(&key).cloned()) {
        return hit;
    }
    let rules = form_request_rules_within(
        backend,
        &fqn,
        current_uri,
        current_content,
        MAX_INHERITANCE_DEPTH,
    );
    RULES_MEMO.with(|memo| {
        if let Some(memo) = memo.borrow_mut().as_mut() {
            memo.insert(key, rules.clone());
        }
    });
    rules
}

/// A `FormRequest`'s rules by (class FQN, requesting file URI); the URI is
/// part of the key because rules read from the requesting file itself
/// point into its content.
type RulesMemo = HashMap<(String, String), Option<ResolvedRules>>;

thread_local! {
    /// When `Some`, memoizes [`form_request_rules`] for the current pass:
    /// every `validated()` call and request-field lookup on the same
    /// request class would otherwise re-read the class, its parents and
    /// its traits.
    static RULES_MEMO: std::cell::RefCell<Option<RulesMemo>> =
        const { std::cell::RefCell::new(None) };
}

/// The guard [`with_rules_memo`] hands back.
pub(crate) type RulesMemoGuard = crate::type_engine::MemoGuard<RulesMemo>;

/// Activate the `FormRequest` rules memo for the current thread.
pub(crate) fn with_rules_memo() -> RulesMemoGuard {
    crate::type_engine::activate_memo(&RULES_MEMO)
}

/// [`form_request_rules`] with an explicit budget of parent-chain steps.
///
/// Walking up to the next ancestor and following a `parent::rules()` call both
/// spend a step, so the total work stays bounded even if the class graph is
/// malformed and its parent links form a cycle.
fn form_request_rules_within(
    backend: &Backend,
    class_fqn: &str,
    current_uri: &str,
    current_content: &str,
    budget: u32,
) -> Option<ResolvedRules> {
    let mut fqn = class_fqn.to_string();
    let mut budget = budget;

    loop {
        let uri = backend.find_class_file_uri(&fqn, current_uri)?;
        // The cursor's own file is already in hand; only another class's file
        // is read, and then as a shared `Arc` rather than a copy.
        let other = (uri != current_uri)
            .then(|| backend.get_file_content_arc(&uri))
            .flatten();
        let content = other.as_deref().map_or(current_content, String::as_str);
        let mut rules = rules_from_class_source(content, short_name(&fqn));
        if !rules.is_empty() || rules.parent_rules_at.is_some() {
            let source = match other {
                None => RulesSource::CurrentFile,
                Some(content) => RulesSource::OtherFile(Arc::new(RulesFile { uri, content })),
            };
            merge_parent_rules(
                backend,
                &fqn,
                current_uri,
                current_content,
                &mut rules,
                budget,
            );
            return (!rules.is_empty()).then_some(ResolvedRules { source, rules });
        }

        // The class body declares no `rules()`. A trait it uses may supply
        // one, and PHP treats that as the class's own method, so it is
        // consulted before the parent chain.
        if let Some(resolved) = trait_rules(backend, &fqn, current_uri, current_content, budget) {
            return Some(resolved);
        }

        budget = budget.checked_sub(1)?;
        let parent = backend
            .find_or_load_class(&fqn)
            .and_then(|c| c.parent_class)?;
        if parent.as_str() == FORM_REQUEST_FQN {
            return None;
        }
        fqn = parent.to_string();
    }
}

/// The rules declared by a `rules()` method that one of `class_fqn`'s traits
/// supplies, searched in `use` order and then into each trait's own traits.
///
/// Entries keep the declaring trait's file as their source, so go-to-definition
/// on an inherited key lands on the trait rather than on the using class. A
/// `parent::rules()` written inside a trait resolves against the *using*
/// class's parent, which is why `class_fqn` — not the trait — is what
/// [`merge_parent_rules`] is given.
fn trait_rules(
    backend: &Backend,
    class_fqn: &str,
    current_uri: &str,
    current_content: &str,
    budget: u32,
) -> Option<ResolvedRules> {
    let budget = budget.checked_sub(1)?;
    let class = backend.find_or_load_class(class_fqn)?;

    for trait_name in class.used_traits.iter() {
        let Some(uri) = backend.find_class_file_uri(trait_name, current_uri) else {
            continue;
        };
        let other = (uri != current_uri)
            .then(|| backend.get_file_content_arc(&uri))
            .flatten();
        let content = other.as_deref().map_or(current_content, String::as_str);
        let mut rules = rules_from_class_source(content, short_name(trait_name));

        if rules.is_empty() && rules.parent_rules_at.is_none() {
            // The trait itself may compose its `rules()` from another trait.
            if let Some(resolved) =
                trait_rules(backend, trait_name, current_uri, current_content, budget)
            {
                return Some(resolved);
            }
            continue;
        }

        let source = match other {
            None => RulesSource::CurrentFile,
            Some(content) => RulesSource::OtherFile(Arc::new(RulesFile { uri, content })),
        };
        merge_parent_rules(
            backend,
            class_fqn,
            current_uri,
            current_content,
            &mut rules,
            budget,
        );
        if !rules.is_empty() {
            return Some(ResolvedRules { source, rules });
        }
    }

    None
}

/// Merge the keys of `parent::rules()` into `rules`, at the position the call
/// occupied in the array.
///
/// The ancestor's entries keep pointing at the file that declares them, so
/// go-to-definition on an inherited key still lands on the parent's rule.
/// When the ancestor's array cannot be read, the key set is marked incomplete
/// rather than silently short.
fn merge_parent_rules(
    backend: &Backend,
    fqn: &str,
    current_uri: &str,
    current_content: &str,
    rules: &mut RulesArray,
    budget: u32,
) {
    let Some(at) = rules.parent_rules_at.take() else {
        return;
    };
    let inherited = budget
        .checked_sub(1)
        .and_then(|budget| {
            let parent = backend
                .find_or_load_class(fqn)
                .and_then(|c| c.parent_class)?;
            (parent.as_str() != FORM_REQUEST_FQN).then_some((parent, budget))
        })
        .and_then(|(parent, budget)| {
            form_request_rules_within(backend, &parent, current_uri, current_content, budget)
        });
    let Some(inherited) = inherited else {
        rules.keys_complete = false;
        return;
    };

    // Re-merge in `array_merge` order: the entries written before the
    // `parent::rules()` argument, then the inherited ones, then the rest.
    let local = std::mem::take(&mut rules.entries);
    let (before, after) = local.split_at(at.min(local.len()));
    let inherited_file = match &inherited.source {
        RulesSource::CurrentFile => None,
        RulesSource::OtherFile(file) => Some(Arc::clone(file)),
    };
    for entry in before
        .iter()
        .cloned()
        .chain(inherited.rules.entries.into_iter().map(|mut entry| {
            // `origin` is already set on a key the ancestor itself inherited.
            entry.origin = entry.origin.or_else(|| inherited_file.clone());
            entry
        }))
        .chain(after.iter().cloned())
    {
        rules.merge_entry(entry);
    }
    rules.keys_complete &= inherited.rules.keys_complete;
}

/// The rules that describe `class` at `offset`.
///
/// A `FormRequest` carries its own `rules()`; every other request-like
/// receiver — a plain `Request`, a `Validator`, a `ValidatedInput` — is
/// described by the nearest `validate()` / `Validator::make()` call preceding
/// the cursor in the same function body.
///
/// This is the single definition of that precedence; both request-input
/// completion and `validated()` shape inference go through it.
pub(crate) fn rules_in_scope(
    backend: &Backend,
    class: &ClassInfo,
    current_uri: &str,
    content: &str,
    offset: usize,
) -> Option<ResolvedRules> {
    let loader = |name: Fqn<'_>| backend.find_or_load_class(&name);
    if is_form_request(class, &loader)
        && let Some(resolved) = form_request_rules(backend, class, current_uri, content)
    {
        return Some(resolved);
    }
    inline_validate_rules(content, offset).map(|rules| ResolvedRules {
        source: RulesSource::CurrentFile,
        rules,
    })
}

/// Parse the array returned by `class_name`'s `rules()` method.
pub(crate) fn rules_from_class_source(content: &str, class_name: &str) -> RulesArray {
    crate::parser::with_parsed_program(content, "rules_from_class_source", |program, content| {
        let mut out = RulesArray::new();
        collect_rules_method(Node::Program(program), class_name, content, &mut out);
        // An enum rule names its class in this file's terms, and this file is
        // already parsed — resolving here saves the caller a second pass over it.
        resolve_enum_names_in_program(&mut out, program);
        Some(out)
    })
    // A parse that failed outright read no keys, not all of them.
    .unwrap_or_else(|| RulesArray {
        keys_complete: false,
        ..RulesArray::new()
    })
}

fn collect_rules_method(node: Node<'_, '_>, class_name: &str, content: &str, out: &mut RulesArray) {
    // A trait declares `rules()` the same way a class does, and a
    // `FormRequest` is free to get its method from one.
    let declaration = match node {
        Node::Class(class) => Some((bytes_to_str(class.name.value), &class.members)),
        Node::Trait(trait_) => Some((bytes_to_str(trait_.name.value), &trait_.members)),
        _ => None,
    };
    if let Some((name, members)) = declaration {
        if !name.eq_ignore_ascii_case(class_name) {
            return;
        }
        for member in members.iter() {
            if let ClassLikeMember::Method(method) = member
                && bytes_to_str(method.name.value).eq_ignore_ascii_case("rules")
                && let MethodBody::Concrete(block) = &method.body
            {
                collect_returned_rules(Node::Block(block), content, out);
            }
        }
        return;
    }
    node.visit_children(|child| collect_rules_method(child, class_name, content, out));
}

fn collect_returned_rules(node: Node<'_, '_>, content: &str, out: &mut RulesArray) {
    if let Node::Return(ret) = node {
        if let Some(value) = ret.value {
            collect_rules_from_expr(value, content, out);
        }
        return;
    }
    node.visit_children(|child| collect_returned_rules(child, content, out));
}

// ─── Candidate keys ─────────────────────────────────────────────────────────

/// A field name offered to the user, paired with the rule that produced it.
pub(crate) struct RuleField {
    /// The completable field name, e.g. `"name"`, `"items"`, `"address.city"`.
    pub name: String,
    /// The rule specification of the entry this name came from.
    pub rules: String,
    /// Byte offset of the declaring key literal's content.
    pub key_start: usize,
    /// The file [`Self::key_start`] indexes, when the declaring entry was
    /// merged in from a `parent::rules()` call. See [`ValidationRule::origin`].
    pub origin: Option<Arc<RulesFile>>,
}

/// Expand rule keys into the field names an input accessor accepts.
///
/// A plain dotted key (`address.city`) is offered whole, since Laravel's dot
/// notation reaches it directly.  A wildcard key (`items.*.id`) names array
/// members that no accessor addresses literally, so only its root segment is
/// offered.  Roots come after the declared keys so the exact rules — and
/// their rule text — rank first.
pub(crate) fn rule_fields(rules: &[ValidationRule]) -> Vec<RuleField> {
    let mut out: Vec<RuleField> = Vec::with_capacity(rules.len());

    for rule in rules {
        if rule.key.split('.').any(|seg| seg == "*") {
            continue;
        }
        if !out.iter().any(|f| f.name == rule.key) {
            out.push(RuleField {
                name: rule.key.clone(),
                rules: rule.rules.clone(),
                key_start: rule.key_start,
                origin: rule.origin.clone(),
            });
        }
    }

    for rule in rules {
        let Some(root) = rule.key.split('.').next() else {
            continue;
        };
        if root.is_empty() || root == "*" || root == rule.key {
            continue;
        }
        if !out.iter().any(|f| f.name == root) {
            out.push(RuleField {
                name: root.to_string(),
                rules: String::new(),
                key_start: rule.key_start,
                origin: rule.origin.clone(),
            });
        }
    }

    out
}

#[cfg(test)]
#[path = "validation_rules_tests.rs"]
mod tests;
