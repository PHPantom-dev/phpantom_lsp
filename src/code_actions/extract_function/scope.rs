use super::*;

// ─── Scope map building ─────────────────────────────────────────────────────

/// Build a `ScopeMap` for the enclosing function/method at `offset`.
pub(crate) fn build_scope_map(content: &str, offset: u32) -> ScopeMap {
    crate::parser::with_parsed_program(content, "extract_function", |program, content| {
        crate::scope_collector::build_scope_map_for_offset(
            program.statements.as_slice(),
            offset,
            content.len() as u32,
        )
    })
}

// ─── Type resolution ────────────────────────────────────────────────────────

/// Resolve the type of a variable at a given offset using the hover
/// pipeline.
pub(crate) fn resolve_var_type(
    backend: &Backend,
    var_name: &str,
    content: &str,
    cursor_offset: u32,
    uri: &str,
) -> Option<PhpType> {
    let ctx = backend.file_context(uri);
    let class_loader = backend.class_loader(&ctx);
    let function_loader = backend.function_loader(&ctx);
    let constant_loader = backend.constant_loader();
    let loaders = Loaders {
        function_loader: Some(
            &function_loader as &dyn Fn(&str) -> Option<crate::types::FunctionInfo>,
        ),
        constant_loader: Some(&constant_loader),
    };

    let current_class = find_class_at_offset(&ctx.classes, cursor_offset);

    crate::completion::variable::resolution::resolve_variable_php_type(
        var_name,
        content,
        cursor_offset,
        current_class,
        &ctx.classes,
        &class_loader,
        loaders,
    )
}
