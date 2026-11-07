//! Options a Symfony form type does not accept.
//!
//! An options array passed to a form builder's `add()`, a form factory's
//! `create()` family, or a controller's `createForm()` is checked against
//! the options of the type the call names (see
//! [`crate::symfony::form_options`]).  The form component throws on an
//! option the type does not define, so an unknown key is an error.  A type
//! whose options could not all be read is not judged.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::symfony::form_options::{FormOptions, options_sites};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code used for an option a form type does not accept.
pub(crate) const UNKNOWN_FORM_OPTION_CODE: &str = "invalid_symfony_form_option";

impl Backend {
    /// Report each key of a form type's options array that names no option
    /// of the type's.
    pub(crate) fn collect_form_option_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !crate::symfony::form_options::may_pass_form_options(content) {
            return;
        }
        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);
        let resolve = |name: &str, offset: u32| ctx.file.resolve_name_at(name, offset);
        with_parsed_program(content, "symfony_form_options", |program, content| {
            let mut options_of: HashMap<String, Arc<FormOptions>> = HashMap::new();
            for site in options_sites(program) {
                let keys: Vec<_> = site
                    .strings
                    .iter()
                    .filter(|string| {
                        string.is_key && super::incremental::reports_site(string.start)
                    })
                    .collect();
                if keys.is_empty() {
                    continue;
                }
                let Some(form_type) = self.confirmed_form_type(&site, content, &loaders, &resolve)
                else {
                    continue;
                };
                let options =
                    Arc::clone(options_of.entry(form_type.clone()).or_insert_with(|| {
                        Arc::new(
                            self.form_type_options(&form_type, loaders.resolved_class_loader()),
                        )
                    }));
                if !options.complete {
                    continue;
                }
                for key in keys {
                    let Some(name) = content.get(key.start as usize..key.end as usize) else {
                        continue;
                    };
                    let is_plain = !name.is_empty()
                        && name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
                    if !is_plain || options.accepts(name) {
                        continue;
                    }
                    let Some(range) = self.offset_range_to_lsp_range(
                        uri,
                        content,
                        key.start as usize,
                        key.end as usize,
                    ) else {
                        continue;
                    };
                    let class = form_type.trim_start_matches('\\');
                    out.push(make_diagnostic(
                        range,
                        DiagnosticSeverity::Error,
                        UNKNOWN_FORM_OPTION_CODE,
                        format!("Unknown form option '{name}' for {class}"),
                    ));
                }
            }
        });
    }
}
