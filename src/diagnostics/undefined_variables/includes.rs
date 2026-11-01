//! Variables an `include`/`require` brings into the including scope.
//!
//! The included file runs in the scope of the statement that includes it,
//! so every variable its top level assigns exists once the include has
//! run. Where the path can be worked out statically (string literals,
//! `__DIR__`, `__FILE__`, `dirname()`, concatenation, and global
//! constants whose `define()` value is itself one of those), the included
//! file's top-level writes are read and treated as writes at the end of
//! the include. Where it cannot, or the included file defines variables
//! no static reading can name (`extract()`, `$$name`, an include of its
//! own that does not resolve), the scope is open from the include on and
//! the include stands for a write of every name ([`ANY_VARIABLE`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::resolution::DefineSearch;
use crate::scope_collector::{AccessKind, ScopeBody, collect_scope};

use super::feature_guards::{has_dynamic_variables, has_extract_call};
use super::is_in_nested_frame;

/// The name an include that may define anything writes: a read of any
/// variable after it counts as having a prior write.
pub(super) const ANY_VARIABLE: &str = "*";

/// How deep includes of includes, and constants defined in terms of
/// other constants, are followed before giving up.
const MAX_DEPTH: u8 = 8;

/// What the top level of an included file leaves defined.
#[derive(Default)]
enum IncludedVars {
    /// Exactly these variables.
    Known(Vec<String>),
    /// Variables no static reading can name.
    #[default]
    Open,
}

/// Resolves includes for one file's undefined-variable pass, reading each
/// included file once however many scopes include it.
pub(super) struct IncludeResolver<'a> {
    backend: &'a Backend,
    /// The including file, for `__FILE__` and `__DIR__`; `None` when its
    /// URI is not a file path, in which case no relative path resolves.
    file: Option<PathBuf>,
    files: RefCell<HashMap<PathBuf, Rc<IncludedVars>>>,
}

impl<'a> IncludeResolver<'a> {
    pub(super) fn new(backend: &'a Backend, uri: &str) -> Self {
        Self {
            backend,
            file: crate::util::uri_to_path(uri),
            files: RefCell::new(HashMap::new()),
        }
    }

    /// The writes the includes in `body` perform, each at the offset where
    /// its include ends, nested scopes included (the caller assigns each
    /// to the frame it sits in).
    pub(super) fn include_writes(
        &self,
        body: ScopeBody<'_, '_>,
        namespace: &Option<String>,
    ) -> Vec<(String, u32)> {
        let mut includes = Vec::new();
        body.walk_with(&IncludeWalker, &mut includes);
        let mut writes = Vec::new();
        for (value, end) in includes {
            self.push_include_writes(value, end, self.file.as_deref(), namespace, 0, &mut writes);
        }
        writes
    }

    fn push_include_writes(
        &self,
        value: &Expression<'_>,
        end: u32,
        file: Option<&Path>,
        namespace: &Option<String>,
        depth: u8,
        writes: &mut Vec<(String, u32)>,
    ) {
        let included = file
            .and_then(|file| self.resolve_path(value, file, namespace, depth))
            .map(|path| self.included_vars(&path, depth))
            .unwrap_or_else(|| Rc::new(IncludedVars::Open));
        match &*included {
            IncludedVars::Known(names) => {
                writes.extend(names.iter().map(|name| (name.clone(), end)));
            }
            IncludedVars::Open => writes.push((ANY_VARIABLE.to_string(), end)),
        }
    }

    /// The file an include's path expression names, when it can be worked
    /// out and exists.
    fn resolve_path(
        &self,
        value: &Expression<'_>,
        file: &Path,
        namespace: &Option<String>,
        depth: u8,
    ) -> Option<PathBuf> {
        let path = PathBuf::from(self.evaluate(value, file, namespace, depth)?);
        let path = if path.is_absolute() {
            path
        } else {
            file.parent()?.join(path)
        };
        let path = crate::document_links::normalize_path(&path);
        path.is_file().then_some(path)
    }

    /// The string an include path expression evaluates to.
    fn evaluate(
        &self,
        expr: &Expression<'_>,
        file: &Path,
        namespace: &Option<String>,
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
                let mut lhs = self.evaluate(binary.lhs, file, namespace, depth)?;
                lhs.push_str(&self.evaluate(binary.rhs, file, namespace, depth)?);
                Some(lhs)
            }
            Expression::Parenthesized(p) => self.evaluate(p.expression, file, namespace, depth),
            Expression::ConstantAccess(access) => {
                self.constant_value(bytes_to_str(access.name.value()), namespace, depth)
            }
            Expression::Call(Call::Function(call)) => {
                self.evaluate_dirname(call, file, namespace, depth)
            }
            _ => None,
        }
    }

    /// `dirname($path)` or `dirname($path, $levels)`.
    fn evaluate_dirname(
        &self,
        call: &FunctionCall<'_>,
        file: &Path,
        namespace: &Option<String>,
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
        let path = self.evaluate(args.next()?.value(), file, namespace, depth)?;
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
    fn constant_value(&self, name: &str, namespace: &Option<String>, depth: u8) -> Option<String> {
        if depth >= MAX_DEPTH {
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
            self.backend
                .find_or_load_define(&candidates, DefineSearch::Indexed, |info| {
                    Some((info.file_uri.clone(), info.value.clone()?))
                })??;
        let defining_file = crate::util::uri_to_path(&uri)?;
        let source = format!("<?php {value};");
        with_parsed_program(&source, "include_constant", |program, _| {
            let Some(Statement::Expression(stmt)) = program.statements.iter().nth(1) else {
                return None;
            };
            self.evaluate(stmt.expression, &defining_file, &None, depth + 1)
        })
    }

    /// The variables the top level of the file at `path` leaves defined.
    fn included_vars(&self, path: &Path, depth: u8) -> Rc<IncludedVars> {
        if let Some(found) = self.files.borrow().get(path) {
            return Rc::clone(found);
        }
        if depth >= MAX_DEPTH {
            return Rc::new(IncludedVars::Open);
        }
        // A file that includes itself, directly or through another, adds
        // nothing the first reading does not.
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), Rc::new(IncludedVars::Known(Vec::new())));

        let uri = crate::util::path_to_uri(path);
        let vars = match self.backend.get_file_content_arc(&uri) {
            Some(content) => with_parsed_program(&content, "include_vars", |program, content| {
                self.top_level_vars(program, content, path, depth)
            }),
            None => IncludedVars::Open,
        };
        let vars = Rc::new(vars);
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), Rc::clone(&vars));
        vars
    }

    fn top_level_vars(
        &self,
        program: &Program<'_>,
        content: &str,
        path: &Path,
        depth: u8,
    ) -> IncludedVars {
        let statements = program.statements.as_slice();
        let body = ScopeBody::Statements(statements);
        if has_dynamic_variables(body) || has_extract_call(body) {
            return IncludedVars::Open;
        }

        let scope = collect_scope(statements, 0, content.len() as u32);
        let Some(top) = scope.frames.first() else {
            return IncludedVars::Known(Vec::new());
        };
        let at_top_level = |offset: u32| !is_in_nested_frame(offset, top, &scope.frames);

        let mut names: Vec<String> = scope
            .accesses
            .iter()
            .filter(|access| matches!(access.kind, AccessKind::Write | AccessKind::ReadWrite))
            .filter(|access| at_top_level(access.offset))
            .map(|access| access.name.clone())
            .collect();

        let mut includes = Vec::new();
        body.walk_with(&IncludeWalker, &mut includes);
        let mut nested = Vec::new();
        let namespace = top_level_namespace(statements);
        for (value, end) in includes.into_iter().filter(|(_, end)| at_top_level(*end)) {
            self.push_include_writes(value, end, Some(path), &namespace, depth + 1, &mut nested);
        }
        for (name, _) in nested {
            if name == ANY_VARIABLE {
                return IncludedVars::Open;
            }
            names.push(name);
        }

        names.sort_unstable();
        names.dedup();
        IncludedVars::Known(names)
    }
}

/// The namespace an included file's top level declares, if any.
fn top_level_namespace(statements: &[Statement<'_>]) -> Option<String> {
    statements.iter().find_map(|stmt| match stmt {
        Statement::Namespace(ns) => ns
            .name
            .as_ref()
            .map(|n| bytes_to_str(n.value()).to_string()),
        _ => None,
    })
}

type IncludeSites<'ast, 'arena> = Vec<(&'ast Expression<'arena>, u32)>;

/// Collects each `include`/`include_once`/`require`/`require_once` path
/// expression with the offset its construct ends at.
///
/// Closures and arrow functions are walked, since the scope map has a
/// frame for each to attribute their includes to. Named functions and
/// class bodies are not: they have no frame there, and their includes
/// run in a scope of their own.
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
