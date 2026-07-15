//! Change Visibility code action.
//!
//! When the cursor is on a method, property, constant, or promoted
//! constructor parameter that has an explicit visibility modifier, this
//! code action offers to change it to each of the other two visibility
//! levels — **filtered by parent constraints**.
//!
//! If the member overrides a parent/interface member, alternatives that
//! would be more restrictive than the parent are suppressed.  For
//! example, a `private` method that overrides a `public` parent method
//! only offers "Make public" (not "Make protected").
//!
//! When a PHPStan `method.visibility` or `property.visibility`
//! diagnostic exists on the line, the matching action is promoted to
//! `quickfix` kind with `is_preferred: true` and the diagnostic is
//! attached so it is cleared on resolve.
//!
//! **Code action kind:** `refactor.rewrite` (or `quickfix` when driven
//! by a PHPStan diagnostic).
//!
//! All actions use two-phase resolve so that PHPStan diagnostic
//! clearing works through the standard `codeAction/resolve` pipeline.
//!
//! This is a single-file edit — it does not update call sites or
//! subclass overrides in other files.

use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
use mago_allocator::LocalArena;
use mago_span::HasSpan;
use mago_syntax::cst::class_like::property::Property;
use mago_syntax::cst::modifier::Modifier;
use tower_lsp::lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::types::{ClassInfo, Visibility};
use crate::util::offset_to_position;

/// The action kind used for the deferred resolve dispatch.
const ACTION_KIND: &str = "refactor.changeVisibility";

/// PHPStan identifiers we look for.
const METHOD_VISIBILITY_ID: &str = "method.visibility";
const PROPERTY_VISIBILITY_ID: &str = "property.visibility";

/// A visibility modifier found in the AST together with its byte span.
struct VisibilityHit {
    /// The current visibility keyword text (e.g. "public").
    current: &'static str,
    /// Byte offset of the start of the visibility keyword.
    start: u32,
    /// Byte offset of the end of the visibility keyword.
    end: u32,
}

/// Information about the member under the cursor, used for parent lookup.
#[derive(Debug, Clone)]
enum MemberKind {
    Method(String),
    Property(String),
    Constant(String),
}

/// Minimum visibility required by the parent hierarchy.
///
/// `None` means no constraint (all three visibilities are valid).
fn min_visibility_level(vis: &Visibility) -> u8 {
    match vis {
        Visibility::Public => 2,
        Visibility::Protected => 1,
        Visibility::Private => 0,
    }
}

fn visibility_level(keyword: &str) -> u8 {
    match keyword {
        "public" => 2,
        "protected" => 1,
        _ => 0,
    }
}

impl Backend {
    /// Collect "Change visibility" code actions for the member under the
    /// cursor.
    ///
    /// **Phase 1**: emits lightweight `CodeAction` values with a `data`
    /// payload but **no `edit`**.  The edit is computed lazily in
    /// [`resolve_change_visibility`](Self::resolve_change_visibility).
    pub(crate) fn collect_change_visibility_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let cursor_offset = crate::util::position_to_offset(content, params.range.start);

        // Resolve the cursor context once and extract the owned data the
        // rest of this function needs (the AST does not escape the closure).
        let Some((hit, member_kind, member_lines)) =
            crate::parser::with_parsed_program(content, "change_visibility", |program, content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);
                let hit = find_visibility_from_context(&ctx, cursor_offset)?;
                // Member kind for parent lookup and the member's line range
                // (used below to find an overlapping PHPStan diagnostic).
                let member_kind = extract_member_kind(&ctx);
                let member_lines = member_line_range(&ctx, content);
                Some((hit, member_kind, member_lines))
            })
        else {
            return;
        };

        // ── Parent-aware filtering ──────────────────────────────────
        // Find the minimum visibility required by the parent hierarchy.
        let min_level = member_kind
            .as_ref()
            .and_then(|mk| self.find_parent_min_visibility(uri, content, cursor_offset, mk))
            .unwrap_or(0); // no constraint

        let all_alternatives: &[(&str, &str)] = match hit.current {
            "public" => &[("protected", "Make protected"), ("private", "Make private")],
            "protected" => &[("public", "Make public"), ("private", "Make private")],
            "private" => &[("public", "Make public"), ("protected", "Make protected")],
            _ => return,
        };

        // Filter: only keep alternatives whose visibility level >= min_level.
        let alternatives: Vec<(&str, &str)> = all_alternatives
            .iter()
            .filter(|&&(kw, _)| visibility_level(kw) >= min_level)
            .copied()
            .collect();

        if alternatives.is_empty() {
            return;
        }

        // ── Check for PHPStan visibility diagnostics on the member ──
        // The diagnostic may land on an attribute line (e.g. #[Override])
        // rather than the method signature line, so search the full
        // line range of the member declaration.
        let phpstan_diag = self.find_visibility_diagnostic(uri, member_lines);

        // Parse the PHPStan message to determine which visibilities are
        // the "correct" targets so we can mark them as preferred.
        let phpstan_targets: Vec<String> = phpstan_diag
            .as_ref()
            .and_then(|d| parse_phpstan_visibility_targets(&d.message))
            .unwrap_or_default();

        for &(new_keyword, title) in &alternatives {
            let is_phpstan_target = phpstan_targets.iter().any(|t| t == new_keyword);

            // When a PHPStan diagnostic drives this action, only the
            // matching targets are emitted as quickfixes; the others
            // remain refactoring actions.
            let (kind, is_preferred, diagnostics) = if is_phpstan_target {
                (
                    CodeActionKind::QUICKFIX,
                    // The most-restrictive valid target is preferred.
                    Some(phpstan_targets.first().is_some_and(|t| t == new_keyword)),
                    phpstan_diag.as_ref().map(|d| vec![d.clone()]),
                )
            } else {
                (CodeActionKind::new("refactor.rewrite"), None, None)
            };

            let extra = serde_json::json!({
                "target_visibility": new_keyword,
                "vis_start": hit.start,
                "vis_end": hit.end,
            });

            let data = make_code_action_data(ACTION_KIND, uri, &params.range, extra);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: title.to_string(),
                kind: Some(kind),
                diagnostics,
                edit: None,
                command: None,
                is_preferred,
                disabled: None,
                data: Some(data),
            }));
        }
    }

    /// Resolve a "Change visibility" code action by computing the full
    /// workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.
    pub(crate) fn resolve_change_visibility(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let target_vis = data.extra.get("target_visibility")?.as_str()?;
        let vis_start = data.extra.get("vis_start")?.as_u64()? as usize;
        let vis_end = data.extra.get("vis_end")?.as_u64()? as usize;

        // Sanity check: the byte range should still point at a valid
        // visibility keyword in the current content.
        if vis_end > content.len() {
            return None;
        }
        let current_keyword = &content[vis_start..vis_end];
        if !matches!(current_keyword, "public" | "protected" | "private") {
            return None;
        }
        // If the keyword already matches the target, the action is stale.
        if current_keyword == target_vis {
            return None;
        }

        let start_pos = offset_to_position(content, vis_start);
        let end_pos = offset_to_position(content, vis_end);

        let doc_uri: Url = uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(
            doc_uri,
            vec![TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: target_vis.to_string(),
            }],
        );

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    // ── Private helpers ─────────────────────────────────────────────────

    /// Find the minimum visibility required for a member by walking the
    /// parent class chain and implemented interfaces.
    ///
    /// Returns the visibility level (0=private, 1=protected, 2=public)
    /// of the most-permissive parent declaration found, or `None` if the
    /// member is not found in any ancestor.
    fn find_parent_min_visibility(
        &self,
        uri: &str,
        _content: &str,
        cursor_offset: u32,
        member_kind: &MemberKind,
    ) -> Option<u8> {
        // Find the enclosing ClassInfo from the uri_classes_index.
        let local_classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let enclosing = crate::util::find_class_at_offset(&local_classes, cursor_offset)?;

        // Collect all ancestors: parent class chain + interfaces + traits.
        let mut best_level: Option<u8> = None;

        // Walk the parent class chain.
        if let Some(ref parent_name) = enclosing.parent_class {
            self.walk_ancestor_visibility(parent_name, member_kind, &mut best_level, 0);
        }

        // Interfaces always require public.
        for iface_name in &enclosing.interfaces {
            self.walk_interface_visibility(iface_name, member_kind, &mut best_level, 0);
        }

        best_level
    }

    /// Recursively walk the parent class chain looking for the member
    /// and recording its visibility.
    fn walk_ancestor_visibility(
        &self,
        class_name: &str,
        member_kind: &MemberKind,
        best_level: &mut Option<u8>,
        depth: usize,
    ) {
        if depth > 20 {
            return; // guard against cycles
        }
        let cls = match self.find_or_load_class(class_name) {
            Some(c) => c,
            None => return,
        };

        if let Some(level) = find_member_visibility_in_class(&cls, member_kind) {
            *best_level = Some(best_level.map_or(level, |prev: u8| prev.max(level)));
        }

        // Continue up the chain.
        if let Some(ref parent) = cls.parent_class {
            self.walk_ancestor_visibility(parent, member_kind, best_level, depth + 1);
        }

        // Also check interfaces of this parent (they require public).
        for iface in &cls.interfaces {
            self.walk_interface_visibility(iface, member_kind, best_level, depth + 1);
        }
    }

    /// Check if an interface (or its parents) declares the member.
    /// Interface members are always public.
    fn walk_interface_visibility(
        &self,
        iface_name: &str,
        member_kind: &MemberKind,
        best_level: &mut Option<u8>,
        depth: usize,
    ) {
        if depth > 20 {
            return;
        }
        let cls = match self.find_or_load_class(iface_name) {
            Some(c) => c,
            None => return,
        };

        let has_member = match member_kind {
            MemberKind::Method(name) => cls.has_method(name),
            MemberKind::Constant(name) => cls.constants.iter().any(|c| c.name == *name),
            MemberKind::Property(_) => false, // interfaces don't have properties
        };

        if has_member {
            // Interface members are always public.
            *best_level = Some(best_level.map_or(2, |prev: u8| prev.max(2)));
        }

        // Walk parent interfaces.
        for parent_iface in &cls.interfaces {
            self.walk_interface_visibility(parent_iface, member_kind, best_level, depth + 1);
        }
    }

    /// Find a PHPStan `method.visibility` or `property.visibility`
    /// diagnostic on any line within the given range.
    ///
    /// PHPStan may report the error on an attribute line (e.g.
    /// `#[Override]`) rather than the method signature line, so we
    /// search the full span of the member declaration.
    fn find_visibility_diagnostic(&self, uri: &str, line_range: (u32, u32)) -> Option<Diagnostic> {
        let cache = self.phpstan_last_diags.lock();
        let diags = cache.get(uri)?;
        let (start_line, end_line) = line_range;
        diags
            .iter()
            .find(|d| {
                let diag_line = d.range.start.line;
                let in_range = diag_line >= start_line && diag_line <= end_line;
                let is_vis_id = match &d.code {
                    Some(NumberOrString::String(s)) => {
                        s == METHOD_VISIBILITY_ID || s == PROPERTY_VISIBILITY_ID
                    }
                    _ => false,
                };
                in_range && is_vis_id
            })
            .cloned()
    }
}

// ── Member line range extraction ────────────────────────────────────────────

/// Return the (start_line, end_line) of the member under the cursor,
/// in 0-based LSP line numbers.
///
/// This covers the full span including attributes, so a PHPStan
/// diagnostic on an `#[Override]` line above the method signature
/// is included.
fn member_line_range(ctx: &CursorContext<'_>, content: &str) -> (u32, u32) {
    let span = match ctx {
        CursorContext::InClassLike { member, .. } => match member {
            MemberContext::Method(method, _) => Some(method.span()),
            MemberContext::Property(property) => Some(property.span()),
            MemberContext::Constant(constant) => Some(constant.span()),
            _ => None,
        },
        _ => None,
    };

    match span {
        Some(s) => {
            let start = offset_to_position(content, s.start.offset as usize);
            let end = offset_to_position(content, s.end.offset as usize);
            (start.line, end.line)
        }
        None => (0, 0),
    }
}

// ── PHPStan message parsing ─────────────────────────────────────────────────

/// Parse the PHPStan visibility error message to extract the target
/// visibilities.
///
/// Returns the valid targets ordered from most-restrictive (preferred)
/// to least-restrictive.
///
/// - `"… should also be public."` → `["public"]`
/// - `"… should be protected or public."` → `["protected", "public"]`
fn parse_phpstan_visibility_targets(message: &str) -> Option<Vec<String>> {
    if let Some(rest) = find_after(message, "should also be ") {
        let target = rest.trim_end_matches('.').trim().to_lowercase();
        if is_valid_visibility(&target) {
            return Some(vec![target]);
        }
    }

    if let Some(rest) = find_after(message, "should be ") {
        let rest = rest.trim_end_matches('.');
        let parts: Vec<&str> = rest.split(" or ").collect();
        if parts.len() == 2 {
            let a = parts[0].trim().to_lowercase();
            let b = parts[1].trim().to_lowercase();
            if is_valid_visibility(&a) && is_valid_visibility(&b) {
                return Some(vec![a, b]);
            }
        }
    }

    None
}

fn find_after<'a>(haystack: &'a str, needle: &str) -> Option<&'a str> {
    let pos = haystack.find(needle)?;
    Some(&haystack[pos + needle.len()..])
}

fn is_valid_visibility(s: &str) -> bool {
    matches!(s, "public" | "protected" | "private")
}

// ── Member kind extraction from CursorContext ───────────────────────────────

/// Extract the member name and kind from the cursor context.
fn extract_member_kind(ctx: &CursorContext<'_>) -> Option<MemberKind> {
    match ctx {
        CursorContext::InClassLike { member, .. } => match member {
            MemberContext::Method(method, _) => Some(MemberKind::Method(
                bytes_to_str(method.name.value).to_string(),
            )),
            MemberContext::Property(property) => {
                let name = match property {
                    Property::Plain(plain) => plain.items.first().map(|item| {
                        let var = item.variable();
                        bytes_to_str(var.name)
                            .strip_prefix('$')
                            .unwrap_or(bytes_to_str(var.name))
                            .to_string()
                    }),
                    Property::Hooked(hooked) => {
                        let var = hooked.item.variable();
                        Some(
                            bytes_to_str(var.name)
                                .strip_prefix('$')
                                .unwrap_or(bytes_to_str(var.name))
                                .to_string(),
                        )
                    }
                };
                name.map(MemberKind::Property)
            }
            MemberContext::Constant(constant) => constant
                .items
                .first()
                .map(|item| MemberKind::Constant(bytes_to_str(item.name.value).to_string())),
            _ => None,
        },
        _ => None,
    }
}

// ── Parent member visibility lookup ─────────────────────────────────────────

/// Find the visibility of a member in a class's own declarations.
fn find_member_visibility_in_class(cls: &ClassInfo, member_kind: &MemberKind) -> Option<u8> {
    match member_kind {
        MemberKind::Method(name) => cls
            .get_method(name)
            .map(|m| min_visibility_level(&m.visibility)),
        MemberKind::Property(name) => cls
            .properties
            .iter()
            .find(|p| p.name == *name)
            .map(|p| min_visibility_level(&p.visibility)),
        MemberKind::Constant(name) => cls
            .constants
            .iter()
            .find(|c| c.name == *name)
            .map(|c| min_visibility_level(&c.visibility)),
    }
}

// ── Visibility extraction from CursorContext ────────────────────────────────

/// Given a `CursorContext`, find the visibility modifier that applies
/// at the cursor position.
fn find_visibility_from_context(ctx: &CursorContext<'_>, cursor: u32) -> Option<VisibilityHit> {
    match ctx {
        CursorContext::InClassLike { member, .. } => match member {
            MemberContext::Method(method, in_body) => {
                if *in_body {
                    // Cursor is inside the body — only check promoted
                    // constructor parameters, not the method-level visibility.
                    find_promoted_param_visibility(method, cursor)
                } else {
                    // Check promoted constructor parameters first.
                    if let Some(hit) = find_promoted_param_visibility(method, cursor) {
                        return Some(hit);
                    }
                    // Then check method-level visibility.
                    find_visibility_in_modifiers(method.modifiers.iter())
                }
            }
            MemberContext::Property(property) => {
                find_visibility_in_modifiers(property.modifiers().iter())
            }
            MemberContext::Constant(constant) => {
                find_visibility_in_modifiers(constant.modifiers.iter())
            }
            MemberContext::TraitUse | MemberContext::EnumCase | MemberContext::None => None,
        },
        CursorContext::InFunction(_, _) | CursorContext::None => None,
    }
}

/// For constructor methods, check if the cursor is on a promoted
/// parameter with a visibility modifier.
fn find_promoted_param_visibility(
    method: &mago_syntax::cst::class_like::method::Method<'_>,
    cursor: u32,
) -> Option<VisibilityHit> {
    use mago_span::HasSpan;

    // Only check constructors — only they can have promoted properties.
    if method.name.value != b"__construct" {
        return None;
    }

    for param in method.parameter_list.parameters.iter() {
        if !param.is_promoted_property() {
            continue;
        }
        let param_span = param.span();
        if cursor < param_span.start.offset || cursor > param_span.end.offset {
            continue;
        }
        if let Some(hit) = find_visibility_in_modifiers(param.modifiers.iter()) {
            return Some(hit);
        }
    }
    None
}

/// Find the first read-visibility modifier (`public`, `protected`, or
/// `private`) in a sequence of modifiers and return a `VisibilityHit`.
fn find_visibility_in_modifiers<'a>(
    modifiers: impl Iterator<Item = &'a Modifier<'a>>,
) -> Option<VisibilityHit> {
    for m in modifiers {
        let (keyword_str, span) = match m {
            Modifier::Public(kw) => ("public", kw.span),
            Modifier::Protected(kw) => ("protected", kw.span),
            Modifier::Private(kw) => ("private", kw.span),
            _ => continue,
        };
        return Some(VisibilityHit {
            current: keyword_str,
            start: span.start.offset,
            end: span.end.offset,
        });
    }
    None
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse PHP and find visibility at a given byte offset.
    fn find_vis(php: &str, offset: u32) -> Option<VisibilityHit> {
        let arena = LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let ctx = find_cursor_context(&program.statements, offset);
        find_visibility_from_context(&ctx, offset)
    }

    #[test]
    fn finds_public_method() {
        let php = "<?php\nclass Foo {\n    public function bar() {}\n}";
        let pos = php.find("public function").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "public");
    }

    #[test]
    fn no_visibility_inside_method_body() {
        let php = "<?php\nclass Foo {\n    public function bar() {\n        $x = 1;\n    }\n}";
        // Place cursor on `$x = 1;` inside the method body.
        let pos = php.find("$x = 1").unwrap() as u32;
        let hit = find_vis(php, pos);
        assert!(
            hit.is_none(),
            "should not offer visibility change inside method body"
        );
    }

    #[test]
    fn no_visibility_on_method_opening_brace() {
        let php = "<?php\nclass Foo {\n    public function bar() {\n        $x = 1;\n    }\n}";
        // Place cursor on the opening brace of the method body.
        let pos = php.find("{\n        $x").unwrap() as u32;
        let hit = find_vis(php, pos);
        assert!(
            hit.is_none(),
            "should not offer visibility change on method body brace"
        );
    }

    #[test]
    fn finds_visibility_on_method_name() {
        let php = "<?php\nclass Foo {\n    public function bar() {\n        $x = 1;\n    }\n}";
        let pos = php.find("bar").unwrap() as u32;
        let hit = find_vis(php, pos).unwrap();
        assert_eq!(hit.current, "public");
    }

    #[test]
    fn finds_visibility_on_method_return_type() {
        let php =
            "<?php\nclass Foo {\n    public function bar(): void {\n        $x = 1;\n    }\n}";
        let pos = php.find("void").unwrap() as u32;
        let hit = find_vis(php, pos).unwrap();
        assert_eq!(hit.current, "public");
    }

    #[test]
    fn finds_protected_property() {
        let php = "<?php\nclass Foo {\n    protected string $bar;\n}";
        let pos = php.find("protected string").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "protected");
    }

    #[test]
    fn finds_private_constant() {
        let php = "<?php\nclass Foo {\n    private const BAR = 1;\n}";
        let pos = php.find("private const").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "private");
    }

    #[test]
    fn finds_promoted_param_visibility() {
        let php = "<?php\nclass Foo {\n    public function __construct(private string $name) {}\n}";
        let pos = php.find("private string").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "private");
    }

    #[test]
    fn no_visibility_on_trait_use() {
        let php = "<?php\nclass Foo {\n    use SomeTrait;\n}";
        let pos = php.find("use SomeTrait").unwrap() as u32;
        let hit = find_vis(php, pos + 2);
        assert!(hit.is_none());
    }

    #[test]
    fn no_visibility_outside_class() {
        let php = "<?php\nfunction foo() {}";
        let pos = php.find("function foo").unwrap() as u32;
        let hit = find_vis(php, pos + 2);
        assert!(hit.is_none());
    }

    #[test]
    fn finds_visibility_in_interface() {
        let php = "<?php\ninterface Foo {\n    public function bar(): void;\n}";
        let pos = php.find("public function").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "public");
    }

    #[test]
    fn finds_visibility_in_enum() {
        let php = "<?php\nenum Foo {\n    public function bar(): void {}\n}";
        let pos = php.find("public function").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "public");
    }

    #[test]
    fn finds_visibility_in_trait() {
        let php = "<?php\ntrait Foo {\n    protected function bar() {}\n}";
        let pos = php.find("protected function").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "protected");
    }

    #[test]
    fn finds_visibility_in_namespace() {
        let php = "<?php\nnamespace App;\nclass Foo {\n    public function bar() {}\n}";
        let pos = php.find("public function").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "public");
    }

    #[test]
    fn finds_visibility_in_braced_namespace() {
        let php = "<?php\nnamespace App {\nclass Foo {\n    private function bar() {}\n}\n}";
        let pos = php.find("private function").unwrap() as u32;
        let hit = find_vis(php, pos + 2).unwrap();
        assert_eq!(hit.current, "private");
    }

    // ── PHPStan message parsing ─────────────────────────────────────

    #[test]
    fn parses_should_also_be_public() {
        let msg = "Private method Foo::bar() overriding public method Parent::bar() should also be public.";
        let targets = parse_phpstan_visibility_targets(msg).unwrap();
        assert_eq!(targets, vec!["public"]);
    }

    #[test]
    fn parses_should_be_protected_or_public() {
        let msg = "Private method Foo::bar() overriding protected method Parent::bar() should be protected or public.";
        let targets = parse_phpstan_visibility_targets(msg).unwrap();
        assert_eq!(targets, vec!["protected", "public"]);
    }

    #[test]
    fn parses_property_message() {
        let msg = "Private property Child::$name overriding protected property Base::$name should be protected or public.";
        let targets = parse_phpstan_visibility_targets(msg).unwrap();
        assert_eq!(targets, vec!["protected", "public"]);
    }

    #[test]
    fn returns_none_for_unrelated_message() {
        let msg = "Method Foo::bar() should return string but returns int.";
        assert!(parse_phpstan_visibility_targets(msg).is_none());
    }

    // ── Visibility level helpers ────────────────────────────────────

    #[test]
    fn visibility_levels() {
        assert_eq!(visibility_level("public"), 2);
        assert_eq!(visibility_level("protected"), 1);
        assert_eq!(visibility_level("private"), 0);
    }
}
