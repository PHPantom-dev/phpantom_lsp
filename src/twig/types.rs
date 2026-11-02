//! The types of a Twig template's variables, and of the attributes read off
//! them.
//!
//! A variable a template receives holds what the places that pass it hand
//! it (see [`super::variables`]): a PHP render's value is resolved with the
//! type engine where the render is written, as Blade's call-site inference
//! does, and an including template's value is typed in that template.  A
//! `types` tag's declaration wins over all of them.  A variable the template
//! defines itself holds what its `set`, `for`, or `with` gives it.  The type
//! of every caller that was found is joined, so the answer is "what the
//! callers that were found pass".
//!
//! An attribute (`post.title`) is looked up the way Twig's `getAttribute()`
//! does at runtime: an array's key, else an object's public property, else
//! its method of that name, or the `get`, `is`, or `has` getter of it.  Only
//! a chain of variables, attributes, and method calls is typed; an
//! expression with a filter or an operator in it is not.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemTextEdit, CompletionResponse, Contents,
    Hover, Location, Position, Range, TextEdit,
};

use super::callables::{is_identifier_byte, is_identifier_start};
use super::variables::{Definition, DefinitionKind, KEYWORDS, OriginValue, Resolved, trim_range};
use crate::Backend;
use crate::blade::call_site_inference::{
    data_shape_entries, join_call_site_types, qualify_class_names,
};
use crate::php_type::PhpType;
use crate::symbol_map::TwigContextValue;
use crate::symfony::translations::string_end;
use crate::types::{ClassInfo, MethodInfo, PropertyInfo, Visibility};

/// The class Symfony hands every template as `app`.
const APP_VARIABLE: &str = "\\Symfony\\Bridge\\Twig\\AppVariable";

/// What the `loop` variable of a `for` holds.
const LOOP_TYPE: &str = "array{index: int, index0: int, revindex: int, revindex0: int, first: bool, last: bool, length: int, parent: array}";

/// An attribute read off a variable, `.name` or `.name(…)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Segment<'c> {
    pub(crate) name: &'c str,
    /// Whether it is called, which makes Twig look for a method only.
    pub(crate) call: bool,
}

/// A variable and the attributes read off it: `post.author.name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chain<'c> {
    pub(crate) root: &'c str,
    pub(crate) root_start: usize,
    pub(crate) segments: Vec<Segment<'c>>,
}

/// The `(` that opens the `)` at `close`, past the brackets inside it.
fn open_before(bytes: &[u8], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = close + 1;
    while cursor > 0 {
        cursor -= 1;
        match bytes[cursor] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
    }
    None
}

/// The chain that ends right before `end`, read backwards.
pub(crate) fn chain_ending_at(content: &str, end: usize) -> Option<Chain<'_>> {
    let bytes = content.as_bytes();
    let mut cursor = end;
    let mut segments = Vec::new();
    loop {
        let call = cursor > 0 && bytes[cursor - 1] == b')';
        if call {
            cursor = open_before(bytes, cursor - 1)?;
        }
        let word_end = cursor;
        let word_start = bytes[..word_end]
            .iter()
            .rposition(|byte| !is_identifier_byte(*byte))
            .map_or(0, |index| index + 1);
        if word_start == word_end || !is_identifier_start(bytes[word_start]) {
            return None;
        }
        let name = &content[word_start..word_end];
        let is_attribute = word_start >= 1
            && bytes[word_start - 1] == b'.'
            && !(word_start >= 2 && bytes[word_start - 2] == b'.');
        if is_attribute {
            segments.push(Segment { name, call });
            cursor = word_start - 1;
            continue;
        }
        // A function's result, a filter, or a word of the language is not a
        // variable.
        let before = content[..word_start].trim_end();
        if call || KEYWORDS.contains(&name) || before.ends_with('|') {
            return None;
        }
        segments.reverse();
        return Some(Chain {
            root: name,
            root_start: word_start,
            segments,
        });
    }
}

/// The chain `start..end` is, when it is one.
fn chain_spanning(content: &str, start: usize, end: usize) -> Option<Chain<'_>> {
    chain_ending_at(content, end).filter(|chain| chain.root_start == start)
}

/// Whether `offset` is inside a `{{ … }}` or `{% … %}` and outside the
/// strings in it.
fn in_expression(content: &str, offset: usize) -> bool {
    let Some(before) = content.get(..offset) else {
        return false;
    };
    let Some(open) = before.rfind("{{").max(before.rfind("{%")) else {
        return false;
    };
    if before[open..].contains("}}") || before[open..].contains("%}") {
        return false;
    }
    let bytes = content.as_bytes();
    let mut scan = open + 2;
    while scan < offset {
        match bytes[scan] {
            quote @ (b'\'' | b'"') => match string_end(bytes, scan + 1, quote) {
                Some(close) if close < offset => scan = close + 1,
                _ => return false,
            },
            _ => scan += 1,
        }
    }
    true
}

/// The attribute the word around `offset` names, with the chain it is read
/// off and its byte range.
fn attribute_at(content: &str, offset: usize) -> Option<(Chain<'_>, Segment<'_>, usize, usize)> {
    let bytes = content.as_bytes();
    let start = bytes[..offset.min(bytes.len())]
        .iter()
        .rposition(|byte| !is_identifier_byte(*byte))
        .map_or(0, |index| index + 1);
    let end = bytes[start..]
        .iter()
        .position(|byte| !is_identifier_byte(*byte))
        .map_or(bytes.len(), |index| start + index);
    if start == end
        || start < 2
        || bytes[start - 1] != b'.'
        || bytes[start - 2] == b'.'
        || !in_expression(content, start)
    {
        return None;
    }
    let chain = chain_ending_at(content, start - 1)?;
    let segment = Segment {
        name: &content[start..end],
        call: bytes.get(end) == Some(&b'('),
    };
    Some((chain, segment, start, end))
}

/// The type a literal written in a template has.
fn literal_type(text: &str) -> Option<PhpType> {
    let first = *text.as_bytes().first()?;
    Some(match first {
        b'\'' | b'"' => PhpType::string(),
        b'[' | b'{' => PhpType::array(),
        b'0'..=b'9'
            if text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'_') =>
        {
            PhpType::int()
        }
        b'0'..=b'9' if text.parse::<f64>().is_ok() => PhpType::float(),
        _ => match text {
            "true" | "false" | "TRUE" | "FALSE" => PhpType::bool(),
            "null" | "none" | "NULL" | "NONE" => PhpType::null(),
            _ => return None,
        },
    })
}

/// A member Twig reads an attribute from.
pub(crate) enum TwigMember {
    Property(Arc<PropertyInfo>),
    Method(Arc<MethodInfo>),
}

/// The Twig names a public method can be read by: its own, and for a
/// `get`, `is`, or `has` getter the name after the prefix, with its first
/// letter lowered (`getFooBar()` is `fooBar`).
fn getter_name(method: &str) -> Option<String> {
    let lower = method.to_ascii_lowercase();
    let prefix = ["get", "is", "has"]
        .into_iter()
        .find(|prefix| lower.starts_with(prefix) && lower.len() > prefix.len())?;
    let mut rest = method[prefix.len()..].chars();
    let first = rest.next()?;
    Some(first.to_lowercase().chain(rest).collect())
}

/// The member of `class` Twig reads `segment` from.
fn twig_member(class: &ClassInfo, segment: Segment<'_>) -> Option<TwigMember> {
    if !segment.call
        && let Some(property) = class.properties.iter().find(|property| {
            property.name.as_str() == segment.name
                && !property.is_static
                && property.visibility == Visibility::Public
        })
    {
        return Some(TwigMember::Property(Arc::clone(property)));
    }
    let method = |name: &str| {
        class
            .methods
            .iter()
            .find(|method| {
                method.name.eq_ignore_ascii_case(name) && method.visibility == Visibility::Public
            })
            .cloned()
    };
    method(segment.name)
        .or_else(|| method(&format!("get{}", segment.name)))
        .or_else(|| method(&format!("is{}", segment.name)))
        .or_else(|| method(&format!("has{}", segment.name)))
        .map(TwigMember::Method)
}

/// One request for the type of a value written in a PHP file.
enum PhpValue<'n> {
    /// The expression spanning `start..end`.
    Expression { start: u32, end: u32 },
    /// The variable `name` as it stands at `offset`.
    Variable { name: &'n str, offset: u32 },
    /// The value written beside the string key whose contents start at
    /// `key_start`: an array entry's, or the next argument's.
    Beside { key_start: u32 },
}

/// Finds the expressions starting at the offsets asked for, and the values
/// written beside the keys asked for.
struct ValueWalker<'a> {
    starts: &'a [u32],
    keys: &'a [u32],
}

#[derive(Default)]
struct ValueFound<'ast, 'arena> {
    expressions: Vec<&'ast Expression<'arena>>,
    beside: Vec<(u32, &'ast Expression<'arena>)>,
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, ValueFound<'ast, 'arena>>
    for ValueWalker<'_>
{
    fn walk_in_expression(
        &self,
        node: &'ast Expression<'arena>,
        found: &mut ValueFound<'ast, 'arena>,
    ) {
        if self.starts.contains(&node.span().start.offset) {
            found.expressions.push(node);
        }
    }

    fn walk_in_key_value_array_element(
        &self,
        node: &'ast KeyValueArrayElement<'arena>,
        found: &mut ValueFound<'ast, 'arena>,
    ) {
        let key = node.key.span().start.offset;
        if self.keys.contains(&key) {
            found.beside.push((key, node.value));
        }
    }

    fn walk_in_argument_list(
        &self,
        node: &'ast ArgumentList<'arena>,
        found: &mut ValueFound<'ast, 'arena>,
    ) {
        let arguments: Vec<&Argument<'arena>> = node.arguments.iter().collect();
        for pair in arguments.windows(2) {
            let key = pair[0].value().span().start.offset;
            if self.keys.contains(&key) {
                found.beside.push((key, pair[1].value()));
            }
        }
    }
}

/// What one [`PhpValue`] request answers: its type, or for a context only
/// its type names, the entries it holds.
enum PhpAnswer {
    Type(Option<PhpType>),
    Shape(Vec<(String, PhpType)>),
}

/// Guards the typing of a template's variables against a template that
/// (through others) passes a variable back to itself.
type Visiting = Vec<(String, String, usize)>;

impl Backend {
    /// Resolve the values `values` ask for in the PHP file at `uri`, each
    /// with the type engine where it is written.  `shapes` asks for the
    /// entries of the expressions spanning each range.
    fn twig_php_values(
        &self,
        uri: &str,
        values: &[PhpValue<'_>],
        shapes: &[(u32, u32)],
    ) -> Vec<PhpAnswer> {
        let unanswered = || {
            values
                .iter()
                .map(|_| PhpAnswer::Type(None))
                .chain(shapes.iter().map(|_| PhpAnswer::Shape(Vec::new())))
                .collect()
        };
        let Some(content) = self.get_file_content_arc(uri) else {
            return unanswered();
        };
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _cache_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);
        let file_ctx = self.file_context(uri);
        let loaders = self.file_loaders(&file_ctx);
        let resolved_class_loader = loaders.resolved_class_loader();
        let _parse_guard = crate::parser::with_parse_cache(&content);
        let starts: Vec<u32> = values
            .iter()
            .filter_map(|value| match value {
                PhpValue::Expression { start, .. } => Some(*start),
                _ => None,
            })
            .chain(shapes.iter().map(|(start, _)| *start))
            .collect();
        // A key's span is its contents, inside the quotes.
        let keys: Vec<u32> = values
            .iter()
            .filter_map(|value| match value {
                PhpValue::Beside { key_start } => key_start.checked_sub(1),
                _ => None,
            })
            .collect();
        crate::parser::with_parsed_program(&content, "twig_render_types", |program, content| {
            let walker = ValueWalker {
                starts: &starts,
                keys: &keys,
            };
            let mut found = ValueFound::default();
            for statement in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, statement, &mut found);
            }
            let expression = |start: u32, end: u32| {
                found.expressions.iter().copied().find(|expression| {
                    let span = expression.span();
                    span.start.offset == start && span.end.offset == end
                })
            };
            let type_of = |expression: &Expression<'_>| {
                let offset = expression.span().start.offset;
                let var_ctx = loaders.var_ctx_at(content, offset);
                let ty = match expression {
                    Expression::Variable(Variable::Direct(variable)) => {
                        crate::type_engine::variable::resolution::resolve_variable_php_type(
                            var_ctx.var_query(crate::atom::bytes_to_str(variable.name), offset),
                        )
                    }
                    _ => crate::type_engine::variable::iterable_elements::resolve_expression_type(
                        expression, &var_ctx,
                    ),
                };
                ty.map(|ty| qualify_class_names(ty, resolved_class_loader))
            };
            let mut answers: Vec<PhpAnswer> = values
                .iter()
                .map(|value| {
                    PhpAnswer::Type(match value {
                        PhpValue::Expression { start, end } => {
                            expression(*start, *end).and_then(type_of)
                        }
                        PhpValue::Variable { name, offset } => {
                            let var_ctx = loaders.var_ctx_at(content, *offset);
                            crate::type_engine::variable::resolution::resolve_variable_php_type(
                                var_ctx.var_query(name, *offset),
                            )
                            .map(|ty| qualify_class_names(ty, resolved_class_loader))
                        }
                        PhpValue::Beside { key_start } => found
                            .beside
                            .iter()
                            .find(|(key, _)| *key + 1 == *key_start)
                            .and_then(|(_, value)| type_of(value)),
                    })
                })
                .collect();
            answers.extend(shapes.iter().map(|(start, end)| {
                let entries = expression(*start, *end)
                    .and_then(|expression| {
                        let var_ctx = loaders.var_ctx_at(content, expression.span().start.offset);
                        data_shape_entries(expression, &var_ctx)
                    })
                    .unwrap_or_default();
                PhpAnswer::Shape(
                    entries
                        .into_iter()
                        .map(|(name, ty)| (name, qualify_class_names(ty, resolved_class_loader)))
                        .collect(),
                )
            }));
            answers
        })
    }

    /// The entries each context spanning one of `shapes` in the PHP file at
    /// `uri` passes, read off its type.
    pub(crate) fn twig_context_shapes(
        &self,
        uri: &str,
        shapes: &[(u32, u32)],
    ) -> Vec<Vec<(String, PhpType)>> {
        self.twig_php_values(uri, &[], shapes)
            .into_iter()
            .map(|answer| match answer {
                PhpAnswer::Shape(entries) => entries,
                PhpAnswer::Type(_) => Vec::new(),
            })
            .collect()
    }

    /// A type a template spells, with the class names it names qualified:
    /// a template has no namespace and no imports.
    fn twig_declared_type(&self, declared: &str) -> Option<PhpType> {
        if declared.trim().is_empty() {
            return None;
        }
        let loader = self.resolved_class_loader_with(&[]);
        Some(qualify_class_names(PhpType::parse(declared), &loader))
    }

    /// The type of the variable `name` written at `offset` in the template
    /// at `uri`.
    pub(crate) fn twig_variable_type(
        &self,
        uri: &str,
        offset: usize,
        name: &str,
    ) -> Option<PhpType> {
        self.twig_variable_type_guarded(uri, offset, name, &mut Vec::new())
    }

    fn twig_variable_type_guarded(
        &self,
        uri: &str,
        offset: usize,
        name: &str,
        visiting: &mut Visiting,
    ) -> Option<PhpType> {
        match self.resolve_twig_variable(uri, Some(offset), name) {
            Resolved::Local(definitions) => {
                // The last definition before the variable is the one it
                // holds; one inside a value it is part of is not yet made.
                let applicable: Vec<&Definition> = definitions
                    .iter()
                    .filter(|definition| {
                        definition.start < offset
                            && !definition_value(definition)
                                .is_some_and(|(start, end)| start <= offset && offset <= end)
                    })
                    .collect();
                let chosen: Vec<&Definition> = match applicable.last() {
                    Some(last) => vec![*last],
                    None => definitions.iter().collect(),
                };
                let types: Vec<PhpType> = chosen
                    .into_iter()
                    .filter_map(|definition| self.twig_definition_type(uri, definition, visiting))
                    .collect();
                (!types.is_empty()).then(|| join_call_site_types(types))
            }
            Resolved::Received { declared, origins } => {
                if let Some(declared) =
                    declared
                        .iter()
                        .find_map(|definition| match &definition.kind {
                            DefinitionKind::Types { declared, .. } => {
                                self.twig_declared_type(declared)
                            }
                            _ => None,
                        })
                {
                    return Some(declared);
                }
                let key = (uri.to_string(), name.to_string(), usize::MAX);
                if visiting.contains(&key) {
                    return None;
                }
                visiting.push(key);
                let mut types: Vec<PhpType> = Vec::new();
                // The values a PHP file holds are resolved together, so the
                // file is parsed once however many of them it passes.
                let mut php: Vec<(&str, Vec<PhpValue<'_>>)> = Vec::new();
                for origin in &origins {
                    let value = match &origin.value {
                        OriginValue::Unknown => continue,
                        OriginValue::Type(ty) => {
                            types.push(ty.clone());
                            continue;
                        }
                        OriginValue::Twig { start, end } => {
                            types.extend(self.twig_expression_type(
                                &origin.uri,
                                *start,
                                *end,
                                visiting,
                            ));
                            continue;
                        }
                        OriginValue::TwigDefinition => {
                            types.extend(self.twig_variable_type_guarded(
                                &origin.uri,
                                origin.end as usize,
                                &origin.name,
                                visiting,
                            ));
                            continue;
                        }
                        OriginValue::Php(TwigContextValue::Expression { start, end }) => {
                            PhpValue::Expression {
                                start: *start,
                                end: *end,
                            }
                        }
                        OriginValue::Php(TwigContextValue::Variable { offset }) => {
                            PhpValue::Variable {
                                name: &origin.name,
                                offset: *offset,
                            }
                        }
                        OriginValue::PhpBeside => PhpValue::Beside {
                            key_start: origin.start,
                        },
                    };
                    match php.iter_mut().find(|(file, _)| *file == origin.uri) {
                        Some((_, values)) => values.push(value),
                        None => php.push((&origin.uri, vec![value])),
                    }
                }
                for (file, values) in &php {
                    types.extend(
                        self.twig_php_values(file, values, &[])
                            .into_iter()
                            .filter_map(|answer| match answer {
                                PhpAnswer::Type(ty) => ty,
                                PhpAnswer::Shape(_) => None,
                            }),
                    );
                }
                visiting.pop();
                (!types.is_empty()).then(|| join_call_site_types(types))
            }
            Resolved::Builtin(_) => match name {
                "app" => self.twig_declared_type(APP_VARIABLE),
                "loop" => Some(PhpType::parse(LOOP_TYPE)),
                _ => None,
            },
        }
    }

    /// What a definition the template at `uri` makes holds.
    fn twig_definition_type(
        &self,
        uri: &str,
        definition: &Definition,
        visiting: &mut Visiting,
    ) -> Option<PhpType> {
        let key = (uri.to_string(), String::new(), definition.start);
        if visiting.contains(&key) {
            return None;
        }
        visiting.push(key);
        let ty = match &definition.kind {
            DefinitionKind::Set {
                value: Some((start, end)),
            }
            | DefinitionKind::With {
                value: Some((start, end)),
            } => self.twig_expression_type(uri, *start, *end, visiting),
            DefinitionKind::Loop { iterable, key } => self
                .twig_expression_type(uri, iterable.0, iterable.1, visiting)
                .and_then(|iterable| self.twig_iteration_type(&iterable, *key)),
            DefinitionKind::Types { declared, .. } => self.twig_declared_type(declared),
            _ => None,
        };
        visiting.pop();
        ty
    }

    /// What a `for` over `iterable` binds its key or its value to.
    fn twig_iteration_type(&self, iterable: &PhpType, key: bool) -> Option<PhpType> {
        use crate::type_engine::variable::iterable_elements as iter;

        let loader = self.resolved_class_loader_with(&[]);
        let current_class = ClassInfo::default();
        let ctx = iter::IterableCtx {
            current_class: &current_class,
            all_classes: &[],
            class_loader: &loader,
            resolved_class_cache: Some(&self.resolved_class_cache),
        };
        if key {
            iter::iteration_key_type(iterable, &ctx)
        } else {
            iter::iteration_value_type(iterable, &ctx)
        }
    }

    /// The type of the expression spanning `start..end` of the template at
    /// `uri`: a literal, or a chain of a variable and its attributes.
    fn twig_expression_type(
        &self,
        uri: &str,
        start: usize,
        end: usize,
        visiting: &mut Visiting,
    ) -> Option<PhpType> {
        let content = self.twig_template_text(uri)?;
        let (start, end) = trim_range(content.as_bytes(), start, end.min(content.len()));
        let text = content.get(start..end)?;
        if let Some(chain) = chain_spanning(&content, start, end) {
            return self.twig_chain_type(uri, &chain, visiting);
        }
        literal_type(text)
    }

    fn twig_chain_type(
        &self,
        uri: &str,
        chain: &Chain<'_>,
        visiting: &mut Visiting,
    ) -> Option<PhpType> {
        let mut ty =
            self.twig_variable_type_guarded(uri, chain.root_start, chain.root, visiting)?;
        for segment in &chain.segments {
            ty = self.twig_attribute_type(&ty, *segment)?;
        }
        Some(ty)
    }

    /// The classes a value of type `ty` can be an instance of, each with
    /// its inherited and virtual members merged in.
    fn twig_classes_of(&self, ty: &PhpType) -> Vec<Arc<ClassInfo>> {
        let loader = self.resolved_class_loader_with(&[]);
        crate::type_engine::type_ops::resolution::type_hint_to_classes_typed(ty, "", &[], &loader)
            .into_iter()
            .map(|class| {
                // A generic class comes back with its arguments already
                // substituted, which the cache keyed by name cannot hold.
                if class.template_params.is_empty() {
                    crate::virtual_members::resolve_class_fully_cached(
                        &class,
                        &loader,
                        &self.resolved_class_cache,
                    )
                } else {
                    class
                }
            })
            .collect()
    }

    /// The members `segment` reads off a value of type `ty`, each with the
    /// class it is read from.
    fn twig_members(
        &self,
        ty: &PhpType,
        segment: Segment<'_>,
    ) -> Vec<(Arc<ClassInfo>, TwigMember)> {
        self.twig_classes_of(ty)
            .into_iter()
            .filter_map(|class| twig_member(&class, segment).map(|member| (class, member)))
            .collect()
    }

    /// The type of what a member holds or returns.
    fn twig_member_type(&self, class: &ClassInfo, member: &TwigMember) -> Option<PhpType> {
        let fqn = class.fqn();
        let ty = match member {
            TwigMember::Property(property) => property.type_hint.clone()?,
            TwigMember::Method(method) => match &method.return_type {
                Some(ty) if !ty.is_mixed() => ty.clone(),
                _ if method.name_offset != 0 && !method.is_virtual => {
                    crate::type_engine::call_resolution::try_infer_body_return_type(
                        self,
                        &fqn,
                        method,
                        &[],
                    )?
                }
                _ => return None,
            },
        };
        let loader = self.resolved_class_loader_with(&[]);
        Some(qualify_class_names(
            ty.resolve_self_refs(&fqn, class.parent_class.as_deref()),
            &loader,
        ))
    }

    /// The type of the attribute `segment` reads off a value of type `ty`.
    fn twig_attribute_type(&self, ty: &PhpType, segment: Segment<'_>) -> Option<PhpType> {
        let ty = ty.non_null_type().unwrap_or_else(|| ty.clone());
        let mut types = Vec::new();
        for member in ty.union_members() {
            if !segment.call
                && let Some(value) = member.shape_value_type(segment.name)
            {
                types.push(value.clone());
                continue;
            }
            for (class, found) in self.twig_members(member, segment) {
                if let Some(ty) = self.twig_member_type(&class, &found) {
                    types.push(ty);
                }
            }
        }
        (!types.is_empty()).then(|| join_call_site_types(types))
    }

    /// The type of the chain that ends right before `end` in the template
    /// at `uri`.
    fn twig_chain_type_at(&self, uri: &str, content: &str, end: usize) -> Option<PhpType> {
        let chain = chain_ending_at(content, end)?;
        self.twig_chain_type(uri, &chain, &mut Vec::new())
    }

    /// Complete the attribute being typed after a `.` in the template at
    /// `uri`: the keys of an array shape, and the public properties and
    /// methods of an object, by the names Twig reads them by.
    pub(crate) fn twig_attribute_completion(
        &self,
        uri: &str,
        content: &str,
        cursor: usize,
        position: Position,
    ) -> Option<CompletionResponse> {
        let bytes = content.as_bytes();
        let start = bytes[..cursor.min(bytes.len())]
            .iter()
            .rposition(|byte| !is_identifier_byte(*byte))
            .map_or(0, |index| index + 1);
        if start < 2
            || bytes[start - 1] != b'.'
            || bytes[start - 2] == b'.'
            || !in_expression(content, start)
        {
            return None;
        }
        let ty = self.twig_chain_type_at(uri, content, start - 1)?;
        let ty = ty.non_null_type().unwrap_or(ty);
        let mut items: Vec<(String, CompletionItemKind, String)> = Vec::new();
        let mut push = |label: String, kind: CompletionItemKind, detail: String| {
            if !items.iter().any(|(existing, ..)| *existing == label) {
                items.push((label, kind, detail));
            }
        };
        for member in ty.union_members() {
            if let crate::php_type::TypeKind::ArrayShape(entries) = member.kind() {
                for entry in entries {
                    if let Some(key) = &entry.key {
                        push(
                            key.to_string(),
                            CompletionItemKind::Field,
                            display_type(&entry.value_type),
                        );
                    }
                }
                continue;
            }
            for class in self.twig_classes_of(member) {
                for property in class.properties.iter() {
                    if !property.is_static && property.visibility == Visibility::Public {
                        let detail = self
                            .twig_member_type(&class, &TwigMember::Property(Arc::clone(property)))
                            .map(|ty| display_type(&ty))
                            .unwrap_or_default();
                        push(
                            property.name.to_string(),
                            CompletionItemKind::Property,
                            detail,
                        );
                    }
                }
                for method in class.methods.iter() {
                    if method.is_static
                        || method.visibility != Visibility::Public
                        || method.name.starts_with("__")
                    {
                        continue;
                    }
                    let detail = self
                        .twig_member_type(&class, &TwigMember::Method(Arc::clone(method)))
                        .map(|ty| display_type(&ty))
                        .unwrap_or_default();
                    let label =
                        getter_name(&method.name).unwrap_or_else(|| method.name.to_string());
                    push(label, CompletionItemKind::Method, detail);
                }
            }
        }
        if items.is_empty() {
            return None;
        }
        let prefix = &content[start..cursor];
        let range = Range {
            start: crate::text_position::offset_to_position(content, start),
            end: position,
        };
        let items = items
            .into_iter()
            .filter(|(label, ..)| {
                label
                    .as_bytes()
                    .get(..prefix.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
            })
            .enumerate()
            .map(|(index, (label, kind, detail))| CompletionItem {
                label: label.clone(),
                kind: Some(kind),
                detail: (!detail.is_empty()).then_some(detail),
                sort_text: Some(format!("{index:05}")),
                filter_text: Some(label.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range,
                    new_text: label,
                })),
                ..CompletionItem::default()
            })
            .collect();
        Some(CompletionResponse::CompletionItemList(items))
    }

    /// The members the attribute at `offset` of the template at `uri` reads,
    /// each with the class it is read from, or the type of the array key it
    /// reads.
    fn twig_attribute_targets(
        &self,
        uri: &str,
        content: &str,
        offset: usize,
    ) -> Option<(AttributeTargets, usize, usize)> {
        let (chain, segment, start, end) = attribute_at(content, offset)?;
        let ty = self.twig_chain_type(uri, &chain, &mut Vec::new())?;
        let ty = ty.non_null_type().unwrap_or(ty);
        let mut targets = AttributeTargets::default();
        for member in ty.union_members() {
            if !segment.call
                && let Some(value) = member.shape_value_type(segment.name)
            {
                targets.keys.push(value.clone());
                continue;
            }
            targets.members.extend(self.twig_members(member, segment));
        }
        Some((targets, start, end))
    }

    /// Hover an attribute read off a variable in a template: the member it
    /// reads, or the type of the array key.
    pub(crate) fn twig_attribute_hover(
        &self,
        uri: &str,
        content: &str,
        offset: usize,
    ) -> Option<Hover> {
        let (targets, start, end) = self.twig_attribute_targets(uri, content, offset)?;
        let loader = self.resolved_class_loader_with(&[]);
        let mut sections: Vec<String> = Vec::new();
        for (class, member) in &targets.members {
            let hover = match member {
                TwigMember::Property(property) => self.hover_for_property(property, class, &loader),
                TwigMember::Method(method) => {
                    self.hover_for_method(method, class, &loader, uri, content)
                }
            };
            if let Contents::MarkupContent(markup) = hover.contents
                && !sections.contains(&markup.value)
            {
                sections.push(markup.value);
            }
        }
        if !targets.keys.is_empty() {
            let ty = join_call_site_types(targets.keys);
            sections.push(format!(
                "**Twig attribute** `{}`\n\n`{}`",
                &content[start..end],
                display_type(&ty)
            ));
        }
        if sections.is_empty() {
            return None;
        }
        let mut hover = crate::hover::formatting::make_hover(sections.join("\n\n---\n\n"));
        hover.range = Some(Range {
            start: crate::text_position::offset_to_position(content, start),
            end: crate::text_position::offset_to_position(content, end),
        });
        Some(hover)
    }

    /// Where the members the attribute at `offset` of the template at `uri`
    /// reads are declared.
    pub(crate) fn twig_attribute_definitions(
        &self,
        uri: &str,
        content: &str,
        offset: usize,
    ) -> Vec<Location> {
        let Some((targets, ..)) = self.twig_attribute_targets(uri, content, offset) else {
            return Vec::new();
        };
        let mut locations: Vec<Location> = Vec::new();
        for (class, member) in targets.members {
            let (name, is_property) = match &member {
                TwigMember::Property(property) => (property.name.to_string(), true),
                TwigMember::Method(method) => (method.name.to_string(), false),
            };
            if let Some(location) =
                self.class_member_declaration_location(&class.fqn(), &name, is_property)
                && !locations.contains(&location)
            {
                locations.push(location);
            }
        }
        locations
    }
}

/// What an attribute reads: members of the classes the value can be, and
/// the types of the array keys it can be.
#[derive(Default)]
struct AttributeTargets {
    members: Vec<(Arc<ClassInfo>, TwigMember)>,
    keys: Vec<PhpType>,
}

/// The byte range of the value a definition is made from, when it has one.
fn definition_value(definition: &Definition) -> Option<(usize, usize)> {
    match &definition.kind {
        DefinitionKind::Set { value } | DefinitionKind::With { value } => *value,
        DefinitionKind::Loop { iterable, .. } => Some(*iterable),
        _ => None,
    }
}

/// A type as a template's reader sees it: class names without the leading
/// `\` a template never writes.
pub(crate) fn display_type(ty: &PhpType) -> String {
    ty.resolve_names(&|name: &str| name.trim_start_matches('\\').to_string())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(content: &str) -> Option<(String, Vec<String>)> {
        chain_ending_at(content, content.len()).map(|chain| {
            (
                chain.root.to_string(),
                chain
                    .segments
                    .iter()
                    .map(|segment| {
                        format!("{}{}", segment.name, if segment.call { "()" } else { "" })
                    })
                    .collect(),
            )
        })
    }

    #[test]
    fn chains_read_a_variable_and_its_attributes() {
        assert_eq!(chain("{{ post"), Some(("post".into(), vec![])));
        assert_eq!(
            chain("{{ post.author.name"),
            Some(("post".into(), vec!["author".into(), "name".into()]))
        );
        assert_eq!(
            chain("{{ post.comments(true, {a: 1}).first"),
            Some(("post".into(), vec!["comments()".into(), "first".into()]))
        );
        // A function's result, a filter's, and a range are not chains.
        assert_eq!(chain("{{ path('x').y"), None);
        assert_eq!(chain("{{ items|first.name"), None);
        assert_eq!(chain("{{ 1..count"), Some(("count".into(), vec![])));
        assert_eq!(chain("{{ x is not"), None);
    }

    #[test]
    fn getters_are_read_by_the_name_after_their_prefix() {
        assert_eq!(getter_name("getTitle").as_deref(), Some("title"));
        assert_eq!(getter_name("getFooBar").as_deref(), Some("fooBar"));
        assert_eq!(getter_name("isPublished").as_deref(), Some("published"));
        assert_eq!(getter_name("hasComments").as_deref(), Some("comments"));
        assert_eq!(getter_name("getÉtat").as_deref(), Some("état"));
        assert_eq!(getter_name("get"), None);
        assert_eq!(getter_name("title"), None);
    }

    #[test]
    fn literals_have_their_types() {
        assert_eq!(literal_type("'x'"), Some(PhpType::string()));
        assert_eq!(literal_type("42"), Some(PhpType::int()));
        assert_eq!(literal_type("4.2"), Some(PhpType::float()));
        assert_eq!(literal_type("true"), Some(PhpType::bool()));
        assert_eq!(literal_type("[1, 2]"), Some(PhpType::array()));
        assert_eq!(literal_type("post"), None);
    }
}
