//! The attributes a Yii Active Record call names (see
//! [`crate::yii::active_record`]).
//!
//! `$post->get('titel')` reads an attribute the record does not have, which
//! `get()` answers with `null` and `set()` throws for.  A record's
//! attributes are its properties, real or `@property` ones; a record that
//! declares none reads them from its table's columns, which are checked
//! when the project's migrations create the table and say all its columns.
//! Otherwise every name passes.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::diagnostics::helpers::make_diagnostic;
use crate::types::ClassInfo;
use crate::yii::active_record::{attribute_names, is_record};

use super::access::MemberAccessSite;
use super::{INVALID_YII_ATTRIBUTE_CODE, UnknownMemberPass};

impl UnknownMemberPass<'_> {
    /// Report the attribute `access` names when a record among `classes`
    /// that declares its attributes does not have it.  On anything that is
    /// not a record the string is an ordinary argument.
    pub(super) fn report_unknown_yii_attribute(
        &self,
        access: &MemberAccessSite,
        classes: &[Arc<ClassInfo>],
        out: &mut Vec<Diagnostic>,
    ) {
        let loader = self.class_loader();
        let name = access.member_name;
        let Some(message) = classes.iter().find_map(|class| {
            if !is_record(class, loader) {
                return None;
            }
            let attributes = attribute_names(class, loader);
            if !attributes.is_empty() {
                return (!attributes.iter().any(|(known, _)| *known == name)).then(|| {
                    format!(
                        "Attribute '{name}' is not a property of record {}",
                        class.fqn()
                    )
                });
            }
            let columns = self.backend.yii_record_columns(class, loader)?;
            (columns.all_known() && columns.column(name).is_none()).then(|| {
                format!(
                    "Attribute '{name}' is not a column of table '{}' of record {}",
                    columns.table_name(),
                    class.fqn()
                )
            })
        }) else {
            return;
        };
        let Some(range) = self.range(access) else {
            return;
        };
        out.push(make_diagnostic(
            range,
            DiagnosticSeverity::Warning,
            INVALID_YII_ATTRIBUTE_CODE,
            message,
        ));
    }
}
