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
use crate::symbol_map::LaravelStringKind;

mod context;
mod enumerate;
#[cfg(test)]
mod tests;

use context::*;

/// The icon an editor shows beside a completed string key: whatever the key
/// names is what it should look like.
fn string_key_item_kind(kind: &LaravelStringKind) -> CompletionItemKind {
    match kind {
        LaravelStringKind::Config => CompletionItemKind::PROPERTY,
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
    fn string_key_candidates(&self, kind: &LaravelStringKind) -> Arc<[String]> {
        match kind {
            LaravelStringKind::Route => self.cached_route_names(),
            // Only the keys `config/` declares: a key written at runtime is
            // extracted from the same literal the cursor is inside, so the
            // half-typed name of a `Storage::fake('…')` under the cursor
            // would be offered back as a completion for itself.
            LaravelStringKind::Config => self.cached_config_keys(),
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
    pub(crate) fn try_laravel_string_key_completion(
        &self,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let ctx = detect_laravel_string_key_context(content, position)?;

        // The candidate lists are shared with every other consumer, so only
        // the names the typed prefix keeps are copied out of them.
        let candidates = self.string_key_candidates(&ctx.kind);
        let prefix_lower = ctx.prefix.to_lowercase();
        let matches_prefix =
            |name: &str| prefix_lower.is_empty() || name.to_lowercase().starts_with(&prefix_lower);

        let names: Vec<String> = match ctx.config_sub_prefix {
            // For config-backed attributes like #[Database('mysql')], filter
            // to sub-keys under the relevant config prefix and strip it so
            // the user sees just the connection/store/channel name.
            Some(sub_prefix) => {
                let mut names: Vec<String> = candidates
                    .iter()
                    .filter_map(|key| key.strip_prefix(sub_prefix))
                    // Only show direct children (no dots = leaf key).
                    .filter(|rest| !rest.contains('.') && matches_prefix(rest))
                    .map(str::to_string)
                    .collect();
                names.sort();
                names.dedup();
                names
            }
            None => candidates
                .iter()
                .filter(|name| matches_prefix(name))
                .cloned()
                .collect(),
        };

        // Build the TextEdit range: from the start of the string content
        // (right after the opening quote) to the current cursor position.
        // This replaces the entire typed prefix with the selected name,
        // so dots in the name don't break the editor's word-based filter.
        let start_pos = crate::text_position::offset_to_position(content, ctx.content_start_offset);
        let edit_range = Range {
            start: start_pos,
            end: position,
        };

        let items: Vec<CompletionItem> = names
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let kind = string_key_item_kind(&ctx.kind);
                CompletionItem {
                    label: name.clone(),
                    kind: Some(kind),
                    sort_text: Some(format!("{:05}", i)),
                    filter_text: Some(name.clone()),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range: edit_range,
                        new_text: name,
                    })),
                    ..Default::default()
                }
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }
}
