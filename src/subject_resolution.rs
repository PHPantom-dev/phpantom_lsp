/// Shared subject type resolution utility.
///
/// Resolves a subject string (`$this`, `self`, `static`, `parent`,
/// bare class names, or `$variable`) to a [`PhpType`].  This is the
/// single point of truth for all features that need to answer "what
/// type does this subject refer to?" without the full completion
/// resolver pipeline.
///
/// Consumers: deprecated diagnostics, find-references, code actions.
use std::collections::HashMap;
use std::sync::Arc;

use crate::class_lookup::find_class_at_offset;
use crate::completion::resolver::Loaders;
use crate::php_type::PhpType;
use crate::types::ClassInfo;

/// Context for resolving a subject expression to its type.
pub(crate) struct SubjectResolutionCtx<'a> {
    /// All classes defined in the current file.
    pub local_classes: &'a [Arc<ClassInfo>],
    /// Use-statement map (short name → FQN).
    pub use_map: &'a HashMap<String, String>,
    /// File namespace (if any).
    pub namespace: &'a Option<String>,
    /// File content.
    pub content: &'a str,
    /// Class loader.
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Function loader (for variable resolution via the forward walker).
    pub function_loader: &'a dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
}

/// Resolve a subject string to a [`PhpType`].
///
/// Handles `$this`, `self`, `static`, `parent`, bare class names (for
/// static access), and `$variable` (delegates to the forward walker).
pub(crate) fn resolve_subject_type(
    subject_text: &str,
    is_static: bool,
    access_offset: u32,
    ctx: &SubjectResolutionCtx<'_>,
) -> Option<PhpType> {
    let trimmed = subject_text.trim();

    match trimmed {
        "$this" | "self" | "static" => {
            let fqn = find_enclosing_class_fqn(ctx.local_classes, ctx.namespace, access_offset)?;
            Some(PhpType::Named(fqn))
        }
        "parent" => {
            let cls = find_class_at_offset(ctx.local_classes, access_offset)?;
            let parent = cls.parent_class.as_ref()?;
            let fqn = resolve_to_fqn(parent, ctx.use_map, ctx.namespace);
            Some(PhpType::Named(fqn))
        }
        _ if is_static && !trimmed.starts_with('$') => {
            let fqn = resolve_to_fqn(trimmed, ctx.use_map, ctx.namespace);
            Some(PhpType::Named(fqn))
        }
        _ if trimmed.starts_with('$') => {
            let current_class = find_class_at_offset(ctx.local_classes, access_offset);
            crate::completion::variable::resolution::resolve_variable_php_type(
                trimmed,
                ctx.content,
                access_offset,
                current_class,
                ctx.local_classes,
                ctx.class_loader,
                Loaders::with_function(Some(ctx.function_loader)),
            )
        }
        _ => None,
    }
}

/// Find the FQN of the class enclosing `offset`.
fn find_enclosing_class_fqn(
    local_classes: &[Arc<ClassInfo>],
    namespace: &Option<String>,
    offset: u32,
) -> Option<String> {
    let cls = local_classes
        .iter()
        .find(|c| {
            // Use the declaration start (which includes leading
            // attributes) so a `self::` reference inside a class-level
            // attribute, which sits before the body braces, still maps
            // to the class it decorates.
            let start = if c.decl_start_offset != 0 {
                c.decl_start_offset
            } else {
                c.start_offset
            };
            !c.name.starts_with("__anonymous@") && offset >= start && offset <= c.end_offset
        })
        .or_else(|| {
            local_classes
                .iter()
                .find(|c| !c.name.starts_with("__anonymous@"))
        })?;

    if let Some(ns) = namespace {
        Some(format!("{}\\{}", ns, cls.name))
    } else {
        Some(cls.name.to_string())
    }
}

/// Resolve a class name to its FQN using the use-map and namespace.
fn resolve_to_fqn(
    name: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> String {
    // Already fully qualified.
    if name.starts_with('\\') {
        return name.trim_start_matches('\\').to_string();
    }

    // Check use-map (by short name / first segment).
    let first_segment = name.split('\\').next().unwrap_or(name);
    if let Some(fqn) = use_map.get(first_segment) {
        if name.contains('\\') {
            // Multi-segment: replace first segment with the imported FQN.
            let rest = &name[first_segment.len()..];
            return format!("{}{}", fqn.trim_start_matches('\\'), rest);
        }
        return fqn.trim_start_matches('\\').to_string();
    }

    // Fall back to namespace-qualified.
    if let Some(ns) = namespace {
        format!("{}\\{}", ns, name)
    } else {
        name.to_string()
    }
}
