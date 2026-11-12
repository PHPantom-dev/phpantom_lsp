//! Drupal form and render callbacks Drupal cannot call (see
//! [`crate::drupal::forms`]).
//!
//! Drupal resolves `'::method'` for the form only in a form callback: in a
//! render callback it names no class and throws.  It calls a
//! `#value_callback` only when it is a native callable, and otherwise
//! ignores it for the element's default value callback, so `'::method'`,
//! `'service:method'` and a non-static method named by its class do
//! nothing there.  An array callable that names its class rather than an
//! object calls its method statically, under every key, so it must name a
//! static method.  The class a `'Class::method'` string names has no class
//! span of its own, so a missing one is reported here; a class name
//! written in code (`X::class . '::method'`) is reported where it is
//! written.  The methods and functions a callback names are checked by
//! the unknown-member and unknown-function checks.

use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::symbol_map::{CallbackKey, CallbackShape, SymbolKind};
use crate::type_engine::resolver::{SubjectOutcome, resolve_subject_outcome};
use crate::types::AccessKind;
use crate::virtual_members::resolve_class_fully_cached;

use super::helpers::{FileDiagnosticContext, make_diagnostic};
use super::unknown_classes::UNKNOWN_CLASS_CODE;

/// Diagnostic code for a callback Drupal cannot resolve or ignores.
pub(crate) const INVALID_DRUPAL_CALLBACK_CODE: &str = "invalid_drupal_callback";

impl Backend {
    /// Report the callbacks the file's form and render arrays name that
    /// Drupal cannot resolve under their key, ignores, or whose class does
    /// not exist.
    pub(crate) fn collect_drupal_callback_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let map = &ctx.symbol_map;
        if map.drupal_callbacks.is_empty() {
            return;
        }
        let Some(source) = map.source(content) else {
            return;
        };
        let loaders = self.file_loaders(&ctx.file);
        let push = |out: &mut Vec<Diagnostic>, start: u32, end: u32, code: &str, message| {
            if let Some(range) =
                self.offset_range_to_lsp_range(uri, content, start as usize, end as usize)
            {
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Warning,
                    code,
                    message,
                ));
            }
        };
        let services = std::cell::OnceCell::new();
        // The method `subject::method` names, written at `at`, when it is
        // not static.
        let instance_method = |subject: &str, method: &str, at: u32| {
            let rctx = loaders.resolution_ctx_at(content, at);
            let SubjectOutcome::Resolved { classes, .. } =
                resolve_subject_outcome(subject, AccessKind::DoubleColon, &rctx)
            else {
                return None;
            };
            classes.iter().find_map(|class| {
                let merged = resolve_class_fully_cached(
                    class,
                    loaders.resolved_class_loader(),
                    &self.resolved_class_cache,
                );
                let found = merged
                    .methods
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(method))?;
                (!found.is_static).then(|| format!("{}::{}()", merged.fqn(), found.name))
            })
        };
        for site in &map.drupal_callbacks {
            if !super::incremental::reports_site(site.start) {
                continue;
            }
            let at = map.spans.partition_point(|span| span.start < site.start);
            let Some((span, subject, method, is_static)) = map.spans[at..]
                .iter()
                .take_while(|span| span.start == site.start)
                .find_map(|span| match &span.kind {
                    SymbolKind::MemberAccess {
                        subject_text,
                        member_name,
                        is_static,
                        ..
                    } => Some((span, subject_text.as_str(source), member_name, *is_static)),
                    _ => None,
                })
            else {
                continue;
            };
            match site.shape {
                CallbackShape::FormMethod if site.key == CallbackKey::Render => push(
                    out,
                    span.start,
                    span.end,
                    INVALID_DRUPAL_CALLBACK_CODE,
                    format!(
                        "Drupal cannot resolve '::{method}' in a render callback, only in a form callback"
                    ),
                ),
                CallbackShape::FormMethod | CallbackShape::ServiceMethod
                    if site.key == CallbackKey::Value =>
                {
                    push(
                        out,
                        span.start,
                        span.end,
                        INVALID_DRUPAL_CALLBACK_CODE,
                        ignored_value_callback(None),
                    );
                }
                CallbackShape::ClassMethod { class }
                    if !self.drupal_callback_names_invokable(subject, &services) =>
                {
                    let (start, end) = class.unwrap_or((span.start, span.end));
                    push(
                        out,
                        start,
                        end,
                        UNKNOWN_CLASS_CODE,
                        format!("Class {} not found", subject.trim_start_matches('\\')),
                    );
                }
                CallbackShape::ClassMethod { .. } | CallbackShape::JoinedMethod
                    if site.key == CallbackKey::Value =>
                {
                    if let Some(instance_method) = instance_method(subject, method, span.start) {
                        push(
                            out,
                            span.start,
                            span.end,
                            INVALID_DRUPAL_CALLBACK_CODE,
                            ignored_value_callback(Some(&instance_method)),
                        );
                    }
                }
                // An array callable that names its class calls the method
                // statically under every key.
                CallbackShape::ArrayCallable if is_static => {
                    if let Some(instance_method) = instance_method(subject, method, span.start) {
                        let message = if site.key == CallbackKey::Value {
                            ignored_value_callback(Some(&instance_method))
                        } else {
                            format!(
                                "Drupal cannot call this callback: {instance_method} is not static"
                            )
                        };
                        push(
                            out,
                            span.start,
                            span.end,
                            INVALID_DRUPAL_CALLBACK_CODE,
                            message,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

/// The message for a value callback Drupal ignores, naming the method when
/// it is ignored for not being static.
fn ignored_value_callback(instance_method: Option<&str>) -> String {
    let reason = match instance_method {
        Some(method) => format!(", and {method} is not static"),
        None => String::new(),
    };
    format!("This value callback is ignored: Drupal calls only a native callable there{reason}")
}
