//! Validation strings share one context and declaration lookup across editor features.

pub(crate) mod catalog;
mod context;
mod references;
mod schema;

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::Location;

use crate::Backend;
use crate::class_lookup::{Fqn, find_class_at_offset, is_subtype_of};
use crate::type_engine::resolver::resolve_target_classes;
use crate::types::{AccessKind, ClassInfo, FileContext};

pub(crate) use context::{
    SiteKind, ValidationOwner, ValidationSite, at_cursor, collect, literal_value, rule_tokens,
    token_at,
};
pub(crate) use references::{
    Candidate, SelectionKind, candidates_at, csv_parameter, database_definition, field_candidates,
    message_key_known, selection_at, selection_value,
};

/// Laravel converts rule names to StudlyCase before a case-insensitive method call.
pub(crate) fn rule_matches(declared: &str, written: &str) -> bool {
    rule_letters(declared).eq(rule_letters(written))
}

fn rule_letters(name: &str) -> impl Iterator<Item = u8> + '_ {
    name.bytes()
        .filter(|byte| !matches!(byte, b'_' | b'-' | b' '))
        .map(|byte| byte.to_ascii_lowercase())
}

/// Match the same rule regardless of snake-case or StudlyCase spelling.
pub(crate) fn rule_prefix_matches(declared: &str, prefix: &str) -> bool {
    let mut declared = rule_letters(declared);
    rule_letters(prefix).all(|letter| declared.next() == Some(letter))
}

/// Reject ordinary strings before walking the syntax tree for validation sites.
pub(crate) fn might_contain_site(
    content: &str,
    cursor: usize,
    ctx: &FileContext,
    code: &crate::completion::source::code_context::CodeContext<'_>,
) -> bool {
    if code.open_string.is_none() {
        return false;
    }
    if code.open_brackets.iter().any(|bracket| {
        bracket.callee_operator.is_some()
            && bracket
                .callee_name_start
                .and_then(|start| content.get(start..bracket.code_before))
                .is_some_and(|name| {
                    ["validate", "validateWithBag", "make", "sometimes"]
                        .iter()
                        .any(|method| name.eq_ignore_ascii_case(method))
                })
    }) {
        return true;
    }
    find_class_at_offset(&ctx.classes, cursor as u32)
        .and_then(|class| {
            class
                .methods
                .iter()
                .filter(|method| method.name_offset <= cursor as u32)
                .max_by_key(|method| method.name_offset)
        })
        .is_some_and(|method| {
            ["rules", "messages", "attributes"]
                .iter()
                .any(|name| method.name.eq_ignore_ascii_case(name))
        })
}

impl Backend {
    /// Installed validation declarations, rebuilt when a contributing file changes.
    pub(crate) fn cached_validation_rules(&self) -> Arc<catalog::RuleCatalog> {
        self.cached_laravel_enumeration(
            &self.laravel.string_key_build_locks.validation_rules,
            |cache| cache.validation_rules.clone(),
            |cache, rules| cache.validation_rules = Some(rules),
            || Arc::new(catalog::discover(self)),
        )
    }
}

/// Confirm a syntactic candidate against the receiver's actual class.
pub(crate) fn confirmed(
    backend: &Backend,
    site: &ValidationSite,
    content: &str,
    ctx: &FileContext,
) -> bool {
    if let Some((class, offset)) = &site.in_rule_class {
        let name = ctx.resolve_name_at(class, *offset);
        if !name.eq_ignore_ascii_case("Illuminate\\Validation\\Rule") {
            return false;
        }
    }
    let loaders = backend.file_loaders(ctx);
    let resolved = loaders.resolved_class_loader();
    let request = |class: &ClassInfo| is_subtype_of(class, super::REQUEST_FQN, resolved);
    let form = |class: &ClassInfo| {
        is_subtype_of(class, super::validation_rules::FORM_REQUEST_FQN, resolved)
            || is_subtype_of(class, "Livewire\\Form", resolved)
    };
    match site.owner.as_ref() {
        ValidationOwner::RulesMethod { offset, .. } => {
            find_class_at_offset(&ctx.classes, *offset).is_some_and(form)
        }
        ValidationOwner::Static { class, offset, .. } => {
            let name = ctx.resolve_name_at(class, *offset);
            resolved(Fqn::new(&name)).is_some_and(|class| {
                is_subtype_of(&class, "Illuminate\\Support\\Facades\\Validator", resolved)
                    || is_subtype_of(
                        &class,
                        "Illuminate\\Contracts\\Validation\\Factory",
                        resolved,
                    )
                    || is_subtype_of(&class, "Illuminate\\Validation\\Factory", resolved)
            })
        }
        ValidationOwner::Method {
            receiver_start,
            receiver_end,
            method,
            rules_argument,
        } => {
            let subject = &content[*receiver_start..*receiver_end];
            let rctx = loaders.resolution_ctx_at(content, *receiver_start as u32);
            resolve_target_classes(subject, AccessKind::Arrow, &rctx)
                .iter()
                .any(|ty| {
                    let Some(class) = ty.class_info.as_deref() else {
                        return false;
                    };
                    if method.eq_ignore_ascii_case("sometimes") {
                        return is_subtype_of(
                            class,
                            "Illuminate\\Contracts\\Validation\\Validator",
                            resolved,
                        ) || is_subtype_of(
                            class,
                            "Illuminate\\Validation\\Validator",
                            resolved,
                        );
                    }
                    let argument = usize::from(method.eq_ignore_ascii_case("validateWithBag"));
                    if request(class)
                        || form(class)
                        || is_subtype_of(class, "Livewire\\Component", resolved)
                    {
                        return rules_argument.is_none_or(|index| index == argument);
                    }
                    let has_validation_trait = |class: &ClassInfo| {
                        class.used_traits.iter().any(|name| {
                            name.as_str() == "Illuminate\\Foundation\\Validation\\ValidatesRequests"
                        })
                    };
                    if has_validation_trait(class) {
                        return rules_argument.is_none_or(|index| index == argument + 1);
                    }
                    let mut current = class
                        .parent_class
                        .and_then(|name| resolved(Fqn::new(&name)));
                    for _ in 0..crate::types::MAX_INHERITANCE_DEPTH {
                        let Some(class) = current else {
                            break;
                        };
                        if has_validation_trait(&class) {
                            return rules_argument.is_none_or(|index| index == argument + 1);
                        }
                        current = class
                            .parent_class
                            .and_then(|name| resolved(Fqn::new(&name)));
                    }
                    false
                })
        }
    }
}

/// A declaration in a file already held by the caller.
fn location(uri: &str, content: &str, start: usize, end: usize) -> Option<Location> {
    Some(Location {
        uri: crate::util::parse_url(uri)?,
        range: tower_lsp_server::gen_lsp_types::Range {
            start: crate::text_position::offset_to_position(content, start),
            end: crate::text_position::offset_to_position(content, end),
        },
    })
}
