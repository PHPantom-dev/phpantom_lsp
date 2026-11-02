//! The file an `include`/`require` path expression names.
//!
//! A path can be worked out statically when it is built from string
//! literals, `__DIR__`, `__FILE__`, `dirname()`, concatenation, and global
//! constants whose `define()` value is itself one of those.

use std::path::{Path, PathBuf};

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::scope_collector::ScopeBody;

use super::DefineSearch;

/// How deep constants defined in terms of other constants are followed
/// before giving up.
const MAX_CONSTANT_DEPTH: u8 = 8;

impl Backend {
    /// The existing file the include path expression `value`, written in
    /// `file` inside `namespace`, names.
    pub(crate) fn resolve_include_path(
        &self,
        value: &Expression<'_>,
        file: &Path,
        namespace: Option<&str>,
    ) -> Option<PathBuf> {
        let path = PathBuf::from(self.evaluate_include_path(value, file, namespace, 0)?);
        let path = if path.is_absolute() {
            path
        } else {
            file.parent()?.join(path)
        };
        let path = crate::document_links::normalize_path(&path);
        path.is_file().then_some(path)
    }

    /// The string an include path expression evaluates to.
    fn evaluate_include_path(
        &self,
        expr: &Expression<'_>,
        file: &Path,
        namespace: Option<&str>,
        depth: u8,
    ) -> Option<String> {
        match expr {
            Expression::Literal(Literal::String(s)) => {
                Some(bytes_to_str(s.value?).to_string()).filter(|v| !v.is_empty())
            }
            Expression::MagicConstant(MagicConstant::Directory(_)) => {
                Some(file.parent()?.to_string_lossy().into_owned())
            }
            Expression::MagicConstant(MagicConstant::File(_)) => {
                Some(file.to_string_lossy().into_owned())
            }
            Expression::Binary(binary) if binary.operator.is_concatenation() => {
                let mut lhs = self.evaluate_include_path(binary.lhs, file, namespace, depth)?;
                lhs.push_str(&self.evaluate_include_path(binary.rhs, file, namespace, depth)?);
                Some(lhs)
            }
            Expression::Parenthesized(p) => {
                self.evaluate_include_path(p.expression, file, namespace, depth)
            }
            Expression::ConstantAccess(access) => {
                self.include_constant_value(bytes_to_str(access.name.value()), namespace, depth)
            }
            Expression::Call(Call::Function(call)) => {
                self.evaluate_include_dirname(call, file, namespace, depth)
            }
            _ => None,
        }
    }

    /// `dirname($path)` or `dirname($path, $levels)`.
    fn evaluate_include_dirname(
        &self,
        call: &FunctionCall<'_>,
        file: &Path,
        namespace: Option<&str>,
        depth: u8,
    ) -> Option<String> {
        let Expression::Identifier(ident) = call.function else {
            return None;
        };
        let name = bytes_to_str(ident.value());
        if !name
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("dirname")
        {
            return None;
        }
        let mut args = call.argument_list.arguments.iter();
        let path = self.evaluate_include_path(args.next()?.value(), file, namespace, depth)?;
        let levels = match args.next().map(|arg| arg.value()) {
            None => 1,
            Some(Expression::Literal(Literal::Integer(int))) => int.value? as usize,
            Some(_) => return None,
        };
        let mut path = Path::new(&path);
        for _ in 0..levels {
            path = path.parent()?;
        }
        Some(path.to_string_lossy().into_owned())
    }

    /// The value of a global constant whose `define()` initializer is
    /// itself a path expression, evaluated in the file that defines it.
    fn include_constant_value(
        &self,
        name: &str,
        namespace: Option<&str>,
        depth: u8,
    ) -> Option<String> {
        if depth >= MAX_CONSTANT_DEPTH {
            return None;
        }
        let qualified;
        let mut candidates: Vec<&str> = Vec::with_capacity(2);
        match name.strip_prefix('\\') {
            Some(absolute) => candidates.push(absolute),
            None => {
                if let Some(ns) = namespace
                    && !name.contains('\\')
                {
                    qualified = format!("{ns}\\{name}");
                    candidates.push(&qualified);
                }
                candidates.push(name);
            }
        }
        let (uri, value) =
            self.find_or_load_define(&candidates, DefineSearch::Indexed, |info| {
                Some((info.file_uri.clone(), info.value.clone()?))
            })??;
        let defining_file = crate::util::uri_to_path(&uri)?;
        let source = format!("<?php {value};");
        with_parsed_program(&source, "include_constant", |program, _| {
            let Some(Statement::Expression(stmt)) = program.statements.iter().nth(1) else {
                return None;
            };
            self.evaluate_include_path(stmt.expression, &defining_file, None, depth + 1)
        })
    }
}

/// Each `include`/`include_once`/`require`/`require_once` path
/// expression in `body`, with the offset its construct ends at.
///
/// Closures and arrow functions are walked, since they run in a frame of
/// the body's own. Named functions and class bodies are not: their
/// includes run in a scope of their own.
pub(crate) fn include_sites<'ast, 'arena>(
    body: ScopeBody<'ast, 'arena>,
) -> IncludeSites<'ast, 'arena> {
    let mut sites = Vec::new();
    body.walk_with(&IncludeWalker, &mut sites);
    sites
}

pub(crate) type IncludeSites<'ast, 'arena> = Vec<(&'ast Expression<'arena>, u32)>;

/// Collects [`include_sites`].
struct IncludeWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, IncludeSites<'ast, 'arena>> for IncludeWalker {
    fn walk_function(
        &self,
        _node: &'ast Function<'arena>,
        _context: &mut IncludeSites<'ast, 'arena>,
    ) {
    }
    fn walk_class(&self, _node: &'ast Class<'arena>, _context: &mut IncludeSites<'ast, 'arena>) {}
    fn walk_trait(&self, _node: &'ast Trait<'arena>, _context: &mut IncludeSites<'ast, 'arena>) {}
    fn walk_enum(&self, _node: &'ast Enum<'arena>, _context: &mut IncludeSites<'ast, 'arena>) {}
    fn walk_anonymous_class(
        &self,
        node: &'ast AnonymousClass<'arena>,
        context: &mut IncludeSites<'ast, 'arena>,
    ) {
        if let Some(argument_list) = &node.argument_list {
            self.walk_partial_argument_list(argument_list, context);
        }
    }

    fn walk_in_construct(
        &self,
        node: &'ast Construct<'arena>,
        context: &mut IncludeSites<'ast, 'arena>,
    ) {
        let value = match node {
            Construct::Include(c) => c.value,
            Construct::IncludeOnce(c) => c.value,
            Construct::Require(c) => c.value,
            Construct::RequireOnce(c) => c.value,
            _ => return,
        };
        context.push((value, node.span().end.offset));
    }
}
