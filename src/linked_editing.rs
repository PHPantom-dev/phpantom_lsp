//! Linked editing ranges (`textDocument/linkedEditingRange`).
//!
//! When the cursor lands on a variable, returns all occurrences of that
//! variable within the current "definition region" so the editor can
//! enter linked editing mode — typing a new name updates every
//! occurrence simultaneously.
//!
//! A definition region spans from one assignment of the variable to the
//! next. This means reassignments split the variable into independent
//! regions: renaming `$foo` in one region does not affect `$foo` in
//! another, which is the correct behaviour when a variable is reused
//! for a different purpose.
//!
//! The tricky case is self-reassignment: `$foo = $foo->value;`. Here
//! the RHS `$foo` reads the *old* value while the LHS `$foo` starts a
//! *new* region. We use `VarDefSite::effective_from` (which points past
//! the end of the full statement for assignments) to decide which region
//! a read belongs to.
//!
//! ## Range boundaries
//!
//! The returned ranges deliberately exclude the leading `$` sigil.
//! Because PHP variable names never have identifier characters before
//! the `$`, this means typing in front of the variable (e.g. wrapping
//! `$foobar` in `array_map($foobar)`) inserts text *outside* the linked
//! region and does not propagate to other occurrences. The `$` itself
//! is never something users want to rename away, so excluding it costs
//! nothing and prevents accidental edits.
//!
//! Only variables are supported. Class names, members, functions, and
//! constants span multiple files and are better served by the full
//! `textDocument/rename` flow.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::{SymbolKind, SymbolMap, VarDefKind, VarDefSite};
use crate::text_position::byte_range_to_lsp_range;

impl Backend {
    /// Compute linked editing ranges for the symbol under the cursor.
    ///
    /// Returns `Some` only when the cursor is on a variable (not a
    /// property declaration) that has at least two occurrences in its
    /// definition region. A single occurrence offers nothing to link.
    pub fn handle_linked_editing_range(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<LinkedEditingRanges> {
        let span = self.lookup_symbol_at_position(uri, content, position)?;

        let maps = self.symbol_maps.read();
        let symbol_map = maps.get(uri)?;

        match &span.kind {
            SymbolKind::Variable { name } => {
                // Property declarations should not trigger linked editing —
                // renaming a property requires cross-file awareness.
                if let Some(VarDefKind::Property) = symbol_map.var_def_kind_at(name, span.start) {
                    return None;
                }

                let ranges =
                    collect_variable_ranges_in_region(symbol_map, content, name, span.start);

                // Linked editing with fewer than 2 ranges is pointless.
                if ranges.len() < 2 {
                    return None;
                }

                Some(LinkedEditingRanges {
                    ranges,
                    word_pattern: None,
                })
            }
            _ => None,
        }
    }
}

/// A definition region identified by its owning [`VarDefSite`].
///
/// The region covers:
/// - The definition token itself (the `$var` on the LHS / parameter /
///   foreach binding).
/// - Every *read* of `$var` for which this is the most recent visible
///   definition (i.e. reads at offsets where `effective_from <= read_offset`
///   and no later definition's `effective_from` is also `<= read_offset`).
struct Region {
    /// Byte offset of the `$var` token at the definition site.
    def_offset: u32,
    /// Byte offset from which reads see this definition.
    ///
    /// For assignments this is past the end of the statement (so the RHS
    /// still sees the *previous* definition). For parameters, foreach
    /// bindings, etc. this equals `def_offset`.
    effective_from: u32,
    /// Upper bound: the `effective_from` of the *next* definition in the
    /// same scope, or `u32::MAX` if this is the last definition.
    ///
    /// A read at offset R belongs to this region when
    /// `self.effective_from <= R && R < self.reads_until`.
    reads_until: u32,
}

/// Collect all definitions for `var_name` in `scope_start`, filtering
/// out property and docblock-only definitions, and build a [`Region`]
/// list sorted by definition offset.
///
/// Definitions at a deeper conditional nesting level than the most
/// recent same-or-shallower definition are excluded from region
/// splitting.  They are treated as reads within the outer region.
fn build_regions(symbol_map: &SymbolMap, var_name: &str, scope_start: u32) -> Vec<Region> {
    let defs: Vec<&VarDefSite> = symbol_map
        .var_defs
        .iter()
        .filter(|d| {
            d.name == var_name
                && d.scope_start == scope_start
                && d.kind != VarDefKind::Property
                && d.kind != VarDefKind::DocblockParam
                && d.kind != VarDefKind::CompoundAssignment
        })
        .collect();

    // Filter: only keep definitions that are at the same or shallower
    // nesting level as the current "active" definition.
    let mut region_defs: Vec<&VarDefSite> = Vec::with_capacity(defs.len());
    for def in &defs {
        if let Some(last) = region_defs.last()
            && def.nesting_depth > last.nesting_depth
        {
            // Deeper nesting — this is a conditional refinement,
            // not a new region. Skip it.
            continue;
        }
        region_defs.push(def);
    }

    let mut regions = Vec::with_capacity(region_defs.len());
    for (i, def) in region_defs.iter().enumerate() {
        let reads_until = region_defs
            .get(i + 1)
            .map(|next| next.effective_from)
            .unwrap_or(u32::MAX);
        regions.push(Region {
            def_offset: def.offset,
            effective_from: def.effective_from,
            reads_until,
        });
    }
    regions
}

/// Find which region the cursor belongs to.
///
/// - If the cursor is *on* a definition token, that definition's region
///   is returned.
/// - Otherwise the cursor is a read, and we find the region whose
///   `effective_from .. reads_until` range contains it.
/// - If the cursor is before any definition becomes effective (e.g. a
///   read in a docblock before any real definition), we return `None`.
fn find_cursor_region(
    regions: &[Region],
    cursor_offset: u32,
    symbol_map: &SymbolMap,
    var_name: &str,
) -> Option<usize> {
    // Check if the cursor is physically on a definition token (the
    // `$var` on an assignment LHS, parameter, foreach binding, etc.).
    // We use `var_def_kind_at` for a precise check rather than offset
    // arithmetic — this avoids misclassifying a RHS read like the
    // `$foo` in `$foo = $foo->value` as the definition.
    if symbol_map
        .var_def_kind_at(var_name, cursor_offset)
        .is_some()
    {
        for (i, region) in regions.iter().enumerate() {
            if cursor_offset >= region.def_offset
                && cursor_offset < region.def_offset + 1 + var_name.len() as u32
            {
                return Some(i);
            }
        }
    }

    // Cursor is a read — find which region's effective range covers it.
    // Walk in reverse so that the most recent (innermost) region wins
    // when ranges overlap at the boundary.
    for (i, region) in regions.iter().enumerate().rev() {
        if cursor_offset >= region.effective_from && cursor_offset < region.reads_until {
            return Some(i);
        }
    }

    // Cursor might be on a parameter def where effective_from ==
    // def_offset. The read-range check above handles this when there
    // are reads after it, but if the parameter has no later reads the
    // cursor might still be on the def token itself. Check once more
    // with an exact offset match.
    for (i, region) in regions.iter().enumerate() {
        if cursor_offset == region.def_offset {
            return Some(i);
        }
    }

    None
}

/// Determine whether a span at `span_offset` belongs to the given region.
///
/// A span belongs to the region if:
/// 1. It is the definition token itself (`span_offset == region.def_offset`), OR
/// 2. It is a read within the region's effective range.
fn span_in_region(
    region: &Region,
    span_offset: u32,
    symbol_map: &SymbolMap,
    var_name: &str,
    all_regions: &[Region],
) -> bool {
    // Case 1: the span is the definition token of this region.
    if span_offset == region.def_offset {
        return true;
    }

    // Case 2: the span is a read (or a compound assignment, which
    // does not start a new region). Check if it falls within the
    // region's effective read range AND is not a plain assignment
    // definition (which would start its own region).
    if span_offset >= region.effective_from && span_offset < region.reads_until {
        match symbol_map.var_def_kind_at(var_name, span_offset) {
            // Not a definition at all — it's a read.
            None => return true,
            // Compound assignments (+=, -=, .=, etc.) modify in place
            // and belong to the current region.
            Some(VarDefKind::CompoundAssignment) => return true,
            // If this definition starts its own region, it doesn't
            // belong here. But if it was skipped by build_regions
            // (e.g. a deeper-nested conditional assignment), it
            // belongs to the enclosing region.
            Some(_) => {
                let starts_own_region = all_regions.iter().any(|r| r.def_offset == span_offset);
                if !starts_own_region {
                    return true;
                }
            }
        }
    }

    false
}

/// Convert a byte range for a `$varName` token into an LSP [`Range`]
/// that excludes the leading `$` sigil.
///
/// The `$` is skipped by advancing `start` by one byte. This is safe
/// because `$` is a single-byte ASCII character and all PHP source
/// files are UTF-8 (or ASCII-compatible).
fn variable_range_without_sigil(content: &str, start: usize, end: usize) -> Range {
    // Skip the `$` at the start of the token.
    let name_start = start + 1;
    debug_assert!(
        name_start <= end,
        "variable token too short to have a name after $"
    );
    byte_range_to_lsp_range(content, name_start, end)
}

/// Collect all LSP [`Range`]s for a variable within the definition
/// region that contains `cursor_offset`.
///
/// Ranges exclude the leading `$` sigil so that typing before the `$`
/// (e.g. wrapping a variable in a function call) does not propagate to
/// other occurrences.
fn collect_variable_ranges_in_region(
    symbol_map: &SymbolMap,
    content: &str,
    var_name: &str,
    cursor_offset: u32,
) -> Vec<Range> {
    let scope_start = symbol_map.find_variable_scope(var_name, cursor_offset);
    let regions = build_regions(symbol_map, var_name, scope_start);

    // If there are no definitions at all, fall back to collecting
    // everything in scope (shouldn't happen for well-formed code).
    let region = match find_cursor_region(&regions, cursor_offset, symbol_map, var_name) {
        Some(idx) => &regions[idx],
        None => return Vec::new(),
    };

    let mut ranges = Vec::new();
    let mut seen_offsets = std::collections::HashSet::new();

    // Gather variable spans within the region.
    for span in &symbol_map.spans {
        if let SymbolKind::Variable { name } = &span.kind {
            if name != var_name {
                continue;
            }
            let span_scope = symbol_map.find_variable_scope(name, span.start);
            if span_scope != scope_start {
                continue;
            }
            if !span_in_region(region, span.start, symbol_map, var_name, &regions) {
                continue;
            }
            if !seen_offsets.insert(span.start) {
                continue;
            }
            ranges.push(variable_range_without_sigil(
                content,
                span.start as usize,
                span.end as usize,
            ));
        }
    }

    // Include var_def sites that may not have a matching Variable span
    // (e.g. parameters, foreach bindings).
    for def in &symbol_map.var_defs {
        if def.name == var_name
            && def.scope_start == scope_start
            && def.kind != VarDefKind::DocblockParam
            && def.offset == region.def_offset
            && seen_offsets.insert(def.offset)
        {
            let end_offset = def.offset + 1 + def.name.len() as u32;
            ranges.push(variable_range_without_sigil(
                content,
                def.offset as usize,
                end_offset as usize,
            ));
        }
    }

    // ── Closure use() capture bridging ──────────────────────────────────
    //
    // When a variable is captured via `use ($var)`, renaming it must
    // propagate across both the outer scope (where the variable is
    // defined) and the inner closure scope (where it is used).  The
    // `use ($var)` token sits physically in the outer scope but has a
    // `ClosureCapture` VarDefSite scoped to the closure body.
    //
    // Case A (cursor in outer scope): find ClosureCapture defs whose
    // offset falls within this region, then include all occurrences of
    // the variable inside each captured closure.
    //
    // Case B (cursor in closure scope): if the region's definition is
    // a ClosureCapture, bridge outward to the outer scope's region and
    // include those occurrences too.

    // Case A: outer scope → closure bodies
    for def in &symbol_map.var_defs {
        if def.name == var_name
            && def.kind == VarDefKind::ClosureCapture
            && span_in_region(region, def.offset, symbol_map, var_name, &regions)
        {
            // The use() token itself (in outer scope) is already collected
            // above.  Now collect all occurrences inside the closure body.
            let closure_scope = def.scope_start;
            for span in &symbol_map.spans {
                if let SymbolKind::Variable { name } = &span.kind {
                    if name != var_name {
                        continue;
                    }
                    let ss = symbol_map.find_variable_scope(name, span.start);
                    if ss != closure_scope {
                        continue;
                    }
                    if !seen_offsets.insert(span.start) {
                        continue;
                    }
                    ranges.push(variable_range_without_sigil(
                        content,
                        span.start as usize,
                        span.end as usize,
                    ));
                }
            }
        }
    }

    // Case B: closure scope → outer scope
    // Check if the current region's definition is a ClosureCapture.
    if let Some(capture_def) = symbol_map.var_defs.iter().find(|d| {
        d.name == var_name
            && d.kind == VarDefKind::ClosureCapture
            && d.offset == region.def_offset
            && d.scope_start == scope_start
    }) {
        // The use() token sits in the outer scope.  Find which outer
        // region it belongs to and include all outer occurrences.
        let outer_scope = symbol_map.find_enclosing_scope(capture_def.offset);
        let outer_regions = build_regions(symbol_map, var_name, outer_scope);
        if let Some(outer_region) = outer_regions.iter().find(|r| {
            capture_def.offset >= r.effective_from && capture_def.offset < r.reads_until
                || capture_def.offset == r.def_offset
        }) {
            for span in &symbol_map.spans {
                if let SymbolKind::Variable { name } = &span.kind {
                    if name != var_name {
                        continue;
                    }
                    let ss = symbol_map.find_variable_scope(name, span.start);
                    if ss != outer_scope {
                        continue;
                    }
                    if !span_in_region(
                        outer_region,
                        span.start,
                        symbol_map,
                        var_name,
                        &outer_regions,
                    ) {
                        continue;
                    }
                    if !seen_offsets.insert(span.start) {
                        continue;
                    }
                    ranges.push(variable_range_without_sigil(
                        content,
                        span.start as usize,
                        span.end as usize,
                    ));
                }
            }
            // Also include the outer region's def site if not yet seen.
            if seen_offsets.insert(outer_region.def_offset) {
                let end_offset = outer_region.def_offset + 1 + var_name.len() as u32;
                ranges.push(variable_range_without_sigil(
                    content,
                    outer_region.def_offset as usize,
                    end_offset as usize,
                ));
            }
        }
    }

    // Sort by position for a deterministic response.
    ranges.sort_by(|a, b| {
        a.start
            .line
            .cmp(&b.start.line)
            .then(a.start.character.cmp(&b.start.character))
    });

    ranges
}
