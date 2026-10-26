//! The class, function, and constant loader closures the type engine is
//! handed, each bound to a file's imports and namespace.

use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader, SourceClassLoader};
use crate::definition::member::MemberKind;
use crate::php_type::PhpType;
use crate::type_engine::resolver::{CtxLoaders, FileCtx, Loaders, ResolutionCtx, VarResolutionCtx};
use crate::types::{ClassInfo, FileContext, FunctionInfo, PerBlock};

impl Backend {
    /// Return a class-loader closure bound to a [`FileContext`].
    ///
    /// This is the convenience wrapper for the common case where the
    /// caller already has a `FileContext`.  For situations that need a
    /// different class list (e.g. patched/effective classes after error
    /// recovery), use [`class_loader_with`](Self::class_loader_with).
    pub(crate) fn class_loader<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> impl Fn(&str) -> Option<Arc<ClassInfo>> + 'a {
        self.class_loader_with(&ctx.classes, &ctx.use_map, &ctx.namespace)
    }

    /// Return one class-loader closure per `namespace` block of the file
    /// behind `ctx`, for a consumer that resolves names across the whole
    /// file rather than at one position.
    ///
    /// Each block's loader resolves source names against that block's own
    /// imports and namespace; pick the one for a name with
    /// [`PerBlock::at`](crate::types::PerBlock::at) at the name's offset.
    pub(crate) fn class_loaders<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> crate::types::PerBlock<'a, impl Fn(&str) -> Option<Arc<ClassInfo>> + 'a> {
        ctx.per_block(|use_map, namespace| self.class_loader_with(&ctx.classes, use_map, namespace))
    }

    /// The [`resolved_class_loader_with`](Self::resolved_class_loader_with)
    /// counterpart of [`class_loader`](Self::class_loader).
    pub(crate) fn resolved_class_loader<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> impl Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> + 'a {
        self.resolved_class_loader_with(&ctx.classes)
    }

    /// The [`resolved_class_loader_with`](Self::resolved_class_loader_with)
    /// counterpart of [`class_loaders`](Self::class_loaders).  A resolved
    /// name means the same class in every block, so the blocks share one
    /// loader; this keeps the two sets interchangeable at call sites.
    pub(crate) fn resolved_class_loaders<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> crate::types::PerBlock<'a, impl Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> + 'a> {
        ctx.per_block(|_, _| self.resolved_class_loader_with(&ctx.classes))
    }

    /// Return a class-loader closure for names as written in source,
    /// from individual file-context components.
    ///
    /// Names follow PHP's rule for a class named in a file: a leading `\`
    /// is exact, otherwise the first segment goes through the block's
    /// imports, else the name is qualified with the current namespace.
    /// There is no fallback to a global class: inside `namespace App`, an
    /// unimported `Foo` is `App\Foo` even when only a global `Foo` exists.
    ///
    /// Only hand this loader names spelled in source.  An already-resolved
    /// global FQN (`Redis`, from `use Redis as PhpRedis;` or `\Redis`)
    /// would be re-resolved through the file's imports and namespace; look
    /// one up with the [resolved loader](Self::resolved_class_loader_with).
    ///
    /// Useful when the class list differs from what is stored in a
    /// `FileContext` (e.g. after re-parsing patched content for error
    /// recovery).
    pub(crate) fn class_loader_with<'a>(
        &'a self,
        classes: &'a [Arc<ClassInfo>],
        use_map: &'a HashMap<String, String>,
        namespace: &'a Option<String>,
    ) -> impl Fn(&str) -> Option<Arc<ClassInfo>> + 'a {
        move |name: &str| self.resolve_written_class_name(name, classes, use_map, namespace)
    }

    /// Resolve a class name written in a file by PHP's rule; see
    /// [`class_loader_with`](Self::class_loader_with).
    fn resolve_written_class_name(
        &self,
        name: &str,
        local_classes: &[Arc<ClassInfo>],
        file_use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<Arc<ClassInfo>> {
        if let Some(stripped) = name.strip_prefix('\\') {
            return self.find_or_load_class(stripped);
        }
        let (first_segment, rest) = match name.find('\\') {
            Some(pos) => (&name[..pos], &name[pos..]),
            None => (name, ""),
        };
        if let Some(fqn_prefix) = crate::names::lookup_import(file_use_map, first_segment) {
            if rest.is_empty() {
                return self.find_or_load_class(fqn_prefix);
            }
            return self.find_or_load_class(&format!("{fqn_prefix}{rest}"));
        }
        if rest.is_empty() {
            if let Some(cls) = self.find_in_own_namespace(name, local_classes, file_namespace) {
                return Some(cls);
            }
            if file_namespace.is_some() {
                return None;
            }
            // Global scope: the name is the FQN, and a facade alias is
            // exactly where PHP's own `class_alias()` would have put it.
            return self.find_or_load_class(name);
        }
        match file_namespace {
            Some(ns) => self.find_or_load_class(&format!("{ns}\\{name}")),
            None => self.find_or_load_class(name),
        }
    }

    /// Return a class-loader closure for already-resolved names.
    ///
    /// Names that arrive here are FQNs: `parent_class`, `used_traits` and
    /// `interfaces` canonicalised by `resolve_parent_class_names`, stored
    /// type hints, and the type strings the type engine carries around.
    /// A bare `Redis` therefore means the global class, even when the
    /// file imports another class under that short name.  A name written
    /// in a file is resolved before it gets here (see
    /// [`class_loader_with`](Self::class_loader_with)), so a miss is not
    /// retried against any file's imports or namespace.
    ///
    /// `classes` are the caller's own classes, which a re-parse of patched
    /// content holds where the index does not.
    pub(crate) fn resolved_class_loader_with<'a>(
        &'a self,
        classes: &'a [Arc<ClassInfo>],
    ) -> impl Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> + 'a {
        move |fqn: Fqn<'_>| {
            let fqn = fqn.as_str();
            // Container keys and the configured-date sentinel are never
            // class names; `find_or_load_class` routes them itself.
            if fqn.contains('.')
                || fqn == crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN
            {
                return self.find_or_load_class(fqn);
            }
            if let Some(cls) = self.find_or_load_class_typed(&PhpType::parse(fqn)) {
                return Some(cls);
            }
            if let Some(local) = crate::class_lookup::find_class_by_fqn(classes, fqn) {
                return Some(self.link_loaded_class(Arc::clone(local)));
            }
            // Laravel's alias tables, which the lookup above leaves out.
            self.resolve_laravel_alias(fqn)
        }
    }

    /// Return a Laravel macro `$this` resolver closure bound to a class
    /// loader.
    ///
    /// Given the FQN of the class a `::macro(...)` call is registered
    /// against (e.g. `Illuminate\Support\Facades\Route`), resolves the
    /// concrete class `$this` is bound to inside the macro's closure body:
    /// loads the target, then maps a facade to its concrete class via
    /// [`facade_macro_concrete`](Self::facade_macro_concrete). Every
    /// [`ResolutionCtx`](crate::type_engine::resolver::ResolutionCtx)
    /// construction site should populate `laravel_macro_this_resolver`
    /// with this so that `$this` inside a macro closure resolves
    /// consistently across completion, hover, go-to-definition, and
    /// diagnostics.
    pub(crate) fn laravel_macro_this_resolver<'a, F>(
        &'a self,
        class_loader: F,
    ) -> impl Fn(&str) -> Option<Arc<ClassInfo>> + 'a
    where
        F: Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> + 'a,
    {
        move |target: &str| {
            let facade = class_loader(Fqn::new(target))?;
            let target_fqn = facade.fqn().to_string();
            let concrete = self
                .facade_macro_concrete(&target_fqn)
                .unwrap_or(target_fqn);
            self.find_or_load_class(&concrete)
        }
    }

    /// Build a [`ResolutionCtx`](crate::type_engine::resolver::ResolutionCtx)
    /// for `cursor_offset` in `file`.
    ///
    /// Fills the fields every request handler repeats: this backend and
    /// its resolved-class cache, no forward-walker scope, and a cursor
    /// that is neither inside a static method nor preserving `static`.
    /// The handful of callers that differ override just those fields
    /// with struct-update syntax, e.g. a diagnostic pass that already
    /// knows whether the span sits in a static method:
    ///
    /// ```text
    /// ResolutionCtx {
    ///     is_in_static_method: symbol_map.is_in_static_method(span.start),
    ///     ..self.resolution_ctx_at(file, current_class, span.start, loaders)
    /// }
    /// ```
    pub(crate) fn resolution_ctx_at<'a>(
        &'a self,
        file: FileCtx<'a>,
        current_class: Option<&'a ClassInfo>,
        cursor_offset: u32,
        loaders: CtxLoaders<'a>,
    ) -> ResolutionCtx<'a> {
        ResolutionCtx {
            laravel_macro_this_resolver: loaders.laravel_macro_this_resolver,
            resolved_class_cache: Some(&self.resolved_class_cache),
            function_loader: loaders.function_loader,
            ..ResolutionCtx::new(file.with_backend(Some(self)), current_class, cursor_offset)
        }
    }

    /// Return a function-loader closure bound to a [`FileContext`].
    ///
    /// This is the convenience wrapper for the common case where the
    /// caller already has a `FileContext`.  For situations that need
    /// explicit use-map / namespace values, use
    /// [`function_loader_with`](Self::function_loader_with).
    pub(crate) fn function_loader<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> impl Fn(&str, u32) -> Option<FunctionInfo> + 'a {
        self.function_loader_in(
            ctx,
            &ctx.use_map,
            &ctx.symbol_imports.functions,
            &ctx.namespace,
        )
    }

    /// The function loader for `ctx`'s file with one block's imports and
    /// namespace in force.  It also binds `$this` in a Pest test's
    /// closures.
    fn function_loader_in<'a>(
        &'a self,
        ctx: &'a FileContext,
        use_map: &'a HashMap<String, String>,
        function_imports: &'a HashMap<String, String>,
        namespace: &'a Option<String>,
    ) -> impl Fn(&str, u32) -> Option<FunctionInfo> + 'a {
        let load = self.function_loader_with(
            ctx.resolved_names.as_deref(),
            use_map,
            function_imports,
            namespace,
        );
        move |name: &str, offset: u32| {
            let mut fi = load(name, offset)?;
            self.bind_pest_closure_this(&mut fi, ctx.pest_uses.as_deref());
            Some(fi)
        }
    }

    /// Everything code in the file behind `ctx` resolves names and
    /// expressions through; see [`FileLoaders`].
    pub(crate) fn file_loaders<'a>(&'a self, ctx: &'a FileContext) -> FileLoaders<'a> {
        FileLoaders {
            backend: self,
            file: ctx,
            class_loaders: ctx.per_block(|use_map, namespace| -> BoxedClassLoader<'a> {
                Box::new(self.class_loader_with(&ctx.classes, use_map, namespace))
            }),
            resolved_class_loader: Box::new(self.resolved_class_loader_with(&ctx.classes)),
            function_loaders: ctx.per_block_with_symbol_imports(
                |use_map, symbol_imports, namespace| -> BoxedFunctionLoader<'a> {
                    Box::new(self.function_loader_in(
                        ctx,
                        use_map,
                        &symbol_imports.functions,
                        namespace,
                    ))
                },
            ),
            constant_loaders: ctx.per_block_with_symbol_imports(
                |use_map, symbol_imports, namespace| -> BoxedConstantLoader<'a> {
                    Box::new(self.constant_loader_with(
                        ctx.resolved_names.as_deref(),
                        use_map,
                        &symbol_imports.constants,
                        namespace,
                    ))
                },
            ),
            placeholders: ctx.per_block(|_, namespace| {
                crate::class_lookup::placeholder_in_namespace(namespace.as_deref())
            }),
            // A macro target is a resolved name, which means the same class
            // in every block, so one resolver serves them all.
            macro_this_resolver: Box::new(
                self.laravel_macro_this_resolver(self.resolved_class_loader_with(&ctx.classes)),
            ),
            config_resolver: Box::new(move |key: &str| self.resolve_config_type(key)),
            trans_resolver: Box::new(move |key: &str| self.resolve_trans_type(key)),
        }
    }

    /// Return a function-loader closure from individual file-context
    /// components.
    ///
    /// Useful when the caller does not have a full `FileContext` or
    /// needs to use different imports / namespace.  `use_map` is the
    /// class import table, which the first segment of a qualified name
    /// goes through, and `function_imports` the `use function` table.
    /// The closure's second argument is the byte offset of the
    /// function-name identifier; pass `0` when no offset is available.
    /// Callers that have per-offset name resolution should supply
    /// `resolved_names` so functions declared in a different `namespace`
    /// block of the same file resolve correctly.
    pub(crate) fn function_loader_with<'a>(
        &'a self,
        resolved_names: Option<&'a crate::names::OwnedResolvedNames>,
        use_map: &'a HashMap<String, String>,
        function_imports: &'a HashMap<String, String>,
        namespace: &'a Option<String>,
    ) -> impl Fn(&str, u32) -> Option<FunctionInfo> + 'a {
        move |name: &str, offset: u32| {
            self.resolve_function_name_at(
                name,
                resolved_names,
                offset,
                use_map,
                function_imports,
                namespace,
            )
        }
    }

    /// Check whether `cursor_offset` is inside a closure whose
    /// enclosing call site declares `@param-closure-this`, and if so
    /// return the overridden class.
    ///
    /// This is a convenience wrapper that builds the [`ResolutionCtx`]
    /// and calls [`find_closure_this_override`] so that callers (hover,
    /// go-to-definition, go-to-type-definition) don't need to duplicate
    /// that boilerplate.
    pub(crate) fn resolve_closure_this_override(
        &self,
        uri: &str,
        content: &str,
        cursor_offset: u32,
    ) -> Option<Arc<ClassInfo>> {
        let ctx = self.file_context_at(uri, cursor_offset);
        if let Some(target) =
            crate::virtual_members::laravel::macro_closure_this_target(content, cursor_offset)
        {
            let target = self.facade_macro_concrete(&target).unwrap_or(target);
            if let Some(class) = self.find_or_load_class(&target) {
                let class_loader = self.resolved_class_loader(&ctx);
                return Some(crate::virtual_members::resolve_class_fully_maybe_cached(
                    &class,
                    &class_loader,
                    Some(&self.resolved_class_cache),
                ));
            }
        }
        let loaders = self.file_loaders(&ctx);
        crate::type_engine::variable::closure_resolution::find_closure_this_override(
            &loaders.resolution_ctx_at(content, cursor_offset),
        )
    }

    /// Return a constant-value-loader closure bound to a [`FileContext`].
    ///
    /// The returned closure takes the constant name as written and the
    /// byte offset of that name in the file, and returns
    /// `Some(Some(value))` when found with a known value,
    /// `Some(None)` when found without a value, and `None` when not
    /// found.  Pass `0` as the offset when none is available.
    pub(crate) fn constant_loader<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> impl Fn(&str, u32) -> Option<Option<String>> + 'a {
        self.constant_loader_with(
            ctx.resolved_names.as_deref(),
            &ctx.use_map,
            &ctx.symbol_imports.constants,
            &ctx.namespace,
        )
    }

    /// Bundle the four cross-file resolvers a diagnostic collector hands
    /// the type engine: function and constant lookup for `ctx`, plus the
    /// Laravel config and translation resolvers.
    ///
    /// Bind the result to a local and call
    /// [`LendsLoaders::loaders`](crate::type_engine::resolver::LendsLoaders::loaders)
    /// for the [`Loaders`](crate::type_engine::resolver::Loaders) itself,
    /// which borrows from it.
    pub(crate) fn diagnostic_loaders<'a>(
        &'a self,
        ctx: &'a FileContext,
    ) -> impl crate::type_engine::resolver::LendsLoaders + 'a {
        self.diagnostic_loaders_over(self.function_loader(ctx), self.constant_loader(ctx))
    }

    /// [`diagnostic_loaders`](Self::diagnostic_loaders), for a caller
    /// that already holds the function and constant loaders and only
    /// needs the two Laravel resolvers added.
    pub(crate) fn diagnostic_loaders_over<'a, F, C>(
        &'a self,
        function_loader: F,
        constant_loader: C,
    ) -> impl crate::type_engine::resolver::LendsLoaders + 'a
    where
        F: Fn(&str, u32) -> Option<crate::types::FunctionInfo> + 'a,
        C: Fn(&str, u32) -> Option<Option<String>> + 'a,
    {
        crate::type_engine::resolver::OwnedLoaders::new(
            function_loader,
            constant_loader,
            move |key: &str| self.resolve_config_type(key),
            move |key: &str| self.resolve_trans_type(key),
        )
    }

    /// Return a constant-value-loader closure from individual file-context
    /// components.
    ///
    /// The counterpart of [`function_loader_with`](Self::function_loader_with)
    /// for constants; see [`constant_loader`](Self::constant_loader) for the
    /// closure's contract.
    pub(crate) fn constant_loader_with<'a>(
        &'a self,
        resolved_names: Option<&'a crate::names::OwnedResolvedNames>,
        use_map: &'a HashMap<String, String>,
        constant_imports: &'a HashMap<String, String>,
        namespace: &'a Option<String>,
    ) -> impl Fn(&str, u32) -> Option<Option<String>> + 'a {
        move |name: &str, offset: u32| {
            self.resolve_constant_name_at(
                name,
                resolved_names,
                offset,
                use_map,
                constant_imports,
                namespace,
            )
        }
    }

    /// Resolve a constant reference to its value, trying the names PHP
    /// would try in the order it would try them.
    ///
    /// Constants are indexed under their fully-qualified name, so the name
    /// a reference writes has to be resolved against the file before it can
    /// be looked up: `Config\GRADES` goes through the `use` table the way a
    /// class name does, and a bare `GRADES` means the current namespace's
    /// constant when there is one and the global constant otherwise.
    ///
    /// `offset` is the starting byte offset of the constant-name identifier;
    /// pass `0` when none is available.  With an offset and `resolved_names`,
    /// mago-names supplies the primary candidate, which is the only source
    /// that gets multi-namespace files and `use const` aliases right.
    pub(crate) fn resolve_constant_name_at(
        &self,
        name: &str,
        resolved_names: Option<&crate::names::OwnedResolvedNames>,
        offset: u32,
        use_map: &HashMap<String, String>,
        constant_imports: &HashMap<String, String>,
        namespace: &Option<String>,
    ) -> Option<Option<String>> {
        // A leading backslash is an explicit absolute reference: it names
        // exactly one constant, with no namespace or import in play.
        if let Some(absolute) = name.strip_prefix('\\') {
            return self.lookup_global_constant_candidates(&[absolute]);
        }

        // `offset == 0` is the "no offset available" sentinel (no PHP
        // constant identifier ever starts at byte 0, before `<?php`).
        let offset_resolved: Option<&str> = if offset != 0 {
            resolved_names.and_then(|rn| rn.get(offset))
        } else {
            None
        };

        // Import-table resolution, for the paths that have no resolved
        // names: a bare name may be a `use const` alias, and the first
        // segment of a qualified name may be an imported namespace.
        let use_resolved: Option<String> = match name.split_once('\\') {
            Some((first, rest)) => {
                crate::names::lookup_import(use_map, first).map(|fqn| format!("{fqn}\\{rest}"))
            }
            None => constant_imports.get(name).cloned(),
        };
        let ns_qualified: Option<String> = namespace.as_ref().map(|ns| format!("{ns}\\{name}"));

        let mut candidates: Vec<&str> = Vec::with_capacity(4);
        if let Some(r) = offset_resolved {
            candidates.push(r);
        }
        if let Some(ref fqn) = use_resolved {
            candidates.push(fqn.as_str());
        }
        if let Some(ref nq) = ns_qualified {
            candidates.push(nq.as_str());
        }
        // The global fallback runs last: PHP only reaches it once the
        // namespace-local name has failed.
        candidates.push(name);
        candidates.dedup();

        self.lookup_global_constant_candidates(&candidates)
    }

    /// The member that an unqualified `@see` reference at `offset` names on
    /// the class whose docblock it sits in.
    ///
    /// phpDocumentor and PHPStorm both read a bare `name()` in `@see` as the
    /// enclosing class's own member before they read it as a global
    /// function, and docblocks in the wild are written against that
    /// convention.  Returns the class with its inherited and virtual members
    /// merged in, alongside the kind of member that matched, so callers can
    /// navigate to it or describe it.
    pub(crate) fn docblock_scope_member(
        &self,
        file: &FileContext,
        offset: u32,
        name: &str,
    ) -> Option<(Arc<ClassInfo>, MemberKind)> {
        let class = crate::class_lookup::find_documented_class_at_offset(&file.classes, offset)?;
        let class_loader = self.resolved_class_loader(file);
        let resolved = crate::virtual_members::resolve_class_fully_cached(
            class,
            &class_loader,
            &self.resolved_class_cache,
        );

        let kind = if resolved.get_method_ci(name).is_some() {
            MemberKind::Method
        } else if resolved.properties.iter().any(|p| p.name == name) {
            MemberKind::Property
        } else if resolved.constants.iter().any(|c| c.name == name) {
            MemberKind::Constant
        } else {
            return None;
        };
        Some((resolved, kind))
    }
}

type BoxedClassLoader<'a> = Box<SourceClassLoader<'a>>;
type BoxedResolvedClassLoader<'a> = Box<ResolvedClassLoader<'a>>;
type BoxedFunctionLoader<'a> = Box<dyn Fn(&str, u32) -> Option<FunctionInfo> + 'a>;
type BoxedConstantLoader<'a> = Box<dyn Fn(&str, u32) -> Option<Option<String>> + 'a>;
type BoxedKeyResolver<'a> = Box<dyn Fn(&str) -> Option<PhpType> + 'a>;

/// Everything code in a file resolves names and expressions through,
/// built by [`Backend::file_loaders`]: a class, function and constant
/// loader for each `namespace` block (see [`Backend::class_loaders`]), the
/// loader for already-resolved names, the Laravel config, translation and
/// macro `$this` resolvers, and for each block the placeholder class that
/// code outside a class reads as its own.
///
/// A pass over the whole file builds it once; a request at one position
/// builds it from the [`FileContext`] it already holds.  It hands out the
/// loaders, and the [`VarResolutionCtx`] or [`ResolutionCtx`], for an
/// offset, built from the loaders of the block the offset is in.
pub(crate) struct FileLoaders<'a> {
    backend: &'a Backend,
    file: &'a FileContext,
    class_loaders: PerBlock<'a, BoxedClassLoader<'a>>,
    /// A resolved name means the same class in every block.
    resolved_class_loader: BoxedResolvedClassLoader<'a>,
    function_loaders: PerBlock<'a, BoxedFunctionLoader<'a>>,
    constant_loaders: PerBlock<'a, BoxedConstantLoader<'a>>,
    placeholders: PerBlock<'a, ClassInfo>,
    macro_this_resolver: BoxedClassLoader<'a>,
    config_resolver: BoxedKeyResolver<'a>,
    trans_resolver: BoxedKeyResolver<'a>,
}

impl<'a> FileLoaders<'a> {
    /// The class loader for names as written in the block containing
    /// `offset`.
    pub(crate) fn class_loader_at(&self, offset: u32) -> &SourceClassLoader<'a> {
        self.class_loaders.at(offset).as_ref()
    }

    /// The class loader for already-resolved names.
    pub(crate) fn resolved_class_loader(&self) -> &ResolvedClassLoader<'a> {
        self.resolved_class_loader.as_ref()
    }

    /// The function loader for the block containing `offset`.
    pub(crate) fn function_loader_at(
        &self,
        offset: u32,
    ) -> &(dyn Fn(&str, u32) -> Option<FunctionInfo> + 'a) {
        self.function_loaders.at(offset).as_ref()
    }

    /// The constant loader for the block containing `offset`.
    pub(crate) fn constant_loader_at(
        &self,
        offset: u32,
    ) -> &(dyn Fn(&str, u32) -> Option<Option<String>> + 'a) {
        self.constant_loaders.at(offset).as_ref()
    }

    /// The class the code at `offset` is written in, if any.
    pub(crate) fn enclosing_class(&self, offset: u32) -> Option<&'a ClassInfo> {
        crate::class_lookup::find_class_at_offset(&self.file.classes, offset)
    }

    /// The class the code at `offset` reads as its own: the one it is
    /// written in, or outside a class a placeholder in its block's
    /// namespace.
    pub(crate) fn class_at(&self, offset: u32) -> &ClassInfo {
        self.enclosing_class(offset)
            .unwrap_or_else(|| self.placeholders.at(offset))
    }

    /// The cross-file loaders for the block containing `offset`.
    pub(crate) fn loaders_at(&self, offset: u32) -> Loaders<'_> {
        Loaders {
            function_loader: Some(self.function_loader_at(offset)),
            constant_loader: Some(self.constant_loader_at(offset)),
            config_resolver: Some(self.config_resolver.as_ref()),
            trans_resolver: Some(self.trans_resolver.as_ref()),
        }
    }

    /// The [`CtxLoaders`] for the block containing `offset`.
    pub(crate) fn ctx_loaders_at(&self, offset: u32) -> CtxLoaders<'_> {
        CtxLoaders::new(
            self.function_loader_at(offset),
            self.macro_this_resolver.as_ref(),
        )
    }

    /// The file these loaders were built for, with `content` as its text
    /// and the loaders and imports of the block containing `offset`.
    pub(crate) fn file_ctx_at<'b>(&'b self, content: &'b str, offset: u32) -> FileCtx<'b> {
        FileCtx::new(
            content,
            &self.file.classes,
            self.resolved_class_loader(),
            self.class_loader_at(offset),
        )
        .with_imports(Some(self.file.use_map_at(offset)))
        .with_backend(Some(self.backend))
    }

    /// The context for resolving an expression written at `offset` of
    /// `content`, the file these loaders were built for.
    pub(crate) fn var_ctx_at<'b>(&'b self, content: &'b str, offset: u32) -> VarResolutionCtx<'b> {
        VarResolutionCtx {
            loaders: self.loaders_at(offset),
            resolved_class_cache: Some(&self.backend.resolved_class_cache),
            ..VarResolutionCtx::new(
                "",
                self.file_ctx_at(content, offset),
                self.class_at(offset),
                offset,
            )
        }
    }

    /// The context for resolving a subject at `offset` of `content`; see
    /// [`Backend::resolution_ctx_at`] for the fields it leaves at their
    /// defaults.
    pub(crate) fn resolution_ctx_at<'b>(
        &'b self,
        content: &'b str,
        offset: u32,
    ) -> ResolutionCtx<'b> {
        self.backend.resolution_ctx_at(
            self.file_ctx_at(content, offset),
            self.enclosing_class(offset),
            offset,
            self.ctx_loaders_at(offset),
        )
    }
}
