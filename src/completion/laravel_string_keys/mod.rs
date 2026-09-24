//! Laravel string key completion.
//!
//! Offers autocompletion for the keys a Laravel call site names, inside
//! whichever helper, facade method, or attribute names them:
//!
//! - `route('|')` / `URL::signedRoute('|')` / `Route::is('|')` → route names
//! - `config('|')` / `Config::get('|')` → config keys
//! - `view('|')` / `View::make('|')` → view names
//! - `__('|')` / `trans('|')` / `Lang::get('|')` → translation keys
//! - `env('|')` / `Env::get('|')` → environment variables

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::{LaravelConfigResource, LaravelStringKind};
use crate::types::FileContext;

mod context;
mod enumerate;
#[cfg(test)]
mod tests;

use context::*;

/// The icon an editor shows beside a completed string key: whatever the key
/// names is what it should look like.
fn string_key_item_kind(kind: &LaravelStringKind) -> CompletionItemKind {
    match kind {
        LaravelStringKind::Config | LaravelStringKind::ConfigResource(_) => {
            CompletionItemKind::PROPERTY
        }
        LaravelStringKind::View => CompletionItemKind::FILE,
        LaravelStringKind::Trans => CompletionItemKind::TEXT,
        LaravelStringKind::MorphAlias => CompletionItemKind::ENUM_MEMBER,
        LaravelStringKind::GateAbility => CompletionItemKind::METHOD,
        LaravelStringKind::Env => CompletionItemKind::CONSTANT,
        LaravelStringKind::Route
        | LaravelStringKind::Command
        | LaravelStringKind::Section
        | LaravelStringKind::Stack
        | LaravelStringKind::ContainerBinding => CompletionItemKind::VALUE,
    }
}

impl Backend {
    /// Every name a string key of `kind` could be, unfiltered.
    ///
    /// Three kinds have no list to offer. A Blade section or stack name is
    /// completed from the raw template instead
    /// (`crate::completion::handler::blade_block_name`): what a name may be
    /// depends on the layouts above the file, and the edit has to land in
    /// Blade coordinates rather than in the virtual PHP this detection reads.
    /// A container binding key is written where a class name is equally
    /// valid, which ordinary class completion already offers, and the set of
    /// keys is open besides — a list of them would read as the whole answer
    /// when it is not.
    fn string_key_candidates(
        &self,
        kind: &LaravelStringKind,
        config_sub_prefix: Option<&str>,
        typed_prefix: &str,
    ) -> Arc<[String]> {
        match kind {
            LaravelStringKind::Route => self.cached_route_names(),
            LaravelStringKind::Config => self.cached_config_keys(),
            LaravelStringKind::ConfigResource(resource) => {
                let prefix = config_sub_prefix.expect("config resources always have a prefix");
                // Runtime writes can contain the half-typed name under the
                // cursor, so offer only names declared by config files.
                let keys = self.cached_config_keys();
                let first = keys.partition_point(|key| key.as_str() < prefix);
                let mut names: Vec<String> = keys[first..]
                    .iter()
                    .take_while(|key| key.starts_with(prefix))
                    .filter_map(|key| {
                        let name = key.strip_prefix(prefix)?;
                        (!name.contains('.')).then(|| name.to_string())
                    })
                    .collect();
                if crate::symbol_map::laravel_resources::is_implicit_resource_name(
                    *resource, "null",
                ) && let Err(index) = names.binary_search_by(|name| name.as_str().cmp("null"))
                {
                    names.insert(index, "null".to_string());
                }
                if *resource == LaravelConfigResource::DatabaseConnection
                    && typed_prefix.contains("::")
                {
                    let mut variants = Vec::with_capacity(
                        names.len()
                            * crate::symbol_map::laravel_resources::DATABASE_ROLE_SUFFIXES.len(),
                    );
                    for name in names {
                        for suffix in crate::symbol_map::laravel_resources::DATABASE_ROLE_SUFFIXES {
                            let mut variant = String::with_capacity(name.len() + suffix.len());
                            variant.push_str(&name);
                            variant.push_str(suffix);
                            variants.push(variant);
                        }
                    }
                    variants.into()
                } else {
                    names.into()
                }
            }
            LaravelStringKind::View => self.cached_view_names(),
            LaravelStringKind::Trans => self.cached_trans_keys(),
            LaravelStringKind::Command => self.laravel_commands.read().all_names().into(),
            LaravelStringKind::MorphAlias => {
                let mut aliases = self.laravel_morph_map.read().all_aliases();
                aliases.sort();
                aliases.into()
            }
            LaravelStringKind::GateAbility => self.cached_gate_abilities(),
            LaravelStringKind::Env => {
                crate::virtual_members::laravel::enumerate_env_keys(self).into()
            }
            LaravelStringKind::Section
            | LaravelStringKind::Stack
            | LaravelStringKind::ContainerBinding => Arc::new([]),
        }
    }

    /// Try Laravel string key completion.
    ///
    /// Detects the cursor inside a supported string argument of `route()`,
    /// `config()`, `Storage::forgetDisk()`, etc. and offers matching names.
    #[cfg(test)]
    pub(crate) fn try_laravel_string_key_completion(
        &self,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        self.try_laravel_string_key_completion_inner(content, position, None, None, None)
    }

    /// Live-request form of Laravel string-key completion. Resolved names
    /// distinguish imported facade aliases from namespace-local homonyms.
    pub(crate) fn try_laravel_string_key_completion_in_file(
        &self,
        content: &str,
        position: Position,
        file_ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let indexed_function_exists = |name: &str| self.has_indexed_function(name);
        let indexed_class_exists = |name: &str| self.has_indexed_class(name);
        self.try_laravel_string_key_completion_inner(
            content,
            position,
            file_ctx.resolved_names.as_deref(),
            Some(&indexed_function_exists),
            Some(&indexed_class_exists),
        )
    }

    fn try_laravel_string_key_completion_inner(
        &self,
        content: &str,
        position: Position,
        resolved_names: Option<&crate::names::OwnedResolvedNames>,
        indexed_function_exists: Option<&dyn Fn(&str) -> bool>,
        indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
    ) -> Option<CompletionResponse> {
        let ctx = detect_laravel_string_key_context_inner(
            content,
            position,
            resolved_names,
            indexed_function_exists,
            indexed_class_exists,
        )?;

        let candidates = self.string_key_candidates(&ctx.kind, ctx.config_sub_prefix, ctx.prefix);

        // Build the TextEdit range: from the start of the string content
        // (right after the opening quote) to the current cursor position.
        // This replaces the entire typed prefix with the selected name,
        // so dots in the name don't break the editor's word-based filter.
        let start_pos = crate::text_position::offset_to_position(content, ctx.content_start_offset);
        let edit_range = Range {
            start: start_pos,
            end: position,
        };

        let prefix = ctx.prefix.as_bytes();
        let item_kind = string_key_item_kind(&ctx.kind);
        let items: Vec<CompletionItem> = candidates
            .iter()
            .filter(|name| {
                name.as_bytes()
                    .get(..prefix.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            })
            .cloned()
            .enumerate()
            .map(|(i, name)| CompletionItem {
                label: name.clone(),
                kind: Some(item_kind),
                sort_text: Some(format!("{:05}", i)),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: edit_range,
                    new_text: name,
                })),
                ..Default::default()
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }
}
