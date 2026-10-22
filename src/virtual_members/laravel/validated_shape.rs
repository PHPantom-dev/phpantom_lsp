//! Translate a Laravel validation rules array into an array shape.
//!
//! `$request->validated()` is declared `array` and nothing more, which is as
//! far as every mainstream tool takes it.  But the rules array is a static
//! type contract — it names every key the validated result can hold and says
//! what each one is — so it translates directly into an `array{…}` shape:
//!
//! ```text
//! 'name'        => 'required|string|max:255'   →  name: string
//! 'age'         => 'nullable|integer'          →  age?: ?int
//! 'active'      => 'boolean'                   →  active?: bool
//! 'items'       => 'array'
//! 'items.*.id'  => 'integer'                   →  items?: list<array{id: int}>
//! ```
//!
//! Two deliberate imprecisions, both matching how the Laravel ecosystem
//! reasons about validated data rather than what PHP holds at runtime:
//!
//! - `validated()` returns the *raw* input, so `'age' => 'integer'` is really
//!   the string `"42"` when it came from a form body.  The shape says `int`,
//!   because that is the value's meaning and every consumer treats it that
//!   way.
//! - A rule object (`new Enum(Role::class)`, `Rule::unique(…)`) still yields
//!   the raw scalar Laravel validated, not the object.  For an enum rule that
//!   scalar is the enum's backing type, so a `string`-backed enum types its
//!   field `string` and an `int`-backed one `int`.
//!
//! Where a key's *type* cannot be read the entry degrades to `mixed`, which
//! accepts anything and so cannot cause a false diagnostic.  Where the key
//! *set* itself is incomplete the whole shape is abandoned for plain `array`,
//! because a shape that omits real keys would report valid code as wrong.

use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, ShapeEntry};
use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, BackedEnumType, ClassInfo, ClassLikeKind};

use super::validation_rules::{
    RulesArray, ValidationRule, is_request_like, is_validated_input, resolve_enum_class_names,
    rules_from_array_text, rules_in_scope,
};

/// Resolves a class name to its `ClassInfo`, the loader every shape lookup
/// that has to read an enum's backing type is handed.
type ClassLoader<'a> = &'a ResolvedClassLoader<'a>;

/// The type of an uploaded file in a validated array.
const UPLOADED_FILE_FQN: &str = "\\Illuminate\\Http\\UploadedFile";

/// The contract every validator implements, including `Validator::make()`'s.
const VALIDATOR_CONTRACT_FQN: &str = "Illuminate\\Contracts\\Validation\\Validator";

/// The concrete validator `Validator::make()` returns.
const VALIDATOR_FQN: &str = "Illuminate\\Validation\\Validator";

/// Translate a rules array into an `array{…}` shape.
///
/// Returns `None` when the shape would be untrustworthy — an incomplete key
/// set, or no keys at all — and the caller should fall back to plain `array`.
fn rules_to_shape(rules: &RulesArray, class_loader: ClassLoader<'_>) -> Option<PhpType> {
    if !rules.keys_complete || rules.is_empty() {
        return None;
    }

    let mut root = Node::default();
    for rule in &rules.entries {
        root.insert(&rule.key, RuleSpec::parse(rule, class_loader));
    }
    root.shape()
}

/// The type of one key of the shape `rules` describes.
///
/// Dot notation reaches nested keys, so `validated('owner.email')` resolves
/// through the shape the same way the runtime lookup walks the array.
/// Returns `None` when the key is not in the shape.
fn rules_member_type(
    rules: &RulesArray,
    key: &str,
    class_loader: ClassLoader<'_>,
) -> Option<PhpType> {
    let shape = rules_to_shape(rules, class_loader)?;
    key.split('.')
        .try_fold(shape, |ty, segment| ty.shape_value_type(segment).cloned())
}

/// The type the rules in scope give one key of a request.
///
/// Shared with the input accessors, which have their own reasons to ask
/// what a field holds: `file('photos')` is a list of uploads or a single
/// one depending on how the field is validated.
pub(crate) fn rules_key_type(
    backend: &Backend,
    receiver: &ClassInfo,
    key: &str,
    content: &str,
    offset: u32,
    class_loader: ClassLoader<'_>,
) -> Option<PhpType> {
    let rules = lookup_rules(backend, receiver, content, offset)?;
    rules_member_type(&rules, key, class_loader)
}

/// Narrow a shape to the listed keys (`safe()->only([…])`) or to everything
/// but them (`safe()->except([…])`).
///
/// Returns `None` when no shape is available.  An empty `keys` list narrows to
/// nothing and yields an empty shape, which matches the runtime because the
/// only way to reach it is a literal `only([])`; [`key_list`] rejects a list it
/// could not read rather than passing an empty one on.
fn narrow_shape(shape: &PhpType, keys: &[String], keep: bool) -> Option<PhpType> {
    let entries = shape.shape_entries()?;
    let narrowed: Vec<ShapeEntry> = entries
        .iter()
        .filter(|entry| {
            let listed = entry
                .key
                .as_deref()
                .is_some_and(|k| keys.iter().any(|wanted| wanted == k));
            listed == keep
        })
        .cloned()
        .collect();
    Some(PhpType::array_shape(narrowed))
}

// ─── Call sites ─────────────────────────────────────────────────────────────

/// The rules that describe a request-like receiver.
///
/// A `FormRequest` carries its own `rules()`; anything else (a plain
/// `Request`, a `Validator`, a `ValidatedInput`) is described by the nearest
/// `validate()` / `Validator::make()` call preceding `offset`.
///
/// `receiver` is re-read from the index by FQN rather than used directly: the
/// class the type engine holds may be a generic substitution or a bare
/// local-file parse, and the parent walk that recognises a `FormRequest`
/// needs the indexed entry.
fn lookup_rules(
    backend: &Backend,
    receiver: &ClassInfo,
    content: &str,
    offset: u32,
) -> Option<RulesArray> {
    let class = backend.find_or_load_class(&receiver.fqn())?;
    // The type engine has no document URI to offer, so the class lookup falls
    // back to the FQN index.  Only the entries are wanted here — which file
    // they came from matters to go-to-definition, not to a type, and the
    // names an enum rule carries were already resolved against that file.
    rules_in_scope(backend, &class, "", content, offset as usize).map(|resolved| resolved.rules)
}

/// A call that can carry a validated-array shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeCall {
    /// `$request->validate([…])` — typed from its own argument.
    Validate,
    /// `$request->validated()` / `validated('key')`.
    Validated,
    /// `safe()->only([…])`.
    Only,
    /// `safe()->except([…])`.
    Except,
}

/// Classify a method name, or `None` when it can never bear a shape.
///
/// Callers gate on this before doing any other work.  It runs on every method
/// call the type engine resolves, so it compares in place rather than
/// lowercasing into a fresh `String`.
pub(crate) fn shape_bearing_method(method: &str) -> Option<ShapeCall> {
    const CALLS: [(&str, ShapeCall); 4] = [
        ("validate", ShapeCall::Validate),
        ("validated", ShapeCall::Validated),
        ("only", ShapeCall::Only),
        ("except", ShapeCall::Except),
    ];
    CALLS
        .iter()
        .find(|(name, _)| method.eq_ignore_ascii_case(name))
        .map(|(_, call)| *call)
}

/// The array shape a validation-aware call returns, or `None` to leave the
/// declared return type alone.
///
/// Recognises, on a request-like receiver:
///
/// - `validated()` → the whole shape; `validated('key')` → that key's type
/// - `validate([…])` → the shape of the rules passed at the call site
/// - `safe()->only([…])` / `safe()->except([…])` → the narrowed shape
///
/// `safe_source` recovers the request a `ValidatedInput` receiver was narrowed
/// from, in whichever expression form the caller holds.  It is a closure
/// because tracing that hop parses the file, and `only`/`except` are common
/// names on `Collection` and `Arr` too: only the arm that reaches a real
/// `ValidatedInput` may pay for it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_shape_at_call(
    receiver: &ClassInfo,
    call: ShapeCall,
    args: &[&str],
    safe_source: &dyn Fn() -> Option<Arc<ClassInfo>>,
    content: &str,
    offset: u32,
    class_loader: &ResolvedClassLoader<'_>,
    backend: Option<&Backend>,
) -> Option<PhpType> {
    match call {
        // `$request->validate([...])` returns exactly what its own argument
        // describes, so the rules come from the call site rather than scope —
        // which is why this arm needs no server state.
        ShapeCall::Validate if is_request_like(receiver, class_loader) => {
            let mut rules = rules_from_array_text(args.first()?)?;
            // The rules array is written at the call site, so an enum rule in
            // it names its class in this file's terms.
            resolve_enum_class_names(&mut rules, content);
            rules_to_shape(&rules, class_loader)
        }
        ShapeCall::Validated
            if is_request_like(receiver, class_loader) || is_validator(receiver, class_loader) =>
        {
            let rules = lookup_rules(backend?, receiver, content, offset)?;
            match args.first() {
                None => rules_to_shape(&rules, class_loader),
                // `validated($key)` returns one field's value, never the whole
                // array, so a key that cannot be read leaves the declared
                // `array` in place rather than typing the call as the shape.
                Some(arg) => rules_member_type(&rules, &unquote(arg)?, class_loader),
            }
        }
        // Narrowing applies to a `ValidatedInput` only.  `$request->only()`
        // reads raw input, which the rules do not describe.
        ShapeCall::Only | ShapeCall::Except if is_validated_input(receiver) => {
            let source = safe_source()?;
            let rules = lookup_rules(backend?, &source, content, offset)?;
            let shape = rules_to_shape(&rules, class_loader)?;
            let keys = key_list(args)?;
            narrow_shape(&shape, &keys, call == ShapeCall::Only)
        }
        _ => None,
    }
}

/// Whether `class` is a validator.
///
/// Matches the concrete class by name and anything implementing the contract,
/// so a custom validator installed through `Validator::resolver()` is
/// recognised too.  Both halves are needed for the same reason
/// [`is_request_like`] needs both: the name check does not require the
/// contract's own file to be indexed, and the subtype walk covers subclasses.
fn is_validator(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    let fqn = class.fqn();
    fqn == VALIDATOR_FQN
        || fqn == VALIDATOR_CONTRACT_FQN
        || crate::class_lookup::is_subtype_of(class, VALIDATOR_CONTRACT_FQN, class_loader)
}

/// The request behind a `ValidatedInput`, given the expression `safe()` was
/// called on.
///
/// For `$request->safe()->only([…])` the receiver of `only` is the `safe()`
/// call, whose own receiver is the request whose rules apply.
pub(crate) fn safe_source_class(
    base: &SubjectExpr,
    ctx: &ResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    match base {
        // `$request->safe()->only(…)` — the request is one hop further left.
        SubjectExpr::CallExpr { callee, .. } => {
            let SubjectExpr::MethodCall {
                base: request_expr,
                method,
            } = callee.as_ref()
            else {
                return None;
            };
            if !method.eq_ignore_ascii_case("safe") {
                return None;
            }
            crate::type_engine::resolver::resolve_target_classes_expr(
                request_expr,
                AccessKind::Arrow,
                ctx,
            )
            .into_iter()
            .find_map(|rt| rt.class_info)
        }
        // `$safe = $request->safe(); $safe->only(…)` — trace the assignment
        // back to the request variable, then resolve that.
        SubjectExpr::Variable(name) => {
            let request = super::validation_rules::safe_source_variable(
                ctx.file.content,
                ctx.cursor_offset as usize,
                name,
            )?;
            crate::type_engine::resolver::resolve_target_classes(&request, AccessKind::Arrow, ctx)
                .into_iter()
                .find_map(|rt| rt.class_info)
        }
        _ => None,
    }
}

// ─── Argument text ──────────────────────────────────────────────────────────

/// Read an argument as a plain string literal.
fn unquote(arg: &str) -> Option<String> {
    crate::text_scan::unquote_php_string(arg.trim()).map(str::to_string)
}

/// Every field name an `only()` / `except()` call lists, written either as one
/// array literal (`only(['name', 'email'])`) or variadically
/// (`only('name', 'email')`).
///
/// `None` as soon as one listed key is not a plain string literal.  A key set
/// that silently drops `$keys` is not the set the runtime narrows to, and
/// `only()` against a short list yields a shape that omits real keys.
fn key_list(args: &[&str]) -> Option<Vec<String>> {
    let single_array = args
        .first()
        .map(|arg| arg.trim())
        .and_then(|arg| arg.strip_prefix('['))
        .and_then(|arg| arg.strip_suffix(']'));

    match single_array {
        Some(inner) => crate::text_scan::split_top_level_commas(inner)
            .into_iter()
            .map(unquote)
            .collect(),
        None => args.iter().map(|arg| unquote(arg)).collect(),
    }
}

// ─── Rule specs ─────────────────────────────────────────────────────────────

/// Rules that drop the field from the validated array depending on the input,
/// so the key may simply not be there.
const CONDITIONAL_EXCLUDE_RULES: [&str; 4] = [
    "exclude_if",
    "exclude_unless",
    "exclude_with",
    "exclude_without",
];

/// What one rules-array entry says about its field.
#[derive(Debug, Clone, Default)]
struct RuleSpec {
    required: bool,
    nullable: bool,
    sometimes: bool,
    /// `exclude`, which keeps the field out of the validated array entirely.
    excluded: bool,
    /// The declared element type, or `None` when no rule names one.
    base: Option<PhpType>,
}

impl RuleSpec {
    fn parse(rule: &ValidationRule, class_loader: ClassLoader<'_>) -> Self {
        let mut spec = RuleSpec::default();
        for token in rule.rules.split('|') {
            // `max:255` and `date_format:Y-m-d` carry a parameter that says
            // nothing about the type.  Rule names are matched case-insensitively
            // in place, since lowercasing every token of every key on every
            // resolution is pure allocation.
            let name = token.split(':').next().unwrap_or(token).trim();
            let is = |candidate: &str| name.eq_ignore_ascii_case(candidate);
            // `required_if`, `required_with` and friends are deliberately not
            // matched here: they only sometimes demand the key, so the shape
            // has to allow it to be missing.
            if is("required") || is("present") {
                spec.required = true;
            } else if is("nullable") {
                spec.nullable = true;
            } else if is("sometimes") {
                spec.sometimes = true;
            } else if is("exclude") {
                spec.excluded = true;
            } else if CONDITIONAL_EXCLUDE_RULES.iter().any(|rule| is(rule)) {
                spec.sometimes = true;
            } else if spec.base.is_none()
                && let Some(ty) = rule_token_type(name)
            {
                spec.base = Some(ty);
            }
        }
        // An enum rule is an object, so it names no type token; the scalar it
        // validates is the enum's own backing type.
        if spec.base.is_none()
            && let Some(fqn) = rule.enum_class.as_deref()
        {
            spec.base = enum_backing_type(fqn, class_loader);
        }
        spec
    }

    /// Whether the key may be absent from the validated array.
    ///
    /// Only `required` (or `present`) guarantees the key is there, and
    /// `sometimes` withdraws even that.  `nullable` does *not* make a field
    /// present: validating `['age' => 'nullable|integer']` against `[]`
    /// yields `[]`, not `['age' => null]`, so a nullable field is an optional
    /// key whose value may additionally be null.
    fn optional(&self) -> bool {
        self.sometimes || !self.required
    }

    fn scalar_type(&self) -> PhpType {
        // No rule named a type — a rule object, or a purely constraining rule
        // like `confirmed`.  It still validated *something*; we just cannot
        // say what, and `mixed` never lies.
        let base = self.base.clone().unwrap_or_else(PhpType::mixed);
        if self.nullable {
            PhpType::nullable(base)
        } else {
            base
        }
    }
}

/// The scalar an enum rule validates: the enum's backing type.
///
/// The validated array holds the raw input rather than the enum case, so a
/// `string`-backed enum validates a `string` and an `int`-backed one an
/// `int`.  A pure enum has no scalar form, and a class name that resolves to
/// nothing — or to something that is not an enum — says nothing about the
/// value: all of these return `None` and leave the field `mixed`, rather than
/// guessing `string` and mistyping every `int`-backed enum.
fn enum_backing_type(fqn: &str, class_loader: ClassLoader<'_>) -> Option<PhpType> {
    let class = class_loader(Fqn::new(fqn))?;
    if class.kind != ClassLikeKind::Enum {
        return None;
    }
    match class.backed_type? {
        BackedEnumType::String => Some(PhpType::string()),
        BackedEnumType::Int => Some(PhpType::int()),
    }
}

/// The value type a single validation rule implies, or `None` when the rule
/// constrains the value without naming its type (`max`, `unique`, `confirmed`).
fn rule_token_type(name: &str) -> Option<PhpType> {
    // Rule names are conventionally written lowercase, so only an
    // unconventional spelling pays for a lowercased copy.
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        return rule_token_type(&name.to_ascii_lowercase());
    }
    let ty = match name {
        // Rules that only accept strings.  `date` included: the validated
        // array holds the raw input, so a date is still its string.
        "string" | "email" | "url" | "active_url" | "uuid" | "ulid" | "ip" | "ipv4" | "ipv6"
        | "json" | "alpha" | "alpha_dash" | "alpha_num" | "ascii" | "date" | "date_format"
        | "timezone" | "mac_address" | "hex_color" | "current_password" => PhpType::string(),
        "integer" | "int" => PhpType::int(),
        "boolean" | "bool" | "accepted" | "declined" => PhpType::bool(),
        "numeric" | "decimal" => PhpType::union(vec![PhpType::int(), PhpType::float()]),
        "file" | "image" | "mimes" | "mimetypes" => {
            PhpType::named(crate::atom::atom(UPLOADED_FILE_FQN))
        }
        "array" | "list" => PhpType::array(),
        _ => return None,
    };
    Some(ty)
}

// ─── Key tree ───────────────────────────────────────────────────────────────

/// One node of the tree that dotted rule keys describe.
///
/// `'items' => 'array'` with `'items.*.id' => 'integer'` builds a node for
/// `items` that carries both its own spec and a wildcard child holding `id`.
#[derive(Debug, Default)]
struct Node {
    /// The spec declared for this exact path, if the rules array names it.
    spec: Option<RuleSpec>,
    /// Children under literal segments, in declaration order.
    children: Vec<(String, Node)>,
    /// The child under a `*` segment, which makes this node a list.
    wildcard: Option<Box<Node>>,
}

impl Node {
    fn insert(&mut self, key: &str, spec: RuleSpec) {
        match key.split_once('.') {
            None => self.child(key).spec = Some(spec),
            Some((head, rest)) => self.child(head).insert(rest, spec),
        }
    }

    fn child(&mut self, segment: &str) -> &mut Node {
        if segment == "*" {
            return self.wildcard.get_or_insert_with(Box::default);
        }
        if let Some(index) = self.children.iter().position(|(name, _)| name == segment) {
            return &mut self.children[index].1;
        }
        self.children.push((segment.to_string(), Node::default()));
        &mut self.children.last_mut().expect("just pushed a child").1
    }

    /// The shape of this node's children, or `None` when it has none that
    /// reach the validated array.
    fn shape(&self) -> Option<PhpType> {
        let entries: Vec<ShapeEntry> = self
            .children
            .iter()
            .filter(|(_, child)| !child.is_excluded())
            .map(|(name, child)| ShapeEntry {
                key: Some(name.clone()),
                value_type: child.value_type(),
                optional: child.optional(),
            })
            .collect();
        (!entries.is_empty()).then(|| PhpType::array_shape(entries))
    }

    /// Whether `exclude` keeps this key out of the validated array entirely.
    ///
    /// Laravel validates the field and then drops it, so the key is never
    /// there — unlike the conditional `exclude_if` family, which only makes it
    /// optional.
    fn is_excluded(&self) -> bool {
        self.spec.as_ref().is_some_and(|spec| spec.excluded)
    }

    /// The type of the value this node holds.
    fn value_type(&self) -> PhpType {
        // A wildcard child means the rules describe the array's *elements*,
        // which is exactly a `list<…>`.
        let structured = match &self.wildcard {
            Some(wildcard) => Some(PhpType::list(wildcard.value_type())),
            None => self.shape(),
        };
        match structured {
            // The dotted child rules say what a *present* value holds; the
            // node's own rules still decide whether it may be null, so
            // `'items' => 'nullable|array'` keeps its null.
            Some(ty) if self.spec.as_ref().is_some_and(|spec| spec.nullable) => {
                PhpType::nullable(ty)
            }
            Some(ty) => ty,
            None => match &self.spec {
                Some(spec) => spec.scalar_type(),
                None => PhpType::mixed(),
            },
        }
    }

    /// Whether the key this node sits under may be absent.
    ///
    /// A node with no spec of its own — `owner` when only `owner.email` is
    /// declared — is required exactly when something beneath it is, since a
    /// required leaf cannot arrive without its parent.
    fn optional(&self) -> bool {
        match &self.spec {
            Some(spec) => spec.optional(),
            None => !self.has_required_descendant(),
        }
    }

    fn has_required_descendant(&self) -> bool {
        if self.is_excluded() {
            return false;
        }
        if self.spec.as_ref().is_some_and(|spec| !spec.optional()) {
            return true;
        }
        self.children
            .iter()
            .any(|(_, child)| child.has_required_descendant())
            || self
                .wildcard
                .as_ref()
                .is_some_and(|child| child.has_required_descendant())
    }
}

#[cfg(test)]
#[path = "validated_shape_tests.rs"]
mod tests;
