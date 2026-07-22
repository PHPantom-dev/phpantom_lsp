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
//! - Namespace rename: when renaming a namespace segment, all
//!   `namespace` declarations, `use` statements, and fully-qualified
//!   references across the workspace are updated.  When a PSR-4
//!   mapping exists, `RenameFile` operations are emitted to move
//!   files so the directory structure stays consistent.

mod class;
mod namespace;
mod prepare;

mod tests;
