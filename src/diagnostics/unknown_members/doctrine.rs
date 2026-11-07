//! Doctrine's repository `__call` accepts a closed set of names.
//!
//! `EntityRepository::__call()` answers `findBy<Field>()`,
//! `findOneBy<Field>()`, and `countBy<Field>()` for the fields its entity
//! maps, and throws for anything else.  Once the entity's mapping is known
//! the resolved repository carries each of those finders (see
//! [`crate::doctrine`]), so a name the member lookup did not find is one the
//! repository rejects.  A repository that overrides `__call` itself, or
//! whose entity's mapping is not known, stays silent.
//!
//! The field names `findBy()`, `findOneBy()`, and `count()` take as keys
//! are checked against the same mapping.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::diagnostics::helpers::make_diagnostic;
use crate::types::ClassInfo;

use super::access::MemberAccessSite;
use super::{UNKNOWN_ENTITY_FIELD_CODE, UnknownMemberPass};

impl UnknownMemberPass<'_> {
    /// Whether every class among `classes` whose magic handler would take
    /// the call is a Doctrine repository that rejects it.
    ///
    /// `base[i]` is the class as the receiver resolved, which carries the
    /// entity a type argument binds (`EntityRepository<User>`), and
    /// `classes[i]` its merged form, which carries the one its own
    /// `@extends` chain binds.
    pub(super) fn doctrine_rejects_call(
        &self,
        base: &[Arc<ClassInfo>],
        classes: &[Arc<ClassInfo>],
        has_magic_call: impl Fn(usize) -> bool,
    ) -> bool {
        let loader = self.class_loader();
        let mut any = false;
        for (i, class) in classes.iter().enumerate() {
            if !has_magic_call(i) {
                continue;
            }
            let rejects = crate::doctrine::magic_finder_rejects(class)
                || base
                    .get(i)
                    .is_some_and(|b| crate::doctrine::magic_finder_rejects(b));
            if !rejects || !crate::doctrine::has_magic_finder_call(class, loader) {
                return false;
            }
            any = true;
        }
        any
    }

    /// Report the field `access` names when an entity the repositories
    /// among `classes` manage does not map it.
    pub(super) fn report_unmapped_field(
        &self,
        access: &MemberAccessSite,
        classes: &[Arc<ClassInfo>],
        out: &mut Vec<Diagnostic>,
    ) {
        let loader = self.class_loader();
        let state = self.backend.resolved_class_cache.read().doctrine().clone();
        let entities = crate::doctrine::managed_entity_classes(
            classes.to_vec(),
            loader,
            &self.backend.resolved_class_cache,
        );
        let Some(entity) = entities.iter().find(|entity| {
            crate::doctrine::entity_fields(entity, loader, &state)
                .is_some_and(|fields| !fields.iter().any(|f| f == access.member_name))
        }) else {
            return;
        };
        let Some(range) = self.range(access) else {
            return;
        };
        out.push(make_diagnostic(
            range,
            DiagnosticSeverity::Warning,
            UNKNOWN_ENTITY_FIELD_CODE,
            format!(
                "Field '{}' is not mapped on entity {}",
                access.member_name,
                entity.fqn()
            ),
        ));
    }
}
