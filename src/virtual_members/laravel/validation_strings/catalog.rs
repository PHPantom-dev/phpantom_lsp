//! Validation rules supplied by the installed framework and application.

use std::collections::{BTreeMap, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;

use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::Location;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::{Fqn, is_subtype_of};
use crate::names::OwnedResolvedNames;
use crate::type_engine::resolver::{VarResolutionCtx, resolve_target_classes};
use crate::types::{AccessKind, ClassInfo, ClassLikeKind, MethodInfo, PropertySource, Visibility};

use super::super::expr_walker::walk_program_expressions;
use super::super::helpers::{camel_to_snake, extract_string_literal};

const VALIDATOR: &str = "Illuminate\\Validation\\Validator";
const ATTRIBUTES: &str = "Illuminate\\Validation\\Concerns\\ValidatesAttributes";
const RULE_PARSER: &str = "Illuminate\\Validation\\ValidationRuleParser";
const RULE_CONTRACT: &str = "Illuminate\\Contracts\\Validation\\ValidationRule";
const FACADE: &str = "Illuminate\\Support\\Facades\\Validator";
const FACTORY: &str = "Illuminate\\Validation\\Factory";
const FACTORY_CONTRACT: &str = "Illuminate\\Contracts\\Validation\\Factory";

/// A named string rule and the declaration that explains it.
#[derive(Clone, Debug)]
pub(crate) struct RuleDefinition {
    /// Laravel's snake-case spelling of the rule.
    pub name: String,
    /// The declaring method's documentation or registration description.
    pub description: String,
    /// Whether the rule accepts the colon-separated parameter part.
    pub takes_parameters: bool,
    /// The method or registration that declares the rule.
    pub location: Option<Location>,
}

/// An application rule object, offered separately from string rules.
#[derive(Clone, Debug)]
pub(crate) struct RuleClass {
    /// The resolved class name, without a leading backslash.
    pub fqn: String,
    /// The class's free-text documentation.
    pub description: String,
}

/// Shared facts used by every validation-string consumer.
#[derive(Default)]
pub(crate) struct RuleCatalog {
    /// Named rules, sorted by name.
    pub rules: Vec<RuleDefinition>,
    /// Concrete application rule classes, sorted by fully-qualified name.
    pub classes: Vec<RuleClass>,
    /// Rules whose parameters refer to other fields, read from Validator.
    pub dependent_rules: Vec<String>,
    /// Resolved application models supplying table, column, and enum metadata.
    pub models: Vec<Arc<ClassInfo>>,
    /// Whether an unknown name can be judged against this catalog.
    pub complete: bool,
    sources: HashSet<String>,
}

impl RuleCatalog {
    /// Whether a changed file contributed to this snapshot.
    pub(crate) fn reads(&self, uri: &str) -> bool {
        self.sources.contains(uri)
    }
}

/// Discover validation facts once; the caller caches the resulting snapshot.
pub(crate) fn discover(backend: &Backend) -> RuleCatalog {
    let mut catalog = RuleCatalog::default();
    let mut definitions = BTreeMap::new();
    for fqn in [ATTRIBUTES, VALIDATOR] {
        let Some(class) = backend.find_or_load_class(fqn) else {
            continue;
        };
        let source = class_source(backend, fqn, &mut catalog.sources);
        for method in &class.methods {
            if let Some(rule) = method_rule(
                method,
                source
                    .as_ref()
                    .map(|(uri, text)| (uri.as_str(), text.as_str())),
            ) {
                definitions.insert(rule.name.clone(), rule);
            }
        }
        if fqn == VALIDATOR {
            catalog.dependent_rules = dependent_rules(&class);
        }
    }
    catalog.complete = !definitions.is_empty();

    if let Some((_, source)) = class_source(backend, RULE_PARSER, &mut catalog.sources) {
        for (alias, target) in rule_aliases(&source) {
            if let Some(definition) = definitions.get(&target) {
                let mut definition = definition.clone();
                definition.name = alias.clone();
                definitions.insert(alias, definition);
            }
        }
    }

    let mut candidates: HashSet<String> = backend
        .user_file_symbol_maps_for_methods(&[
            "extend",
            "extendImplicit",
            "extendDependent",
            "resolver",
        ])
        .into_iter()
        .map(|(uri, _)| uri)
        .collect();
    let providers = backend.laravel_providers();
    for fqn in providers.fqns() {
        let Some(uri) = backend.resolve_class_uri(fqn) else {
            catalog.complete = false;
            continue;
        };
        candidates.insert(uri.clone());
        let Some(content) = backend.get_file_content(&uri) else {
            catalog.complete = false;
            continue;
        };
        for helper in super::super::parse_provider_referenced_classes(&content) {
            if let Some(helper_uri) = backend.resolve_class_uri(&helper)
                && backend.is_macro_helper_uri_allowed(&uri, &helper_uri)
            {
                candidates.insert(helper_uri);
            }
        }
    }
    let mut candidates: Vec<_> = candidates.into_iter().collect();
    candidates.sort_unstable();
    for uri in candidates {
        catalog.sources.insert(uri.clone());
        let Some(content) = backend.get_file_content(&uri) else {
            catalog.complete = false;
            continue;
        };
        let registration = source_registrations(backend, &uri, &content);
        catalog.complete &= !registration.dynamic;
        catalog.dependent_rules.extend(registration.dependent);
        for validator in registration.validators {
            catalog.dependent_rules.extend(dependent_rules(&validator));
            for rule in validator_rules(backend, &validator, &mut catalog.sources) {
                definitions.insert(rule.name.clone(), rule);
            }
        }
        for rule in registration.rules {
            definitions.insert(rule.name.clone(), rule);
        }
    }

    let vendor_prefixes = backend.workspace.vendor_uri_prefixes.lock().clone();
    let classes: Vec<_> = backend
        .symbols
        .uri_classes_index
        .read()
        .iter()
        .filter(|(uri, _)| {
            !crate::stubs::is_stub_uri(uri)
                && !vendor_prefixes
                    .iter()
                    .any(|prefix| uri.starts_with(prefix.as_str()))
        })
        .flat_map(|(uri, classes)| classes.iter().map(|class| (uri.clone(), Arc::clone(class))))
        .collect();
    let loader = |fqn: Fqn<'_>| backend.find_or_load_class(&fqn);
    for (uri, class) in classes {
        if class.kind != ClassLikeKind::Class || class.is_abstract {
            continue;
        }
        if is_subtype_of(&class, RULE_CONTRACT, &loader) {
            catalog.sources.insert(uri.clone());
            let description = class
                .class_docblock
                .as_deref()
                .and_then(crate::docblock::parse_docblock_for_tags)
                .and_then(|info| info.description)
                .unwrap_or_default();
            catalog.classes.push(RuleClass {
                fqn: class.fqn().to_string(),
                description,
            });
        }
        if is_subtype_of(&class, super::super::ELOQUENT_MODEL_FQN, &loader) {
            catalog.sources.insert(uri);
            catalog
                .models
                .push(crate::virtual_members::resolve_class_fully_cached(
                    &class,
                    &loader,
                    &backend.resolved_class_cache,
                ));
        }
    }
    catalog.classes.sort_unstable_by(|a, b| a.fqn.cmp(&b.fqn));
    catalog.models.sort_unstable_by_key(|class| class.fqn());
    catalog.dependent_rules.sort_unstable();
    catalog.dependent_rules.dedup();
    catalog.rules = definitions.into_values().collect();
    catalog
}

fn class_source(
    backend: &Backend,
    fqn: &str,
    sources: &mut HashSet<String>,
) -> Option<(String, String)> {
    let uri = backend.resolve_class_uri(fqn)?;
    sources.insert(uri.clone());
    let content = backend.get_file_content(&uri)?;
    Some((uri, content))
}

fn declaration_location(uri: &str, content: &str, offset: usize) -> Option<Location> {
    Some(crate::definition::point_location(
        crate::util::parse_url(uri)?,
        crate::text_position::offset_to_position(content, offset),
    ))
}

fn method_rule(method: &MethodInfo, source: Option<(&str, &str)>) -> Option<RuleDefinition> {
    let suffix = method.name.get(8..)?;
    if !method.name.get(..8)?.eq_ignore_ascii_case("validate")
        || suffix.is_empty()
        || suffix.eq_ignore_ascii_case("d")
        || suffix.eq_ignore_ascii_case("WithBag")
        || method.visibility != Visibility::Public
    {
        return None;
    }
    Some(RuleDefinition {
        name: camel_to_snake(suffix),
        description: method.description.clone().unwrap_or_default(),
        // Laravel passes the field, value, and parameter array positionally.
        takes_parameters: method.parameters.len() >= 3
            || method
                .parameters
                .iter()
                .any(|parameter| parameter.is_variadic),
        location: source.and_then(|(uri, content)| {
            declaration_location(uri, content, method.name_offset as usize)
        }),
    })
}

fn dependent_rules(class: &ClassInfo) -> Vec<String> {
    let value = class.properties.iter().find_map(|property| {
        if property.name == "dependentRules"
            && let Some(PropertySource::DeclaredDefault { value }) = property.source.as_ref()
        {
            return Some(value);
        }
        None
    });
    let Some(value) = value else {
        return Vec::new();
    };
    let source = format!("<?php {value};");
    crate::parser::with_parsed_program(&source, "validation_dependent_rules", |program, _| {
        let mut rules = Vec::new();
        walk_program_expressions(program, &mut |expression| {
            if let Some((value, _, _)) = extract_string_literal(expression, &source) {
                rules.push(camel_to_snake(value));
            }
            ControlFlow::Continue(())
        });
        rules
    })
}

#[derive(Default)]
struct Registrations {
    rules: Vec<RuleDefinition>,
    dependent: Vec<String>,
    validators: Vec<Arc<ClassInfo>>,
    dynamic: bool,
}

fn source_registrations(backend: &Backend, uri: &str, content: &str) -> Registrations {
    // The request's scope and chain caches describe another file. Provider
    // byte offsets must resolve against the provider's own declarations.
    let _isolated = (
        crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
        crate::type_engine::resolver::with_isolated_chain_cache(),
    );
    if backend.file_state(uri).is_none() {
        backend.parse_and_cache_content(content, uri);
    }
    let ctx = backend.file_context(uri);
    let loaders = backend.file_loaders(&ctx);
    let resolved = loaders.resolved_class_loader();
    registrations(
        uri,
        content,
        |receiver| {
            let span = receiver.span();
            let subject = &content[span.start.offset as usize..span.end.offset as usize];
            let rctx = loaders.resolution_ctx_at(content, span.start.offset);
            resolve_target_classes(subject, AccessKind::Arrow, &rctx)
                .iter()
                .filter_map(|ty| ty.class_info.as_deref())
                .any(|class| {
                    is_subtype_of(class, FACTORY, resolved)
                        || is_subtype_of(class, FACTORY_CONTRACT, resolved)
                })
        },
        |resolver| {
            let rctx = loaders.var_ctx_at(content, resolver.span().start.offset);
            registered_validator(resolver, &rctx)
        },
    )
}

fn registrations(
    uri: &str,
    content: &str,
    instance_is_factory: impl Fn(&Expression<'_>) -> bool,
    resolver_validator: impl Fn(&Expression<'_>) -> Option<Arc<ClassInfo>>,
) -> Registrations {
    if !content
        .as_bytes()
        .windows(6)
        .any(|part| part.eq_ignore_ascii_case(b"extend"))
        && !content
            .as_bytes()
            .windows(8)
            .any(|part| part.eq_ignore_ascii_case(b"resolver"))
    {
        return Registrations::default();
    }
    crate::parser::with_parsed_program(content, "validation_registrations", |program, _| {
        let arena = mago_allocator::LocalArena::new();
        let names = NameResolver::new(&arena).resolve(program);
        let names = OwnedResolvedNames::from_resolved(&names);
        let mut out = Registrations::default();
        walk_program_expressions(program, &mut |expression| {
            let (receiver, selector, arguments, is_static) = match expression {
                Expression::Call(Call::StaticMethod(call)) => {
                    (call.class, &call.method, &call.argument_list, true)
                }
                Expression::Call(Call::Method(call)) => {
                    (call.object, &call.method, &call.argument_list, false)
                }
                Expression::Call(Call::NullSafeMethod(call)) => {
                    (call.object, &call.method, &call.argument_list, false)
                }
                _ => return ControlFlow::Continue(()),
            };
            let ClassLikeMemberSelector::Identifier(method) = selector else {
                return ControlFlow::Continue(());
            };
            let method = bytes_to_str(method.value);
            if !["extend", "extendImplicit", "extendDependent", "resolver"]
                .iter()
                .any(|name| method.eq_ignore_ascii_case(name))
            {
                return ControlFlow::Continue(());
            }
            let is_factory = if is_static {
                matches!(receiver, Expression::Identifier(identifier) if names
                    .get(identifier.span().start.offset)
                    .is_some_and(|name| name.eq_ignore_ascii_case(FACADE)))
            } else {
                instance_is_factory(receiver)
            };
            if !is_factory {
                return ControlFlow::Continue(());
            }
            if method.eq_ignore_ascii_case("resolver") {
                if let Some(validator) = crate::call_args::first_param_arg(arguments, "resolver")
                    .and_then(|argument| resolver_validator(argument.value()))
                {
                    out.validators.push(validator);
                } else {
                    out.dynamic = true;
                }
                return ControlFlow::Continue(());
            }
            let Some((name, start, _)) = crate::call_args::first_param_arg(arguments, "rule")
                .and_then(|argument| {
                    super::super::helpers::extract_string_literal_including_empty(
                        argument.value(),
                        content,
                    )
                })
            else {
                out.dynamic = true;
                return ControlFlow::Continue(());
            };
            if name.is_empty() {
                return ControlFlow::Continue(());
            }
            if method.eq_ignore_ascii_case("extendDependent") {
                out.dependent.push(name.to_string());
            }
            out.rules.push(RuleDefinition {
                name: name.to_string(),
                description: "Custom validation rule registered by the application.".to_string(),
                takes_parameters: true,
                location: declaration_location(uri, content, start),
            });
            ControlFlow::Continue(())
        });
        out
    })
}

fn validator_rules(
    backend: &Backend,
    validator: &Arc<ClassInfo>,
    sources: &mut HashSet<String>,
) -> Vec<RuleDefinition> {
    let loader = |name: Fqn<'_>| backend.find_or_load_class(&name);
    let resolved = crate::virtual_members::resolve_class_fully_cached(
        validator,
        &loader,
        &backend.resolved_class_cache,
    );
    let mut source_files = BTreeMap::new();
    resolved
        .methods
        .iter()
        .filter_map(|method| {
            if method.is_virtual || method_rule(method, None).is_none() {
                return None;
            }
            let declares = |class: &ClassInfo| {
                class
                    .methods
                    .iter()
                    .any(|own| own.name.eq_ignore_ascii_case(&method.name))
            };
            let declaring = if declares(validator) {
                Arc::clone(validator)
            } else {
                crate::inheritance::find_declaring_ancestor(validator, &loader, &declares)?.1
            };
            let fqn = declaring.fqn().to_string();
            let source = source_files
                .entry(fqn.clone())
                .or_insert_with(|| class_source(backend, &fqn, sources));
            method_rule(
                method,
                source
                    .as_ref()
                    .map(|(uri, content)| (uri.as_str(), content.as_str())),
            )
        })
        .collect()
}

fn registered_validator(
    resolver: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    let returned = if let Some(construction) = resolver_construction(resolver) {
        // A declared base return type may hide the concrete class being
        // constructed. Use the shared expression pipeline at that return.
        crate::type_engine::variable::iterable_elements::resolve_expression_type(construction, ctx)?
    } else {
        crate::type_engine::variable::rhs_resolution::infer_closure_literal_type(resolver, ctx)
            .callable_return_type()?
            .clone()
    };
    let class = (ctx.file.class_loader)(Fqn::new(returned.base_name()?))?;
    (is_subtype_of(&class, VALIDATOR, ctx.file.class_loader)
        && resolver_return_is_concrete(resolver, &class))
    .then_some(class)
}

fn resolver_return_is_concrete(resolver: &Expression<'_>, class: &ClassInfo) -> bool {
    if class.kind != ClassLikeKind::Class || class.is_abstract {
        return false;
    }
    if !class.fqn().eq_ignore_ascii_case(VALIDATOR) {
        return true;
    }
    resolver_construction(resolver).is_some()
}

fn resolver_construction<'a>(resolver: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
    let returned = match resolver {
        Expression::ArrowFunction(arrow) => Some(arrow.expression),
        Expression::Closure(closure) if closure.body.statements.len() == 1 => {
            match closure.body.statements.first() {
                Some(Statement::Return(ret)) => ret.value,
                _ => None,
            }
        }
        _ => None,
    };
    let mut returned = returned?;
    while let Expression::Parenthesized(parenthesized) = returned {
        returned = parenthesized.expression;
    }
    matches!(returned, Expression::Instantiation(_)).then_some(returned)
}

fn rule_aliases(content: &str) -> Vec<(String, String)> {
    crate::parser::with_parsed_program(content, "validation_rule_aliases", |program, _| {
        let mut out = Vec::new();
        collect_aliases(Node::Program(program), content, &mut out);
        out
    })
}

fn collect_aliases(node: Node<'_, '_>, content: &str, out: &mut Vec<(String, String)>) {
    if let Node::Method(method) = node {
        if method.name.value.eq_ignore_ascii_case(b"normalizeRule")
            && let MethodBody::Concrete(body) = &method.body
        {
            super::super::expr_walker::walk_block_expressions(body, &mut |expression| {
                if let Expression::Match(expression) = expression {
                    for arm in expression.arms.iter() {
                        let MatchArm::Expression(arm) = arm else {
                            continue;
                        };
                        let Some((target, _, _)) = extract_string_literal(arm.expression, content)
                        else {
                            continue;
                        };
                        for condition in arm.conditions.iter() {
                            if let Some((alias, _, _)) = extract_string_literal(condition, content)
                            {
                                out.push((camel_to_snake(alias), camel_to_snake(target)));
                            }
                        }
                    }
                }
                ControlFlow::Continue(())
            });
        }
        return;
    }
    node.visit_children(|child| collect_aliases(child, content, out));
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
