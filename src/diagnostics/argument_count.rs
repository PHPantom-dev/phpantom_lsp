//! Argument count and named-argument diagnostics.
//!
//! Walk the precomputed [`CallSite`] entries in the symbol map and flag
//! every call that passes too few or too many arguments relative to the
//! resolved callable's parameter list, and every named argument that
//! callable declares no parameter for.
//!
//! Diagnostics use `Severity::Error` because passing the wrong number
//! of arguments crashes at runtime with a `TypeError`.
//!
//! Suppression rules:
//! - Calls that cannot be resolved are skipped (we already have
//!   unknown-function and unknown-member diagnostics for those).
//! - Calls that use argument unpacking (`...$args`) are skipped because
//!   the actual argument count is unknown at static analysis time.
//! - Methods routed through `__call` / `__callStatic` are skipped
//!   because the magic method accepts arbitrary arguments.
//! - Constructor calls on classes with no explicit `__construct` are
//!   skipped (PHP allows `new Foo()` even without a constructor).
//! - Functions listed in the overload map are checked against
//!   alternative minimum argument counts.  Some PHP built-in functions
//!   have genuinely overloaded signatures (e.g. `array_keys` accepts
//!   1 or 2-3 arguments, `mt_rand` accepts 0 or 2) that the
//!   phpstorm-stubs format cannot express with a single declaration.
//! - Unknown named arguments are reported unless the callee collects them:
//!   a variadic parameter takes any label passed for it, and a method the
//!   engine synthesized from an `@method` tag, a macro or a forwarder reaches
//!   `__call`, which collects them too.  A name the engine read from a
//!   callable *type* rather than from a declaration bounds no caller either.
//!   An extension shipped apart from php-src names its parameters differently
//!   from one release to the next, so its stub bounds no caller either.
//!   At a built-in, where the bundled stub rather than PHP's own arginfo
//!   supplies the names, a label that differs from a declared parameter only
//!   in case or underscores is accepted: the stubs spell some parameters
//!   otherwise than the runtime does.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::parser::with_parse_cache;

use super::helpers::{
    CallTargetCache, FileDiagnosticContext, classes_instantiated_by_variable,
    is_position_independent_call_expression, make_diagnostic,
};

/// Diagnostic code used for argument-count diagnostics.
pub(crate) const ARGUMENT_COUNT_MISMATCH_CODE: &str = "argument_count_mismatch";

/// Diagnostic code for a named argument no parameter of the call is named
/// after, which PHP throws on.
pub(crate) const UNKNOWN_NAMED_ARGUMENT_CODE: &str = "unknown_named_argument";

/// Alternative minimum argument counts for built-in functions whose
/// signatures in phpstorm-stubs declare more required parameters than
/// PHP actually demands.
///
/// These are genuine overloads where PHP accepts fewer arguments than
/// any single stub declaration can express (e.g. `mt_rand()` accepts
/// 0 or 2 arguments, but the stub can only declare the 2-arg form).
/// Version-specific optional parameters (declared via
/// `#[PhpStormStubsElementAvailable]`) don't need an entry here — the
/// AST parser filters those by the configured PHP version (default
/// 8.5) directly, so the stub's required count is already correct.
///
/// This map is derived from PHPStan's `functionMap.php` diffed against
/// phpstorm-stubs with proper version filtering applied.
fn overload_min_args(name: &str) -> Option<u32> {
    // Strip a leading namespace separator so `\mt_rand()` in namespaced code
    // matches the same overload entry as `mt_rand()`. Compare lowercase to
    // match PHP's case-insensitive function names.
    match name.trim_start_matches('\\').to_ascii_lowercase().as_str() {
        "apc_add" => Some(1),
        "apc_store" => Some(1),
        "apcu_add" => Some(1),
        "apcu_store" => Some(1),
        "array_keys" => Some(1),
        "array_multisort" => Some(1),
        "array_walk" => Some(2),
        "array_walk_recursive" => Some(2),
        "assert" => Some(1),
        "assert_options" => Some(1),
        "bcscale" => Some(0),
        "bzcompress" => Some(1),
        "collator_get_sort_key" => Some(2),
        "collator_sort_with_sort_keys" => Some(2),
        "compact" => Some(0),
        "crypt" => Some(1),
        "cubrid_put" => Some(2),
        "curl_version" => Some(0),
        "date_time_set" => Some(3),
        "datefmt_get_locale" => Some(1),
        "datefmt_get_timezone" => Some(0),
        "datefmt_localtime" => Some(1),
        "datefmt_parse" => Some(1),
        "debug_print_backtrace" => Some(0),
        "debug_zval_dump" => Some(0),
        "dirname" => Some(1),
        "easter_date" => Some(0),
        "eio_sendfile" => Some(4),
        "extract" => Some(1),
        "fgetcsv" => Some(1),
        "fputcsv" => Some(2),
        "fscanf" => Some(2),
        "fsockopen" => Some(1),
        "gearman_job_handle" => Some(0),
        "get_class" => Some(0),
        "get_defined_functions" => Some(0),
        "get_html_translation_table" => Some(0),
        "get_parent_class" => Some(0),
        "getenv" => Some(0),
        "getopt" => Some(1),
        "gettimeofday" => Some(0),
        "gmmktime" => Some(0),
        "gnupg_addsignkey" => Some(2),
        "grapheme_stripos" => Some(2),
        "grapheme_stristr" => Some(2),
        "grapheme_strpos" => Some(2),
        "grapheme_strripos" => Some(2),
        "grapheme_strrpos" => Some(2),
        "grapheme_strstr" => Some(2),
        "grapheme_substr" => Some(2),
        "gzgetss" => Some(2),
        "hash" => Some(2),
        "hash_file" => Some(2),
        "hash_init" => Some(1),
        "hash_pbkdf2" => Some(4),
        "http_persistent_handles_ident" => Some(0),
        "ibase_blob_info" => Some(1),
        "ibase_blob_open" => Some(1),
        "ibase_query" => Some(0),
        "idn_to_ascii" => Some(1),
        "idn_to_utf8" => Some(1),
        "imagefilter" => Some(2),
        "imagerotate" => Some(3),
        "imagettfbbox" => Some(4),
        "imagettftext" => Some(8),
        "imagexbm" => Some(1),
        "ini_get_all" => Some(0),
        "intlcal_from_date_time" => Some(1),
        "intlcal_set" => Some(3),
        "libxml_use_internal_errors" => Some(0),
        "locale_filter_matches" => Some(2),
        "locale_get_display_language" => Some(1),
        "locale_get_display_name" => Some(1),
        "locale_get_display_region" => Some(1),
        "locale_get_display_script" => Some(1),
        "locale_get_display_variant" => Some(1),
        "locale_lookup" => Some(2),
        "max" => Some(0),
        "mb_eregi_replace" => Some(3),
        "mb_parse_str" => Some(1),
        "microtime" => Some(0),
        "min" => Some(0),
        "mktime" => Some(0),
        "mt_rand" => Some(0),
        "mysqli_fetch_all" => Some(1),
        "mysqli_get_cache_stats" => Some(0),
        "mysqli_get_client_info" => Some(0),
        "mysqli_get_client_version" => Some(0),
        "mysqli_query" => Some(2),
        "mysqli_real_connect" => Some(0),
        "mysqli_stmt_execute" => Some(1),
        "mysqli_store_result" => Some(1),
        "normalizer_get_raw_decomposition" => Some(1),
        "number_format" => Some(1),
        "numfmt_format" => Some(1),
        "oci_free_descriptor" => Some(0),
        "oci_register_taf_callback" => Some(1),
        "odbc_exec" => Some(2),
        "openssl_decrypt" => Some(3),
        "openssl_encrypt" => Some(3),
        "openssl_pkcs7_verify" => Some(2),
        "openssl_seal" => Some(4),
        "pack" => Some(1),
        "parse_str" => Some(1),
        "pathinfo" => Some(1),
        "pcntl_async_signals" => Some(0),
        "pcntl_wait" => Some(1),
        "pcntl_waitpid" => Some(2),
        "pfsockopen" => Some(1),
        "pg_connect" => Some(1),
        "pg_pconnect" => Some(1),
        "php_uname" => Some(0),
        "phpinfo" => Some(0),
        "posix_getrlimit" => Some(0),
        "preg_replace_callback" => Some(3),
        "preg_replace_callback_array" => Some(2),
        "rand" => Some(0),
        "round" => Some(1),
        "session_set_save_handler" => Some(1),
        "session_start" => Some(0),
        "snmp_set_valueretrieval" => Some(0),
        "socket_cmsg_space" => Some(2),
        "socket_recvmsg" => Some(2),
        "sodium_crypto_pwhash_scryptsalsa208sha256" => Some(5),
        "sodium_crypto_scalarmult_base" => Some(1),
        "sprintf" => Some(1),
        "sscanf" => Some(2),
        "stomp_abort" => Some(1),
        "stomp_ack" => Some(1),
        "stomp_begin" => Some(1),
        "stomp_commit" => Some(1),
        "stomp_read_frame" => Some(0),
        "stomp_send" => Some(2),
        "stomp_subscribe" => Some(1),
        "stomp_unsubscribe" => Some(1),
        "str_getcsv" => Some(1),
        "stream_context_set_option" => Some(2),
        "stream_filter_append" => Some(2),
        "stream_filter_prepend" => Some(2),
        "stream_set_timeout" => Some(2),
        "strrchr" => Some(2),
        "strtok" => Some(1),
        "strtr" => Some(2),
        "svn_propget" => Some(2),
        "svn_proplist" => Some(1),
        "swoole_event_add" => Some(1),
        "token_get_all" => Some(1),
        "unpack" => Some(2),
        "unserialize" => Some(1),
        "wincache_ucache_add" => Some(1),
        "wincache_ucache_set" => Some(1),
        "xdebug_dump_aggr_profiling_data" => Some(0),
        "xdebug_get_function_stack" => Some(0),
        "xdiff_file_patch" => Some(3),
        "xdiff_string_patch" => Some(2),
        "zend_send_buffer" => Some(1),
        "zend_send_file" => Some(1),
        _ => None,
    }
}

impl Backend {
    /// Collect argument-count diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_argument_count_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_argument_count_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_argument_count_diagnostics`] over an already-gathered
    /// [`FileDiagnosticContext`].
    pub(crate) fn collect_argument_count_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        let file_ctx = &ctx.file;

        // Activate the thread-local parse cache so that every call to
        // `with_parsed_program(content, …)` in the resolution pipeline
        // reuses the same parsed AST instead of re-parsing the file.
        let _parse_guard = with_parse_cache(content);

        // Call-expression resolution cache: avoids re-resolving the
        // same call expression (e.g. `Fqn::method`) at every call site
        // that uses it.
        //
        // Only expressions that are guaranteed to resolve to the same
        // target everywhere in the file are cached (see
        // `is_position_independent_call_expression`). Variable-based
        // calls (`$purchaseFile->save`) and `self::`/`static::`/
        // `parent::` calls are resolved fresh every time because their
        // target depends on the call site's position. There is one cache
        // per `namespace` block, since the same text can name a different
        // target under another block's imports.
        let call_targets = CallTargetCache::new(file_ctx);
        let mut loaders = None;

        // ── Walk every call site ────────────────────────────────────
        for call_site in &symbol_map.call_sites {
            // Skip calls with argument unpacking — actual count is
            // unknown at static analysis time.
            if call_site.has_unpacking || !super::incremental::reports_site(call_site.args_start) {
                continue;
            }

            let expr = call_site.call_expression;

            // `new $class(…)` is checked against the constructor of the
            // one class the variable spells, never against the bound of a
            // `class-string<T>`, whose subclasses may take other arguments.
            if let Some(var_name) = expr.strip_prefix("new ")
                && var_name.starts_with('$')
            {
                let loaders = loaders.get_or_insert_with(|| self.file_loaders(file_ctx));
                let rctx = loaders.resolution_ctx_at(content, call_site.args_start);
                if !classes_instantiated_by_variable(var_name, &rctx)
                    .is_some_and(|classes| classes.len() == 1)
                {
                    continue;
                }
            }

            // Look up or populate the call expression cache.
            let resolved = if is_position_independent_call_expression(expr) {
                call_targets.target(self, expr, content, call_site.args_start, file_ctx)
            } else {
                self.resolve_callable_target_at_offset(
                    expr,
                    content,
                    call_site.args_start,
                    file_ctx,
                )
            };

            let resolved = match resolved {
                Some(r) => r,
                None => continue,
            };

            // ── Unknown named arguments ─────────────────────────────
            // PHP binds a label to the parameter that shares its name and
            // throws `Unknown named parameter $x` when the callee declares
            // none, so a label that matches nothing is a crash rather than a
            // style choice.
            self.report_unknown_named_arguments(uri, content, call_site, &resolved, file_ctx, out);

            // A callable that accepts any number of arguments (e.g. a class
            // with no explicit constructor, which PHP lets you call with
            // extra arguments) is never an argument-count error.
            if resolved.accepts_any_args {
                continue;
            }

            let params = &resolved.parameters;
            let actual_args = call_site.arg_count;

            let mut required_count = params.iter().filter(|p| p.is_required).count() as u32;

            // Consult the overload map: if this function has an
            // alternative minimum that is lower than the stub's
            // required count, use that instead.  The call expression
            // for standalone function calls is just the function name
            // (e.g. "array_keys"), so we can look it up directly.
            let overload_applied = overload_min_args(expr).is_some_and(|m| m < required_count);
            if let Some(overload_min) = overload_min_args(expr)
                && overload_min < required_count
            {
                required_count = overload_min;
            }

            // Unlimited if variadic, otherwise the declared parameter count.
            let has_variadic = resolved.reads_func_args || params.iter().any(|p| p.is_variadic);
            let max_count = if has_variadic {
                None // unlimited trailing args
            } else {
                Some(params.len() as u32)
            };

            // ── Too few arguments ───────────────────────────────────
            // When the call uses named arguments, a raw count comparison is
            // wrong: a named argument can fill a later parameter while an
            // earlier required one is left unsupplied (and vice versa).
            // Resolve named arguments to their parameters by name and report
            // the specific required parameters that no argument provides.
            // Overloaded built-ins are excluded because their stubs may
            // over-declare required parameters; the count path already
            // accounts for the overload minimum.
            let named_too_few = if !call_site.named_arg_indices.is_empty() && !overload_applied {
                let positional_count =
                    actual_args.saturating_sub(call_site.named_arg_indices.len() as u32);
                let missing = crate::call_args::missing_required_params(
                    params,
                    positional_count,
                    call_site.named_arg_names.iter(),
                );
                if missing.is_empty() {
                    None
                } else {
                    Some(format!(
                        "Missing required argument{}: {}",
                        if missing.len() == 1 { "" } else { "s" },
                        missing.join(", "),
                    ))
                }
            } else {
                None
            };

            let positional_too_few =
                call_site.named_arg_indices.is_empty() && actual_args < required_count;

            if named_too_few.is_some() || positional_too_few {
                let range = match self.offset_range_to_lsp_range(
                    uri,
                    content,
                    call_site.args_start.saturating_sub(1) as usize,
                    call_site.args_end.saturating_add(1) as usize,
                ) {
                    Some(r) => r,
                    None => continue,
                };

                let message = if let Some(named_message) = named_too_few {
                    named_message
                } else if has_variadic {
                    format!(
                        "Expected at least {} argument{}, got {}",
                        required_count,
                        if required_count == 1 { "" } else { "s" },
                        actual_args,
                    )
                } else if required_count == max_count.unwrap_or(0) {
                    format!(
                        "Expected {} argument{}, got {}",
                        required_count,
                        if required_count == 1 { "" } else { "s" },
                        actual_args,
                    )
                } else {
                    format!(
                        "Expected at least {} argument{}, got {}",
                        required_count,
                        if required_count == 1 { "" } else { "s" },
                        actual_args,
                    )
                };

                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Error,
                    ARGUMENT_COUNT_MISMATCH_CODE,
                    message,
                ));
                continue;
            }

            // ── Too many arguments ──────────────────────────────────
            if !self.config().diagnostics.extra_arguments_enabled() {
                continue;
            }

            if let Some(max) = max_count
                && actual_args > max
            {
                let range = match self.offset_range_to_lsp_range(
                    uri,
                    content,
                    call_site.args_start.saturating_sub(1) as usize,
                    call_site.args_end.saturating_add(1) as usize,
                ) {
                    Some(r) => r,
                    None => continue,
                };

                let message = if required_count == max {
                    format!(
                        "Expected {} argument{}, got {}",
                        max,
                        if max == 1 { "" } else { "s" },
                        actual_args,
                    )
                } else {
                    format!(
                        "Expected at most {} argument{}, got {}",
                        max,
                        if max == 1 { "" } else { "s" },
                        actual_args,
                    )
                };

                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Error,
                    ARGUMENT_COUNT_MISMATCH_CODE,
                    message,
                ));
            }
        }
    }

    /// Whether the method `expr` calls on `owner` is declared by the stub
    /// of an extension php-src does not ship, whose parameter names follow
    /// the installed release rather than the stub.
    ///
    /// Only asked of a call that passes a named argument, so the ancestor
    /// walk stays off the path every other call takes.
    fn method_declared_by_unbundled_extension(
        &self,
        owner: &str,
        expr: &str,
        file_ctx: &crate::types::FileContext,
    ) -> bool {
        let method = if expr.starts_with("new ") {
            "__construct"
        } else if expr.starts_with('(') {
            "__invoke"
        } else {
            let after_arrow = expr.rsplit("->").next().unwrap_or(expr);
            after_arrow.rsplit("::").next().unwrap_or(after_arrow)
        };
        let loader = self.resolved_class_loader(file_ctx);
        let Some(class) = loader(crate::class_lookup::Fqn::new(owner)) else {
            return false;
        };
        if class.get_method_ci(method).is_some() {
            return class.from_unbundled_extension;
        }
        let declares = |c: &crate::types::ClassInfo| c.get_method_ci(method).is_some();
        crate::inheritance::find_declaring_ancestor(&class, &loader, &declares)
            .map_or(class.from_unbundled_extension, |(_, declaring)| {
                declaring.from_unbundled_extension
            })
    }

    /// Flag every named argument the callee declares no parameter for.
    ///
    /// Runs from the argument-count pass, which already resolves each call
    /// site's callable and reads its parameter list.  Attributes are recorded
    /// as `new` call sites, so `#[Length(mni: 3)]` is checked here too.
    ///
    /// A label is proved wrong only against a parameter list that bounds what
    /// a caller may write; the notes below name the lists that do not.
    fn report_unknown_named_arguments(
        &self,
        uri: &str,
        content: &str,
        call_site: crate::symbol_map::CallSite<'_>,
        resolved: &crate::types::ResolvedCallableTarget,
        file_ctx: &crate::types::FileContext,
        out: &mut Vec<Diagnostic>,
    ) {
        if call_site.named_arg_indices.is_empty() {
            return;
        }

        // `binds_named_args` marks the targets whose declaration names its
        // parameters the way the runtime binds them: it is false for a
        // callable type, a partial application, and a built-in of an
        // extension php-src does not ship, whose stub cannot say which
        // names the installed release takes.  The same reads the method
        // behind a class the call resolves to.
        if !resolved.binds_named_args
            || resolved.owner_class.as_ref().is_some_and(|owner| {
                self.method_declared_by_unbundled_extension(
                    owner.as_str(),
                    call_site.call_expression,
                    file_ctx,
                )
            })
        {
            return;
        }

        // `accepts_any_args` marks a callable whose parameter list is not the
        // whole story: a bare `callable` value, or a class with no
        // constructor of its own.  The second still rejects labels — PHP
        // throws for `new WithoutConstructor(x: 1)` even though it swallows
        // positional arguments — but that reading only applies to a call the
        // engine spells `new …`.
        let is_instantiation = call_site.call_expression.starts_with("new ");
        if resolved.accepts_any_args && !is_instantiation {
            return;
        }

        // A variadic parameter collects the labels passed for it, which is how
        // `sprintf(format: 'x', 1, 2)` and `filter_var_array(value: $v,
        // filters: $f)` stay legal.
        //
        // Reading `func_get_args()` wins no such thing: it takes extra
        // *positional* arguments, and a label the signature does not declare
        // still throws — `collector(anything: 1)` against
        // `function collector() { return func_get_args(); }` is an error at
        // runtime, so a call like that is reported.
        let declaration_bounds_labels =
            |parameters: &[crate::types::ParameterInfo]| !parameters.iter().any(|p| p.is_variadic);
        if !declaration_bounds_labels(&resolved.parameters)
            || resolved
                .overloads
                .iter()
                .any(|overload| !declaration_bounds_labels(overload))
        {
            return;
        }
        // A method the engine synthesized — an `@method` tag, a macro, a
        // forwarder — documents what `__call` will receive rather than
        // bounding it, and `__call` collects whatever label the caller writes.
        if resolved.is_virtual_method {
            return;
        }
        // A callable *type* says what a callee takes and returns, never what
        // its parameters are called, so the names on such a list are the
        // engine's placeholders and no label can be proved wrong against them.
        if resolved.synthesized_parameter_names {
            return;
        }
        // A built-in's stub is a description of PHP's signature rather than the
        // signature itself, and the bundled stubs spell some parameters
        // otherwise than the runtime's arginfo does, so a name a stub does not
        // list is only proof that PHP throws when it is no parameter's name in
        // any spelling.  A name that differs from one the stub lists by case
        // or underscores alone is left alone; a name that matches nothing at
        // all is still reported.
        let spelling_is_a_guess = self.signature_from_embedded_stub(uri, call_site, resolved);

        for (name_index, arg_index) in call_site.named_arg_indices.iter().enumerate() {
            let Some(name) = call_site.named_arg_names.get(name_index) else {
                continue;
            };
            // PHP binds a label to the parameter whose name spells it, and a
            // parameter's name is case-sensitive, so `name:` does not fill a
            // `$Name` parameter.  A name any overload declares is one the
            // function takes, since which overload runs is the arguments'
            // business rather than the label's.
            let matches_a_parameter = resolved
                .parameters
                .iter()
                .chain(resolved.overloads.iter().flatten())
                .any(|p| {
                    let declared = bare_parameter_name(p);
                    declared == name
                        || (spelling_is_a_guess && same_name_spelled_differently(declared, name))
                });
            if matches_a_parameter {
                continue;
            }
            let Some(&offset) = call_site.arg_offsets.get(*arg_index as usize) else {
                continue;
            };
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                offset as usize,
                offset as usize + name.len(),
            ) else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Error,
                UNKNOWN_NAMED_ARGUMENT_CODE,
                format!("Unknown named parameter ${name}"),
            ));
        }
    }

    /// Whether the callee a call resolves to is declared by an embedded stub.
    ///
    /// The bundled stubs are the engine's own description of the built-in
    /// symbols, parsed with no file behind them; a consumer that would hold a
    /// caller to what they list is reading an approximation of PHP's signature
    /// rather than PHP's signature.
    fn signature_from_embedded_stub(
        &self,
        call_uri: &str,
        call_site: crate::symbol_map::CallSite<'_>,
        resolved: &crate::types::ResolvedCallableTarget,
    ) -> bool {
        if let Some(class) = resolved.owner_class.as_ref() {
            return self
                .find_class_file_uri(class, call_uri)
                .is_some_and(|uri| crate::stubs::is_stub_uri(&uri));
        }
        if call_site.call_expression.starts_with("new ") {
            // A `new` whose class the engine could not name resolved to no
            // declaration, so no list bounds it.
            return false;
        }
        let functions = self.global_functions().read();
        self.function_lookup_candidates_at(
            call_uri,
            call_site.args_start,
            call_site.call_expression,
        )
        .iter()
        .find_map(|fqn| functions.get(fqn.as_str()))
        .is_some_and(|(uri, _)| crate::stubs::is_stub_uri(uri))
    }
}

/// A parameter's name without the `$` its declaration carries.
fn bare_parameter_name(parameter: &crate::types::ParameterInfo) -> &str {
    parameter
        .name
        .strip_prefix('$')
        .unwrap_or(parameter.name.as_str())
}

/// Whether two parameter names are one name written differently — `xRounding`
/// and `x_rounding`, or `Name` and `name`.
///
/// The bundled stubs of an extension spell some of its parameters in the style
/// the stub's author chose rather than the one PHP's arginfo uses, and it is
/// the arginfo a named argument binds to.  Nothing else about such a signature
/// is in doubt, so this is the only forgiveness the built-in check extends.
fn same_name_spelled_differently(declared: &str, label: &str) -> bool {
    let same = |a: u8, b: u8| a == b || (a.is_ascii_alphabetic() && a.eq_ignore_ascii_case(&b));
    let mut left = declared.bytes().filter(|b| *b != b'_');
    let mut right = label.bytes().filter(|b| *b != b'_');
    loop {
        match (left.next(), right.next()) {
            (Some(a), Some(b)) if same(a, b) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}
