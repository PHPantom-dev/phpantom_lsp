//! Template-parameter hover helpers.
//!
//! When the cursor lands on a type that is a `@template` parameter (on a
//! method or its owning class), these helpers render the parameter's
//! variance and bound (e.g. `**template-covariant** `TNode` of
//! `AstNode``) so the reader sees the constraint.

use tower_lsp::lsp_types::Hover;

use crate::Backend;
use crate::docblock::extract_template_params_full;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo};

use super::formatting::make_hover;

impl Backend {
    /// Build a template-info line for a type string that might be a
    /// template parameter.  Returns `None` when the type is not a
    /// template param in scope.
    ///
    /// For example, `"TNode"` at a cursor inside a class with
    /// `@template-covariant TNode of AstNode` returns
    /// `Some("**template-covariant** \`TNode\` of \`AstNode\`")`.
    pub(super) fn find_template_info_for_type(
        &self,
        ty: &PhpType,
        uri: &str,
        cursor_offset: u32,
    ) -> Option<String> {
        // Only bare named types can be template params.
        let name = match ty {
            PhpType::Named(n) if is_bare_identifier(n) => n.as_str(),
            _ => return None,
        };

        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        let def = map.find_template_def(name, cursor_offset)?;

        let bound_display = if let Some(ref bound) = def.bound {
            format!(" of `{}`", bound.shorten())
        } else {
            String::new()
        };

        Some(format!(
            "**{}** `{}`{}",
            def.variance.tag_name(),
            def.name,
            bound_display
        ))
    }

    /// Check whether `name` is a `@template` parameter in scope at
    /// `cursor_offset` and, if so, produce a hover showing the template
    /// name and its upper bound.
    pub(super) fn find_template_def_for_hover(
        &self,
        uri: &str,
        name: &str,
        cursor_offset: u32,
    ) -> Option<Hover> {
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        let def = map.find_template_def(name, cursor_offset)?;

        let bound_display = if let Some(ref bound) = def.bound {
            format!(" of `{}`", bound)
        } else {
            String::new()
        };

        Some(make_hover(format!(
            "**{}** `{}`{}",
            def.variance.tag_name(),
            def.name,
            bound_display
        )))
    }
}

/// Check whether `type_str` is a `@template` parameter declared on
/// the method's own docblock or the owning class's docblock.  Method-level
/// templates take priority.  Returns a formatted info line like
/// `"**template** \`T\` of \`Model\`"`, or `None` when the type is
/// not a template param in either scope.
pub(super) fn find_template_info_in_method_or_class(
    ty: &PhpType,
    method: &MethodInfo,
    owner: &ClassInfo,
) -> Option<String> {
    if let Some(line) = find_template_info_in_method(ty, method) {
        return Some(line);
    }
    find_template_info_in_class(ty, owner)
}

/// Check whether `type_str` is a `@template` parameter declared on
/// the method's own docblock.  Returns a formatted info line like
/// `"**template** \`T\` of \`Model\`"`, or `None` when the type is
/// not a method-level template param.
fn find_template_info_in_method(ty: &PhpType, method: &MethodInfo) -> Option<String> {
    let name = match ty {
        PhpType::Named(n) => n.as_str(),
        _ => return None,
    };

    // Method-level template_params stores just the names.
    if !method.template_params.iter().any(|p| p == name) {
        return None;
    }

    let bound_display = method
        .template_param_bounds
        .get(&crate::atom::atom(name))
        .map(|b| format!(" of `{}`", b.shorten()))
        .unwrap_or_default();

    // Method-level templates don't carry variance info (always invariant).
    Some(format!("**template** `{}`{}", name, bound_display))
}

/// Check whether `type_str` is a `@template` parameter declared on
/// `owner`'s class docblock.  Returns a formatted info line like
/// `"**template-covariant** \`TNode\` of \`AstNode\`"`, or `None`
/// when the type is not a template param on the class.
pub(super) fn find_template_info_in_class(ty: &PhpType, owner: &ClassInfo) -> Option<String> {
    let name = match ty {
        PhpType::Named(n) => n.as_str(),
        _ => return None,
    };

    let docblock = owner.class_docblock.as_deref()?;
    let tpl = extract_template_params_full(docblock)
        .into_iter()
        .find(|(tpl_name, _, _, _)| tpl_name == name)?;

    let (tpl_name, bound, variance, default) = tpl;
    let bound_display = bound
        .map(|b| format!(" of `{}`", b.shorten()))
        .unwrap_or_default();
    let default_display = default.map(|d| format!(" = `{}`", d)).unwrap_or_default();

    Some(format!(
        "**{}** `{}`{}{}",
        variance.tag_name(),
        tpl_name,
        bound_display,
        default_display
    ))
}

/// Returns `true` when `s` is a simple, unqualified identifier (no
/// namespace separator).  The caller guarantees that `s` came from a
/// [`PhpType::Named`] match, so we only need to check for `\`.
fn is_bare_identifier(s: &str) -> bool {
    !s.is_empty() && !s.contains('\\')
}
