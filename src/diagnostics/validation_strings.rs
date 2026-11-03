//! Unknown validation rules and form-request message/attribute keys.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::virtual_members::laravel::validation_strings as validation;

use super::helpers::{FileDiagnosticContext, make_diagnostic};

impl Backend {
    /// Judge validation names only after confirming their Laravel receiver.
    pub(super) fn collect_validation_string_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let sites = validation::collect(content);
        if sites.is_empty() {
            return;
        }
        let mut catalog = None;
        let mut previous_owner = None;
        let mut previous_fields = None;
        for site in sites
            .iter()
            .filter(|site| super::incremental::reports_site(site.start as u32))
        {
            if site.kind == validation::SiteKind::InValue {
                continue;
            }
            let owner = std::sync::Arc::as_ptr(&site.owner);
            let confirmed = match previous_owner {
                Some((previous, confirmed)) if previous == owner => confirmed,
                _ => {
                    let confirmed = validation::confirmed(self, site, content, &ctx.file);
                    previous_owner = Some((owner, confirmed));
                    confirmed
                }
            };
            if !confirmed {
                continue;
            }
            let catalog = catalog.get_or_insert_with(|| self.cached_validation_rules());
            match site.kind {
                validation::SiteKind::Rule if catalog.complete => {
                    for token in validation::rule_tokens(site, content) {
                        if let Some(name) =
                            validation::literal_value(site, content, token.start, token.name_end)
                            && !name.is_empty()
                            && !catalog
                                .rules
                                .iter()
                                .any(|rule| validation::rule_matches(&rule.name, &name))
                            && let Some(range) = self.offset_range_to_lsp_range(
                                uri,
                                content,
                                token.start,
                                token.name_end,
                            )
                        {
                            out.push(make_diagnostic(
                                range,
                                DiagnosticSeverity::Warning,
                                "invalid_validation_rule",
                                format!("Unknown validation rule '{name}'."),
                            ));
                        }
                    }
                }
                validation::SiteKind::MessageKey | validation::SiteKind::AttributeKey => {
                    if previous_fields
                        .as_ref()
                        .is_some_and(|(previous, _)| *previous != owner)
                    {
                        previous_fields = None;
                    }
                    let (_, (fields, complete)) = previous_fields.get_or_insert_with(|| {
                        (
                            owner,
                            validation::field_candidates(self, uri, content, &ctx.file, site),
                        )
                    });
                    if *complete
                        && let Some(key) =
                            validation::literal_value(site, content, site.start, site.end)
                        && !validation::message_key_known(&key, site.kind, fields, catalog)
                        && let Some(range) =
                            self.offset_range_to_lsp_range(uri, content, site.start, site.end)
                    {
                        out.push(make_diagnostic(
                            range,
                            DiagnosticSeverity::Warning,
                            "invalid_validation_message",
                            format!("Unknown validation field or rule in '{key}'."),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}
