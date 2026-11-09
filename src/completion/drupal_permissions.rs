//! Completion for Drupal permission names in PHP, where the call or array
//! that reads one is not a plain argument the Symfony completion reads: the
//! permissions `AccessResult::allowedIfHasPermission()` takes second, the
//! lists `allowedIfHasPermissions()` and the user module's
//! `user_role_grant_permissions()` and `user_role_revoke_permissions()`
//! take, a route's `_permission` requirement (each part of `a+b` and `a,b`),
//! and an entity type annotation's `admin_permission = "…"`.
//!
//! An account's or a role's `hasPermission('|')` is a call on a typed
//! receiver, which the Symfony completion confirms (see
//! `symfony_receiver_method`), and an entity type attribute's
//! `admin_permission:` an attribute argument, which it reads too.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::source::code_context::CodeContext;
use crate::completion::symfony_string_keys::{is_instantiation, trailing_name};
use crate::symbol_map::StringKeyKind;
use crate::text_position::position_to_offset;
use crate::types::FileContext;

impl Backend {
    /// Complete the permission the string at the cursor names.
    pub(crate) fn try_drupal_permission_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let start = match code.last_code_byte()? {
            // `'_permission' => '|'`, in a route's requirements array.
            b'>' if names_permission_requirement(code.code_before) => {
                requirement_part_start(content, literal_start, cursor)
            }
            b'(' | b',' | b'[' => {
                self.permission_argument_start(content, cursor, literal_start, code, ctx)?
            }
            _ => return None,
        };
        self.symfony_key_response(
            uri,
            StringKeyKind::DrupalPermission,
            content,
            start,
            cursor,
            position,
        )
    }

    /// Where the permission being typed starts, when the string at the
    /// cursor is an argument, or an element of an array argument, of a call
    /// that reads one.
    fn permission_argument_start(
        &self,
        content: &str,
        cursor: usize,
        literal_start: usize,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<usize> {
        let call = detect_string_call_context(content, cursor, code)?;
        let paren = code.enclosing_paren()?;
        // An array's element, right inside the call's argument.
        let in_list = match &code.open_brackets[..] {
            [.., outer, inner] if inner.byte == b'[' && outer.byte == b'(' => true,
            [.., inner] if inner.byte == b'(' => false,
            _ => return None,
        };
        if call.arg_index != 1 {
            return None;
        }
        let Some(subject) = call.subject.as_deref() else {
            let written = trailing_name(&content[..paren.code_before]);
            let offset = paren.code_before - written.len();
            return (in_list
                && !is_instantiation(&content[..offset])
                && crate::drupal::permissions::is_role_permissions_function(written))
            .then_some(literal_start);
        };
        if call.is_static {
            let class = ctx.resolve_name_at(subject, cursor as u32);
            return (crate::drupal::permissions::static_permission_call(&class, &call.method_name)
                == Some(in_list))
            .then_some(literal_start);
        }
        // `setRequirement('_permission', '|')` on a route.
        (!in_list
            && call.method_name.eq_ignore_ascii_case("setRequirement")
            && first_argument_is_permission_key(content, paren.offset))
        .then(|| requirement_part_start(content, literal_start, cursor))
    }

    /// Complete the permission an entity type annotation's
    /// `admin_permission = "…"` names, in the docblock at the cursor.
    pub(crate) fn try_drupal_annotation_permission_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let line_start = content[..cursor].rfind('\n').map_or(0, |at| at + 1);
        let before = &content[line_start..cursor];
        let at = before.rfind("admin_permission")?;
        let rest = before[at + "admin_permission".len()..]
            .trim_start()
            .strip_prefix('=')?
            .trim_start()
            .strip_prefix('"')?;
        if rest.contains('"') {
            return None;
        }
        let start = cursor - rest.len();
        self.symfony_key_response(
            uri,
            StringKeyKind::DrupalPermission,
            content,
            start,
            cursor,
            position,
        )
    }
}

/// Whether the code before the cursor ends with `'_permission' =>`.
fn names_permission_requirement(code_before: &str) -> bool {
    let Some(key) = code_before.strip_suffix("=>") else {
        return false;
    };
    let key = key.trim_end();
    key.ends_with("'_permission'") || key.ends_with("\"_permission\"")
}

/// Where the part of a `_permission` requirement being typed starts: after
/// the last `,` or `+` typed in the literal.
fn requirement_part_start(content: &str, literal_start: usize, cursor: usize) -> usize {
    content[literal_start..cursor]
        .rfind([',', '+'])
        .map_or(literal_start, |at| literal_start + at + 1)
}

/// Whether the call whose `(` is at `paren` passes `'_permission'` first.
fn first_argument_is_permission_key(content: &str, paren: usize) -> bool {
    let after = content[paren + 1..].trim_start();
    after.starts_with("'_permission'") || after.starts_with("\"_permission\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requirement_key_is_read_before_the_arrow() {
        assert!(names_permission_requirement("['_permission' =>"));
        assert!(names_permission_requirement("[\"_permission\"=>"));
        assert!(!names_permission_requirement("['_access' =>"));
        assert!(!names_permission_requirement("['_permission',"));
    }

    #[test]
    fn a_requirement_part_starts_after_its_separator() {
        let php = "'access content+adm";
        assert_eq!(requirement_part_start(php, 1, php.len()), 16);
        assert_eq!(requirement_part_start(php, 1, 5), 1);
    }
}
