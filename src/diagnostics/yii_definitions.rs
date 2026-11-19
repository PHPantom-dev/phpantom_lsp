//! Yii DI definitions checked against the class they build.
//!
//! ```php
//! LoggerInterface::class => [
//!     'class' => Logger::class,
//!     'alert' => '',                           // neither 'alert()' nor '$alert'
//!     '$targets' => [],                        // Logger::$targets is private
//!     'setFlushInterval()' => ['not-an-int'],  // takes an int
//! ],
//! ```
//!
//! `yiisoft/definitions` validates a definition when the container is
//! built, and throws for a key that names neither a method nor a property,
//! and for a method or property that is not public.  Building the object
//! then passes the values on, and PHP throws for one the method or the
//! property does not accept.  The members a key names that the class does
//! not have are the unknown-member check's (see
//! [`crate::symbol_map::DocblockMemberRef::YiiDefinition`]), and the
//! argument names a method does not take the string-key check's.
//!
//! The arguments go through the argument check every call gets
//! ([`Backend::check_passed_arguments`]), and a property's value through
//! the rule an assignment gets.  A `Reference`, `DynamicReference`, or
//! `ReferencesArray` is resolved to an object of the class it names before
//! it is passed, so it is typed as that class.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::array::{Array, ArrayElement, LegacyArray};
use mago_syntax::cst::call::Call;
use mago_syntax::cst::class_like::member::ClassLikeMemberSelector;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::{Atom, bytes_to_str};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of_names};
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{PhpType, ShapeEntry};
use crate::resolution::FileLoaders;
use crate::types::{ClassInfo, Visibility};
use crate::virtual_members::laravel::helpers::resolved_string_literal;
use crate::yii::definitions::definition_key;

use super::helpers::{FileDiagnosticContext, make_diagnostic};
use super::member_visibility::INVALID_MEMBER_ACCESS_CODE;
use super::property_type_errors::{
    TYPE_MISMATCH_PROPERTY_CODE, declared_property_type, property_mismatch_message,
    property_value_mismatches,
};
use super::readonly_writes::INVALID_READONLY_WRITE_CODE;
use super::type_errors::{PassedArgument, PassedCall};

/// Diagnostic code for a definition key that is not one a definition takes.
pub(crate) const INVALID_YII_DEFINITION_KEY_CODE: &str = "invalid_yii_definition_key";

/// The package the reference classes are in.
const DEFINITIONS_NAMESPACE: &str = "Yiisoft\\Definitions\\";

/// What the container resolves to an object of another class before it
/// passes it on.
const DEFINITION_INTERFACE: &str = "Yiisoft\\Definitions\\Contract\\DefinitionInterface";

/// A key a definition does not take.
pub(crate) struct InvalidDefinitionKey {
    /// Where the key's text starts, after the opening quote.
    pub(crate) start: u32,
    /// Where the key's text ends, before the closing quote.
    pub(crate) end: u32,
    /// The key as written.
    pub(crate) key: String,
    /// The key the class has a public member for, when the key names one
    /// without the `()` or `$` that says which kind it is.
    pub(crate) suggestion: Option<String>,
}

impl InvalidDefinitionKey {
    pub(crate) fn message(&self) -> String {
        let key = &self.key;
        match &self.suggestion {
            Some(method) if method.ends_with("()") => {
                format!("Unknown definition key '{key}'; call the method as '{method}'")
            }
            Some(property) => {
                format!("Unknown definition key '{key}'; set the property as '{property}'")
            }
            None => format!("Unknown definition key '{key}'"),
        }
    }
}

/// The definition arrays a file's symbol map recorded, found in its AST,
/// each with the class it builds.
struct DefinitionArrays<'s> {
    classes: &'s HashMap<u32, Atom>,
}

type FoundDefinitions<'a> = Vec<(&'a TokenSeparatedSequence<'a, ArrayElement<'a>>, Atom)>;

impl<'a> Walker<'a, 'a, FoundDefinitions<'a>> for DefinitionArrays<'_> {
    fn walk_in_array(&self, array: &'a Array<'a>, found: &mut FoundDefinitions<'a>) {
        if let Some(class) = self.classes.get(&array.span().start.offset) {
            found.push((&array.elements, *class));
        }
    }

    fn walk_in_legacy_array(&self, array: &'a LegacyArray<'a>, found: &mut FoundDefinitions<'a>) {
        if let Some(class) = self.classes.get(&array.span().start.offset) {
            found.push((&array.elements, *class));
        }
    }
}

/// Call `each` with every definition array of the file, its class with
/// what it inherits, and the file's AST, skipping one whose class or an
/// ancestor does not load.
fn for_each_definition(
    backend: &Backend,
    ctx: &FileDiagnosticContext,
    content: &str,
    mut each: impl FnMut(&TokenSeparatedSequence<'_, ArrayElement<'_>>, &Arc<ClassInfo>),
) {
    let bodies = &ctx.symbol_map.yii_definition_bodies;
    if bodies.is_empty() || !backend.is_yii_project() {
        return;
    }
    let classes: HashMap<u32, Atom> = bodies.iter().map(|body| (body.start, body.class)).collect();
    with_parsed_program(content, "yii_definitions", |program, _| {
        let mut found = Vec::new();
        let walker = DefinitionArrays { classes: &classes };
        for statement in program.statements.iter() {
            walker.walk_statement(statement, &mut found);
        }
        for (elements, class) in found {
            if let Some(class) = backend.yii_defined_class(class.as_str()) {
                each(elements, &class);
            }
        }
    });
}

/// The key `key` should be, when the class has a public member it names
/// without the `()` or `$`: a method first, as `yiisoft/definitions`
/// suggests.
fn suggested_key(class: &ClassInfo, key: &str) -> Option<String> {
    let name = key.replace("()", "").replace('$', "");
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let has_method = class
        .methods
        .iter()
        .any(|m| m.name == name && m.visibility == Visibility::Public && !m.name.starts_with("__"));
    if has_method {
        return Some(format!("{name}()"));
    }
    class
        .properties
        .iter()
        .find(|p| p.name == name && !p.is_virtual)
        .filter(|p| p.visibility == Visibility::Public && !is_readonly(class, p))
        .map(|_| format!("${name}"))
}

/// Whether `property` of `class` cannot be written from outside it.
fn is_readonly(class: &ClassInfo, property: &crate::types::PropertyInfo) -> bool {
    property.is_readonly || (class.is_readonly && !property.is_static)
}

/// The keys of a definition array, with their text and range, and the
/// value each is given.  A key that is not a string literal is not read.
fn literal_keys<'e, 'a>(
    elements: &'e TokenSeparatedSequence<'a, ArrayElement<'a>>,
    content: &str,
) -> impl Iterator<Item = (&'a str, u32, u32, &'e Expression<'a>)> {
    elements.iter().filter_map(move |element| {
        let ArrayElement::KeyValue(entry) = element else {
            return None;
        };
        let (key, start, end) = resolved_string_literal(entry.key, content)?;
        super::incremental::reports_site(start as u32).then_some((
            key,
            start as u32,
            end as u32,
            entry.value,
        ))
    })
}

impl Backend {
    /// The keys of the file's DI definitions that name neither a method
    /// nor a property.
    pub(crate) fn invalid_yii_definition_keys(
        &self,
        ctx: &FileDiagnosticContext,
        content: &str,
    ) -> Vec<InvalidDefinitionKey> {
        let mut invalid = Vec::new();
        for_each_definition(self, ctx, content, |elements, class| {
            for (key, start, end, _) in literal_keys(elements, content) {
                if takes_key(key) {
                    continue;
                }
                invalid.push(InvalidDefinitionKey {
                    start,
                    end,
                    key: key.to_string(),
                    suggestion: suggested_key(class, key),
                });
            }
        });
        invalid
    }

    /// Report what the file's DI definitions get wrong about the class
    /// they build: a key that names no member, a member that is not
    /// public, and a value the member does not accept.
    pub(crate) fn collect_yii_definition_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if ctx.symbol_map.yii_definition_bodies.is_empty() {
            return;
        }
        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);
        let check = DefinitionCheck {
            backend: self,
            ctx,
            uri,
            content,
            loaders: &loaders,
            docblock_types: self.docblock_type_mismatches_enabled(),
            report_possibly_empty: self.possibly_empty_mismatches_enabled(),
            report_partial_union: self.partial_union_mismatches_enabled(),
        };
        for_each_definition(self, ctx, content, |elements, class| {
            check.check_definition(elements, class, out);
        });
    }
}

/// What checking one file's definitions shares.
struct DefinitionCheck<'a> {
    backend: &'a Backend,
    ctx: &'a FileDiagnosticContext,
    uri: &'a str,
    content: &'a str,
    loaders: &'a FileLoaders<'a>,
    docblock_types: bool,
    report_possibly_empty: bool,
    report_partial_union: bool,
}

impl DefinitionCheck<'_> {
    fn class_loader(&self) -> &ResolvedClassLoader<'_> {
        self.loaders.resolved_class_loader()
    }

    fn push(
        &self,
        start: u32,
        end: u32,
        severity: DiagnosticSeverity,
        code: &str,
        message: String,
        out: &mut Vec<Diagnostic>,
    ) {
        if let Some(range) = self.backend.offset_range_to_lsp_range(
            self.uri,
            self.content,
            start as usize,
            end as usize,
        ) {
            out.push(make_diagnostic(range, severity, code, message));
        }
    }

    fn check_definition(
        &self,
        elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
        class: &Arc<ClassInfo>,
        out: &mut Vec<Diagnostic>,
    ) {
        let fqn = class.fqn();
        for (key, start, end, value) in literal_keys(elements, self.content) {
            match key {
                "class" | "tags" | "reset" => {}
                // The container passes the constructor's arguments from
                // its own code, which declares `strict_types`.
                "__construct()" => {
                    self.check_arguments(&format!("new \\{fqn}"), value, true, out);
                }
                _ => match definition_key(key) {
                    Some((name, true)) => {
                        self.check_method(class, name, start, value, out);
                    }
                    Some((name, false)) => {
                        self.check_property(class, name, start + 1, end, value, out);
                    }
                    None => {
                        let invalid = InvalidDefinitionKey {
                            start,
                            end,
                            key: key.to_string(),
                            suggestion: suggested_key(class, key),
                        };
                        self.push(
                            start,
                            end,
                            DiagnosticSeverity::Error,
                            INVALID_YII_DEFINITION_KEY_CODE,
                            invalid.message(),
                            out,
                        );
                    }
                },
            }
        }
    }

    /// Check a `'name()'` key: the method is public, and takes what the
    /// key's value passes.  One the class does not have is the
    /// unknown-member check's.
    fn check_method(
        &self,
        class: &ClassInfo,
        name: &str,
        start: u32,
        value: &Expression<'_>,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(method) = class.get_method_ci(name) else {
            return;
        };
        if let Some(modifier) = restricted(method.visibility) {
            self.push(
                start,
                start + name.len() as u32,
                DiagnosticSeverity::Error,
                INVALID_MEMBER_ACCESS_CODE,
                format!(
                    "Cannot call {modifier} method {}::{}() from a definition",
                    class.fqn(),
                    method.name
                ),
                out,
            );
            return;
        }
        // The container calls a method through `call_user_func_array()`,
        // which passes the arguments as a call without `strict_types`
        // does.
        self.check_arguments(
            &format!("\\{}::{}", class.fqn(), method.name),
            value,
            false,
            out,
        );
    }

    /// Check a `'$name'` key, whose name is at `[start, end)`: the property
    /// is public and writable, and takes the key's value.  One the class
    /// does not have is the unknown-member check's.
    fn check_property(
        &self,
        class: &Arc<ClassInfo>,
        name: &str,
        start: u32,
        end: u32,
        value: &Expression<'_>,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(property) = class.properties.iter().find(|p| p.name == name) else {
            return;
        };
        let fqn = class.fqn();
        if let Some(modifier) = restricted(property.visibility) {
            self.push(
                start,
                end,
                DiagnosticSeverity::Error,
                INVALID_MEMBER_ACCESS_CODE,
                format!("Cannot set {modifier} property {fqn}::${name} from a definition"),
                out,
            );
            return;
        }
        if is_readonly(class, property) {
            self.push(
                start,
                end,
                DiagnosticSeverity::Error,
                INVALID_READONLY_WRITE_CODE,
                format!("Cannot modify readonly property {fqn}::${name} from a definition"),
                out,
            );
            return;
        }
        let Some(declared) = declared_property_type(
            class,
            name,
            false,
            self.docblock_types,
            self.class_loader(),
            self.backend,
        ) else {
            return;
        };
        let Some(value_type) = self.value_type(value) else {
            return;
        };
        // The container sets a property from its own code, which declares
        // `strict_types`.
        if property_value_mismatches(
            &value_type,
            &declared,
            self.class_loader(),
            true,
            self.report_possibly_empty,
            self.report_partial_union,
        ) {
            let span = value.span();
            self.push(
                span.start.offset,
                span.end.offset,
                DiagnosticSeverity::Error,
                TYPE_MISMATCH_PROPERTY_CODE,
                property_mismatch_message(&format!("{fqn}::${name}"), &declared, &value_type),
                out,
            );
        }
    }

    /// Check the arguments the array `value` passes to the callable
    /// `callee` (`new \Foo` or `\Foo::bar`), positionally or by name.
    ///
    /// An array that mixes the two, spreads another, or keys an argument
    /// by number is not read: the container rejects the first, and the
    /// other two place arguments in ways a call does not.
    fn check_arguments(
        &self,
        callee: &str,
        value: &Expression<'_>,
        strict_types: bool,
        out: &mut Vec<Diagnostic>,
    ) {
        let elements = match value {
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            _ => return,
        };
        let mut arguments = Vec::with_capacity(elements.len());
        let (mut named, mut positional) = (false, false);
        for element in elements.iter() {
            let (name, value) = match element {
                ArrayElement::KeyValue(entry) => {
                    let Some((name, _, _)) = resolved_string_literal(entry.key, self.content)
                    else {
                        return;
                    };
                    named = true;
                    (Some(name), entry.value)
                }
                ArrayElement::Value(entry) => {
                    positional = true;
                    (None, entry.value)
                }
                _ => return,
            };
            arguments.push((name, value));
        }
        if arguments.is_empty() || (named && positional) {
            return;
        }
        let offset = value.span().start.offset;
        let Some(target) = self.backend.resolve_callable_target_with_args_at_offset(
            callee,
            self.content,
            offset,
            &self.ctx.file,
            None,
        ) else {
            return;
        };
        let passed = arguments
            .into_iter()
            // A variadic parameter named by a key takes an array of the
            // arguments it collects, which a call cannot pass it.
            .filter(|(name, _)| {
                !name.is_some_and(|name| {
                    target
                        .parameters
                        .iter()
                        .any(|p| p.is_variadic && p.name.trim_start_matches('$') == name)
                })
            })
            .filter_map(|(name, value)| {
                let span = value.span();
                Some(PassedArgument {
                    ty: self.value_type(value)?,
                    start: span.start.offset as usize,
                    end: span.end.offset as usize,
                    name: name.map(str::to_string),
                })
            })
            .collect();
        self.backend.check_passed_arguments(
            self.uri,
            self.content,
            self.class_loader(),
            PassedCall {
                target: &target,
                args: passed,
                strict_types,
            },
            out,
        );
    }

    /// The type of what the container passes for `value`: a reference as
    /// the object of the class it names, an array with the references in
    /// it resolved, and anything else as it is written.  `None` when that
    /// cannot be told.
    fn value_type(&self, value: &Expression<'_>) -> Option<PhpType> {
        if let Some(reference) = self.reference_call(value) {
            return reference;
        }
        let elements = match value {
            Expression::Array(array) if self.holds_reference(value) => &array.elements,
            Expression::LegacyArray(array) if self.holds_reference(value) => &array.elements,
            _ => return self.written_type(value),
        };
        let mut entries = Vec::with_capacity(elements.len());
        let mut keyed = false;
        for element in elements.iter() {
            let (key, value) = match element {
                ArrayElement::KeyValue(entry) => (Some(literal_key(entry.key)?), entry.value),
                ArrayElement::Value(entry) => (None, entry.value),
                _ => return None,
            };
            keyed |= key.is_some();
            entries.push(ShapeEntry {
                key,
                value_type: self.value_type(value)?,
                optional: false,
            });
        }
        Some(if keyed {
            PhpType::array_shape(entries)
        } else {
            PhpType::list_shape(entries)
        })
    }

    /// The type of `value` as the code that writes it would have it.
    ///
    /// `None` when the type holds a definition object anywhere in it (a
    /// reference made where this does not see it, such as in one branch of
    /// a ternary), which the container resolves to something else before
    /// passing it.
    fn written_type(&self, value: &Expression<'_>) -> Option<PhpType> {
        let start = value.span().start.offset;
        let var_ctx = self.loaders.var_ctx_at(self.content, start);
        let ty = crate::type_engine::variable::iterable_elements::resolve_expression_type(
            value, &var_ctx,
        )?;
        let loader = self.class_loader();
        let ty = ty.resolve_names(&|name: &str| match loader(Fqn::new(name)) {
            Some(class) if !name.contains("__anonymous@") => class.fqn().to_string(),
            _ => name.to_string(),
        });
        let resolved_by_container = ty
            .extract_class_names()
            .iter()
            .any(|name| is_subtype_of_names(name, DEFINITION_INTERFACE, loader));
        (!resolved_by_container).then_some(ty)
    }

    /// Whether the array `value` holds a reference, at any depth.
    fn holds_reference(&self, value: &Expression<'_>) -> bool {
        let elements = match value {
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            _ => return self.reference_call(value).is_some(),
        };
        elements.iter().any(|element| match element {
            ArrayElement::KeyValue(entry) => self.holds_reference(entry.value),
            ArrayElement::Value(entry) => self.holds_reference(entry.value),
            _ => false,
        })
    }

    /// The type of the object a reference resolves to, when `value` makes
    /// one: `Some(None)` for a reference whose target is not a class the
    /// file names.
    fn reference_call(&self, value: &Expression<'_>) -> Option<Option<PhpType>> {
        let Expression::Call(Call::StaticMethod(call)) = value else {
            return None;
        };
        let Expression::Identifier(identifier) = call.class else {
            return None;
        };
        let ClassLikeMemberSelector::Identifier(method) = &call.method else {
            return None;
        };
        let class = self.ctx.file.resolve_name_at(
            bytes_to_str(identifier.value()),
            identifier.span().start.offset,
        );
        let class = class.trim_start_matches('\\');
        let short = class.strip_prefix(DEFINITIONS_NAMESPACE)?;
        let method = bytes_to_str(method.value);
        let argument = call.argument_list.arguments.first().map(|a| a.value());
        Some(match (short, method.to_ascii_lowercase().as_str()) {
            ("Reference" | "DynamicReference", "to") => argument.and_then(|a| self.class_named(a)),
            ("Reference", "optional") => argument
                .and_then(|a| self.class_named(a))
                .map(PhpType::nullable),
            ("ReferencesArray" | "DynamicReferencesArray", "from") => {
                argument.and_then(|a| self.references_array(a))
            }
            _ => return None,
        })
    }

    /// The array of objects `ReferencesArray::from()` makes of the class
    /// names in `ids`.
    fn references_array(&self, ids: &Expression<'_>) -> Option<PhpType> {
        let elements = match ids {
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            _ => return None,
        };
        let mut entries = Vec::with_capacity(elements.len());
        let mut keyed = false;
        for element in elements.iter() {
            let (key, id) = match element {
                ArrayElement::KeyValue(entry) => (Some(literal_key(entry.key)?), entry.value),
                ArrayElement::Value(entry) => (None, entry.value),
                _ => return None,
            };
            keyed |= key.is_some();
            entries.push(ShapeEntry {
                key,
                value_type: self.class_named(id)?,
                optional: false,
            });
        }
        Some(if keyed {
            PhpType::array_shape(entries)
        } else {
            PhpType::list_shape(entries)
        })
    }

    /// The class a `Foo::class` expression names, as a type.
    fn class_named(&self, value: &Expression<'_>) -> Option<PhpType> {
        let Expression::Identifier(identifier) = crate::parser::class_fetch_target(value)? else {
            return None;
        };
        let class = self.ctx.file.resolve_name_at(
            bytes_to_str(identifier.value()),
            identifier.span().start.offset,
        );
        Some(PhpType::named(crate::atom::atom(
            class.trim_start_matches('\\'),
        )))
    }
}

/// The key a literal array key writes, as a shape entry spells it.
fn literal_key(key: &Expression<'_>) -> Option<String> {
    match key {
        Expression::Literal(Literal::String(string)) => string
            .value
            .and_then(crate::atom::literal_bytes_to_str)
            .map(str::to_string),
        Expression::Literal(Literal::Integer(integer)) => integer.value.map(|v| v.to_string()),
        _ => None,
    }
}

/// Whether a definition takes `key` as something other than a member: the
/// class it builds, its constructor's arguments, or the metadata
/// `yiisoft/di` reads beside them.
fn takes_key(key: &str) -> bool {
    matches!(key, "class" | "__construct()" | "tags" | "reset") || definition_key(key).is_some()
}

/// The modifier that keeps a member from being reached from outside its
/// class, or `None` for a public one.
fn restricted(visibility: Visibility) -> Option<&'static str> {
    match visibility {
        Visibility::Public => None,
        Visibility::Protected => Some("protected"),
        Visibility::Private => Some("private"),
    }
}
