//! Whether PHPantom may run the project's own tooling.
//!
//! PHPStan, PHP_CodeSniffer, PHPMD, Mago and the external formatters all
//! execute code from the project they work on: a script under the
//! Composer bin-dir, a PHPStan bootstrap file, a php-cs-fixer config
//! written in PHP.  Opening a repository must not be enough to run that
//! code, so the language server starts none of them until the workspace
//! is trusted.  Everything else (indexing, completion, PHPantom's own
//! diagnostics) only reads the project and is not gated.
//!
//! A workspace is trusted when, in order:
//!
//! 1. the editor says so, through `workspaceTrusted` in
//!    `initializationOptions` (an explicit `false` is final);
//! 2. the editor only starts language servers in a workspace the user
//!    has already trusted (see [`client_gates_language_servers`]);
//! 3. the user answered for this workspace before, recorded in the
//!    trust store;
//! 4. the global config sets `[trust] all-workspaces = true`.
//!
//! Otherwise the user is asked, once per session, the first time a tool
//! would run.  The CLI commands (`analyze`, `fix`, `format`) are run on
//! purpose and are not gated.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use tower_lsp_server::gen_lsp_types::{MessageActionItem, MessageType};

use crate::Backend;

/// Name of the trust store under the user's data directory.
const STORE_FILE_NAME: &str = "trusted-workspaces.json";

/// The prompt's answers.
const TRUST_ACTION: &str = "Trust";
const DISTRUST_ACTION: &str = "Don't trust";

/// The decision for the current workspace, before the global config's
/// `all-workspaces` is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TrustDecision {
    /// Nobody has said yet.
    #[default]
    Unknown,
    Trusted,
    Untrusted,
}

/// Per-session trust state.  Lives on the `BackendState`, so every clone
/// of a `Backend` sees the same decision.
#[derive(Debug, Default)]
pub(crate) struct WorkspaceTrust {
    decision: Mutex<TrustDecision>,
    /// Set once the user has been asked this session, answered or not,
    /// so a dismissed prompt is not raised again on every save.
    prompted: AtomicBool,
    /// Set when the workspace diagnostic pass skipped its external tools
    /// for want of trust, so granting it can run them after all.
    workspace_tools_deferred: AtomicBool,
}

impl WorkspaceTrust {
    pub(crate) fn decision(&self) -> TrustDecision {
        *self.decision.lock()
    }

    pub(crate) fn set_decision(&self, decision: TrustDecision) {
        *self.decision.lock() = decision;
    }

    /// Record that the workspace pass skipped its external tools.
    pub(crate) fn defer_workspace_tools(&self) {
        self.workspace_tools_deferred.store(true, Ordering::Release);
    }
}

/// The trust store's location: `trusted-workspaces.json` under the
/// platform's data directory (`~/.local/share/phpantom_lsp/` on Linux and
/// macOS, `%APPDATA%\phpantom_lsp\` on Windows).
pub fn trust_store_path() -> Option<PathBuf> {
    use etcetera::BaseStrategy as _;
    etcetera::choose_base_strategy()
        .ok()
        .map(|s| s.data_dir().join("phpantom_lsp").join(STORE_FILE_NAME))
}

/// The key a workspace is stored under: its canonical path, so the same
/// folder reached through a symlink or another spelling shares one entry.
fn store_key(root: &Path) -> String {
    dunce::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Read the store.  A missing or unreadable file is an empty store: the
/// worst outcome is being asked again.
fn read_store(store: &Path) -> BTreeMap<String, bool> {
    std::fs::read_to_string(store)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The recorded answer for `root`, if any.
pub fn stored_decision(store: &Path, root: &Path) -> Option<bool> {
    read_store(store).get(&store_key(root)).copied()
}

/// Record `trusted` for `root`, or forget the answer when `None`.
pub fn record_decision(store: &Path, root: &Path, trusted: Option<bool>) -> std::io::Result<()> {
    let mut entries = read_store(store);
    let key = store_key(root);
    match trusted {
        Some(trusted) => {
            entries.insert(key, trusted);
        }
        None => {
            entries.remove(&key);
        }
    }
    if let Some(parent) = store.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&entries).map_err(std::io::Error::other)?;
    std::fs::write(store, text + "\n")
}

/// Whether the editor named by `clientInfo` only starts language servers
/// in a workspace the user has already trusted, so a server it started is
/// running in a trusted workspace.
///
/// Zed opens new worktrees in Restricted Mode, where it starts no
/// language servers, since 0.218.
///
/// Claude Code is not on the list: its folder-trust question is skipped
/// in non-interactive (`-p`) runs, which is how it runs in CI and bots
/// against checkouts nobody has reviewed.
///
/// VS Code gates extensions too, but whether a given extension runs in
/// Restricted Mode is the extension's own declaration, so VS Code
/// extensions say it through `workspaceTrusted` instead.
pub(crate) fn client_gates_language_servers(name: &str, version: Option<&str>) -> bool {
    let is_zed = name
        .get(.."Zed".len())
        .is_some_and(|head| head.eq_ignore_ascii_case("Zed"));
    is_zed && version.and_then(parse_major_minor) >= Some((0, 218))
}

/// The leading `major.minor` of a version string such as `0.218.2` or
/// `0.219.0+preview.1`.
fn parse_major_minor(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split(['.', '+', '-']);
    let major = parts.next()?.trim().parse().ok()?;
    let minor = parts.next()?.trim().parse().ok()?;
    Some((major, minor))
}

/// The editor's own trust verdict from `initializationOptions`, read as
/// `workspaceTrusted` at the top level or under a `phpantom` key.
pub(crate) fn client_trust_option(options: &serde_json::Value) -> Option<bool> {
    let scoped = options.get("phpantom").unwrap_or(options);
    scoped.get("workspaceTrusted")?.as_bool()
}

impl Backend {
    /// Settle what can be settled about trust at `initialize`: the
    /// editor's verdict, an editor that gates servers itself, and the
    /// store's answer for the workspace root.
    pub(crate) fn init_workspace_trust(
        &self,
        client_trust: Option<bool>,
        client_gates: bool,
        root: Option<&Path>,
    ) {
        let decision = match client_trust {
            Some(true) => TrustDecision::Trusted,
            Some(false) => TrustDecision::Untrusted,
            None if client_gates => TrustDecision::Trusted,
            None => self
                .workspace
                .trust_store_path
                .as_deref()
                .zip(root)
                .and_then(|(store, root)| stored_decision(store, root))
                .map_or(TrustDecision::Unknown, |trusted| {
                    if trusted {
                        TrustDecision::Trusted
                    } else {
                        TrustDecision::Untrusted
                    }
                }),
        };
        self.workspace_trust.set_decision(decision);
    }

    /// Whether the project's own tooling may run now.
    ///
    /// A `false` from an undecided workspace also raises the prompt (once
    /// per session), so a caller only has to skip the tool; granting
    /// trust re-runs what was skipped.
    pub(crate) fn may_run_project_tools(&self) -> bool {
        match self.workspace_trust.decision() {
            TrustDecision::Trusted => true,
            TrustDecision::Untrusted => false,
            TrustDecision::Unknown => {
                if self.config().trust.trusts_all_workspaces() {
                    return true;
                }
                self.request_workspace_trust();
                false
            }
        }
    }

    /// Ask the user whether to trust the workspace, unless that has
    /// already happened this session.
    fn request_workspace_trust(&self) {
        if self.client.is_none() || self.workspace_trust.prompted.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let backend = self.clone_for_blocking();
        runtime.spawn(async move { backend.prompt_for_workspace_trust().await });
    }

    async fn prompt_for_workspace_trust(&self) {
        let Some(client) = &self.client else {
            return;
        };
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return;
        };
        let message = format!(
            "Allow PHPantom to run this project's tools (PHPStan, PHP_CodeSniffer, formatters \
             and the like) in {}? They execute code from the project, so only allow this for a \
             project you trust.",
            root.display()
        );
        let actions = vec![
            MessageActionItem {
                title: TRUST_ACTION.to_string(),
                properties: Default::default(),
            },
            MessageActionItem {
                title: DISTRUST_ACTION.to_string(),
                properties: Default::default(),
            },
        ];
        let answer = client
            .show_message_request(MessageType::Warning, message, Some(actions))
            .await;
        let trusted = match answer {
            Ok(Some(item)) if item.title == TRUST_ACTION => true,
            Ok(Some(item)) if item.title == DISTRUST_ACTION => false,
            // Dismissed, or a client that cannot ask.
            _ => {
                self.log(
                    MessageType::Info,
                    "Project tools such as PHPStan are not run until this workspace is \
                     trusted. Run `phpantom_lsp trust` in the project, or set \
                     `[trust] all-workspaces = true` in the global config."
                        .to_string(),
                )
                .await;
                return;
            }
        };

        if let Some(store) = self.workspace.trust_store_path.as_deref()
            && let Err(e) = record_decision(store, &root, Some(trusted))
        {
            self.log(
                MessageType::Warning,
                format!(
                    "Could not save the trust decision to {}: {e}",
                    store.display()
                ),
            )
            .await;
        }

        if trusted {
            self.workspace_trust.set_decision(TrustDecision::Trusted);
            self.run_project_tools_after_trust();
        } else {
            self.workspace_trust.set_decision(TrustDecision::Untrusted);
        }
    }

    /// Run the tools that were skipped while the workspace was undecided.
    fn run_project_tools_after_trust(&self) {
        let open: Vec<String> = self.open_files.read().keys().cloned().collect();
        for uri in open {
            self.schedule_external_diagnostics(uri);
        }
        if self
            .workspace_trust
            .workspace_tools_deferred
            .swap(false, Ordering::AcqRel)
        {
            self.rerun_workspace_external_tools();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zed_gates_from_the_release_that_added_restricted_mode() {
        for (name, version, gates) in [
            ("Zed", Some("0.218.2"), true),
            ("Zed Preview", Some("0.219.0+preview.1"), true),
            ("Zed", Some("1.0.0"), true),
            ("Zed", Some("0.217.9"), false),
            ("Zed", None, false),
            ("Claude Code", Some("2.1.0"), false),
            ("Visual Studio Code", Some("1.100.0"), false),
            ("Neovim", Some("0.11.0"), false),
            ("", None, false),
        ] {
            assert_eq!(
                client_gates_language_servers(name, version),
                gates,
                "{name} {version:?}"
            );
        }
    }

    #[test]
    fn client_trust_is_read_bare_or_under_phpantom() {
        let bare = serde_json::json!({ "workspaceTrusted": true });
        let scoped = serde_json::json!({ "phpantom": { "workspaceTrusted": false } });
        let absent = serde_json::json!({ "inlayHints": {} });
        assert_eq!(client_trust_option(&bare), Some(true));
        assert_eq!(client_trust_option(&scoped), Some(false));
        assert_eq!(client_trust_option(&absent), None);
    }

    #[test]
    fn store_round_trips_and_forgets() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("data").join(STORE_FILE_NAME);
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();

        assert_eq!(stored_decision(&store, &project), None);
        record_decision(&store, &project, Some(true)).unwrap();
        assert_eq!(stored_decision(&store, &project), Some(true));
        record_decision(&store, &project, Some(false)).unwrap();
        assert_eq!(stored_decision(&store, &project), Some(false));
        record_decision(&store, &project, None).unwrap();
        assert_eq!(stored_decision(&store, &project), None);
    }

    #[test]
    fn editor_verdict_wins_over_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join(STORE_FILE_NAME);
        record_decision(&store, dir.path(), Some(true)).unwrap();

        let mut backend = Backend::new_test();
        backend.state_mut().workspace.trust_store_path = Some(store);

        backend.init_workspace_trust(Some(false), false, Some(dir.path()));
        assert_eq!(backend.workspace_trust.decision(), TrustDecision::Untrusted);
        assert!(!backend.may_run_project_tools());

        backend.init_workspace_trust(None, false, Some(dir.path()));
        assert_eq!(backend.workspace_trust.decision(), TrustDecision::Trusted);
        assert!(backend.may_run_project_tools());
    }

    #[test]
    fn undecided_workspace_runs_nothing_unless_trust_is_global() {
        let backend = Backend::new_test();
        backend.init_workspace_trust(None, false, None);
        assert!(!backend.may_run_project_tools());

        let mut config = backend.config();
        config.trust.all_workspaces = Some(true);
        backend.set_config(config);
        assert!(backend.may_run_project_tools());
    }

    #[test]
    fn gated_client_is_trusted_without_asking() {
        let backend = Backend::new_test();
        backend.init_workspace_trust(None, true, None);
        assert!(backend.may_run_project_tools());
    }
}
