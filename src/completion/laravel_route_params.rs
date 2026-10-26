//! Completion for route parameter names.
//!
//! The parameters array of `route('users.show', ['|' => 1])` is keyed by the
//! URI parameters of the named route, so the URI recorded alongside each route
//! name (`/users/{user}`) drives array-key completion.  The same applies to
//! `to_route()`, `URL::signedRoute()`, and `URL::temporarySignedRoute()`.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::helpers::enclosing_array_key_call;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::virtual_members::laravel::route_uri_parameters;

/// The route name whose parameters the cursor is completing, and the key text
/// typed so far.
struct RouteParamContext {
    /// Name of the route the parameters array belongs to.
    route_name: String,
    /// The partial key already typed inside the quotes.
    prefix: String,
    /// Byte offset just after the opening quote of the key being typed.
    content_start_offset: usize,
}

/// Detect the cursor inside a key of a route parameters array.
fn detect_context(
    content: &str,
    cursor_offset: usize,
    code: &CodeContext<'_>,
) -> Option<RouteParamContext> {
    let (quote_pos, _) = code.open_string?;

    // The character before the key is `[` for the first key and `,` for the
    // ones after it.
    if !matches!(code.last_code_byte(), Some(b'[') | Some(b',')) {
        return None;
    }

    let call = enclosing_array_key_call(content, code)?;
    let receiver_is_call = call.before_callee.ends_with("::") || call.before_callee.ends_with("->");
    let names_a_route = match call.callee.as_str() {
        // `signedRoute()` / `temporarySignedRoute()` only exist on the `URL`
        // facade and the URL generator, so the name alone identifies them.
        "signedroute" | "temporarysignedroute" => true,
        // `route()` is the helper as well as a method on the `URL`, `Redirect`
        // and `Response` facades and on the `redirect()`/`url()` helpers, all
        // of which take the same `(name, parameters)` pair.
        "route" => true,
        // `to_route()` is a helper function only.
        "to_route" => !receiver_is_call,
        _ => false,
    };
    if !names_a_route {
        return None;
    }

    Some(RouteParamContext {
        route_name: call.name,
        prefix: content[quote_pos + 1..cursor_offset].to_string(),
        content_start_offset: quote_pos + 1,
    })
}

impl Backend {
    /// Try completing a route parameter name.
    ///
    /// Returns `None` when the cursor is not inside the parameters array of a
    /// route-URL call, or when the route's URI takes no parameters.
    pub(crate) fn try_route_param_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor_offset = position_to_offset(content, position) as usize;
        let ctx = detect_context(content, cursor_offset, code)?;

        let discovery = self.cached_routes();
        let route = discovery
            .routes
            .iter()
            .find(|route| route.name == ctx.route_name)?;
        let params = route_uri_parameters(&route.uri);
        if params.is_empty() {
            return None;
        }

        let edit_range = Range {
            start: offset_to_position(content, ctx.content_start_offset),
            end: position,
        };
        let prefix_lower = ctx.prefix.to_lowercase();

        let items: Vec<CompletionItem> = params
            .into_iter()
            .filter(|name| {
                prefix_lower.is_empty() || name.to_lowercase().starts_with(&prefix_lower)
            })
            .enumerate()
            .map(|(i, name)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::Field),
                detail: Some(route.uri.clone()),
                sort_text: Some(format!("{:05}", i)),
                filter_text: Some(name.to_string()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: name.to_string(),
                })),
                ..Default::default()
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::CompletionItemList(items))
        }
    }
}

#[cfg(test)]
#[path = "laravel_route_params_tests.rs"]
mod tests;
