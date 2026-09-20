//! The variables a service provider puts in a template's scope.
//!
//! `View::share('key', $value)` adds a variable to every template's data, and
//! a view composer adds one to the data of the views it targets. Neither is
//! written in a template nor passed by any `view()` call site, so a template
//! that reads one has nothing to resolve it against unless the provider
//! registrations feed the declaration chain documented in [`super::signature`].
//!
//! The registrations themselves are scanned with the rest of a provider's
//! resources (see [`crate::virtual_members::laravel::view_data`]). What
//! happens here is resolving each recorded value expression to a type through
//! the same pipeline call-site inference uses, and matching a composer's view
//! patterns against the template's own names.
//!
//! Resolving means parsing the file each value expression sits in, which is
//! far too much work to repeat per template, so the whole set is resolved
//! once and cached until the next provider scan replaces it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::Backend;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::types::ClassInfo;
use crate::virtual_members::laravel::SharedViewVar;

use super::call_site_inference::InjectedVars;

/// One provider registration's variables, with the views they reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedVarGroup {
    /// The view name patterns the group applies to, as written. Empty means
    /// every template, which is what `View::share()` registers.
    pub(crate) views: Vec<String>,
    pub(crate) vars: InjectedVars,
}

impl Backend {
    /// The variables a service provider puts in the scope of a template
    /// addressable as any of `view_names`.
    pub(crate) fn blade_provider_vars(&self, view_names: &[String]) -> InjectedVars {
        let mut vars: InjectedVars = Vec::new();
        for group in self.blade_shared_var_groups().iter() {
            let reaches = group.views.is_empty()
                || group.views.iter().any(|pattern| {
                    view_names
                        .iter()
                        .any(|name| view_pattern_matches(pattern, name))
                });
            if !reaches {
                continue;
            }
            for (name, ty) in &group.vars {
                if vars.iter().any(|(declared, _)| declared == name) {
                    continue;
                }
                vars.push((name.clone(), ty.clone()));
            }
        }
        vars
    }

    /// The resolved registrations, built once per provider scan.
    fn blade_shared_var_groups(&self) -> Arc<Vec<SharedVarGroup>> {
        if let Some(cached) = self
            .laravel_string_key_cache
            .read()
            .shared_view_vars
            .clone()
        {
            return cached;
        }
        // One builder resolves while the rest wait, rather than every
        // diagnostic worker repeating the same parse of the same providers.
        let _guard = self.laravel_string_key_build_locks.shared_view_vars.lock();
        if let Some(cached) = self
            .laravel_string_key_cache
            .read()
            .shared_view_vars
            .clone()
        {
            return cached;
        }
        let groups = Arc::new(self.build_shared_var_groups());
        self.laravel_string_key_cache.write().shared_view_vars = Some(Arc::clone(&groups));
        groups
    }

    fn build_shared_var_groups(&self) -> Vec<SharedVarGroup> {
        let (shared, composers) = {
            let resources = self.laravel_provider_resources.read();
            (
                resources.shared_view_vars.clone(),
                resources.view_composers.clone(),
            )
        };
        if shared.is_empty() && composers.is_empty() {
            return Vec::new();
        }

        // A composer runs once the view's own data is set and the shared data
        // merged under it, so what it writes stands over both.
        let mut groups: Vec<(Vec<String>, Vec<SharedViewVar>)> = Vec::new();
        for composer in composers {
            let mut vars = composer.inline;
            if let Some(class) = composer.class.as_deref() {
                vars.extend(self.composer_class_vars(class));
            }
            if !vars.is_empty() {
                groups.push((composer.views, vars));
            }
        }
        if !shared.is_empty() {
            groups.push((Vec::new(), shared));
        }

        // Every value expression of one file is resolved in a single parse of
        // it: two providers' registrations commonly sit side by side.
        let mut by_file: HashMap<PathBuf, Vec<u32>> = HashMap::new();
        for (_, vars) in &groups {
            for var in vars {
                by_file
                    .entry(var.file.clone())
                    .or_default()
                    .push(var.offset);
            }
        }
        let mut types: HashMap<(PathBuf, u32), String> = HashMap::new();
        for (file, offsets) in by_file {
            let uri = crate::util::path_to_uri(&file);
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            for (offset, ty) in self.resolve_expression_types_at(&uri, &content, &offsets) {
                types.insert((file.clone(), offset), ty);
            }
        }

        groups
            .into_iter()
            .map(|(views, vars)| SharedVarGroup {
                views,
                vars: vars
                    .into_iter()
                    .map(|var| {
                        let ty = types
                            .get(&(var.file, var.offset))
                            .cloned()
                            .unwrap_or_else(|| "mixed".to_string());
                        (var.name, ty)
                    })
                    .collect(),
            })
            .collect()
    }

    /// The variables a composer class's `compose()` body declares.
    fn composer_class_vars(&self, fqn: &str) -> Vec<SharedViewVar> {
        // Loading the class also puts its file in the index, so `$this->…`
        // in the composer body resolves against the class it belongs to.
        if self.find_or_load_class(fqn).is_none() {
            return Vec::new();
        }
        let Some(uri) = self.resolve_class_uri(fqn) else {
            return Vec::new();
        };
        let path = tower_lsp::lsp_types::Url::parse(&uri)
            .ok()
            .and_then(|url| url.to_file_path().ok());
        let (Some(path), Some(content)) = (path, self.get_file_content(&uri)) else {
            return Vec::new();
        };
        crate::virtual_members::laravel::composer_class_vars(&content, &path)
    }

    /// Resolve the expression starting at each of `offsets` in one file.
    ///
    /// Class names are rendered fully qualified, so the `@var` the template's
    /// prologue is given resolves from its namespace-less scope.
    fn resolve_expression_types_at(
        &self,
        uri: &str,
        content: &str,
        offsets: &[u32],
    ) -> Vec<(u32, String)> {
        let file_ctx = self.file_context(uri);
        let function_loader = self.function_loader(&file_ctx);
        let function_loader_cl = |name: &str, offset: u32| function_loader(name, offset);

        with_parsed_program(content, "laravel_view_data_types", |program, content| {
            let default_class = ClassInfo::default();
            // The imports come from this parse rather than the index: a
            // provider is scanned the moment the project is classified as
            // Laravel, which can be long before the workspace walk reaches
            // its file, and a short class name resolves against nothing
            // until its `use` line is known.
            let mut use_map = HashMap::new();
            Backend::extract_use_statements_from_statements(
                program.statements.iter(),
                &mut use_map,
            );
            let namespace = file_namespace(program).or_else(|| file_ctx.namespace.clone());
            let class_loader = self.class_loader_with(&file_ctx.classes, &use_map, &namespace);

            let mut found: Vec<(u32, &Expression<'_>)> = Vec::new();
            let walker = ExpressionAtOffsetWalker { offsets };
            let mut ctx = ExpressionCollectCtx { found: &mut found };
            for stmt in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, stmt, &mut ctx);
            }

            found
                .into_iter()
                .map(|(offset, expr)| {
                    let enclosing =
                        crate::class_lookup::find_class_at_offset(&file_ctx.classes, offset);
                    let current_class = enclosing.unwrap_or(&default_class);
                    let loaders = Loaders::with_function(Some(&function_loader_cl));
                    let var_ctx = VarResolutionCtx {
                        var_name: "",
                        top_level_scope: None,
                        current_class,
                        all_classes: &file_ctx.classes,
                        content,
                        cursor_offset: offset,
                        class_loader: &class_loader,
                        backend: Some(self),
                        loaders,
                        resolved_class_cache: Some(&self.resolved_class_cache),
                        enclosing_return_type: None,
                        branch_aware: false,
                        match_arm_narrowing: HashMap::new(),
                        scope_var_resolver: None,
                        scope_proofs: None,
                    };
                    let ty =
                        crate::type_engine::variable::foreach_resolution::resolve_expression_type(
                            expr, &var_ctx,
                        )
                        .unwrap_or_else(PhpType::mixed);
                    let ty = ty.resolve_names(&|name: &str| match class_loader(name) {
                        Some(class) => format!("\\{}", class.fqn()),
                        None => name.to_string(),
                    });
                    (offset, ty.to_string())
                })
                .collect()
        })
    }
}

/// The namespace a file declares, when it declares one.
fn file_namespace(program: &Program<'_>) -> Option<String> {
    program
        .statements
        .iter()
        .find_map(|statement| match statement {
            Statement::Namespace(ns) => Some(
                crate::atom::bytes_to_str(ns.name.as_ref()?.value())
                    .trim_matches('\\')
                    .to_string(),
            ),
            _ => None,
        })
}

/// Whether a composer's view pattern covers a view name, with `*` matching
/// any run of characters, as Laravel's own `Str::is()` does.
fn view_pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern == name {
        return true;
    }
    let mut segments = pattern.split('*');
    let Some(first) = segments.next() else {
        return false;
    };
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let mut segments: Vec<&str> = segments.collect();
    // Nothing followed the last `*`, so a literal pattern reaching here has
    // already failed the equality check above.
    let Some(last) = segments.pop() else {
        return false;
    };
    for segment in segments {
        match rest.find(segment) {
            Some(at) => rest = &rest[at + segment.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// Collects the outermost expression starting at each requested offset.
///
/// A method call and its receiver variable share a start offset, so the
/// pre-order visit order is what makes `$this->service->all()` resolve as the
/// call rather than as `$this`.
struct ExpressionAtOffsetWalker<'a> {
    offsets: &'a [u32],
}

struct ExpressionCollectCtx<'w, 'ast, 'arena> {
    found: &'w mut Vec<(u32, &'ast Expression<'arena>)>,
}

impl<'ast, 'arena, 'w>
    mago_syntax::walker::Walker<'ast, 'arena, ExpressionCollectCtx<'w, 'ast, 'arena>>
    for ExpressionAtOffsetWalker<'_>
{
    fn walk_in_expression(
        &self,
        node: &'ast Expression<'arena>,
        ctx: &mut ExpressionCollectCtx<'w, 'ast, 'arena>,
    ) {
        let offset = node.span().start.offset;
        if self.offsets.contains(&offset) && !ctx.found.iter().any(|(seen, _)| *seen == offset) {
            ctx.found.push((offset, node));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::view_pattern_matches;

    #[test]
    fn a_wildcard_pattern_covers_the_names_below_it() {
        assert!(view_pattern_matches("profile", "profile"));
        assert!(!view_pattern_matches("profile", "profiles"));
        assert!(view_pattern_matches("*", "partials.header"));
        assert!(view_pattern_matches("partials.*", "partials.header"));
        assert!(!view_pattern_matches("partials.*", "layouts.header"));
        assert!(view_pattern_matches("*.header", "partials.header"));
        assert!(view_pattern_matches("admin.*.form", "admin.users.form"));
        assert!(!view_pattern_matches("admin.*.form", "admin.users.index"));
    }

    /// The two halves of a pattern must not be allowed to overlap in the
    /// name, or `a*a` would match the single `a` in `admin`.
    #[test]
    fn wildcard_segments_cannot_overlap() {
        assert!(!view_pattern_matches("ad*min", "admin.users"));
        assert!(view_pattern_matches("ad*min", "administrator.admin"));
    }
}
