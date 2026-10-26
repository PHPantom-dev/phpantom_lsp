//! Object shape property and Eloquent array entry position lookup.
//!
//! These helpers locate property keys inside `object{…}` shape annotations
//! and string literal entries inside Eloquent array properties (`$casts`,
//! `$fillable`, etc.) so that go-to-definition can jump to the right
//! position in the source file.

use tower_lsp_server::gen_lsp_types::Position;

use crate::Backend;

impl Backend {
    /// Find the position of a property key inside an `object{…}` shape
    /// annotation within docblock comments.
    ///
    /// Scans `content` for docblock lines containing `object{` (or
    /// `?object{`, `\object{`) and, within matching braces, looks for
    /// `key_name:` or `key_name?:`.  Returns the `Position` of the
    /// first character of the key name.
    ///
    /// When `near_offset` is provided, the match closest to that byte
    /// offset (in either direction) is returned.  This handles both
    /// inline `@var` annotations above the cursor and `@return`
    /// docblocks on methods defined below the usage site.
    pub(in crate::definition) fn find_object_shape_property_position(
        content: &str,
        key_name: &str,
        near_offset: Option<usize>,
    ) -> Option<Position> {
        // We need to find `key_name:` or `key_name?:` inside an
        // `object{…}` block that appears inside a docblock comment.
        //
        // Strategy: scan every line.  Track whether we are inside a
        // `/** … */` comment.  When we see `object{` (case-insensitive
        // base word) at brace depth 0, enter shape-scanning mode and
        // look for the key.

        let mut matches: Vec<usize> = Vec::new(); // byte offsets of the keys
        let mut in_docblock = false;

        for (byte_offset, line) in crate::text_position::lines_with_offsets(content) {
            // Track docblock boundaries.
            if line.contains("/**") {
                in_docblock = true;
            }

            if in_docblock {
                // Search for object shape property keys in this line.
                // Look for `object{` patterns (possibly preceded by `?` or `\`).
                if let Some(pos) = Self::find_shape_key_in_line(line, key_name) {
                    matches.push(byte_offset + pos);
                }
            }

            if line.contains("*/") {
                in_docblock = false;
            }
        }

        // Pick the match closest to the cursor.  When no near_offset
        // is given, return the last match (highest line number).
        let best = match near_offset {
            Some(cursor) => matches.into_iter().min_by_key(|off| cursor.abs_diff(*off)),
            None => matches.into_iter().last(),
        };

        best.map(|offset| crate::text_position::offset_to_position(content, offset))
    }

    /// Search a single line for a property key inside an `object{…}`
    /// shape.  Returns the byte offset of the key within the line, or
    /// `None`.
    fn find_shape_key_in_line(line: &str, key_name: &str) -> Option<usize> {
        let bytes = line.as_bytes();

        // Find every `object{` (case-insensitive) in the line.
        let lower = line.to_ascii_lowercase();
        let mut search_from = 0usize;

        while let Some(obj_pos) = lower[search_from..].find("object{") {
            let abs_obj = search_from + obj_pos;
            let brace_start = abs_obj + "object".len(); // index of `{`

            // Walk from the `{` respecting nesting to find keys.
            let mut depth = 0i32;
            let mut i = brace_start;
            while i < bytes.len() {
                match bytes[i] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ if depth == 1 && line.is_char_boundary(i) => {
                        // At depth 1 we are inside the outermost `object{…}`.
                        // Check if the key starts here.
                        if let Some(col) = Self::match_shape_key_at(line, i, key_name) {
                            return Some(col);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }

            search_from = abs_obj + 1;
        }

        None
    }

    /// Check whether `key_name` (possibly quoted) starts at position
    /// `pos` within `line`.  Returns the column of the first character
    /// of the key (inside quotes if quoted).
    fn match_shape_key_at(line: &str, pos: usize, key_name: &str) -> Option<usize> {
        let rest = &line[pos..];
        let rest_trimmed = rest.trim_start();
        let leading_ws = rest.len() - rest_trimmed.len();
        let col_base = pos + leading_ws;

        // Bare key: `name:` or `name?:`
        if let Some(after) = rest_trimmed.strip_prefix(key_name)
            && (after.starts_with(':') || after.starts_with("?:"))
        {
            return Some(col_base);
        }

        // Single-quoted key: `'name':` or `'name'?:`
        if let Some(inner) = rest_trimmed.strip_prefix('\'')
            && let Some(after_key) = inner.strip_prefix(key_name)
            && (after_key.starts_with("':") || after_key.starts_with("'?:"))
        {
            // Point inside the quote at the first letter.
            return Some(col_base + 1);
        }

        // Double-quoted key: `"name":` or `"name"?:`
        if let Some(inner) = rest_trimmed.strip_prefix('"')
            && let Some(after_key) = inner.strip_prefix(key_name)
            && (after_key.starts_with("\":") || after_key.starts_with("\"?:"))
        {
            return Some(col_base + 1);
        }

        None
    }

    /// Find the string literal that names `member_name` in the Eloquent
    /// attribute arrays (`$casts`, `$fillable`, …) of the class declared at
    /// `class_range` in `content`.
    ///
    /// Returns the position of the literal's first character inside the
    /// quotes, so go-to-definition can jump to it.
    pub(in crate::definition) fn find_eloquent_array_entry(
        content: &str,
        member_name: &str,
        class_range: Option<(usize, usize)>,
    ) -> Option<Position> {
        let (class_start, _) = class_range?;
        let offset =
            crate::parser::with_parsed_program(content, "eloquent_array_entry", |program, _| {
                let members = crate::parser::innermost_node_at(
                    mago_syntax::cst::Node::Program(program),
                    class_start as u32,
                    crate::parser::class_like_members,
                )?;
                crate::virtual_members::laravel::attribute_entry_offset(members, member_name)
            })?;
        Some(crate::text_position::offset_to_position(
            content,
            offset as usize,
        ))
    }
}
