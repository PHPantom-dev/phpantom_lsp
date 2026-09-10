//! Rename (`textDocument/rename`) and prepare-rename support.
//!
//! When the user triggers a rename on a symbol, the LSP first calls
//! `prepareRename` to validate that the symbol is renameable and to
//! return the range + current name of the symbol.  If the user
//! confirms, `rename` is called with the new name, and we produce a
//! `WorkspaceEdit` that replaces every occurrence across the workspace.
//!
//! The heavy lifting (finding all references) is delegated to the
//! existing `find_references` infrastructure.  This module adds:
//!
//! - Vendor rejection: symbols defined under the vendor directory
//!   cannot be renamed.
//! - Non-renameable symbol rejection: keywords like `self`, `static`,
//!   `parent`, and `$this` cannot be renamed.
//! - Property name fixup: `$this->foo` references need the edit to
//!   replace only `foo`, not the `$` prefix.  Static properties
//!   (`self::$prop`) include the `$` in the source but the rename
//!   should replace the whole `$prop` token consistently.
//! - Use-statement-aware class rename: when renaming a class, the
//!   `use` import FQN is updated (last segment only), aliases are
//!   preserved, and collisions with existing imports are resolved by
//!   introducing an alias.
//! - Staleness guards: every range that becomes a `TextEdit` is checked
//!   against the text of the file it belongs to before the response goes
//!   out.  See [`validate`] for why.
//! - Namespace rename: when renaming a namespace segment, all
//!   `namespace` declarations, `use` statements, and fully-qualified
//!   references across the workspace are updated.  When a PSR-4
//!   mapping exists, `RenameFile` operations are emitted to move
//!   files so the directory structure stays consistent.

mod blade;
mod class;
mod namespace;
mod prepare;
mod validate;

mod tests;

/// What a rename request answers with.
///
/// `Ok(Some(edit))` is the rename, `Ok(None)` means there was nothing
/// renameable under the cursor, and `Err(message)` is a refusal the user
/// needs to read — the editor surfaces it as the request's error.
pub(crate) type RenameOutcome = Result<Option<tower_lsp::lsp_types::WorkspaceEdit>, String>;
