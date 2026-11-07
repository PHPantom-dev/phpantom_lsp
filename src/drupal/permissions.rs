//! Drupal's permission names: the keys an extension's
//! `<extension>.permissions.yml` declares, and the parts of a route's
//! `_permission` requirement.
//!
//! A permissions file's `permission_callbacks` key names the methods that
//! build more permissions at runtime, which are not read: it declares no
//! permission of its own.

use super::routing::{entries, lines, scalar};
use crate::Backend;
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};

/// The key of a permissions file that lists callbacks, not a permission.
const CALLBACKS_KEY: &str = "permission_callbacks";

/// The spans of the permissions a permissions file declares, in source
/// order: each top-level key but [`CALLBACKS_KEY`].
pub(crate) fn permissions_spans(content: &str) -> Vec<SymbolSpan> {
    lines(content)
        .iter()
        .filter(|line| line.indent == 0 && !line.key.is_empty() && line.key != CALLBACKS_KEY)
        .map(|line| SymbolSpan {
            start: line.key_start as u32,
            end: (line.key_start + line.key.len()) as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalPermission,
                key: line.key.to_string(),
                is_write: true,
                is_optional: false,
            },
        })
        .collect()
}

/// The byte ranges of the permissions a `_permission` requirement names,
/// relative to its text: Drupal splits it on `,` when it has one (all of
/// the permissions are needed), else on `+` (any of them grants access),
/// and trims nothing.
pub(crate) fn permission_parts(value: &str) -> Vec<(usize, usize)> {
    let separator = if value.contains(',') { ',' } else { '+' };
    let mut parts = Vec::new();
    let mut start = 0;
    for part in value.split(separator) {
        if !part.is_empty() {
            parts.push((start, start + part.len()));
        }
        start += part.len() + 1;
    }
    parts
}

/// The spans of the permissions a `_permission` requirement whose text
/// starts at `start` names.
pub(crate) fn requirement_spans(value: &str, start: usize) -> Vec<SymbolSpan> {
    permission_parts(value)
        .into_iter()
        .map(|(from, to)| SymbolSpan {
            start: (start + from) as u32,
            end: (start + to) as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalPermission,
                key: value[from..to].to_string(),
                is_write: false,
                is_optional: false,
            },
        })
        .collect()
}

/// Where the permission being typed at `cursor` starts, when `cursor` is
/// in the value of a `_permission:` key of a routing file: after the
/// value's opening quote, or after the last `+` or `,` before the cursor.
pub(crate) fn routing_permission_start(content: &str, cursor: usize) -> Option<usize> {
    let line_start = content[..cursor].rfind('\n').map_or(0, |at| at + 1);
    let before = &content[line_start..cursor];
    let indent = crate::framework::leading_spaces(before);
    let rest = before[indent..].strip_prefix("_permission")?;
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix(':')?;
    let value = rest.trim_start_matches([' ', '\t']);
    let value = value.strip_prefix(['\'', '"']).unwrap_or(value);
    if value.contains(['\'', '"', '#']) {
        return None;
    }
    let typed = value.rfind([',', '+']).map_or(0, |at| at + 1);
    Some(cursor - value.len() + typed)
}

/// Whether a static call of `method` on `class`, a resolved class name,
/// checks permissions in its second argument: `Some(false)` for
/// `AccessResult::allowedIfHasPermission()`, which takes one, and
/// `Some(true)` for `allowedIfHasPermissions()`, which takes a list.
pub(crate) fn static_permission_call(class: &str, method: &str) -> Option<bool> {
    if !class
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Drupal\\Core\\Access\\AccessResult")
    {
        return None;
    }
    if method.eq_ignore_ascii_case("allowedIfHasPermission") {
        Some(false)
    } else if method.eq_ignore_ascii_case("allowedIfHasPermissions") {
        Some(true)
    } else {
        None
    }
}

/// Whether a function written as `written` is the user module's
/// `user_role_grant_permissions()` or `user_role_revoke_permissions()`,
/// which take a list of permissions second.  A name written with a
/// namespace is another function.
pub(crate) fn is_role_permissions_function(written: &str) -> bool {
    let name = written.strip_prefix('\\').unwrap_or(written);
    name.eq_ignore_ascii_case("user_role_grant_permissions")
        || name.eq_ignore_ascii_case("user_role_revoke_permissions")
}

/// What a permissions file says about one permission.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PermissionDetails {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Whether the permission is marked `restrict access: true`: one that
    /// should only go to trusted roles.
    pub restrict_access: bool,
}

/// What the permissions file `content` says about the permission `name`.
pub(crate) fn permission_details(content: &str, name: &str) -> Option<PermissionDetails> {
    let lines = lines(content);
    let (_, block) = entries(&lines)
        .into_iter()
        .find(|(entry, _)| entry.indent == 0 && entry.key == name)?;
    let mut details = PermissionDetails::default();
    for (setting, nested) in entries(block) {
        let (value, _) = scalar(setting.value, setting.value_start);
        // A block scalar (`>` or `|`) is written on the lines below its
        // key, which hold no `key:` of their own once folded: read them
        // from the text.
        let value = if matches!(value.trim(), ">" | "|" | ">-" | "|-") {
            block_scalar(content, setting.value_start)
        } else if nested.is_empty() {
            value
        } else {
            continue;
        };
        match setting.key {
            "title" => details.title = Some(value),
            "description" => details.description = Some(value),
            "restrict access" => details.restrict_access = value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    Some(details)
}

/// The lines of a block scalar whose indicator ends the line that holds
/// `at`, folded into one.
fn block_scalar(content: &str, at: usize) -> String {
    let line_end = content[at..]
        .find('\n')
        .map_or(content.len(), |end| at + end);
    let key_indent = {
        let line_start = content[..at].rfind('\n').map_or(0, |start| start + 1);
        crate::framework::leading_spaces(&content[line_start..])
    };
    let mut folded = String::new();
    for line in content[line_end..].lines().skip(1) {
        if !line.trim().is_empty() && crate::framework::leading_spaces(line) <= key_indent {
            break;
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        if !folded.is_empty() {
            folded.push(' ');
        }
        folded.push_str(text);
    }
    folded
}

impl Backend {
    /// The hover text of the permission `name`: its title, its
    /// description, whether it is restricted, and the file declaring it.
    pub(crate) fn drupal_permission_detail(&self, name: &str) -> String {
        let mut declared: Option<String> = None;
        self.visit_symfony_declarations(StringKeyKind::DrupalPermission, |uri, _, _, _, key| {
            if declared.is_none() && key == name {
                declared = Some(uri.to_string());
            }
        });
        let Some(uri) = declared else {
            return "Not declared in the project's permissions files".to_string();
        };
        let mut parts = Vec::new();
        if let Some(details) = self
            .get_file_content_arc(&uri)
            .and_then(|content| permission_details(&content, name))
        {
            if let Some(title) = details.title.filter(|title| !title.is_empty()) {
                parts.push(format!("**{title}**"));
            }
            if let Some(description) = details.description.filter(|text| !text.is_empty()) {
                parts.push(description);
            }
            if details.restrict_access {
                parts.push("Restricted: give it to trusted roles only".to_string());
            }
        }
        if let Some(path) = self.workspace_relative_path(&uri) {
            parts.push(format!("Declared in `{path}`"));
        }
        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(yaml: &str) -> Vec<(&str, String)> {
        permissions_spans(yaml)
            .iter()
            .map(|span| {
                let SymbolKind::StringKey { key, is_write, .. } = &span.kind else {
                    unreachable!()
                };
                assert!(is_write);
                (&yaml[span.start as usize..span.end as usize], key.clone())
            })
            .collect()
    }

    #[test]
    fn top_level_keys_declare_permissions_but_the_callbacks() {
        let yaml = "administer acme:\n  title: 'Administer acme'\n  restrict access: true\n'view acme notes':\n  title: View notes\n# a comment: here\npermission_callbacks:\n  - \\Drupal\\acme\\AcmePermissions::permissions\n";
        assert_eq!(
            declared(yaml),
            [
                ("administer acme", "administer acme".to_string()),
                ("view acme notes", "view acme notes".to_string()),
            ]
        );
    }

    #[test]
    fn requirements_split_the_way_drupal_splits_them() {
        let parts = |value: &'static str| {
            permission_parts(value)
                .into_iter()
                .map(|(from, to)| &value[from..to])
                .collect::<Vec<_>>()
        };
        assert_eq!(parts("access content"), ["access content"]);
        assert_eq!(parts("a+b"), ["a", "b"]);
        assert_eq!(parts("a,b+c"), ["a", "b+c"]);
        assert_eq!(parts("a,,b"), ["a", "b"]);
        assert!(parts("").is_empty());
    }

    #[test]
    fn a_routing_permission_is_typed_after_its_last_separator() {
        let yaml = "a:\n  requirements:\n    _permission: 'access content+adm";
        let start = routing_permission_start(yaml, yaml.len()).unwrap();
        assert_eq!(&yaml[start..], "adm");
        let yaml = "a:\n  requirements:\n    _permission: acc";
        let start = routing_permission_start(yaml, yaml.len()).unwrap();
        assert_eq!(&yaml[start..], "acc");
        let yaml = "a:\n  requirements:\n    _permission: ";
        assert_eq!(routing_permission_start(yaml, yaml.len()), Some(yaml.len()));
        let yaml = "a:\n  requirements:\n    _access: 'TRUE";
        assert_eq!(routing_permission_start(yaml, yaml.len()), None);
        let yaml = "a:\n  requirements:\n    _permission: 'done' ";
        assert_eq!(routing_permission_start(yaml, yaml.len()), None);
    }

    #[test]
    fn details_read_the_title_description_and_restriction() {
        let yaml = "administer acme:\n  title: 'Administer acme'\n  description: >\n    Change every acme\n    setting.\n  restrict access: true\nview acme:\n  title: View acme\n";
        assert_eq!(
            permission_details(yaml, "administer acme"),
            Some(PermissionDetails {
                title: Some("Administer acme".to_string()),
                description: Some("Change every acme setting.".to_string()),
                restrict_access: true,
            })
        );
        assert_eq!(
            permission_details(yaml, "view acme"),
            Some(PermissionDetails {
                title: Some("View acme".to_string()),
                description: None,
                restrict_access: false,
            })
        );
        assert_eq!(permission_details(yaml, "missing"), None);
    }
}
