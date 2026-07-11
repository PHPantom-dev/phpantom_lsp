//! Config-driven diagnostic suppression via `[[diagnostics.ignore]]`.
//!
//! This is the `.phpantom.toml` analogue of PHPStan's `ignoreErrors`:
//! rules constrain by message (regex), path (glob, relative to the
//! workspace root), and/or diagnostic identifier. A diagnostic is
//! suppressed when it matches every constraint present on a rule.
//!
//! Unlike `@phpantom-ignore` comments (see `filter_ignored_by_comment`
//! in the parent module), these rules live in the project config and
//! apply uniformly across every file, which is what makes them
//! suitable for mirroring a project's existing PHPStan `excludePaths`
//! / `ignoreErrors` conventions (test fixtures, vendored code, etc.).

use globset::{Glob, GlobMatcher};
use regex::Regex;
use tower_lsp::lsp_types::{Diagnostic, NumberOrString};

use crate::config::IgnoreRule;

/// A `[[diagnostics.ignore]]` rule with its `message`/`path` patterns
/// pre-compiled so matching is cheap across many files and diagnostics.
pub(crate) struct CompiledIgnoreRule {
    message: Option<Regex>,
    path: Option<GlobMatcher>,
    identifier: Option<String>,
}

impl CompiledIgnoreRule {
    fn matches(&self, relative_path: &str, diagnostic: &Diagnostic) -> bool {
        if let Some(re) = &self.message
            && !re.is_match(&diagnostic.message)
        {
            return false;
        }

        if let Some(glob) = &self.path
            && !glob.is_match(relative_path)
        {
            return false;
        }

        if let Some(identifier) = &self.identifier {
            let code = match &diagnostic.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => "",
            };
            if code != identifier {
                return false;
            }
        }

        true
    }
}

/// Compile the `[[diagnostics.ignore]]` rules from `.phpantom.toml`.
///
/// Rules with an invalid `message` regex or `path` glob, or with no
/// constraints at all (which would silently suppress every
/// diagnostic in the project), are skipped with a warning printed to
/// stderr rather than failing the whole config load.
pub(crate) fn compile_ignore_rules(rules: &[IgnoreRule]) -> Vec<CompiledIgnoreRule> {
    rules
        .iter()
        .filter_map(|rule| {
            if rule.message.is_none() && rule.path.is_none() && rule.identifier.is_none() {
                eprintln!(
                    "warning: skipping [[diagnostics.ignore]] rule with no message, path, or identifier"
                );
                return None;
            }

            let message = match rule.message.as_deref().map(Regex::new) {
                Some(Ok(re)) => Some(re),
                Some(Err(e)) => {
                    eprintln!(
                        "warning: skipping [[diagnostics.ignore]] rule with invalid message regex `{}`: {}",
                        rule.message.as_deref().unwrap_or_default(),
                        e
                    );
                    return None;
                }
                None => None,
            };

            let path = match rule.path.as_deref().map(Glob::new) {
                Some(Ok(glob)) => Some(glob.compile_matcher()),
                Some(Err(e)) => {
                    eprintln!(
                        "warning: skipping [[diagnostics.ignore]] rule with invalid path glob `{}`: {}",
                        rule.path.as_deref().unwrap_or_default(),
                        e
                    );
                    return None;
                }
                None => None,
            };

            Some(CompiledIgnoreRule {
                message,
                path,
                identifier: rule.identifier.clone(),
            })
        })
        .collect()
}

/// Remove diagnostics matching any compiled `[[diagnostics.ignore]]` rule.
///
/// `relative_path` should be the file path relative to the workspace
/// root, using `/` separators (matching glob convention regardless of
/// platform).
pub(crate) fn filter_ignored_by_config(
    diagnostics: &mut Vec<Diagnostic>,
    relative_path: &str,
    rules: &[CompiledIgnoreRule],
) {
    if rules.is_empty() || diagnostics.is_empty() {
        return;
    }

    diagnostics.retain(|d| !rules.iter().any(|rule| rule.matches(relative_path, d)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{Position, Range};

    fn diag(message: &str, code: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            message: message.to_string(),
            code: Some(NumberOrString::String(code.to_string())),
            ..Default::default()
        }
    }

    #[test]
    fn path_only_rule_suppresses_everything_under_path() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: None,
            path: Some("tests/**".to_string()),
            identifier: None,
        }]);
        let mut diagnostics = vec![diag("anything", "unknown_class")];
        filter_ignored_by_config(&mut diagnostics, "tests/Fixture/Broken.php", &rules);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn path_only_rule_does_not_suppress_outside_path() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: None,
            path: Some("tests/**".to_string()),
            identifier: None,
        }]);
        let mut diagnostics = vec![diag("anything", "unknown_class")];
        filter_ignored_by_config(&mut diagnostics, "src/Foo.php", &rules);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn identifier_only_rule_suppresses_matching_code_anywhere() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: None,
            path: None,
            identifier: Some("unused_variable".to_string()),
        }]);
        let mut diagnostics = vec![
            diag("unused $x", "unused_variable"),
            diag("boom", "unknown_class"),
        ];
        filter_ignored_by_config(&mut diagnostics, "src/Foo.php", &rules);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "boom");
    }

    #[test]
    fn message_regex_only_matches_message() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: Some(r"^Call to deprecated function legacy_helper\(\)".to_string()),
            path: None,
            identifier: None,
        }]);
        let mut diagnostics = vec![
            diag(
                "Call to deprecated function legacy_helper()",
                "deprecated_usage",
            ),
            diag(
                "Call to deprecated function other_helper()",
                "deprecated_usage",
            ),
        ];
        filter_ignored_by_config(&mut diagnostics, "src/Foo.php", &rules);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "Call to deprecated function other_helper()"
        );
    }

    #[test]
    fn all_three_constraints_must_match() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: Some("legacy".to_string()),
            path: Some("tests/**".to_string()),
            identifier: Some("deprecated_usage".to_string()),
        }]);

        // Wrong path: not suppressed.
        let mut diagnostics = vec![diag("legacy call", "deprecated_usage")];
        filter_ignored_by_config(&mut diagnostics, "src/Foo.php", &rules);
        assert_eq!(diagnostics.len(), 1);

        // Wrong identifier: not suppressed.
        let mut diagnostics = vec![diag("legacy call", "unknown_class")];
        filter_ignored_by_config(&mut diagnostics, "tests/Foo.php", &rules);
        assert_eq!(diagnostics.len(), 1);

        // Everything matches: suppressed.
        let mut diagnostics = vec![diag("legacy call", "deprecated_usage")];
        filter_ignored_by_config(&mut diagnostics, "tests/Foo.php", &rules);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn rule_with_no_constraints_is_skipped() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: None,
            path: None,
            identifier: None,
        }]);
        assert!(rules.is_empty());
    }

    #[test]
    fn invalid_regex_is_skipped() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: Some("(unclosed".to_string()),
            path: None,
            identifier: None,
        }]);
        assert!(rules.is_empty());
    }

    #[test]
    fn invalid_glob_is_skipped() {
        let rules = compile_ignore_rules(&[IgnoreRule {
            message: None,
            path: Some("[unclosed".to_string()),
            identifier: None,
        }]);
        assert!(rules.is_empty());
    }
}
