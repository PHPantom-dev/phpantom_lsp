//! Artisan console command index and signature parsing.
//!
//! Laravel encodes console commands as classes extending
//! `Illuminate\Console\Command`.  Each command declares a name through one
//! of four surfaces, all statically recoverable from source:
//!
//! - `protected $signature = 'app:sync {user} {--queue}';`
//! - `#[Signature('app:sync {user} {--queue}')]`
//! - `protected $name = 'app:sync';`
//! - `#[AsCommand(name: 'app:sync')]`
//!
//! Command aliases (extra names a command answers to) are recovered from
//! `#[Aliases([...])]`, the `aliases:` argument of `#[Signature]` /
//! `#[AsCommand]`, the `protected $aliases` property, and Symfony's inline
//! `'app:sync|app:s'` name form.  All of them are indexed alongside the
//! primary name.
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
//! `$this->option('queue')` against that same command's own parameters,
//! and gives both accessors the type the named parameter really holds
//! ([`resolve_accessor_type`]) instead of the framework's raw
//! `array|string|int|bool|null` union.

use std::collections::HashMap;
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;

use super::file_contributions::FileContributions;
use super::helpers::{extract_string_literal, walks_parent_chain};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, PropertySource};

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
    /// Alternative names the command answers to (`#[Aliases]`,
    /// `#[Signature(aliases:)]`, `#[AsCommand(aliases:)]`).
    pub aliases: Vec<String>,
    /// Best-effort fully-qualified class name (`App\Console\Commands\Sync`).
    pub fqn: Option<String>,
    /// URI of the file declaring the command.
    pub uri: String,
    /// Byte offset of the command-name string literal (inside the quotes),
    /// used for go-to-definition.
    pub name_offset: u32,
    /// The parsed `$signature`.  Arguments and options are empty when the
    /// command declares only a `$name`/`#[AsCommand]` with no signature, in
    /// which case `signature.name` mirrors [`Self::name`].
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
    pub(crate) files: FileContributions<Vec<CommandEntry>>,
    by_name: HashMap<String, CommandEntry>,
}

impl LaravelCommandIndex {
    /// Rebuild the merged name → entry lookup from per-file contributions.
    ///
    /// Primary names are inserted before aliases so a command that *is*
    /// named `foo` always wins over one that merely answers to `foo` as an
    /// alias, matching Artisan's own resolution.  Within each of those two
    /// passes the first entry encountered wins; that ordering is
    /// deterministic only up to hash map iteration order, which is
    /// acceptable for a diagnostic / navigation aid.
    pub(crate) fn rebuild(&mut self) {
        let mut by_name = HashMap::new();
        for entries in self.files.values() {
            for entry in entries {
                by_name
                    .entry(entry.name.clone())
                    .or_insert_with(|| entry.clone());
            }
        }
        for entries in self.files.values() {
            for entry in entries {
                for alias in &entry.aliases {
                    by_name
                        .entry(alias.clone())
                        .or_insert_with(|| entry.clone());
                }
            }
        }
        self.by_name = by_name;
    }

    /// Whether the index contains no commands at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Look up a command by name.
    pub(crate) fn get(&self, name: &str) -> Option<&CommandEntry> {
        self.by_name.get(name)
    }

    /// Look up a command by the class that declares it.
    ///
    /// The by-name lookup cannot answer this: a command reached through its
    /// own `$this` is identified by class, and the signature is what the
    /// accessor types need.  Commands number in the tens even in a large
    /// project, so the scan is cheaper than a second index.
    pub(crate) fn get_by_fqn(&self, fqn: &str) -> Option<&CommandEntry> {
        self.files
            .values()
            .flatten()
            .find(|entry| entry.fqn.as_deref() == Some(fqn))
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

// ─── Accessor return types ─────────────────────────────────────────────────────

/// The base class every Artisan command extends.
const CONSOLE_COMMAND_FQN: &str = "Illuminate\\Console\\Command";

/// Whether `method_name` is one of the two signature-typed accessors.
pub(crate) fn is_command_accessor(method_name: &str) -> bool {
    matches!(method_name, "argument" | "option")
}

/// The type `$this->argument($key)` / `$this->option($key)` hands back on a
/// command class, read off that command's own `$signature`.
///
/// Laravel declares both accessors as
/// `array|string|int|bool|null`, the union of every shape a console
/// parameter can take, because the framework cannot see which parameter a
/// call names.  The `$signature` can: `{--flag}` is a boolean flag,
/// `{--opt=}` is an optional value, `{arg}` is required, `{arg*}` collects
/// a list.  Symfony's `InputDefinition` decides the rest — every value that
/// reaches PHP comes off the command line as a string.
///
/// Returns `None` — leaving the declared union in place — when the receiver
/// is not a command, the class declares no signature, or the signature does
/// not name `key`.  A `None` `key` is the no-argument form, which hands back
/// the whole parsed set.
pub(crate) fn resolve_accessor_type(
    class: &ClassInfo,
    method_name: &str,
    key: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&crate::Backend>,
) -> Option<PhpType> {
    if !is_command_accessor(method_name) {
        return None;
    }
    if !walks_parent_chain(class, class_loader, |name| name == CONSOLE_COMMAND_FQN) {
        return None;
    }
    // A command that writes its own `argument()` / `option()` keeps whatever
    // it declared; only the accessors inherited from the framework are ours
    // to reinterpret.  The receiver may already be a merged class, so the
    // own-member check goes through the loader, which hands back the class
    // as parsed.
    if class_loader(&class.fqn()).is_some_and(|raw| raw.get_method_ci(method_name).is_some()) {
        return None;
    }

    let Some(key) = key else {
        // `argument()` / `option()` with no key return the whole parsed
        // set, keyed by parameter name.  The values still span every
        // parameter shape in the signature, so `mixed` is as far as this
        // form narrows.
        return Some(PhpType::generic_array(PhpType::string(), PhpType::mixed()));
    };

    let signature = class_signature(class, backend)?;
    let is_option = method_name == "option";
    let param = if is_option {
        signature.option(key)?
    } else {
        signature.argument(key)?
    };
    Some(param_type(param, is_option))
}

/// The parsed signature of a command class.
///
/// The `$signature` property is read straight off the class: the receiver
/// may be a merged class, which carries the base class's own valueless
/// `protected $signature;` alongside the command's initialised one, so the
/// scan looks for the declaration that actually holds a value.  A command
/// that declares itself through `#[Signature]` / `#[AsCommand]` instead has
/// no such property, and comes from the command index, which parses both
/// attribute forms while scanning.
fn class_signature(
    class: &ClassInfo,
    backend: Option<&crate::Backend>,
) -> Option<CommandSignature> {
    let declared = class
        .properties
        .iter()
        .filter(|p| p.name == "signature")
        .find_map(|p| match p.source.as_ref() {
            Some(PropertySource::DeclaredDefault { value }) => {
                crate::util::unescape_php_string_literal(value.trim())
            }
            _ => None,
        });
    if let Some(expression) = declared {
        return Some(parse_signature(&expression));
    }

    let backend = backend?;
    let index = backend.laravel_commands.read();
    let entry = index.get_by_fqn(&class.fqn())?;
    Some(entry.signature.clone())
}

/// The type a single parsed signature parameter holds.
fn param_type(param: &CommandParam, is_option: bool) -> PhpType {
    // A value-less option is a flag Symfony reports as a boolean.
    if is_option && !param.takes_value {
        return PhpType::bool();
    }
    // `{arg*}` / `{--opt=*}` collect every occurrence into a list of the
    // strings that were passed.
    if param.is_array {
        return PhpType::list(PhpType::string());
    }
    // A parameter that may be left out and carries no default is `null`
    // when it is: `{arg?}` and `{--opt=}`.
    if param.default.is_none() && (param.optional || is_option) {
        return PhpType::nullable(PhpType::string());
    }
    PhpType::string()
}

// ─── Source scanner ────────────────────────────────────────────────────────────

/// Whether `uri` sits in a directory that conventionally holds console
/// commands.
///
/// Command classes are usually named `*Command`, but plenty of packages drop
/// the suffix and lean on the directory instead (`src/Commands/Reload.php` in
/// `monicahq/laravel-cloudflare`), so the directory is the second signal for
/// picking scan candidates.  It only widens the candidate set: a candidate
/// still has to declare a command name and extend a `*Command` class before
/// [`scan_command_file`] yields an entry.
pub(crate) fn is_command_directory_uri(uri: &str) -> bool {
    uri.contains("/Console/") || uri.contains("/Commands/") || uri.contains("/Command/")
}

/// Scan a PHP source file for Artisan command declarations.
///
/// A class is treated as a command when it `extends` a class whose short
/// name ends in `Command`, or carries an `#[AsCommand]` attribute.  For each
/// such class the command name is recovered from the signature
/// (`#[Signature]` or `$signature`), then the `$name` property, then the
/// `#[AsCommand]` attribute, matching Laravel's own precedence.
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

    // Alias names from #[Aliases([...])] (which wins, mirroring
    // Illuminate\Console\Command::configureFromAttributes), the `aliases:`
    // argument of #[Signature] / #[AsCommand], or the `$aliases` property.
    let aliases = command_aliases(class);

    // The branches follow Laravel's own precedence: `Command::__construct()`
    // takes the signature whenever one is set and never reaches Symfony's
    // `getDefaultName()`; without a signature it passes `$this->name` to the
    // parent, which again wins over `#[AsCommand]`.

    // 1. #[Signature('...')] / $signature = '...'.
    if let Some((sig, offset)) = command_signature_value(class, content) {
        let mut signature = parse_signature(sig);
        if let Some((name, offset, inline)) = split_piped_name(&signature.name, offset) {
            signature.name = name.clone();
            return Some(CommandEntry {
                name,
                aliases: merge_aliases(aliases.clone(), inline),
                fqn,
                uri: uri.to_string(),
                name_offset: offset,
                signature,
            });
        }
    }

    // 2. $name = '...'.
    if let Some((raw, offset)) = string_property_value_ref(class, "name", content)
        && let Some((name, offset, inline)) = split_piped_name(raw, offset)
    {
        return Some(CommandEntry {
            name: name.clone(),
            aliases: merge_aliases(aliases.clone(), inline),
            fqn,
            uri: uri.to_string(),
            name_offset: offset,
            signature: CommandSignature {
                name,
                ..Default::default()
            },
        });
    }

    // 3. #[AsCommand(name: '...')] / #[AsCommand('...')].
    if let Some((raw, offset)) = attribute_first_string_arg(class, b"AsCommand", content)
        && let Some((name, offset, inline)) = split_piped_name(raw, offset)
    {
        return Some(CommandEntry {
            name: name.clone(),
            aliases: merge_aliases(aliases, inline),
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

/// Split Symfony's `name|alias1|alias2` declaration form into the primary
/// name (with the byte offset it starts at, given the literal's own offset)
/// and the inline aliases.  A leading `|` — `'|app:sync'` — marks the command
/// hidden and contributes no name of its own.
///
/// `Symfony\Component\Console\Command\Command::__construct()` applies this to
/// every name it is handed, and Laravel routes all three declaration surfaces
/// through it, so the split is not specific to `#[AsCommand]`.
///
/// Returns `None` when no name survives, which is the caller's signal to fall
/// through to the next declaration surface.
fn split_piped_name(raw: &str, offset: u32) -> Option<(String, u32, Vec<String>)> {
    if !raw.contains('|') {
        return (!raw.is_empty()).then(|| (raw.to_string(), offset, Vec::new()));
    }

    let mut parts = raw.split('|');
    let mut delta = 0u32;
    let mut name = parts.next().unwrap_or_default();
    if name.is_empty() {
        delta = 1;
        name = parts.next().unwrap_or_default();
    }
    if name.is_empty() {
        return None;
    }

    let inline = parts
        .filter(|alias| !alias.is_empty())
        .map(str::to_string)
        .collect();
    Some((name.to_string(), offset + delta, inline))
}

/// Union of the declared aliases and the inline `name|alias` ones,
/// order-preserving and deduplicated.
///
/// Symfony merges the two lists while Laravel's `setAliases()` lets the
/// attribute overwrite the inline ones.  The index only answers "is this a
/// name the command responds to", so taking the union avoids a false
/// "unknown command" report under either framework version.
fn merge_aliases(mut aliases: Vec<String>, inline: Vec<String>) -> Vec<String> {
    for alias in inline {
        if !aliases.contains(&alias) {
            aliases.push(alias);
        }
    }
    aliases
}

/// The first string argument of the named class attribute, with its inner
/// byte offset.
fn attribute_first_string_arg<'c>(
    class: &Class<'_>,
    attr_name: &[u8],
    content: &'c str,
) -> Option<(&'c str, u32)> {
    for list in class.attribute_lists.iter() {
        for attr in list.attributes.iter() {
            if last_segment(attr.name.value()) != attr_name {
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
                return Some((value, start as u32));
            }
        }
    }
    None
}

/// Collect the command's alias names, mirroring
/// `Illuminate\Console\Command`: the attribute forms (see
/// [`attribute_aliases`]) are assigned by `configureFromAttributes()` and so
/// override the `protected $aliases = [...]` property, which is otherwise
/// what `setAliases()` receives.
fn command_aliases(class: &Class<'_>) -> Vec<String> {
    if let Some(aliases) = attribute_aliases(class) {
        return aliases;
    }
    // `setAliases((array) $this->aliases)` casts, so a bare string is a
    // one-element list here.
    array_property_strings(class, "aliases").unwrap_or_default()
}

/// The alias names declared by class attributes: a standalone
/// `#[Aliases([...])]` wins (mirroring the assignment order in
/// `configureFromAttributes()`), otherwise the `aliases:` argument of
/// `#[Signature]` / `#[AsCommand]`.
///
/// `None` means no alias-bearing attribute was present at all, which is
/// distinct from an attribute whose argument could not be read statically —
/// the latter yields `Some(vec![])` and still suppresses the property.
fn attribute_aliases(class: &Class<'_>) -> Option<Vec<String>> {
    let mut aliases = None;
    for list in class.attribute_lists.iter() {
        for attr in list.attributes.iter() {
            let segment = last_segment(attr.name.value());
            match segment {
                // #[Aliases(['a', 'b'])] — the single positional argument.
                b"Aliases" => {
                    return Some(
                        attr.argument_list
                            .as_ref()
                            .and_then(|args| args.arguments.first())
                            .and_then(|arg| arg.value())
                            .and_then(string_array_literal)
                            .unwrap_or_default(),
                    );
                }
                // `aliases:` named argument (positional index for
                // Signature(sig, aliases) / AsCommand(name, desc, aliases)).
                b"Signature" | b"AsCommand" => {
                    let Some(arg_list) = attr.argument_list.as_ref() else {
                        continue;
                    };
                    let positional_index = if segment == b"Signature" { 1 } else { 2 };
                    let mut positional = 0usize;
                    let mut found: Option<Vec<String>> = None;
                    for arg in arg_list.arguments.iter() {
                        match arg {
                            PartialArgument::Named(named) => {
                                if bytes_to_string(named.name.value) == "aliases" {
                                    found = string_array_literal(named.value);
                                }
                            }
                            PartialArgument::Positional(_) => {
                                if positional == positional_index && found.is_none() {
                                    found = arg.value().and_then(|expr| string_array_literal(expr));
                                }
                                positional += 1;
                            }
                            PartialArgument::NamedPlaceholder(_)
                            | PartialArgument::Placeholder(_)
                            | PartialArgument::VariadicPlaceholder(_) => {}
                        }
                    }
                    if let Some(list) = found
                        && !list.is_empty()
                    {
                        aliases = Some(list);
                    }
                }
                _ => {}
            }
        }
    }
    aliases
}

/// The named array property's string elements, e.g.
/// `protected $aliases = ['app:s'];`.
fn array_property_strings(class: &Class<'_>, prop: &str) -> Option<Vec<String>> {
    for member in class.members.iter() {
        let ClassLikeMember::Property(Property::Plain(plain)) = member else {
            continue;
        };
        for item in plain.items.iter() {
            let PropertyItem::Concrete(concrete) = item else {
                continue;
            };
            if trim_dollar(concrete.variable.name) != prop.as_bytes() {
                continue;
            }
            return string_array_literal(concrete.value);
        }
    }
    None
}

/// Collect the string literals of a `['a', 'b']` array expression, or the
/// single element of a bare string literal (the shape `(array) $aliases`
/// produces for `protected $aliases = 'app:s';`).
fn string_array_literal(expr: &Expression<'_>) -> Option<Vec<String>> {
    match expr {
        Expression::Literal(Literal::String(s)) => s.value.map(|v| vec![bytes_to_string(v)]),
        Expression::Array(arr) => Some(collect_array_strings(arr.elements.iter())),
        Expression::LegacyArray(arr) => Some(collect_array_strings(arr.elements.iter())),
        _ => None,
    }
}

fn collect_array_strings<'a, 'b>(
    elements: impl IntoIterator<Item = &'a ArrayElement<'b>>,
) -> Vec<String>
where
    'b: 'a,
{
    elements
        .into_iter()
        .filter_map(|el| match el {
            ArrayElement::Value(v) => Some(v.value),
            _ => None,
        })
        .filter_map(|inner| {
            if let Expression::Literal(Literal::String(s)) = inner {
                s.value.map(bytes_to_string)
            } else {
                None
            }
        })
        .collect()
}

/// The command signature expression and the inner byte offset of its string
/// literal: the `#[Signature('…')]` attribute when present, else the
/// `$signature` property.  The attribute wins because Laravel's
/// `configureFromAttributes()` assigns it over the property.
fn command_signature_value<'c>(class: &Class<'_>, content: &'c str) -> Option<(&'c str, u32)> {
    attribute_first_string_arg(class, b"Signature", content)
        .or_else(|| string_property_value_ref(class, "signature", content))
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
                && let Some((sig, _)) = command_signature_value(class, content)
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
