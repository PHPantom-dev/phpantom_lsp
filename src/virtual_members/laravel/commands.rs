//! Artisan console command index and signature parsing.
//!
//! Laravel encodes console commands as classes extending
//! `Illuminate\Console\Command`.  Each command declares a name through one
//! of three surfaces, all statically recoverable from source:
//!
//! - `protected $signature = 'app:sync {user} {--queue}';`
//! - `protected $name = 'app:sync';`
//! - `#[AsCommand(name: 'app:sync')]`
//!
//! This module scans project and vendor command classes for those literals
//! (see [`scan_command_file`]), parses the `$signature` grammar into
//! arguments and options ([`parse_signature`]), and stores everything in a
//! [`LaravelCommandIndex`] keyed by command name.  The index powers:
//!
//! - completion / go-to-definition / hover / unknown-name diagnostics for
//!   command-name string literals (`Artisan::call('app:sync')`,
//!   `Schedule::command('app:sync')`, `$this->call('app:sync')`), and
//! - array-key completion for the parameter array of
//!   `Artisan::call('app:sync', [...])`.
//!
//! The parsed signature of the *enclosing* command class also drives
//! completion / validation of `$this->argument('user')` and
//! `$this->option('queue')` against that same command's own parameters.

use std::collections::HashMap;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;

use super::helpers::extract_string_literal;

/// A single parsed argument or option from a command `$signature`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandParam {
    /// The parameter name without any decoration: `user`, `queue`.
    pub name: String,
    /// The `:`-delimited description, if any.
    pub description: Option<String>,
    /// Optional default value (the text after `=`).
    pub default: Option<String>,
    /// Single-character shortcut for options (`--queue|-q` → `q`).
    pub shortcut: Option<String>,
    /// Whether the parameter accepts multiple values (`*`).
    pub is_array: bool,
    /// Arguments: whether the argument is optional (`?`).
    /// Options: always effectively optional, so this stays `false`.
    pub optional: bool,
    /// Options only: whether the option takes a value (`--queue=`).
    /// Value-less options are boolean flags.
    pub takes_value: bool,
}

/// A parsed command signature: the command name plus its arguments and
/// options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommandSignature {
    /// The command name (first whitespace-delimited token of the signature).
    pub name: String,
    pub arguments: Vec<CommandParam>,
    pub options: Vec<CommandParam>,
}

impl CommandSignature {
    /// Find an argument by name (case-sensitive, Laravel names are literal).
    pub(crate) fn argument(&self, name: &str) -> Option<&CommandParam> {
        self.arguments.iter().find(|p| p.name == name)
    }

    /// Find an option by name.
    pub(crate) fn option(&self, name: &str) -> Option<&CommandParam> {
        self.options.iter().find(|p| p.name == name)
    }
}

/// One command discovered in a source file.
#[derive(Debug, Clone)]
pub(crate) struct CommandEntry {
    /// The command name, e.g. `app:sync` or `migrate`.
    pub name: String,
    /// Best-effort fully-qualified class name (`App\Console\Commands\Sync`).
    pub fqn: Option<String>,
    /// URI of the file declaring the command.
    pub uri: String,
    /// Byte offset of the command-name string literal (inside the quotes),
    /// used for go-to-definition.
    pub name_offset: u32,
    /// The parsed `$signature`.  Arguments and options are empty when the
    /// command declares only a `$name`/`#[AsCommand]` with no signature.
    pub signature: CommandSignature,
}

/// Index of Artisan commands keyed by command name.
///
/// Mirrors [`super::LaravelMacroIndex`]'s per-URI storage so an edit to a
/// single command file can replace just that file's contribution
/// ([`Self::set_file`]) before a cheap [`Self::rebuild`] refreshes the
/// merged by-name lookup.
#[derive(Default)]
pub(crate) struct LaravelCommandIndex {
    by_uri: HashMap<String, Vec<CommandEntry>>,
    by_name: HashMap<String, CommandEntry>,
}

impl LaravelCommandIndex {
    /// Replace the commands contributed by `uri`.  An empty vector removes
    /// the file's contribution.  Call [`Self::rebuild`] afterwards.
    pub(crate) fn set_file(&mut self, uri: String, entries: Vec<CommandEntry>) {
        if entries.is_empty() {
            self.by_uri.remove(&uri);
        } else {
            self.by_uri.insert(uri, entries);
        }
    }

    /// Rebuild the merged name → entry lookup from per-file contributions.
    ///
    /// When two files declare the same command name, the first one
    /// encountered wins; the ordering is deterministic only up to the
    /// hash map iteration order, which is acceptable for a diagnostic /
    /// navigation aid.
    pub(crate) fn rebuild(&mut self) {
        let mut by_name = HashMap::new();
        for entries in self.by_uri.values() {
            for entry in entries {
                by_name
                    .entry(entry.name.clone())
                    .or_insert_with(|| entry.clone());
            }
        }
        self.by_name = by_name;
    }

    /// Whether `uri` currently contributes any commands.
    pub(crate) fn has_uri(&self, uri: &str) -> bool {
        self.by_uri.contains_key(uri)
    }

    /// Whether the index contains no commands at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Look up a command by name.
    pub(crate) fn get(&self, name: &str) -> Option<&CommandEntry> {
        self.by_name.get(name)
    }

    /// All known command names, sorted and deduplicated.
    pub(crate) fn all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.by_name.keys().cloned().collect();
        names.sort();
        names.dedup();
        names
    }
}

// ─── Signature grammar parser ─────────────────────────────────────────────────

/// Parse a Laravel command signature expression into its name, arguments
/// and options.
///
/// Mirrors `Illuminate\Console\Parser`:
/// - the name is the first whitespace-delimited token;
/// - each `{...}` token is an option when it starts with `--`, otherwise an
///   argument;
/// - a ` : ` splits a token from its description;
/// - decorations: `?` (optional), `*` (array), `=default` (default value),
///   `=*` (array with defaults), and `shortcut|name` for options.
pub(crate) fn parse_signature(expression: &str) -> CommandSignature {
    let name = expression
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();

    let mut arguments = Vec::new();
    let mut options = Vec::new();

    for token in signature_tokens(expression) {
        let (body, description) = extract_description(&token);
        if let Some(rest) = body.strip_prefix("--") {
            // Strip any extra leading dashes (`-{2,}`).
            let rest = rest.trim_start_matches('-');
            options.push(parse_option(rest, description));
        } else {
            arguments.push(parse_argument(&body, description));
        }
    }

    CommandSignature {
        name,
        arguments,
        options,
    }
}

/// Extract the raw `{...}` token bodies from a signature expression.
///
/// Laravel uses the non-greedy regex `\{\s*(.*?)\s*\}`, so the first `}`
/// closes a token; the inner text is trimmed of surrounding whitespace.
fn signature_tokens(expression: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let bytes = expression.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(close_rel) = expression[i + 1..].find('}') {
                let inner = &expression[i + 1..i + 1 + close_rel];
                tokens.push(inner.trim().to_string());
                i = i + 1 + close_rel + 1;
                continue;
            } else {
                break;
            }
        }
        i += 1;
    }
    tokens
}

/// Split a token into its body and optional description on the first ` : `
/// (whitespace-colon-whitespace) separator, matching `\s+:\s+`.
fn extract_description(token: &str) -> (String, Option<String>) {
    let trimmed = token.trim();
    let bytes = trimmed.as_bytes();
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b':'
            && idx > 0
            && bytes[idx - 1].is_ascii_whitespace()
            && idx + 1 < bytes.len()
            && bytes[idx + 1].is_ascii_whitespace()
        {
            let body = trimmed[..idx].trim().to_string();
            let desc = trimmed[idx + 1..].trim().to_string();
            let desc = if desc.is_empty() { None } else { Some(desc) };
            return (body, desc);
        }
    }
    (trimmed.to_string(), None)
}

fn parse_argument(token: &str, description: Option<String>) -> CommandParam {
    // Match order follows Illuminate\Console\Parser::parseArgument.
    if token.ends_with("?*") {
        return CommandParam {
            name: token.trim_matches(|c| c == '?' || c == '*').to_string(),
            description,
            default: None,
            shortcut: None,
            is_array: true,
            optional: true,
            takes_value: false,
        };
    }
    if token.ends_with('*') {
        return CommandParam {
            name: token.trim_matches('*').to_string(),
            description,
            default: None,
            shortcut: None,
            is_array: true,
            optional: false,
            takes_value: false,
        };
    }
    if token.ends_with('?') {
        return CommandParam {
            name: token.trim_matches('?').to_string(),
            description,
            default: None,
            shortcut: None,
            is_array: false,
            optional: true,
            takes_value: false,
        };
    }
    if let Some((name, default)) = split_default_array(token) {
        return CommandParam {
            name,
            description,
            default: Some(default),
            shortcut: None,
            is_array: true,
            optional: true,
            takes_value: false,
        };
    }
    if let Some((name, default)) = token.split_once('=') {
        return CommandParam {
            name: name.to_string(),
            description,
            default: Some(default.to_string()),
            shortcut: None,
            is_array: false,
            optional: true,
            takes_value: false,
        };
    }
    CommandParam {
        name: token.to_string(),
        description,
        default: None,
        shortcut: None,
        is_array: false,
        optional: false,
        takes_value: false,
    }
}

fn parse_option(token: &str, description: Option<String>) -> CommandParam {
    // Split a leading `shortcut|name` (regex `\s*\|\s*`, limit 2).
    let (shortcut, token) = match token.split_once('|') {
        Some((short, rest)) => (Some(short.trim().to_string()), rest.trim().to_string()),
        None => (None, token.to_string()),
    };

    // Match order follows Illuminate\Console\Parser::parseOption.
    if token.ends_with("=*") {
        return CommandParam {
            name: token.trim_end_matches("=*").to_string(),
            description,
            default: None,
            shortcut,
            is_array: true,
            optional: true,
            takes_value: true,
        };
    }
    if token.ends_with('=') {
        return CommandParam {
            name: token.trim_end_matches('=').to_string(),
            description,
            default: None,
            shortcut,
            is_array: false,
            optional: true,
            takes_value: true,
        };
    }
    if let Some((name, default)) = split_default_array(&token) {
        return CommandParam {
            name,
            description,
            default: Some(default),
            shortcut,
            is_array: true,
            optional: true,
            takes_value: true,
        };
    }
    if let Some((name, default)) = token.split_once('=') {
        return CommandParam {
            name: name.to_string(),
            description,
            default: Some(default.to_string()),
            shortcut,
            is_array: false,
            optional: true,
            takes_value: true,
        };
    }
    // Value-less option — a boolean flag.
    CommandParam {
        name: token,
        description,
        default: None,
        shortcut,
        is_array: false,
        optional: true,
        takes_value: false,
    }
}

/// Split `name=*value` into `(name, value)`.  Returns `None` when the token
/// does not contain the `=*` array-default marker.
fn split_default_array(token: &str) -> Option<(String, String)> {
    let idx = token.find("=*")?;
    let default = &token[idx + 2..];
    if default.is_empty() {
        return None;
    }
    Some((token[..idx].to_string(), default.to_string()))
}

// ─── Source scanner ────────────────────────────────────────────────────────────

/// Scan a PHP source file for Artisan command declarations.
///
/// A class is treated as a command when it `extends` a class whose short
/// name ends in `Command`, or carries an `#[AsCommand]` attribute.  For each
/// such class the command name is recovered from (in priority order) the
/// `#[AsCommand]` attribute, the `$signature` property, or the `$name`
/// property.
pub(crate) fn scan_command_file(content: &str, uri: &str) -> Vec<CommandEntry> {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());

    let mut entries = Vec::new();
    for stmt in program.statements.iter() {
        scan_stmt_for_commands(stmt, None, content, uri, &mut entries);
    }
    entries
}

fn scan_stmt_for_commands(
    stmt: &Statement<'_>,
    namespace: Option<&str>,
    content: &str,
    uri: &str,
    out: &mut Vec<CommandEntry>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            let ns_name = ns.name.map(|n| bytes_to_string(n.value()));
            for inner in ns.statements().iter() {
                scan_stmt_for_commands(inner, ns_name.as_deref(), content, uri, out);
            }
        }
        Statement::Class(class) => {
            if let Some(entry) = command_from_class(class, namespace, content, uri) {
                out.push(entry);
            }
        }
        _ => {}
    }
}

fn command_from_class(
    class: &Class<'_>,
    namespace: Option<&str>,
    content: &str,
    uri: &str,
) -> Option<CommandEntry> {
    let has_as_command = class
        .attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
        .any(|attr| last_segment(attr.name.value()) == b"AsCommand");

    let extends_command = class
        .extends
        .as_ref()
        .map(|ext| {
            ext.types
                .iter()
                .any(|ty| last_segment(ty.value()).ends_with(b"Command"))
        })
        .unwrap_or(false);

    if !has_as_command && !extends_command {
        return None;
    }

    let fqn = match namespace {
        Some(ns) => Some(format!("{}\\{}", ns, bytes_to_string(class.name.value))),
        None => Some(bytes_to_string(class.name.value)),
    };

    // 1. #[AsCommand(name: '...')] / #[AsCommand('...')].
    if let Some((name, offset)) = as_command_name(class, content) {
        let signature = signature_property_value(class, content)
            .map(|(sig, _)| parse_signature(sig))
            .unwrap_or_else(|| CommandSignature {
                name: name.clone(),
                ..Default::default()
            });
        return Some(CommandEntry {
            name,
            fqn,
            uri: uri.to_string(),
            name_offset: offset,
            signature,
        });
    }

    // 2. $signature = '...'.
    if let Some((sig, offset)) = signature_property_value(class, content) {
        let signature = parse_signature(sig);
        if signature.name.is_empty() {
            return None;
        }
        return Some(CommandEntry {
            name: signature.name.clone(),
            fqn,
            uri: uri.to_string(),
            name_offset: offset,
            signature,
        });
    }

    // 3. $name = '...'.
    if let Some((name, offset)) = string_property_value(class, "name", content) {
        if name.is_empty() {
            return None;
        }
        return Some(CommandEntry {
            name: name.clone(),
            fqn,
            uri: uri.to_string(),
            name_offset: offset,
            signature: CommandSignature {
                name,
                ..Default::default()
            },
        });
    }

    None
}

/// The first string argument of an `#[AsCommand]` attribute, with its inner
/// byte offset.
fn as_command_name(class: &Class<'_>, content: &str) -> Option<(String, u32)> {
    for list in class.attribute_lists.iter() {
        for attr in list.attributes.iter() {
            if last_segment(attr.name.value()) != b"AsCommand" {
                continue;
            }
            let Some(arg_list) = attr.argument_list.as_ref() else {
                continue;
            };
            let Some(first) = arg_list.arguments.first() else {
                continue;
            };
            let Some(expr) = first.value() else {
                continue;
            };
            if let Some((value, start, _)) = extract_string_literal(expr, content) {
                return Some((value.to_string(), start as u32));
            }
        }
    }
    None
}

/// The `$signature` property's string value and the inner byte offset of the
/// literal.
fn signature_property_value<'c>(class: &Class<'_>, content: &'c str) -> Option<(&'c str, u32)> {
    string_property_value_ref(class, "signature", content)
}

/// The named string property's value (owned) plus its inner byte offset.
fn string_property_value(class: &Class<'_>, prop: &str, content: &str) -> Option<(String, u32)> {
    string_property_value_ref(class, prop, content).map(|(v, o)| (v.to_string(), o))
}

/// The named string property's value (borrowed) plus its inner byte offset.
fn string_property_value_ref<'c>(
    class: &Class<'_>,
    prop: &str,
    content: &'c str,
) -> Option<(&'c str, u32)> {
    for member in class.members.iter() {
        let ClassLikeMember::Property(Property::Plain(plain)) = member else {
            continue;
        };
        for item in plain.items.iter() {
            let PropertyItem::Concrete(concrete) = item else {
                continue;
            };
            let var_name = concrete.variable.name;
            if trim_dollar(var_name) != prop.as_bytes() {
                continue;
            }
            if let Some((value, start, _)) = extract_string_literal(concrete.value, content) {
                return Some((value, start as u32));
            }
        }
    }
    None
}

// ─── Enclosing-signature lookup ────────────────────────────────────────────────

/// Parse the command `$signature` of the class enclosing `offset`, if any.
///
/// Used for completing / validating `$this->argument('user')` and
/// `$this->option('queue')` against the *current* command's own parameters.
/// Returns `None` when `offset` is not inside a class, or the enclosing class
/// declares no `$signature`.
pub(crate) fn command_signature_at_offset(
    content: &str,
    offset: usize,
) -> Option<CommandSignature> {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let mut found: Option<CommandSignature> = None;
    for stmt in program.statements.iter() {
        find_signature_at_offset(stmt, offset as u32, content, &mut found);
        if found.is_some() {
            break;
        }
    }
    found
}

fn find_signature_at_offset(
    stmt: &Statement<'_>,
    offset: u32,
    content: &str,
    out: &mut Option<CommandSignature>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                find_signature_at_offset(inner, offset, content, out);
                if out.is_some() {
                    return;
                }
            }
        }
        Statement::Class(class) => {
            let start = class.left_brace.start.offset;
            let end = class.right_brace.end.offset;
            if offset >= start
                && offset <= end
                && let Some((sig, _)) = signature_property_value(class, content)
            {
                *out = Some(parse_signature(sig));
            }
        }
        _ => {}
    }
}

// ─── Byte helpers ────────────────────────────────────────────────────────────

fn last_segment(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b'\\') {
        Some(idx) => &name[idx + 1..],
        None => name,
    }
}

fn trim_dollar(name: &[u8]) -> &[u8] {
    name.strip_prefix(b"$").unwrap_or(name)
}

fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_start_matches('\\')
        .to_string()
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
