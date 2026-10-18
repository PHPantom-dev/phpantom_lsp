//! Function hover: the signature and docblock of a called function, and
//! the member an unqualified `@see name()` names on the documented class.

use tower_lsp::lsp_types::Hover;

use crate::Backend;
use crate::definition::member::MemberKind;
use crate::types::FunctionInfo;

use super::HoverSite;
use super::formatting::hover_for_function;
use super::member::HoverMemberHit;

impl Backend {
    /// Hover for a function call or a `@see name()` reference.
    pub(super) fn hover_function_symbol(
        &self,
        site: &HoverSite,
        symbol_start: u32,
        name: &str,
        is_docblock_reference: bool,
    ) -> Option<Hover> {
        let HoverSite {
            uri,
            content,
            offset: cursor_offset,
            ctx,
            loaders,
        } = *site;
        let resolved_class_loader = loaders.resolved_class_loader();
        let function_loader = loaders.function_loader_at(cursor_offset);
        // An unqualified `@see name()` describes the documented
        // class's own member first, as phpDocumentor reads it.
        if is_docblock_reference
            && let Some((class, kind)) = self.docblock_scope_member(ctx, symbol_start, name)
            && let Some(hit) = Self::find_member_for_hover(&class, name, kind == MemberKind::Method)
        {
            return Some(match hit {
                HoverMemberHit::Method(m) => {
                    self.hover_for_method(&m, &class, &resolved_class_loader, uri, content)
                }
                HoverMemberHit::Property(p) => {
                    self.hover_for_property(&p, &class, &resolved_class_loader)
                }
                HoverMemberHit::Constant(c) => {
                    self.hover_for_constant(&c, &class, &resolved_class_loader)
                }
            });
        }

        self.hover_function_call(name, uri, content, function_loader, symbol_start)
    }

    /// Produce hover information for a function call.
    fn hover_function_call(
        &self,
        name: &str,
        uri: &str,
        content: &str,
        function_loader: &dyn Fn(&str, u32) -> Option<FunctionInfo>,
        offset: u32,
    ) -> Option<Hover> {
        if let Some(mut func) = function_loader(name, offset) {
            let is_configured_date_helper = matches!(
                name.trim_start_matches('\\').rsplit('\\').next(),
                Some("now" | "today")
            );
            // Only when the configured date class actually resolves do we
            // override the declared return type; the `(inferred)` annotation
            // must reflect that override, not merely the helper's name (a
            // user-defined `now()` in a non-Laravel project keeps its own
            // return type unannotated).
            let mut inferred_date_return = false;
            if is_configured_date_helper
                && let Some(date_class) = self
                    .find_or_load_class(crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN)
            {
                let date_type = crate::php_type::PhpType::named(date_class.fqn());
                func.return_type = Some(date_type);
                inferred_date_return = true;
            }
            let resolved_see = self.resolve_see_refs(&func.see_refs, uri, content);
            let provenance = self.provenance_line_for_function(name);
            Some(hover_for_function(
                &func,
                Some(&resolved_see),
                provenance,
                inferred_date_return,
            ))
        } else {
            None
        }
    }
}
