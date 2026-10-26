//! PHP parsing and AST extraction.
//!
//! This module contains the logic for parsing PHP source text using the
//! mago_syntax parser and extracting class information (methods, properties,
//! constants), `use` statement mappings, and namespace declarations from
//! the resulting AST.
//!
//! Sub-modules:
//! - [`classes`]: Class, interface, trait, and enum extraction
//! - [`params`]: Type hints, parameter lists and visibility modifiers
//! - [`anonymous`]: Anonymous class (`new class { ... }`) extraction
//! - [`attributes`]: Native PHP `#[Attribute]` target bitmask extraction
//! - [`stub_attributes`]: phpstorm-stubs version, type and shape attributes,
//!   and `#[Deprecated]`
//! - [`functions`]: Standalone function and `define()` constant extraction
//! - [`signature`]: Signature extraction shared by functions and methods
//! - [`use_statements`]: `use` statement and namespace extraction
//! - [`ast_update`]: The `update_ast` orchestrator
//! - [`file_declarations`]: The top-level walk for a file's class-likes,
//!   imports, namespace blocks and functions
//! - [`ast_publish`]: Publishing a batch of parsed files to the indexes
//! - [`name_resolution`]: Qualifying the names a parsed declaration spells
//! - [`initializers`]: Qualifying the classes a constant initializer names
//! - [`parse_cache`]: The thread-local cache that shares one parse per pass
//! - [`recovery`]: Parsing again the code the parser drops mid-edit
//! - [`bracket_recovery`]: Closing the brackets an unfinished expression
//!   leaves open before a `}`
//! - [`namespace_recovery`]: Keeping the braced `namespace` blocks the
//!   parser drops whole
//! - [`ast_helpers`]: Small helpers for walking the AST
//! - [`error_format`]: Human-readable formatting for Mago parse errors

use std::collections::HashMap;

mod anonymous;
mod ast_helpers;
mod ast_publish;
mod ast_update;
mod attributes;
mod bracket_recovery;
mod classes;
pub(crate) mod error_format;
mod file_declarations;
mod functions;
mod initializers;
mod name_resolution;
mod namespace_recovery;
mod params;
mod parse_cache;
mod recovery;
mod signature;
mod stub_attributes;
mod use_statements;

pub(crate) use ast_helpers::*;
pub(crate) use params::*;
pub(crate) use parse_cache::*;
pub(crate) use stub_attributes::*;

use crate::atom::{atom, bytes_to_str};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::types::*;

/// Context for resolving PHPDoc type annotations from docblock comments.
///
/// Bundles the program's trivia (comments/whitespace) and the raw source
/// text so that extraction functions can look up the `/** ... */` comment
/// preceding any AST node and parse `@return` / `@var` tags from it.
pub(crate) struct DocblockCtx<'a> {
    pub trivias: &'a [Trivia<'a>],
    pub content: &'a str,
    /// Target PHP version for version-aware stub filtering.
    ///
    /// When `Some`, elements annotated with
    /// `#[PhpStormStubsElementAvailable]` whose version range excludes
    /// this version are filtered out during extraction.  Set when
    /// parsing phpstorm-stubs; left as `None` for user code (where the
    /// attribute is never used).
    pub php_version: Option<PhpVersion>,
    /// Use-statement map for the file being parsed.
    ///
    /// Maps short (imported or aliased) names to their fully-qualified
    /// equivalents, e.g. `"Available"` → `"JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable"`.
    /// Used to resolve attribute names that appear under an alias.
    pub use_map: HashMap<String, String>,
    /// The file-level namespace, if any.
    ///
    /// Used by `is_known_deprecated_attr` to distinguish unqualified
    /// `#[Deprecated]` in the global namespace (which is the native PHP
    /// 8.4 attribute) from `#[Deprecated]` inside a user namespace (which
    /// would resolve to `App\Deprecated`, not the built-in).
    pub namespace: Option<String>,
}

impl Backend {
    /// Parse PHP source text and extract class information.
    /// Returns a Vec of ClassInfo for all classes found in the file.
    pub fn parse_php(&self, content: &str) -> Vec<ClassInfo> {
        Self::parse_php_versioned(content, None)
    }

    /// [`parse_php`](Self::parse_php) with the names each class declares
    /// resolved against its `namespace` block, the way an indexed file's
    /// classes are.  For re-parsing patched content whose classes the type
    /// engine reads in place of the file's own.
    pub(crate) fn parse_php_resolved(&self, content: &str) -> Vec<ClassInfo> {
        let _parse_guard = with_parse_cache(content);
        let use_map = self.parse_use_statements(content);
        let constant_imports = self.parse_symbol_imports(content).constants;
        let namespace = self.parse_namespace(content);
        let (mut classes, blocks) = Self::parse_php_classes_by_block(content, None);
        Self::resolve_parent_class_names_by_block(
            &mut classes,
            &blocks,
            &use_map,
            &constant_imports,
            &namespace,
        );
        classes
            .into_iter()
            .map(|(mut cls, ns, _)| {
                cls.file_namespace = ns.as_deref().map(atom);
                cls.cache_fqn();
                cls
            })
            .collect()
    }

    /// Version-aware variant of [`parse_php`].
    ///
    /// When `php_version` is `Some`, elements annotated with
    /// `#[PhpStormStubsElementAvailable]` whose version range excludes
    /// the target version are filtered out during extraction.
    pub fn parse_php_versioned(content: &str, php_version: Option<PhpVersion>) -> Vec<ClassInfo> {
        Self::parse_php_versioned_with_namespaces(content, php_version)
            .into_iter()
            .map(|(cls, _ns)| cls)
            .collect()
    }

    /// Like [`parse_php_versioned`] but returns each class together with
    /// the namespace block it was declared in.
    ///
    /// This is needed by [`parse_and_cache_content_versioned`](crate::resolution)
    /// so that multi-namespace stub files (e.g. PDO.php with both
    /// `namespace { }` and `namespace Pdo { }`) resolve parent class
    /// names against the correct namespace context.
    pub fn parse_php_versioned_with_namespaces(
        content: &str,
        php_version: Option<PhpVersion>,
    ) -> Vec<(ClassInfo, Option<String>)> {
        Self::parse_php_classes_by_block(content, php_version)
            .0
            .into_iter()
            .map(|(cls, ns, _)| (cls, ns))
            .collect()
    }

    /// Like [`parse_php_versioned_with_namespaces`] but also returns the
    /// file's `namespace` blocks, each with its own imports, and tags every
    /// class with the index of the block that declared it (`None` outside
    /// any block).
    ///
    /// The blocks are only returned for a file with more than one; a file
    /// with a single block resolves everything against its file-wide
    /// imports.
    ///
    /// [`parse_php_versioned_with_namespaces`]: Self::parse_php_versioned_with_namespaces
    #[allow(clippy::type_complexity)]
    pub(crate) fn parse_php_classes_by_block(
        content: &str,
        php_version: Option<PhpVersion>,
    ) -> (
        Vec<(ClassInfo, Option<String>, Option<usize>)>,
        Vec<NamespaceSpan>,
    ) {
        with_parsed_program(content, "parse_php", |program, content| {
            let mut use_map = HashMap::new();
            Self::extract_use_statements_from_statements(
                program.statements.iter(),
                &mut use_map,
                None,
            );
            let namespace = Self::extract_namespace_from_statements(program.statements.iter());

            let mut blocks: Vec<NamespaceSpan> = Vec::new();
            for statement in program.statements.iter() {
                if let Statement::Namespace(ns) = statement {
                    let mut block_use_map = HashMap::new();
                    let mut block_symbol_imports = crate::types::SymbolImports::default();
                    Self::extract_use_statements_from_statements(
                        ns.statements().iter(),
                        &mut block_use_map,
                        Some(&mut block_symbol_imports),
                    );
                    let span = ns.span();
                    blocks.push(NamespaceSpan {
                        namespace: ns
                            .name
                            .as_ref()
                            .map(|ident| bytes_to_str(ident.value()).to_string())
                            .filter(|n| !n.is_empty()),
                        start: span.start.offset,
                        end: span.end.offset,
                        use_map: block_use_map,
                        symbol_imports: block_symbol_imports,
                    });
                }
            }
            if blocks.len() <= 1 {
                blocks.clear();
            }

            let doc_ctx = DocblockCtx {
                trivias: program.trivia.as_slice(),
                content,
                php_version,
                use_map,
                namespace,
            };

            let mut result: Vec<(ClassInfo, Option<String>, Option<usize>)> = Vec::new();
            let mut block_index = 0;

            for statement in program.statements.iter() {
                match statement {
                    Statement::Namespace(ns) => {
                        let block_ns: Option<String> = ns
                            .name
                            .as_ref()
                            .map(|ident| bytes_to_str(ident.value()).to_string())
                            .filter(|n| !n.is_empty());

                        let block = blocks.get(block_index).map(|span| (block_index, span));
                        block_index += 1;
                        let block_doc_ctx = block.map(|(_, span)| DocblockCtx {
                            trivias: doc_ctx.trivias,
                            content: doc_ctx.content,
                            php_version: doc_ctx.php_version,
                            use_map: span.use_map.clone(),
                            namespace: span.namespace.clone(),
                        });

                        let mut block_classes = Vec::new();
                        Self::extract_classes_from_statements(
                            ns.statements().iter(),
                            &mut block_classes,
                            Some(block_doc_ctx.as_ref().unwrap_or(&doc_ctx)),
                        );
                        for cls in block_classes {
                            result.push((cls, block_ns.clone(), block.map(|(i, _)| i)));
                        }
                    }
                    statement if Self::is_classlike_extraction_candidate(statement) => {
                        let mut top_classes = Vec::new();
                        Self::extract_classes_from_statements(
                            std::iter::once(statement),
                            &mut top_classes,
                            Some(&doc_ctx),
                        );
                        for cls in top_classes {
                            result.push((cls, None, None));
                        }
                    }
                    _ => {}
                }
            }

            // A class-like declared in two branches of a conditional yields
            // one entry per branch; keep the first so resolution is
            // deterministic (see `dedup_class_likes_first_wins`).
            Self::dedup_class_likes_first_wins(&mut result, |(cls, ns, _)| (cls, ns));

            (result, blocks)
        })
    }

    /// Parse PHP source text and extract standalone function definitions.
    ///
    /// Returns a list of `FunctionInfo` for every `function` declaration
    /// found at the top level (or inside a namespace block).
    pub fn parse_functions(&self, content: &str) -> Vec<FunctionInfo> {
        self.parse_functions_versioned(content, None)
    }

    /// Version-aware variant of [`parse_functions`].
    ///
    /// When `php_version` is `Some`, functions and parameters annotated
    /// with `#[PhpStormStubsElementAvailable]` whose version range
    /// excludes the target version are filtered out.
    pub fn parse_functions_versioned(
        &self,
        content: &str,
        php_version: Option<PhpVersion>,
    ) -> Vec<FunctionInfo> {
        with_parsed_program(content, "parse_functions", |program, content| {
            let mut use_map = HashMap::new();
            Self::extract_use_statements_from_statements(
                program.statements.iter(),
                &mut use_map,
                None,
            );
            let namespace = Self::extract_namespace_from_statements(program.statements.iter());

            let doc_ctx = DocblockCtx {
                trivias: program.trivia.as_slice(),
                content,
                php_version,
                use_map,
                namespace,
            };

            let mut functions = Vec::new();
            Self::extract_functions_from_statements(
                program.statements.iter(),
                &mut functions,
                &None,
                Some(&doc_ctx),
            );
            functions
        })
    }

    /// Parse PHP source text and extract constant names from `define()` calls
    /// and top-level `const` statements.
    ///
    /// Returns a list of `(name, offset, value)` tuples for every
    /// `define('NAME', value)` call or `const NAME = value;` statement
    /// found at the top level, inside namespace blocks, block statements,
    /// or `if` guards.  A `const` is named fully-qualified, a `define()`
    /// exactly as it was written.
    pub fn parse_defines(&self, content: &str) -> Vec<(String, u32, Option<String>)> {
        with_parsed_program(content, "parse_defines", |program, content| {
            let mut defines = Vec::new();
            Self::extract_defines_from_statements(
                program.statements.iter(),
                &mut defines,
                content,
                None,
                None,
            );
            defines
        })
    }

    /// Parse PHP source text and extract `use` statement mappings.
    ///
    /// Returns a `HashMap` mapping short (imported) names to their
    /// fully-qualified equivalents.
    ///
    /// For example, `use Klarna\Rest\Resource;` produces
    /// `"Resource" → "Klarna\Rest\Resource"`.
    ///
    /// Handles:
    ///   - Simple use: `use Foo\Bar;`
    ///   - Aliased use: `use Foo\Bar as Baz;`
    ///   - Grouped use: `use Foo\{Bar, Baz};`
    ///   - Mixed grouped use: `use Foo\{Bar, function baz, const QUX};`
    ///     (function / const imports are skipped; see
    ///     [`parse_symbol_imports`](Self::parse_symbol_imports))
    ///   - Use statements inside namespace bodies
    pub(crate) fn parse_use_statements(&self, content: &str) -> HashMap<String, String> {
        with_parsed_program(content, "parse_use_statements", |program, _content| {
            let mut use_map = HashMap::new();
            Self::extract_use_statements_from_statements(
                program.statements.iter(),
                &mut use_map,
                None,
            );
            use_map
        })
    }

    /// Parse PHP source text and extract its `use function` and `use const`
    /// imports.
    pub(crate) fn parse_symbol_imports(&self, content: &str) -> crate::types::SymbolImports {
        with_parsed_program(content, "parse_symbol_imports", |program, _content| {
            let mut imports = crate::types::SymbolImports::default();
            Self::extract_use_statements_from_statements(
                program.statements.iter(),
                &mut HashMap::new(),
                Some(&mut imports),
            );
            imports
        })
    }

    /// Parse PHP source text and extract the declared namespace (if any).
    ///
    /// Returns the namespace string (e.g. `"Klarna\Rest\Checkout"`) or
    /// `None` if the file has no namespace declaration.
    pub(crate) fn parse_namespace(&self, content: &str) -> Option<String> {
        with_parsed_program(content, "parse_namespace", |program, _content| {
            Self::extract_namespace_from_statements(program.statements.iter())
        })
    }
}
