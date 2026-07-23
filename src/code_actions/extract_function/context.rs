use super::*;

// ─── Statement boundary validation ─────────────────────────────────────────

/// Check whether the selected byte range `[start, end)` covers one or
/// more complete statements.
///
/// We parse the file and walk the AST to verify that every statement
/// whose span overlaps the selection is *fully* contained within it.
/// If any statement is only partially selected, the selection is
/// invalid for extraction.
pub(crate) fn selection_covers_complete_statements(
    content: &str,
    start: usize,
    end: usize,
) -> bool {
    crate::parser::with_parsed_program(content, "extract_function", |program, _| {
        // Find the enclosing function/method body statements.
        let body_stmts = find_enclosing_body_statements(&program.statements, start as u32);
        if body_stmts.is_empty() {
            return false;
        }

        let mut found_any = false;
        for stmt in &body_stmts {
            let span = stmt.span();
            let stmt_start = span.start.offset as usize;
            let stmt_end = span.end.offset as usize;

            // Statement fully outside the selection — fine, skip it.
            if stmt_end <= start || stmt_start >= end {
                continue;
            }

            // Statement overlaps the selection — it must be fully contained.
            if stmt_start < start || stmt_end > end {
                return false;
            }

            found_any = true;
        }

        found_any
    })
}

/// Collect references to top-level statements in the enclosing
/// function/method body that contains `offset`.
///
/// Returns byte ranges `(start, end)` for each direct child statement.
pub(crate) fn find_enclosing_body_statements<'a>(
    statements: &'a Sequence<'a, Statement<'a>>,
    offset: u32,
) -> Vec<&'a Statement<'a>> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Function(func) => {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                if offset >= body_start && offset <= body_end {
                    return func.body.statements.iter().collect();
                }
            }
            Statement::Class(class) => {
                if let Some(block) = crate::util::find_enclosing_method_block_in_members(
                    class.members.iter(),
                    offset,
                ) {
                    return block.statements.iter().collect();
                }
            }
            Statement::Trait(tr) => {
                if let Some(block) =
                    crate::util::find_enclosing_method_block_in_members(tr.members.iter(), offset)
                {
                    return block.statements.iter().collect();
                }
            }
            Statement::Enum(en) => {
                if let Some(block) =
                    crate::util::find_enclosing_method_block_in_members(en.members.iter(), offset)
                {
                    return block.statements.iter().collect();
                }
            }
            Statement::Namespace(ns) => {
                let result = find_enclosing_body_statements(ns.statements(), offset);
                if !result.is_empty() {
                    return result;
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

// ─── Context detection ──────────────────────────────────────────────────────

/// Whether the extracted code should become a method or a standalone function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtractionTarget {
    /// Extract as a private method on the enclosing class.
    Method,
    /// Extract as a standalone function after the enclosing function.
    Function,
}

/// Information about the enclosing function/method for insertion purposes.
#[derive(Debug, Clone)]
pub(crate) struct EnclosingContext {
    /// Whether to extract as a method or function.
    pub(crate) target: ExtractionTarget,
    /// Byte offset of the closing `}` of the enclosing class (for method
    /// insertion) or the enclosing function (for function insertion).
    pub(crate) insert_offset: usize,
    /// The body's opening `{` offset — used to determine indentation.
    pub(crate) body_start: usize,
    /// Whether the enclosing method is static.
    pub(crate) is_static: bool,
    /// The name of the enclosing function/method (e.g. `"run"`, `"process"`).
    /// Used by name generation to produce contextual names like `runGuard`.
    pub(crate) enclosing_name: String,
    /// Method names that already exist in the enclosing class (for
    /// deduplication when extracting a method).  Empty when extracting
    /// a standalone function.
    pub(crate) sibling_method_names: Vec<String>,
}

/// Determine the extraction target and insertion point by walking the AST.
pub(crate) fn find_enclosing_context(
    content: &str,
    offset: u32,
    uses_this: bool,
) -> Option<EnclosingContext> {
    crate::parser::with_parsed_program(content, "extract_function", |program, content| {
        let ctx = find_cursor_context(&program.statements, offset);

        match ctx {
            CursorContext::InClassLike {
                member,
                all_members,
                ..
            } => {
                if let MemberContext::Method(method, true) = member {
                    let is_static = method.modifiers.iter().any(|m| m.is_static());
                    let enclosing_name = bytes_to_str(method.name.value).to_string();

                    // Collect sibling method names for scoped deduplication.
                    let sibling_method_names: Vec<String> = all_members
                        .iter()
                        .filter_map(|m| {
                            if let ClassLikeMember::Method(m) = m {
                                Some(bytes_to_str(m.name.value).to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    // For method extraction, insert before the closing `}` of
                    // the class.  Find the class closing brace by walking up
                    // from the method.
                    let class_end = find_class_end_offset(&program.statements, offset);

                    if let MethodBody::Concrete(block) = &method.body {
                        let body_start = block.left_brace.start.offset as usize;

                        if uses_this && is_static {
                            // $this in a static method — can't extract as method.
                            // Fall back to extracting as a function.
                            let func_end = block.right_brace.end.offset as usize;
                            return Some(EnclosingContext {
                                target: ExtractionTarget::Function,
                                insert_offset: find_after_class_end(&program.statements, offset)
                                    .unwrap_or(func_end),
                                body_start,
                                is_static,
                                enclosing_name,
                                sibling_method_names: Vec::new(),
                            });
                        }

                        return Some(EnclosingContext {
                            target: ExtractionTarget::Method,
                            insert_offset: class_end
                                .unwrap_or(block.right_brace.end.offset as usize),
                            body_start,
                            is_static,
                            enclosing_name,
                            sibling_method_names,
                        });
                    }
                }
                None
            }
            CursorContext::InFunction(func, true) => {
                let body_start = func.body.left_brace.start.offset as usize;
                let func_end = func.body.right_brace.end.offset as usize;
                let enclosing_name = bytes_to_str(func.name.value).to_string();

                // For function extraction, insert after the enclosing function.
                // Find the end of the line containing the closing `}`.
                let insert_offset = find_line_end(content, func_end);

                Some(EnclosingContext {
                    target: ExtractionTarget::Function,
                    insert_offset,
                    body_start,
                    is_static: false,
                    enclosing_name,
                    sibling_method_names: Vec::new(),
                })
            }
            _ => None,
        }
    })
}

/// Find the byte offset of the closing `}` of the class containing `offset`.
pub(crate) fn find_class_end_offset(
    statements: &Sequence<'_, Statement<'_>>,
    offset: u32,
) -> Option<usize> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Class(class) => {
                let span = class.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(class.right_brace.start.offset as usize);
                }
            }
            Statement::Trait(tr) => {
                let span = tr.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(tr.right_brace.start.offset as usize);
                }
            }
            Statement::Enum(en) => {
                let span = en.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(en.right_brace.start.offset as usize);
                }
            }
            Statement::Namespace(ns) => {
                if let Some(offset) = find_class_end_offset(ns.statements(), offset) {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the byte offset after the closing `}` of the class containing `offset`.
pub(crate) fn find_after_class_end(
    statements: &Sequence<'_, Statement<'_>>,
    offset: u32,
) -> Option<usize> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Class(class) => {
                let span = class.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(span.end.offset as usize);
                }
            }
            Statement::Trait(tr) => {
                let span = tr.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(span.end.offset as usize);
                }
            }
            Statement::Enum(en) => {
                let span = en.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(span.end.offset as usize);
                }
            }
            Statement::Namespace(ns) => {
                if let Some(end) = find_after_class_end(ns.statements(), offset) {
                    return Some(end);
                }
            }
            _ => {}
        }
    }
    None
}
