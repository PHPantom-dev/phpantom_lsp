//! The named-argument labels that reference a parameter or the property it
//! promotes.
//!
//! A label — `name:` in `new SelectField(name: 1)` — is a reference to a
//! parameter written at the call site rather than an access, so it is not
//! found by the searches that resolve a receiver. Both label searches here
//! resolve the same thing instead: the callee each label's call spells, and
//! the parameter that callee binds the label to (see
//! [`named_arguments`](crate::type_engine::call_resolution)).
//!
//! Two labels name the same parameter when their declarations land on the
//! same bytes, which is what makes the comparison exact across inheritance
//! and across overrides.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::Location;

use super::member_scope::MemberScope;
use super::{CandidateFile, member_candidate_keys};
use crate::Backend;
use crate::atom::Atom;
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::type_engine::call_resolution::NamedArgumentDeclaration;

impl Backend {
    /// The labels across the workspace that bind the parameter whose `$name`
    /// token is `declaration_token` in `declaration_uri`, a parameter of the
    /// function-like called `function`.
    ///
    /// A parameter's own uses are scoped to its function, which the variable
    /// search answers; this is the other half of what a rename of a
    /// parameter has to reach — the call sites that pass it a value by name.
    /// Only a call whose callee text names `function` can bind it, so the
    /// labels of every other call are left unresolved.
    pub(crate) fn find_parameter_label_references(
        &self,
        name: &str,
        function: &str,
        declaration_uri: &str,
        declaration_token: (u32, u32),
    ) -> Vec<Location> {
        self.scan_label_candidates(
            name,
            |call_expression| callee_may_be(call_expression, function),
            move |declaration| {
                declaration.name_token == Some(declaration_token)
                    && &*declaration.uri == declaration_uri
            },
        )
    }

    /// Push the labels in `file` that bind a promoted property of the searched
    /// member.
    ///
    /// A promoted property is declared by its constructor parameter, so the
    /// label that fills it is a reference to the property just as `$field`
    /// is; which class a label's `new` names is read off the call, and only a
    /// class inside the searched hierarchy counts. Without a hierarchy the
    /// search takes any receiver of the right name, as the member-access
    /// fallback does.
    pub(super) fn push_property_label_matches(
        &self,
        file: &CandidateFile<'_>,
        symbol_map: &Arc<SymbolMap>,
        member: &Atom,
        hierarchy: Option<&MemberScope>,
        locations: &mut Vec<Location>,
    ) {
        // Only a call that constructs an object binds a constructor
        // parameter, so the other labels of the name are not resolved.
        let constructs = |call_expression: &str| call_expression.starts_with("new ");
        self.for_each_label_match(
            file,
            symbol_map,
            member,
            constructs,
            locations,
            |declaration| {
                declaration.promotes_property
                    && match hierarchy {
                        Some(hierarchy) => declaration
                            .class
                            .is_some_and(|class| hierarchy.contains(self, &class)),
                        None => true,
                    }
            },
        );
    }

    /// The batch counterpart: each property query of `queries` gets the labels
    /// that bind its hierarchy's promoted property, attributed to that query.
    pub(super) fn label_matches_for_queries<'q>(
        &self,
        file: &CandidateFile<'_>,
        symbol_map: &Arc<SymbolMap>,
        queries: impl Iterator<Item = (usize, Atom, Option<&'q MemberScope>)>,
    ) -> Vec<(usize, Location)> {
        let mut matches = Vec::new();
        for (query_index, member, hierarchy) in queries {
            let mut found = Vec::new();
            self.push_property_label_matches(file, symbol_map, &member, hierarchy, &mut found);
            matches.extend(found.into_iter().map(|location| (query_index, location)));
        }
        matches
    }

    /// Every label written `name`, across the workspace, at a call whose
    /// callee text `admits_call` accepts, that `keep` accepts.
    fn scan_label_candidates(
        &self,
        name: &str,
        admits_call: impl Fn(&str) -> bool + Sync,
        keep: impl Fn(&NamedArgumentDeclaration) -> bool + Sync,
    ) -> Vec<Location> {
        let member = crate::atom::atom(name);
        self.scan_reference_candidates(
            &member_candidate_keys(name, false, None),
            "Scanning for named-argument references",
            |file, symbol_map, locations| {
                self.for_each_label_match(
                    file,
                    symbol_map,
                    &member,
                    &admits_call,
                    locations,
                    &keep,
                );
            },
        )
    }

    /// The labels in one file that bind a parameter `keep` accepts.
    ///
    /// Only files the reference index already picked as candidates are read,
    /// and each label's callee is resolved once: a label whose call does not
    /// resolve (a call through a callable value, a declaration in a stub)
    /// names nothing the engine can prove and is left out.  A label at a
    /// call whose callee text `admits_call` rejects is skipped before its
    /// callee is resolved, since resolving one can take a walk of the
    /// enclosing function.
    fn for_each_label_match(
        &self,
        file: &CandidateFile<'_>,
        symbol_map: &Arc<SymbolMap>,
        name: &Atom,
        admits_call: impl Fn(&str) -> bool,
        locations: &mut Vec<Location>,
        keep: impl Fn(&NamedArgumentDeclaration) -> bool,
    ) {
        let indices = symbol_map.named_argument_indices(name.as_str());
        if indices.is_empty() {
            return;
        }
        let Some(content) = file.content() else {
            return;
        };
        for &index in indices {
            let span = &symbol_map.spans[index];
            let SymbolKind::NamedArgument {
                call_args_start, ..
            } = &span.kind
            else {
                continue;
            };
            if !symbol_map
                .call_sites
                .site_at(*call_args_start)
                .is_some_and(|site| admits_call(site.call_expression))
            {
                continue;
            }
            let Some(declaration) =
                self.named_argument_location_at(file.uri(), content, *call_args_start, name)
            else {
                continue;
            };
            if !keep(&declaration) {
                continue;
            }
            if let Some(location) = file.location(span.start, span.end) {
                locations.push(location);
            }
        }
    }
}

/// Whether a call spelled `call_expression` can invoke a function-like named
/// `function`: a constructor through `new` or `parent::__construct()`, and
/// anything else through the last segment of its callee, the method of a
/// member call or the short name of a function.  A call of a callable value
/// names no declaration, so it can bind no declared parameter either.
fn callee_may_be(call_expression: &str, function: &str) -> bool {
    let last = call_expression
        .rsplit(['>', ':', '\\'])
        .next()
        .unwrap_or(call_expression);
    if function.eq_ignore_ascii_case("__construct") {
        return call_expression.starts_with("new ") || last.eq_ignore_ascii_case(function);
    }
    !call_expression.starts_with("new ") && last.eq_ignore_ascii_case(function)
}
