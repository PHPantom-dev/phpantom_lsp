//! Environment-variable names: `env('APP_NAME')` and `Env::get('APP_NAME')`.
//!
//! The symbol map records both spellings as
//! [`crate::symbol_map::LaravelStringKind::Env`] spans, so find-references
//! and hover come from the same index every other Laravel string key uses.
//! What a name resolves *to* is the line that declares it in the project's
//! `.env`, which is what this module supplies.

use std::path::Path;

use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;

/// The dotenv files a project declares its variables in, in the order a
/// reader should prefer them.
///
/// `.env` is the one the framework actually loads; `.env.example` is the
/// committed inventory of what a project expects, and is all a fresh clone
/// has before anyone copies it; Laravel also loads a `.env.<environment>`
/// file (e.g. `.env.testing`) in place of `.env` when it exists. Which
/// environment is active is not statically known, so the workspace root is
/// scanned for every name that matches rather than checking a fixed list.
fn dotenv_file_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name == ".env" || name.starts_with(".env."))
        .collect();
    names.sort_by_key(|name| match name.as_str() {
        ".env" => (0u8, String::new()),
        ".env.example" => (1u8, String::new()),
        _ => (2u8, name.clone()),
    });
    names
}

/// Resolve an environment-variable name to the line declaring it in each
/// dotenv file that does.
///
/// A name neither file declares still resolves to the top of the first file
/// that exists: an environment a process runs with is not on disk, so a name
/// missing from `.env` is usually one that belongs there, and opening the
/// file says more than refusing to navigate.
pub(crate) fn resolve_env_definitions(backend: &Backend, key: &str) -> Vec<Location> {
    let Some(root) = backend.workspace.workspace_root.read().clone() else {
        return Vec::new();
    };
    let mut fallback = None;
    let mut declarations = Vec::new();
    for name in dotenv_file_names(&root) {
        let path = root.join(&name);
        let (Ok(content), Ok(uri)) = (std::fs::read_to_string(&path), Url::from_file_path(&path))
        else {
            continue;
        };
        match declaring_line(&content, key) {
            Some(line) => declarations.push(crate::definition::point_location(
                uri,
                Position::new(line, 0),
            )),
            None if fallback.is_none() => {
                fallback = Some(crate::definition::point_location(uri, Position::new(0, 0)));
            }
            None => {}
        }
    }
    if declarations.is_empty() {
        return fallback.into_iter().collect();
    }
    declarations
}

/// What a dotenv file says about one variable: which file declares it and
/// the value it is set to.
pub(crate) struct EnvDeclaration {
    /// The dotenv file, as a workspace-relative name.
    pub file: String,
    /// The value as written, with surrounding quotes and any trailing
    /// comment removed.  Empty for a name declared with no value.
    pub value: String,
}

/// The dotenv file that declares `key`, and the value it gives it.
///
/// Separate from [`resolve_env_definitions`], which points at a file even
/// when nothing in it declares the name: hover has to tell the two apart.
pub(crate) fn env_declaration(backend: &Backend, key: &str) -> Option<EnvDeclaration> {
    let root = backend.workspace.workspace_root.read().clone()?;
    dotenv_file_names(&root).into_iter().find_map(|file| {
        let content = std::fs::read_to_string(root.join(&file)).ok()?;
        let value = declared_value(&content, key)?;
        Some(EnvDeclaration { file, value })
    })
}

/// Whether a variable's name reads as naming a credential, in which case
/// hover shows that it is set rather than what it is set to.
///
/// Matched on the whole name split into words, so `API_TOKEN` is a secret
/// while `TOKENIZER_PATH` is not.
pub(crate) fn env_name_is_sensitive(key: &str) -> bool {
    const SENSITIVE_WORDS: [&str; 8] = [
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "TOKEN",
        "KEY",
        "SALT",
        "CREDENTIALS",
        "DSN",
    ];
    key.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| SENSITIVE_WORDS.contains(&word.to_ascii_uppercase().as_str()))
}

/// Every variable name the project's dotenv files declare, sorted and
/// deduplicated.
///
/// Read from disk on demand rather than cached: there are only ever a
/// handful of dotenv files, and an edit to `.env` has to show up in the next
/// completion.
pub(crate) fn enumerate_env_keys(backend: &Backend) -> Vec<String> {
    let Some(root) = backend.workspace.workspace_root.read().clone() else {
        return Vec::new();
    };
    let mut keys: Vec<String> = dotenv_file_names(&root)
        .into_iter()
        .filter_map(|name| std::fs::read_to_string(root.join(name)).ok())
        .flat_map(|content| {
            content
                .lines()
                .filter_map(declared_key)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The variable one dotenv line declares, or `None` for a comment, a blank
/// line, or anything else that is not an assignment.
fn declared_key(line: &str) -> Option<&str> {
    declared_entry(line).map(|(key, _)| key)
}

/// The variable and the raw right-hand side one dotenv line declares.
fn declared_entry(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    // Laravel's dotenv reader accepts an `export ` prefix.
    let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
    let (key, raw) = line.split_once('=')?;
    let key = key.trim_end();
    (!key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'))
    .then_some((key, raw))
}

/// The value `key` is set to, or `None` when this file does not declare it.
///
/// phpdotenv lets a later line of the same file overwrite an earlier one, so a
/// redeclared name takes its last value.
fn declared_value(env_content: &str, key: &str) -> Option<String> {
    env_content
        .lines()
        .filter_map(declared_entry)
        .rfind(|(name, _)| *name == key)
        .map(|(_, raw)| unquote_value(raw))
}

/// One dotenv right-hand side as the reader would see it: quotes stripped,
/// an unquoted trailing comment dropped.
fn unquote_value(raw: &str) -> String {
    let raw = raw.trim_start();
    let mut chars = raw.chars();
    let Some(quote @ ('"' | '\'')) = chars.next() else {
        // A `#` only opens a comment when whitespace precedes it, so
        // `APP_URL=http://x#y` keeps its fragment.
        let end = raw
            .char_indices()
            .find(|(i, c)| *c == '#' && raw[..*i].ends_with(char::is_whitespace))
            .map_or(raw.len(), |(i, _)| i);
        return raw[..end].trim_end().to_string();
    };
    let mut value = String::new();
    let mut escaped = false;
    for c in chars {
        if escaped {
            // Only the quote itself and a backslash are escapes; anything
            // else keeps the backslash it was written with.
            if c != quote && c != '\\' {
                value.push('\\');
            }
            value.push(c);
            escaped = false;
        } else if c == '\\' && quote == '"' {
            escaped = true;
        } else if c == quote {
            break;
        } else {
            value.push(c);
        }
    }
    value
}

/// The zero-based line `key` is declared on, or `None` when this file does
/// not declare it.
fn declaring_line(env_content: &str, key: &str) -> Option<u32> {
    env_content
        .lines()
        .position(|line| declared_key(line) == Some(key))
        .map(|index| index as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_env_key_line() {
        let env = "APP_NAME=Laravel\nDB_HOST=127.0.0.1\n";
        assert_eq!(declaring_line(env, "DB_HOST"), Some(1));
    }

    #[test]
    fn an_undeclared_key_has_no_line() {
        let env = "APP_NAME=Laravel\n";
        assert_eq!(declaring_line(env, "MISSING"), None);
    }

    /// A commented-out variable is documentation of what could be set, not a
    /// declaration, and a key mentioned inside a value is not one either.
    #[test]
    fn only_assignments_declare_a_variable() {
        let env = "# DB_HOST=127.0.0.1\nAPP_NAME=Laravel\nexport MAIL_MAILER=log\nnonsense\n";
        let declared: Vec<&str> = env.lines().filter_map(declared_key).collect();
        assert_eq!(declared, vec!["APP_NAME", "MAIL_MAILER"]);
    }

    /// A name that shares a prefix with a declared one is a different
    /// variable.
    #[test]
    fn a_prefix_of_a_declared_name_is_not_declared() {
        let env = "DB_HOSTNAME=example.test\n";
        assert_eq!(declaring_line(env, "DB_HOST"), None);
        assert_eq!(declaring_line(env, "DB_HOSTNAME"), Some(0));
    }

    #[test]
    fn reads_the_value_a_name_is_set_to() {
        let env = "APP_NAME=Laravel\nAPP_DEBUG=\n";
        assert_eq!(declared_value(env, "APP_NAME").as_deref(), Some("Laravel"));
        assert_eq!(declared_value(env, "APP_DEBUG").as_deref(), Some(""));
        assert_eq!(declared_value(env, "MISSING"), None);
    }

    /// Quotes belong to the file, not the value, and a `#` only opens a
    /// comment when it follows whitespace outside them.
    #[test]
    fn a_value_is_read_the_way_the_dotenv_reader_reads_it() {
        assert_eq!(unquote_value(" \"Acme App\" "), "Acme App");
        assert_eq!(unquote_value("'Acme # App'"), "Acme # App");
        assert_eq!(unquote_value("log  # the mailer"), "log");
        assert_eq!(
            unquote_value("http://acme.test#top"),
            "http://acme.test#top"
        );
        assert_eq!(unquote_value(r#""say \"hi\"""#), "say \"hi\"");
    }

    /// A word of the name decides, so a name that merely contains the
    /// letters of one is not a credential.
    #[test]
    fn only_whole_words_make_a_name_look_like_a_secret() {
        assert!(env_name_is_sensitive("STRIPE_SECRET"));
        assert!(env_name_is_sensitive("APP_KEY"));
        assert!(!env_name_is_sensitive("APP_NAME"));
        assert!(!env_name_is_sensitive("KEYCLOAK_URL"));
    }
}
