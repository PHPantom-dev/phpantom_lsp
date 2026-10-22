//! The type of a subject a feature already holds as text.
//!
//! The last step after [`subject_extraction`](super::subject_extraction)
//! finds a subject's text and [`subject_expr`](super::subject_expr) parses
//! it.  The class keywords (`self`, `static`, `parent`, `$this`) and a bare
//! class name on static access are answered here from the enclosing class
//! and the imports; a variable goes to the forward walker and anything else
//! to the shared chain resolver, so this adds no resolution of its own.
//!
//! Consumers: deprecated diagnostics, find-references, code actions.

use crate::atom::atom;
use crate::class_lookup::{ResolvedClassLoader, find_class_at_offset};
use std::collections::HashMap;
use std::sync::Arc;

use crate::names::resolve_to_fqn;
use crate::php_type::PhpType;
use crate::type_engine::resolver::{FileCtx, Loaders};
use crate::type_engine::subject_expr::SubjectExpr;
use crate::type_engine::variable::resolution::VarQuery;
use crate::types::{AccessKind, ClassInfo, ResolvedType};

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
    /// Class loader for already-resolved names.
    pub class_loader: &'a ResolvedClassLoader<'a>,
    /// See [`FileCtx::source_class_loader`](crate::type_engine::resolver::FileCtx::source_class_loader).
    pub source_class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Server state for project-wide answers.  See
    /// [`FileCtx::backend`](crate::type_engine::resolver::FileCtx::backend).
    pub backend: Option<&'a crate::Backend>,
    /// Function loader (for variable resolution via the forward walker).
    pub function_loader: &'a dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
}

impl<'a> SubjectResolutionCtx<'a> {
    /// The file the subject is read in, as the type engine takes it.
    fn file(&self) -> FileCtx<'a> {
        FileCtx::new(
            self.content,
            self.local_classes,
            self.class_loader,
            self.source_class_loader,
        )
        .with_imports(Some(self.use_map))
        .with_backend(self.backend)
    }
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
        "self" => {
            let fqn = find_enclosing_class_fqn(ctx.local_classes, access_offset)?;
            Some(PhpType::named(atom(&fqn)))
        }
        "static" => {
            let fqn = find_enclosing_class_fqn(ctx.local_classes, access_offset)?;
            Some(PhpType::static_type(atom(&fqn)))
        }
        "$this" => {
            let fqn = find_enclosing_class_fqn(ctx.local_classes, access_offset)?;
            Some(PhpType::this_type(atom(&fqn)))
        }
        "parent" => {
            let cls = find_class_at_offset(ctx.local_classes, access_offset)?;
            let parent = cls.parent_class.as_ref()?;
            Some(PhpType::named(*parent))
        }
        _ if is_static && !trimmed.starts_with('$') => {
            let fqn = resolve_to_fqn(trimmed, ctx.use_map, ctx.namespace);
            Some(PhpType::named(atom(&fqn)))
        }
        _ => {
            let expr = SubjectExpr::parse(trimmed);
            if matches!(expr, SubjectExpr::Variable(_)) {
                // A bare variable resolves through the forward walker,
                // which is position-accurate (reassignment, narrowing).
                let current_class = find_class_at_offset(ctx.local_classes, access_offset);
                return crate::type_engine::variable::resolution::resolve_variable_php_type(
                    VarQuery {
                        loaders: Loaders::with_function(Some(ctx.function_loader)),
                        ..VarQuery::new(trimmed, ctx.file(), current_class, access_offset)
                    },
                );
            }

            // Everything else — property chains (`$this->context`), method
            // calls, static accesses, and combinations thereof — goes
            // through the shared chain resolver, the same path completion
            // and hover use.
            let current_class = find_class_at_offset(ctx.local_classes, access_offset);
            let rctx = crate::type_engine::resolver::ResolutionCtx {
                resolved_class_cache: ctx.backend.map(|b| &b.resolved_class_cache),
                function_loader: Some(ctx.function_loader),
                ..crate::type_engine::resolver::ResolutionCtx::new(
                    ctx.file(),
                    current_class,
                    access_offset,
                )
            };
            let access_kind = AccessKind::of_member_access(is_static);
            let resolved = crate::type_engine::resolver::resolve_target_classes_expr(
                &expr,
                access_kind,
                &rctx,
            );
            if resolved.is_empty() {
                return None;
            }
            Some(ResolvedType::types_joined(&resolved))
        }
    }
}

/// Find the FQN of the class enclosing `offset`.
fn find_enclosing_class_fqn(local_classes: &[Arc<ClassInfo>], offset: u32) -> Option<String> {
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

    Some(cls.fqn().to_string())
}
