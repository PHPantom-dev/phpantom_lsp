//! Argument count diagnostics.
//!
//! Walk the precomputed [`CallSite`] entries in the symbol map and flag
//! every call that passes too few or too many arguments relative to the
//! resolved callable's parameter list.
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

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::parser::with_parse_cache;
use crate::types::ResolvedCallableTarget;

use super::helpers::make_diagnostic;

/// Diagnostic code used for argument-count diagnostics.
pub(crate) const ARGUMENT_COUNT_MISMATCH_CODE: &str = "argument_count_mismatch";

/// Alternative minimum argument counts for built-in functions whose
/// signatures in phpstorm-stubs declare more required parameters than
/// PHP actually demands.
///
/// These are genuine overloads where PHP accepts fewer arguments than
/// any single stub declaration can express (e.g. `mt_rand()` accepts
/// 0 or 2 arguments, but the stub can only declare the 2-arg form).
///
/// Entries that previously existed because the stub parser did not
/// handle `#[PhpStormStubsElementAvailable]` attributes on parameters
/// have been removed. The AST parser now filters version-specific
/// parameters by the configured PHP version (default 8.5), producing
/// correct required counts without this workaround.
///
/// This map is derived from PHPStan's `functionMap.php` diffed against
/// phpstorm-stubs with proper version filtering applied.
/// Regenerate with `php scripts/check_overloads.php`.
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
        // ── Gather context under locks ──────────────────────────────
        let symbol_map = {
            let maps = self.symbol_maps.read();
            match maps.get(uri) {
                Some(sm) => sm.clone(),
                None => return,
            }
        };

        let file_ctx = self.file_context(uri);

        // Activate the thread-local parse cache so that every call to
        // `with_parsed_program(content, …)` in the resolution pipeline
        // reuses the same parsed AST instead of re-parsing the file.
        let _parse_guard = with_parse_cache(content);

        // Call-expression resolution cache: avoids re-resolving the
        // same call expression (e.g. `$purchaseFile->save`) at every
        // call site that uses it.
        let mut call_cache: HashMap<String, Option<ResolvedCallableTarget>> = HashMap::new();

        // ── Walk every call site ────────────────────────────────────
        for call_site in &symbol_map.call_sites {
            // Skip calls with argument unpacking — actual count is
            // unknown at static analysis time.
            if call_site.has_unpacking {
                continue;
            }

            let expr = &call_site.call_expression;

            // Look up or populate the call expression cache.
            let resolved = call_cache
                .entry(expr.clone())
                .or_insert_with(|| {
                    let position =
                        crate::util::offset_to_position(content, call_site.args_start as usize);
                    self.resolve_callable_target(expr, content, position, &file_ctx)
                })
                .clone();

            // Resolve the call expression to a callable target.
            let resolved = match resolved {
                Some(r) => r,
                None => continue,
            };

            // A callable that accepts any number of arguments (e.g. a class
            // with no explicit constructor, which PHP lets you call with
            // extra arguments) is never an argument-count error.
            if resolved.accepts_any_args {
                continue;
            }

            let params = &resolved.parameters;
            let actual_args = call_site.arg_count;

            // Count required parameters (no default, not variadic).
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

            // Count total non-variadic parameters.
            let has_variadic = params.iter().any(|p| p.is_variadic);
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
                    &call_site.named_arg_names,
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
                    DiagnosticSeverity::ERROR,
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
                    DiagnosticSeverity::ERROR,
                    ARGUMENT_COUNT_MISMATCH_CODE,
                    message,
                ));
            }
        }
    }
}
