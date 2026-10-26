//! The top-level declaration walk of a parsed file.
//!
//! Gathers the class-likes, imports, namespace blocks and standalone
//! functions a file declares from its AST, for
//! [`super::ast_update`] to assemble into an index update.

use std::collections::HashMap;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::types::{ClassInfo, FunctionInfo, NamespaceSpan};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::DocblockCtx;

impl Backend {
    /// Pull the imports out of the wrapper *method* a template whose
    /// `$this` is bound is wrapped in, for the same reason the wrapper
    /// function's are pulled out.  A no-op for every other class.
    fn extract_blade_wrapper_method_use_statements(
        statement: &Statement<'_>,
        use_map: &mut HashMap<String, String>,
        symbol_imports: &mut crate::types::SymbolImports,
    ) {
        use mago_syntax::cst::class_like::member::ClassLikeMember;
        use mago_syntax::cst::class_like::method::MethodBody;

        let Statement::Class(class) = statement else {
            return;
        };
        if !crate::blade::is_scope_class(bytes_to_str(class.name.value)) {
            return;
        }
        for member in class.members.iter() {
            if let ClassLikeMember::Method(method) = member
                && bytes_to_str(method.name.value) == crate::blade::WRAPPER_FUNCTION
                && let MethodBody::Concrete(block) = &method.body
            {
                Self::extract_use_statements_from_statements(
                    block.statements.iter(),
                    use_map,
                    Some(&mut *symbol_imports),
                );
            }
        }
    }

    /// Walk the file's statements once for its class-likes and imports,
    /// block by block.
    pub(super) fn collect_file_declarations<'a>(
        program: &'a Program<'a>,
        doc_ctx: &DocblockCtx<'a>,
    ) -> FileDeclarations {
        let mut decls = FileDeclarations::default();
        for statement in program.statements.iter() {
            match statement {
                Statement::Use(use_stmt) => {
                    Self::extract_use_items(
                        &use_stmt.items,
                        &mut decls.use_map,
                        Some(&mut decls.symbol_imports),
                    );
                }
                Statement::Namespace(ns) => Self::collect_namespace_block(ns, doc_ctx, &mut decls),
                statement if Self::is_classlike_extraction_candidate(statement) => {
                    // A template whose `$this` is bound wraps its body
                    // in a method rather than a function, which buries
                    // the template's own imports just the same (see the
                    // wrapper-function arm below).
                    Self::extract_blade_wrapper_method_use_statements(
                        statement,
                        &mut decls.use_map,
                        &mut decls.symbol_imports,
                    );
                    let mut top_classes = Vec::new();
                    Self::extract_classes_from_statements(
                        std::iter::once(statement),
                        &mut top_classes,
                        Some(doc_ctx),
                    );
                    for cls in top_classes {
                        decls.classes_with_ns.push((cls, None, None));
                    }
                }
                // Laravel compiles a template's `@php` and `<?php`
                // regions into the top level of the generated view
                // file, so a `use` written in one imports for the whole
                // template. Wrapping the body in a function to make it
                // analysable buries those imports in a function body,
                // where the file's use-map would never see them.
                Statement::Function(func)
                    if bytes_to_str(func.name.value) == crate::blade::WRAPPER_FUNCTION =>
                {
                    Self::extract_use_statements_from_statements(
                        func.body.statements.iter(),
                        &mut decls.use_map,
                        Some(&mut decls.symbol_imports),
                    );
                }
                _ => {
                    // Walk other top-level statements (expression statements,
                    // function declarations, etc.) for anonymous classes.
                    let mut anon_classes = Vec::new();
                    Self::find_anonymous_classes_in_statement(
                        statement,
                        &mut anon_classes,
                        Some(doc_ctx),
                    );
                    for cls in anon_classes {
                        decls.classes_with_ns.push((cls, None, None));
                    }
                }
            }
        }
        decls.fold_block_imports();

        // A class-like declared in two branches of a conditional yields
        // one entry per branch; keep the first so resolution is
        // deterministic (see `dedup_class_likes_first_wins`).
        Self::dedup_class_likes_first_wins(&mut decls.classes_with_ns, |(cls, ns, _)| (cls, ns));
        decls
    }

    /// Collect the class-likes and imports of one `namespace` block,
    /// tagging each class with the block's namespace.
    fn collect_namespace_block<'a>(
        ns: &'a Namespace<'a>,
        doc_ctx: &DocblockCtx<'a>,
        decls: &mut FileDeclarations,
    ) {
        let block_ns: Option<String> = ns
            .name
            .as_ref()
            .map(|ident| bytes_to_str(ident.value()).to_string())
            .filter(|n| !n.is_empty());

        // The file-level namespace is the FIRST non-empty one.
        if decls.namespace.is_none() {
            decls.namespace = block_ns.clone();
        }

        let mut block_classes = Vec::new();
        let mut block_use_map = HashMap::new();
        let mut block_symbol_imports = crate::types::SymbolImports::default();
        for inner in ns.statements().iter() {
            match inner {
                Statement::Use(use_stmt) => {
                    Self::extract_use_items(
                        &use_stmt.items,
                        &mut block_use_map,
                        Some(&mut block_symbol_imports),
                    );
                }
                inner if Self::is_classlike_extraction_candidate(inner) => {
                    Self::extract_classes_from_statements(
                        std::iter::once(inner),
                        &mut block_classes,
                        Some(doc_ctx),
                    );
                }
                Statement::Namespace(inner_ns) => {
                    // Nested namespaces (rare but valid)
                    Self::extract_use_statements_from_statements(
                        inner_ns.statements().iter(),
                        &mut block_use_map,
                        Some(&mut block_symbol_imports),
                    );
                    Self::extract_classes_from_statements(
                        inner_ns.statements().iter(),
                        &mut block_classes,
                        Some(doc_ctx),
                    );
                }
                _ => {
                    // Walk other statements (expression statements,
                    // control flow, etc.) for anonymous classes.
                    Self::find_anonymous_classes_in_statement(
                        inner,
                        &mut block_classes,
                        Some(doc_ctx),
                    );
                }
            }
        }

        let block_index = decls.namespace_spans.len();
        for cls in block_classes {
            decls
                .classes_with_ns
                .push((cls, block_ns.clone(), Some(block_index)));
        }

        let ns_span = ns.span();
        decls.namespace_spans.push(NamespaceSpan {
            namespace: block_ns,
            start: ns_span.start.offset,
            end: ns_span.end.offset,
            use_map: block_use_map,
            symbol_imports: block_symbol_imports,
        });
    }

    /// Extract standalone functions (including those inside if-guards
    /// like `if (! function_exists('...'))`) using the shared helper which
    /// recurses into if/block statements.
    pub(super) fn extract_file_functions<'a>(
        program: &'a Program<'a>,
        decls: &FileDeclarations,
        doc_ctx: &DocblockCtx<'a>,
    ) -> Vec<FunctionInfo> {
        let mut functions = Vec::new();
        // Give the docblock context the use-map and namespace in force
        // where each function is declared so that parameter default values
        // (e.g. `Application::class`) can be resolved to FQNs during
        // extraction.
        let doc_ctx_with =
            |use_map: &HashMap<String, String>, namespace: &Option<String>| DocblockCtx {
                trivias: doc_ctx.trivias,
                content: doc_ctx.content,
                php_version: doc_ctx.php_version,
                use_map: use_map.clone(),
                namespace: namespace.clone(),
            };
        let func_doc_ctx = doc_ctx_with(&decls.use_map, &decls.namespace);
        let multi_block = decls.namespace_spans.len() > 1;
        let mut block_index = 0;
        for statement in program.statements.iter() {
            let block = match statement {
                Statement::Namespace(_) if multi_block => {
                    block_index += 1;
                    Some(&decls.namespace_spans[block_index - 1])
                }
                _ => None,
            };
            let block_doc_ctx = block.map(|span| doc_ctx_with(&span.use_map, &span.namespace));
            Self::extract_functions_from_statements(
                std::iter::once(statement),
                &mut functions,
                &decls.namespace,
                Some(block_doc_ctx.as_ref().unwrap_or(&func_doc_ctx)),
            );
        }
        functions
    }
}

/// The class-likes and imports a file declares, gathered in one walk of
/// its top level.
#[derive(Default)]
pub(super) struct FileDeclarations {
    /// Each extracted class together with the namespace block it was
    /// declared in (its namespace, and its index in `namespace_spans`, or
    /// `None` outside any block).  This is critical for files that contain
    /// multiple `namespace { }` blocks, each declaring classes under a
    /// different namespace with its own imports.  The per-class namespace
    /// is used later when building the `fqn_uri_index`, and the block when
    /// resolving parent/trait names.
    pub(super) classes_with_ns: Vec<(ClassInfo, Option<String>, Option<usize>)>,
    pub(super) use_map: HashMap<String, String>,
    pub(super) symbol_imports: crate::types::SymbolImports,
    /// The file-level namespace: the first non-empty one.
    pub(super) namespace: Option<String>,
    pub(super) namespace_spans: Vec<NamespaceSpan>,
}

impl FileDeclarations {
    /// The file-wide table holds every block's imports.  A file with one
    /// block keeps them only there (see `NamespaceSpan::use_map`).
    fn fold_block_imports(&mut self) {
        let multi_block = self.namespace_spans.len() > 1;
        for span in &mut self.namespace_spans {
            if multi_block {
                self.use_map.extend(
                    span.use_map
                        .iter()
                        .map(|(alias, fqn)| (alias.clone(), fqn.clone())),
                );
                let block = &span.symbol_imports;
                self.symbol_imports
                    .functions
                    .extend(block.functions.iter().map(|(a, f)| (a.clone(), f.clone())));
                self.symbol_imports
                    .constants
                    .extend(block.constants.iter().map(|(a, f)| (a.clone(), f.clone())));
            } else {
                self.use_map.extend(std::mem::take(&mut span.use_map));
                let block = std::mem::take(&mut span.symbol_imports);
                self.symbol_imports.functions.extend(block.functions);
                self.symbol_imports.constants.extend(block.constants);
            }
        }
    }
}
