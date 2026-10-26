//! `[diagnostics]` section: diagnostic toggles and ignore rules.

use serde::Deserialize;

/// `[diagnostics]` section — toggle individual diagnostic providers.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DiagnosticsConfig {
    /// Show diagnostics in the editor at all.
    ///
    /// On by default. Set to `false` and PHPantom reports nothing: no
    /// native diagnostics (syntax errors included), no external tool
    /// runs, and no workspace pass. Every other key in this section is
    /// then moot. The `analyze` CLI ignores it, since running it is an
    /// explicit request for diagnostics.
    pub enabled: Option<bool>,

    /// Report member access on a subject whose type cannot answer for it.
    ///
    /// Off by default. When enabled, PHPantom emits a hint-level
    /// diagnostic on every `->`, `?->`, or `::` access where the subject
    /// is `mixed` or where its type could not be worked out at all. The
    /// message says which, since the first is an annotation missing from
    /// the codebase and the second a gap in PHPantom's own inference.
    /// This is useful for discovering gaps in type coverage but produces
    /// too many diagnostics on codebases without comprehensive type
    /// annotations.
    #[serde(rename = "unresolved-member-access")]
    pub unresolved_member_access: Option<bool>,

    /// Report calls that pass more arguments than the function accepts.
    ///
    /// Off by default. PHP does not error on extra arguments to
    /// user-defined functions (the extras are silently ignored), and
    /// many libraries exploit this for flexible APIs. Enable this if
    /// you want stricter checking.
    #[serde(rename = "extra-arguments")]
    pub extra_arguments: Option<bool>,

    /// Report property access on classes with `__get` when virtual
    /// properties are defined.
    ///
    /// Off by default. When enabled, classes that have `__get` but
    /// also declare virtual properties (via `@property` docblock tags,
    /// Laravel Eloquent column inference, or any other virtual member
    /// provider) will flag unknown property access instead of
    /// suppressing it. This matches PHPStan's `reportMagicProperties`
    /// behaviour.
    #[serde(rename = "report-magic-properties")]
    pub report_magic_properties: Option<bool>,

    /// Report a member that only some classes of a union declare.
    ///
    /// Off by default. The unknown-member check stays silent as long as
    /// any class the subject may hold declares the member. When enabled,
    /// PHPantom also reports the access when some of those classes do
    /// not, since the access fails whenever one of them arrives. This
    /// matches PHPStan's `checkUnionTypes` (level 7), and is only as
    /// precise as the inferred union: a variable whose classes are
    /// correlated with something the type engine does not track will
    /// be reported too.
    #[serde(rename = "partial-union-members")]
    pub partial_union_members: Option<bool>,

    /// Compute diagnostics for the whole workspace in the background.
    ///
    /// Off by default. When enabled (which requires the default `full`
    /// indexing strategy), PHPantom runs its native diagnostic
    /// collectors over every user file in the workspace once the initial
    /// startup and the full background index finish — not just the files
    /// open in the editor — so project-wide problems appear in the
    /// editor's problems panel. The pass is throttled to leave CPU
    /// headroom for interactive requests, but it still costs a
    /// project-wide sweep on every session. While it is off, only open
    /// files are diagnosed.
    pub workspace: Option<bool>,

    /// Run configured external tools (PHPStan, PHPCS, PHPMD, Mago) once
    /// over the whole project after workspace diagnostics finish.
    ///
    /// On by default, but it only takes effect when `workspace` is
    /// enabled, since the project-wide run is chained onto that pass.
    /// Each tool only runs when it is enabled, resolvable, and has its
    /// own project-level configuration file (`phpstan.neon`,
    /// `phpcs.xml`, `phpmd.yml`, `mago.toml`) so the tool itself decides which paths
    /// to analyse. Set to `false` to keep external tools per-file only.
    #[serde(rename = "workspace-external")]
    pub workspace_external: Option<bool>,

    /// Downgrade `type_mismatch_argument` to a warning when the only
    /// reason an argument's type fails to satisfy its parameter is a stray
    /// `null` (every non-null member of the argument's type is already
    /// compatible).
    ///
    /// Off by default, matching current (error) behaviour.
    #[serde(rename = "downgrade-nullable-argument-mismatch")]
    pub downgrade_nullable_argument_mismatch: Option<bool>,

    /// Rules that suppress matching diagnostics, similar to PHPStan's
    /// `ignoreErrors`.
    ///
    /// Each rule may constrain by `message` (regex), `path` (glob,
    /// relative to the workspace root), and/or `identifier` (the
    /// diagnostic code, e.g. `"unused_import"`). A diagnostic is
    /// suppressed when it matches every constraint present on a rule;
    /// omitted constraints match anything. A rule with no constraints
    /// at all is rejected (it would silently suppress every
    /// diagnostic in the project).
    ///
    /// ```toml
    /// [[diagnostics.ignore]]
    /// path = "tests/**"
    ///
    /// [[diagnostics.ignore]]
    /// identifier = "deprecated_usage"
    /// message = "^Call to deprecated function some_legacy_helper\\(\\)"
    /// ```
    pub ignore: Vec<IgnoreRule>,
}

/// A single `[[diagnostics.ignore]]` rule.
///
/// See [`DiagnosticsConfig::ignore`] for the matching semantics.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct IgnoreRule {
    /// Regex matched against the diagnostic message.
    pub message: Option<String>,
    /// Glob matched against the file path, relative to the workspace
    /// root. Follows gitignore-style glob syntax: `*` does not cross
    /// `/`, use `**` to match across directories.
    pub path: Option<String>,
    /// Diagnostic identifier (the diagnostic `code`, e.g.
    /// `"unused_import"`, `"deprecated_usage"`).
    pub identifier: Option<String>,
}

impl DiagnosticsConfig {
    /// Whether the editor is shown any diagnostics.
    ///
    /// Defaults to `true` (on) when not explicitly set.
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// Whether the unresolved-member-access diagnostic is enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set.
    pub fn unresolved_member_access_enabled(&self) -> bool {
        self.unresolved_member_access.unwrap_or(false)
    }

    /// Whether the extra-arguments diagnostic is enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set.
    pub fn extra_arguments_enabled(&self) -> bool {
        self.extra_arguments.unwrap_or(false)
    }

    /// Whether magic property reporting is enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set.
    pub fn report_magic_properties_enabled(&self) -> bool {
        self.report_magic_properties.unwrap_or(false)
    }

    /// Whether the partial-union-members diagnostic is enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set.
    pub fn partial_union_members_enabled(&self) -> bool {
        self.partial_union_members.unwrap_or(false)
    }

    /// Whether background workspace diagnostics are enabled.
    ///
    /// Defaults to `false` (off) when not explicitly set, and is off
    /// whenever diagnostics as a whole are switched off.
    pub fn workspace_enabled(&self) -> bool {
        self.enabled() && self.workspace.unwrap_or(false)
    }

    /// Whether project-wide external tool runs are enabled.
    ///
    /// Defaults to `true` (on) when not explicitly set, though it only
    /// has an effect when [`workspace`](Self::workspace) is enabled.
    pub fn workspace_external_enabled(&self) -> bool {
        self.workspace_external.unwrap_or(true)
    }

    /// Whether a nullability-only argument type mismatch is downgraded
    /// to a warning.
    pub fn downgrade_nullable_argument_mismatch_enabled(&self) -> bool {
        self.downgrade_nullable_argument_mismatch.unwrap_or(false)
    }
}
