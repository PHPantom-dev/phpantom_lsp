//! Yii translation message IDs (`yiisoft/translator`): the IDs a
//! `translate()` call names, and the message files that declare them.
//!
//! ```php
//! $translator->translate('menu.blog', category: 'app');
//!
//! // resources/messages/en/app.php
//! return ['menu.blog' => 'Blog'];
//! ```
//!
//! The translator looks an ID up in the message source of its category.
//! `yiisoft/translator-message-php` keeps one PHP file per locale and
//! category, `<directory>/<locale>/<category>.php`, returning an array of
//! the IDs and their messages, so a category is the name of a file.  The
//! directories are the ones the DI definitions construct a `MessageSource`
//! with, the project's and the installed packages', read statically.
//!
//! A call names its category by its `category` argument, or else leaves it
//! to the translator: the one a `withDefaultCategory()` in the receiver
//! chain or in the class gives it, or the default the DI configuration
//! gives it from `params['yiisoft/translator']['defaultCategory']`.  A
//! [`YiiTranslation`](crate::symbol_map::StringKeyKind::YiiTranslation)
//! span stores the category with the ID (see [`message_key`]), and an empty
//! one stands for the default, which is only read when the key is looked up.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::text_position::LineIndex;

/// The class whose constructor names a directory of message files.
const MESSAGE_SOURCE: &str = "Yiisoft\\Translator\\Message\\Php\\MessageSource";

/// The params key the translator's DI definition reads its settings from.
const TRANSLATOR_PARAM: &str = "yiisoft/translator";

/// The category and the locale `Translator` falls back to when it is given
/// none.
const DEFAULT_CATEGORY: &str = "app";
const DEFAULT_LOCALE: &str = "en-US";

/// Separates the category from the ID in a message key.  A category is a
/// file name, which never holds it.
const CATEGORY_SEPARATOR: char = '\u{1f}';

/// The key a [`YiiTranslation`](crate::symbol_map::StringKeyKind::YiiTranslation)
/// span stores for the ID `id` in `category`, or in the translator's default
/// category when `category` is `None`.
pub(crate) fn message_key(category: Option<&str>, id: &str) -> String {
    let category = category.unwrap_or_default();
    let mut key = String::with_capacity(category.len() + 1 + id.len());
    key.push_str(category);
    key.push(CATEGORY_SEPARATOR);
    key.push_str(id);
    key
}

/// The category and the ID a message key was made from, the category
/// `None` when it is the translator's default.
pub(crate) fn split_message_key(key: &str) -> (Option<&str>, &str) {
    match key.split_once(CATEGORY_SEPARATOR) {
        Some(("", id)) => (None, id),
        Some((category, id)) => (Some(category), id),
        None => (None, key),
    }
}

/// Whether `id` can be a message ID a call names.  An ID may be a whole
/// sentence, but not one spread over several lines.
pub(crate) fn is_message_id(id: &str) -> bool {
    !id.trim().is_empty() && !id.contains(['\n', '\r', CATEGORY_SEPARATOR])
}

/// Whether `name` is a locale `MessageSource` reads files for.
fn is_locale(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// One ID a message file declares.
#[derive(Debug)]
pub(crate) struct Message {
    pub(crate) id: String,
    /// The message, when it is a string literal.
    pub(crate) text: Option<String>,
    /// The range of the ID's string, inside the quotes.
    pub(crate) range: Range,
}

/// The file of one category in one locale.
#[derive(Debug)]
pub(crate) struct MessageFile {
    pub(crate) locale: String,
    pub(crate) path: PathBuf,
    pub(crate) messages: Vec<Message>,
    /// Whether the file declares IDs that cannot be read: it returns no
    /// array literal, or one with a spread or a computed key.
    open: bool,
}

/// The message files of one project, and the translator's settings.
#[derive(Debug, Default)]
pub(crate) struct Messages {
    /// The files of each category, ordered by locale.
    categories: BTreeMap<String, Vec<MessageFile>>,
    default_category: Option<String>,
    locale: Option<String>,
    fallback_locale: Option<String>,
}

impl Messages {
    /// Read the message files under each of `dirs`.
    fn load(dirs: &[PathBuf]) -> Self {
        let mut messages = Self::default();
        for dir in dirs {
            let Ok(locales) = std::fs::read_dir(dir) else {
                continue;
            };
            for locale in locales.flatten() {
                let Some(name) = locale.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if !is_locale(&name) || !locale.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let Ok(files) = std::fs::read_dir(locale.path()) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().is_none_or(|ext| ext != "php") {
                        continue;
                    }
                    let Some(category) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let category = category.to_string();
                    messages
                        .categories
                        .entry(category)
                        .or_default()
                        .push(read_message_file(&name, path));
                }
            }
        }
        for files in messages.categories.values_mut() {
            files.sort_by(|a, b| a.locale.cmp(&b.locale).then_with(|| a.path.cmp(&b.path)));
        }
        messages
    }

    /// The category a call that names none translates in.
    pub(crate) fn default_category(&self) -> &str {
        self.default_category.as_deref().unwrap_or(DEFAULT_CATEGORY)
    }

    /// The category `category` stands for, the default for `None`.
    fn category<'a>(&'a self, category: Option<&'a str>) -> &'a str {
        category.unwrap_or_else(|| self.default_category())
    }

    /// The IDs the files of `category` declare, sorted.
    pub(crate) fn ids(&self, category: Option<&str>) -> Vec<String> {
        let mut ids: Vec<String> = self
            .categories
            .get(self.category(category))
            .into_iter()
            .flatten()
            .flat_map(|file| file.messages.iter().map(|message| message.id.clone()))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Each file of `category` that declares `id`, with the entry there.
    pub(crate) fn declarations<'a>(
        &'a self,
        category: Option<&'a str>,
        id: &'a str,
    ) -> impl Iterator<Item = (&'a MessageFile, &'a Message)> {
        self.categories
            .get(self.category(category))
            .into_iter()
            .flatten()
            .filter_map(move |file| {
                file.messages
                    .iter()
                    .find(|message| message.id == id)
                    .map(|message| (file, message))
            })
    }

    /// Whether some file of `category` declares `id`, or `None` when that
    /// cannot be told: the category has no message file this reads, which
    /// a source of another kind may serve, or one of its files declares IDs
    /// that cannot be read.
    pub(crate) fn declares(&self, category: Option<&str>, id: &str) -> Option<bool> {
        let files = self.categories.get(self.category(category))?;
        if files
            .iter()
            .any(|file| file.messages.iter().any(|m| m.id == id))
        {
            return Some(true);
        }
        (!files.iter().any(|file| file.open)).then_some(false)
    }

    /// The locales the translator reads a message in, in the order it
    /// tries them: its locale, then that locale without its region
    /// (`en-US`, then `en`), then its fallback locale the same way.
    fn lookup_locales(&self) -> Vec<String> {
        let mut locales = Vec::new();
        let configured = [
            Some(self.locale.as_deref().unwrap_or(DEFAULT_LOCALE)),
            self.fallback_locale.as_deref(),
        ];
        for locale in configured.into_iter().flatten() {
            let mut current = locale;
            loop {
                if !locales.iter().any(|known: &String| known == current) {
                    locales.push(current.to_string());
                }
                match current.rfind(['-', '_']) {
                    Some(at) => current = &current[..at],
                    None => break,
                }
            }
        }
        locales
    }

    /// The message the translator gives `id` in `category` in its own
    /// locale, with the locale of the file it is read from.
    pub(crate) fn default_text<'a>(
        &'a self,
        category: Option<&'a str>,
        id: &'a str,
    ) -> Option<(&'a str, &'a str)> {
        let declared: Vec<_> = self.declarations(category, id).collect();
        self.lookup_locales().iter().find_map(|locale| {
            declared.iter().find_map(|(file, message)| {
                (file.locale.eq_ignore_ascii_case(locale))
                    .then_some(())
                    .and(message.text.as_deref())
                    .map(|text| (file.locale.as_str(), text))
            })
        })
    }
}

/// Read the IDs the message file at `path`, of `locale`, declares.
fn read_message_file(locale: &str, path: PathBuf) -> MessageFile {
    let mut file = MessageFile {
        locale: locale.to_string(),
        path: crate::util::canonical_path_for_comparison(&path),
        messages: Vec::new(),
        open: true,
    };
    let Ok(content) = std::fs::read_to_string(&file.path) else {
        return file;
    };
    crate::parser::with_parsed_program(&content, "yii_messages", |program, content| {
        let returned = crate::virtual_members::laravel::array_file::returned_exprs(program);
        let elements = match returned.first() {
            Some(Expression::Array(array)) => &array.elements,
            Some(Expression::LegacyArray(array)) => &array.elements,
            _ => return,
        };
        file.open = false;
        let lines = LineIndex::new(content);
        for element in elements.iter() {
            let ArrayElement::KeyValue(entry) = element else {
                file.open = true;
                continue;
            };
            let Expression::Literal(Literal::String(key)) = entry.key else {
                file.open = true;
                continue;
            };
            let Some(id) = key.value.and_then(crate::atom::literal_bytes_to_str) else {
                file.open = true;
                continue;
            };
            let text = match entry.value {
                Expression::Literal(Literal::String(text)) => text
                    .value
                    .and_then(crate::atom::literal_bytes_to_str)
                    .map(str::to_string),
                _ => None,
            };
            file.messages.push(Message {
                id: id.to_string(),
                text,
                range: Range::new(
                    lines.position(key.span.start.offset as usize + 1),
                    lines.position((key.span.end.offset as usize).saturating_sub(1)),
                ),
            });
        }
    });
    file
}

/// The directories the `MessageSource`s the PHP file at `path` constructs
/// read, with `aliases` resolving a path that starts with one.
fn message_source_dirs(path: &Path, aliases: &super::aliases::Aliases, out: &mut Vec<PathBuf>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    if !content.contains("MessageSource") {
        return;
    }
    let Some(dir) = path.parent() else {
        return;
    };
    crate::parser::with_parsed_program(&content, "yii_message_sources", |program, _| {
        let mut use_map = HashMap::new();
        Backend::extract_use_statements_from_statements(
            program.statements.iter(),
            &mut use_map,
            None,
        );
        let finder = MessageSourceFinder {
            use_map: &use_map,
            dir,
            aliases,
        };
        for statement in program.statements.iter() {
            mago_syntax::walker::Walker::walk_statement(&finder, statement, out);
        }
    });
}

/// Finds the directories `new MessageSource(…)` is given.
struct MessageSourceFinder<'a> {
    use_map: &'a HashMap<String, String>,
    dir: &'a Path,
    aliases: &'a super::aliases::Aliases,
}

impl MessageSourceFinder<'_> {
    /// Whether `class`, as written, names the message source.
    fn is_message_source(&self, class: &Expression<'_>) -> bool {
        let Expression::Identifier(identifier) = class else {
            return false;
        };
        let written = crate::atom::bytes_to_str(identifier.value());
        let resolved = match written.strip_prefix('\\') {
            Some(fqn) => fqn.to_string(),
            None => {
                let (first, rest) = written.split_once('\\').unwrap_or((written, ""));
                match self.use_map.get(first) {
                    Some(fqn) if rest.is_empty() => fqn.clone(),
                    Some(fqn) => format!("{fqn}\\{rest}"),
                    None => written.to_string(),
                }
            }
        };
        resolved
            .trim_start_matches('\\')
            .eq_ignore_ascii_case(MESSAGE_SOURCE)
    }

    /// The directory a message source is given: a path built from
    /// `__DIR__` and `dirname()`, or one that starts with an alias, written
    /// as it is or passed through `Aliases::get()`.
    fn directory(&self, value: &Expression<'_>) -> Option<PathBuf> {
        let aliased = match value {
            Expression::Call(Call::Method(call)) if matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if name.value.eq_ignore_ascii_case(b"get")) => {
                call.argument_list
                    .arguments
                    .first()
                    .map(|argument| argument.value())
            }
            Expression::Literal(Literal::String(_)) => Some(value),
            _ => None,
        };
        if let Some(Expression::Literal(Literal::String(literal))) = aliased
            && let Some(text) = literal.value.and_then(crate::atom::literal_bytes_to_str)
            && text.starts_with('@')
        {
            return self.aliases.resolve(text).map(PathBuf::from);
        }
        let path = crate::document_links::try_evaluate_path_expr(value, self.dir)?;
        Some(crate::document_links::normalize_path(Path::new(&path)))
    }
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<PathBuf>>
    for MessageSourceFinder<'_>
{
    fn walk_in_instantiation(&self, node: &'ast Instantiation<'arena>, out: &mut Vec<PathBuf>) {
        if !self.is_message_source(node.class) {
            return;
        }
        let Some(argument_list) = &node.argument_list else {
            return;
        };
        let path = argument_list
            .arguments
            .iter()
            .find_map(|argument| match argument {
                Argument::Named(named) if named.name.value == b"path" => Some(named.value),
                Argument::Positional(positional) => Some(positional.value),
                Argument::Named(_) => None,
            });
        if let Some(dir) = path
            .and_then(|value| self.directory(value))
            .map(|dir| crate::util::canonical_path_for_comparison(&dir))
            && !out.contains(&dir)
        {
            out.push(dir);
        }
    }
}

impl Backend {
    /// The project's message files, read on first use from the directories
    /// the project's and the installed packages' DI definitions give their
    /// message sources.
    pub(crate) fn yii_messages(&self) -> Arc<Messages> {
        if let Some(known) = self.yii.messages.lock().as_ref() {
            return Arc::clone(known);
        }
        // Read without the lock held: the aliases and the params take
        // locks of their own.
        let aliases = self.yii_aliases();
        let mut files: Vec<PathBuf> = self.yii_config_plan().config_files().cloned().collect();
        files.extend(self.yii_package_definition_paths());
        files.sort_unstable();
        files.dedup();
        let mut dirs = Vec::new();
        for file in &files {
            message_source_dirs(file, &aliases, &mut dirs);
        }
        let mut messages = Messages::load(&dirs);
        let params = self.yii_params();
        let setting = |name: &str| {
            params
                .get(&super::params::param_key(&[TRANSLATOR_PARAM, name]))
                .and_then(|node| node.string_value())
                .map(str::to_string)
        };
        messages.default_category = setting("defaultCategory");
        messages.locale = setting("locale");
        messages.fallback_locale = setting("fallbackLocale");
        let messages = Arc::new(messages);
        *self.yii.messages.lock() = Some(Arc::clone(&messages));
        *self.yii.message_dirs.write() = dirs;
        messages
    }

    /// Forget the message files, once a configuration file, a message
    /// file, or the installed packages changed.
    pub(crate) fn forget_yii_messages(&self) {
        *self.yii.messages.lock() = None;
    }

    /// Forget the message files when the file at `path` is one of them.
    pub(crate) fn note_yii_message_change(&self, path: &Path) {
        if self.is_yii_project()
            && path.extension().is_some_and(|ext| ext == "php")
            && self
                .yii
                .message_dirs
                .read()
                .iter()
                .any(|dir| path.starts_with(dir))
        {
            self.forget_yii_messages();
        }
    }

    /// The IDs a call translating in `category` can name.
    pub(crate) fn yii_message_ids(&self, category: Option<&str>) -> Vec<String> {
        self.yii_messages().ids(category)
    }

    /// Go-to-definition on a message ID: its entry in the file of each
    /// locale that declares it.
    pub(crate) fn yii_message_definitions(&self, key: &str) -> Vec<Location> {
        let (category, id) = split_message_key(key);
        self.yii_messages()
            .declarations(category, id)
            .filter_map(|(file, message)| {
                Some(Location {
                    uri: crate::util::path_to_url(&file.path)?,
                    range: message.range,
                })
            })
            .collect()
    }

    /// What the hover of a message ID shows: the category it is looked up
    /// in, the message the translator gives it in its own locale with that
    /// locale, and the locales whose files declare it.
    pub(crate) fn yii_message_detail(&self, key: &str) -> MessageDetail {
        let (category, id) = split_message_key(key);
        let messages = self.yii_messages();
        MessageDetail {
            category: messages.category(category).to_string(),
            text: messages
                .default_text(category, id)
                .map(|(locale, text)| (locale.to_string(), text.to_string())),
            locales: messages
                .declarations(category, id)
                .map(|(file, _)| file.locale.clone())
                .collect(),
        }
    }

    /// The keys that name the same message as `key`: a call that leaves its
    /// category to the translator and one that names the default category.
    pub(crate) fn yii_message_key_spellings(&self, key: &str) -> Vec<String> {
        let (category, id) = split_message_key(key);
        let messages = self.yii_messages();
        let default = messages.default_category();
        match category {
            Some(category) if category != default => vec![key.to_string()],
            _ => vec![message_key(None, id), message_key(Some(default), id)],
        }
    }
}

/// What the hover of a message ID shows (see
/// [`Backend::yii_message_detail`]).
pub(crate) struct MessageDetail {
    pub(crate) category: String,
    pub(crate) text: Option<(String, String)>,
    pub(crate) locales: Vec<String>,
}
