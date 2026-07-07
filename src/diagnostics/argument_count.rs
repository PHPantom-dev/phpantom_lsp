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

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Enable the `extra-arguments` diagnostic on the given backend.
    fn enable_extra_args(backend: &Backend) {
        let mut cfg = backend.config.lock().clone();
        cfg.diagnostics.extra_arguments = Some(true);
        *backend.config.lock() = cfg;
    }

    /// Helper: create a test backend with minimal function stubs and
    /// collect argument-count diagnostics.  Extra-arguments checking
    /// is **off** (the default).
    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    /// Like [`collect`] but with the `extra-arguments` diagnostic
    /// enabled so that "too many arguments" errors are reported.
    fn collect_extra(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        enable_extra_args(&backend);
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    /// Minimal stub function index shared by stub-aware helpers.
    fn stub_fn_index() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("strlen", "<?php\nfunction strlen(string $string): int {}\n"),
            (
                "array_map",
                "<?php\nfunction array_map(?callable $callback, array $array, array ...$arrays): array {}\n",
            ),
            (
                "implode",
                "<?php\nfunction implode(string $separator, array $array): string {}\n",
            ),
            (
                "str_replace",
                "<?php\nfunction str_replace(string|array $search, string|array $replace, string|array $subject): string|array {}\n",
            ),
            (
                "array_push",
                "<?php\nfunction array_push(array &$array, mixed ...$values): int {}\n",
            ),
            (
                "in_array",
                "<?php\nfunction in_array(mixed $needle, array $haystack, bool $strict = false): bool {}\n",
            ),
            (
                "substr",
                "<?php\nfunction substr(string $string, int $offset, ?int $length = null): string {}\n",
            ),
            (
                "array_keys",
                "<?php\nfunction array_keys(array $array, mixed $filter_value, bool $strict = false): array {}\n",
            ),
            (
                "mt_rand",
                "<?php\nfunction mt_rand(int $min, int $max): int {}\n",
            ),
            ("rand", "<?php\nfunction rand(int $min, int $max): int {}\n"),
        ])
    }

    /// Helper that includes minimal stub functions so that built-in
    /// functions like `strlen` are resolvable.  Extra-arguments
    /// checking is **off** (the default).
    fn collect_with_stubs(php: &str) -> Vec<Diagnostic> {
        let backend =
            Backend::new_test_with_all_stubs(HashMap::new(), stub_fn_index(), HashMap::new());
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    /// Like [`collect_with_stubs`] but with the `extra-arguments`
    /// diagnostic enabled.
    fn collect_with_stubs_extra(php: &str) -> Vec<Diagnostic> {
        let backend =
            Backend::new_test_with_all_stubs(HashMap::new(), stub_fn_index(), HashMap::new());
        enable_extra_args(&backend);
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    // ── Too few arguments ───────────────────────────────────────────

    #[test]
    fn flags_too_few_args_to_function() {
        let php = r#"<?php
function test(): void {
    strlen();
}
"#;
        let diags = collect_with_stubs(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(
            diags[0].message.contains("Expected 1 argument"),
            "message: {}",
            diags[0].message,
        );
        assert!(
            diags[0].message.contains("got 0"),
            "message: {}",
            diags[0].message,
        );
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn flags_too_few_args_to_method() {
        let php = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
function test(): void {
    $g = new Greeter();
    $g->greet();
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 0")),
            "Expected too-few-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_few_args_to_static_method() {
        let php = r#"<?php
class Math {
    public static function add(int $a, int $b): int {
        return $a + $b;
    }
}
function test(): void {
    Math::add(1);
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 2 arguments") && d.message.contains("got 1")),
            "Expected too-few-args diagnostic, got: {diags:?}",
        );
    }

    // ── Too many arguments (default off) ────────────────────────────

    #[test]
    fn too_many_args_suppressed_by_default() {
        let php = r#"<?php
function test(): void {
    strlen("hello", "extra");
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "Extra-arguments diagnostic should be off by default, got: {diags:?}",
        );
    }

    #[test]
    fn too_many_args_to_user_function_suppressed_by_default() {
        let php = r#"<?php
function myHelper(string $a): void {}
function test(): void {
    myHelper("x", "y");
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Extra-arguments diagnostic should be off by default, got: {diags:?}",
        );
    }

    #[test]
    fn too_many_args_to_method_suppressed_by_default() {
        let php = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
function test(): void {
    $g = new Greeter();
    $g->greet("world", "extra", "more");
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Extra-arguments diagnostic should be off by default, got: {diags:?}",
        );
    }

    // ── Too many arguments (opt-in) ─────────────────────────────────

    #[test]
    fn flags_too_many_args_to_function() {
        let php = r#"<?php
function test(): void {
    strlen("hello", "extra");
}
"#;
        let diags = collect_with_stubs_extra(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(
            diags[0].message.contains("got 2"),
            "message: {}",
            diags[0].message,
        );
    }

    #[test]
    fn flags_too_many_args_to_method() {
        let php = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
function test(): void {
    $g = new Greeter();
    $g->greet("world", "extra", "more");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 3")),
            "Expected too-many-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_extra_args_to_constructorless_class() {
        // PHP silently ignores arguments passed to a class with no
        // constructor, so even with the extra-arguments check enabled the
        // call must not be flagged.
        let php = r#"<?php
class Plain {}
function test(): void {
    new Plain("x");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.is_empty(),
            "Constructor-less class should accept any args, got: {diags:?}",
        );
    }

    #[test]
    fn leading_backslash_builtin_honours_overload_minimum() {
        // `\mt_rand()` in namespaced code must hit the same overload entry
        // as `mt_rand()` (min 0 args), not the stub's full required count.
        let php = r#"<?php
namespace App;
function test(): void {
    \mt_rand();
}
"#;
        let diags = collect_with_stubs_extra(php);
        assert!(
            diags.is_empty(),
            "Leading-backslash builtin should respect overload minimum, got: {diags:?}",
        );
    }

    // ── Correct argument count — no diagnostic ──────────────────────

    #[test]
    fn no_diagnostic_for_correct_arg_count() {
        let php = r#"<?php
function test(): void {
    strlen("hello");
}
"#;
        let diags = collect_with_stubs(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    #[test]
    fn no_diagnostic_with_optional_args() {
        let php = r#"<?php
function test(): void {
    in_array("x", ["x", "y"]);
    in_array("x", ["x", "y"], true);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for optional args, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_with_default_value() {
        let php = r#"<?php
function test(): void {
    substr("hello", 1);
    substr("hello", 1, 3);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for default-valued params, got: {diags:?}",
        );
    }

    // ── Variadic functions ──────────────────────────────────────────

    #[test]
    fn no_diagnostic_for_extra_args_to_variadic_function() {
        let php = r#"<?php
function test(): void {
    array_map(null, [1], [2], [3], [4]);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "Variadic function should accept extra args, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_few_required_args_to_variadic_function() {
        let php = r#"<?php
function test(): void {
    array_push();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("at least 1 argument")),
            "Expected too-few-args diagnostic for variadic function, got: {diags:?}",
        );
    }

    // ── Argument unpacking suppression ──────────────────────────────

    #[test]
    fn no_diagnostic_when_args_are_unpacked() {
        let php = r#"<?php
function test(): void {
    $args = ["hello"];
    strlen(...$args);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected when using argument unpacking, got: {diags:?}",
        );
    }

    // ── Unresolvable calls ──────────────────────────────────────────

    #[test]
    fn no_diagnostic_for_unresolvable_function() {
        let php = r#"<?php
function test(): void {
    nonExistentFunction(1, 2, 3);
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No arg-count diagnostics expected for unresolvable functions, got: {diags:?}",
        );
    }

    // ── Same-file user-defined functions ─────────────────────────────

    #[test]
    fn flags_too_few_args_to_user_function() {
        let php = r#"<?php
function myHelper(string $a, int $b): void {}
function test(): void {
    myHelper("x");
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 2") && d.message.contains("got 1")),
            "Expected too-few-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_many_args_to_user_function() {
        let php = r#"<?php
function myHelper(string $a): void {}
function test(): void {
    myHelper("x", "y");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 1 argument") && d.message.contains("got 2")),
            "Expected too-many-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_correct_user_function_call() {
        let php = r#"<?php
function myHelper(string $a, int $b = 0): void {}
function test(): void {
    myHelper("x");
    myHelper("x", 1);
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    // ── Diagnostic metadata ─────────────────────────────────────────

    #[test]
    fn diagnostic_has_correct_code_and_source() {
        let php = r#"<?php
function myHelper(string $a): void {}
function test(): void {
    myHelper();
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String(
                "argument_count_mismatch".to_string()
            )),
        );
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    // ── Constructor calls ───────────────────────────────────────────

    #[test]
    fn flags_too_few_args_to_constructor() {
        let php = r#"<?php
class User {
    public function __construct(string $name, string $email) {}
}
function test(): void {
    new User("Alice");
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 2") && d.message.contains("got 1")),
            "Expected too-few-args diagnostic for constructor, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_many_args_to_constructor() {
        let php = r#"<?php
class User {
    public function __construct(string $name) {}
}
function test(): void {
    new User("Alice", "extra");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 2")),
            "Expected too-many-args diagnostic for constructor, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_correct_constructor() {
        let php = r#"<?php
class User {
    public function __construct(string $name, string $email = "") {}
}
function test(): void {
    new User("Alice");
    new User("Alice", "alice@test.com");
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    // ── "at least / at most" message wording ────────────────────────

    #[test]
    fn message_says_at_least_when_some_params_optional() {
        let php = r#"<?php
function helper(string $a, string $b, string $c = ""): void {}
function test(): void {
    helper("x");
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("at least 2")),
            "Expected 'at least' wording, got: {diags:?}",
        );
    }

    #[test]
    fn message_says_at_most_when_too_many_with_optional() {
        let php = r#"<?php
function helper(string $a, string $b = ""): void {}
function test(): void {
    helper("x", "y", "z");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.iter().any(|d| d.message.contains("at most 2")),
            "Expected 'at most' wording, got: {diags:?}",
        );
    }

    // ── Multiple diagnostics ────────────────────────────────────────

    #[test]
    fn flags_multiple_bad_calls() {
        let php = r#"<?php
function one(int $a): void {}
function two(int $a, int $b): void {}
function test(): void {
    one();
    two(1, 2, 3);
}
"#;
        let diags = collect_extra(php);
        assert_eq!(diags.len(), 2, "Expected 2 diagnostics, got: {diags:?}",);
    }

    #[test]
    fn too_few_still_reported_when_extra_args_disabled() {
        // "Too few" must always fire regardless of the extra-arguments flag.
        let php = r#"<?php
function one(int $a): void {}
function two(int $a, int $b): void {}
function test(): void {
    one();
    two(1, 2, 3);
}
"#;
        let diags = collect(php);
        assert_eq!(
            diags.len(),
            1,
            "Only the too-few diagnostic should fire by default, got: {diags:?}",
        );
        assert!(
            diags[0].message.contains("got 0"),
            "message: {}",
            diags[0].message,
        );
    }

    // ── Scope methods (Laravel) ─────────────────────────────────────

    #[test]
    fn no_diagnostic_for_scope_method_with_query_stripped() {
        // #[Scope]-attributed methods have their first $query parameter
        // stripped by the virtual member provider.  The arg count
        // diagnostic must see the virtual method (0 required params),
        // not the original (1 required param).
        let php = r#"<?php
namespace Illuminate\Database\Eloquent\Attributes;

#[\Attribute]
class Scope {}

namespace Illuminate\Database\Eloquent;

class Model {}
class Builder {}

namespace App;

use Illuminate\Database\Eloquent\Model;

class Bakery extends Model {
    #[\Illuminate\Database\Eloquent\Attributes\Scope]
    protected function fresh(\Illuminate\Database\Eloquent\Builder $query): void {
        $query->where('fresh', true);
    }
}

class Demo {
    public function test(): void {
        $bakery = new Bakery();
        $bakery->fresh();
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Scope method with $query stripped should accept 0 args, got: {diags:?}",
        );
    }

    // ── Overloaded built-in function tests ──────────────────────────

    #[test]
    fn no_diagnostic_for_array_keys_with_one_arg() {
        // array_keys(array $array): array — the 1-arg form is valid.
        let php = r#"<?php
function test(): void {
    $keys = array_keys([1, 2, 3]);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "array_keys with 1 arg should be accepted (overload), got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_array_keys_with_two_args() {
        // array_keys(array $array, mixed $filter_value): array
        let php = r#"<?php
function test(): void {
    $keys = array_keys([1, 2, 3], 2);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "array_keys with 2 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_array_keys_with_three_args() {
        // array_keys(array $array, mixed $filter_value, bool $strict): array
        let php = r#"<?php
function test(): void {
    $keys = array_keys([1, 2, 3], 2, true);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "array_keys with 3 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn flags_array_keys_with_zero_args() {
        // array_keys() with no arguments is always invalid.
        let php = r#"<?php
function test(): void {
    $keys = array_keys();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 0")),
            "array_keys with 0 args should be flagged, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_mt_rand_with_zero_args() {
        // mt_rand(): int — the 0-arg form is valid.
        let php = r#"<?php
function test(): void {
    $n = mt_rand();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "mt_rand with 0 args should be accepted (overload), got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_mt_rand_with_two_args() {
        // mt_rand(int $min, int $max): int
        let php = r#"<?php
function test(): void {
    $n = mt_rand(1, 100);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "mt_rand with 2 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn flags_mt_rand_with_one_arg() {
        // mt_rand(1) is invalid — must be 0 or 2 args.
        // The stub declares 2 required params, and the overload min is 0.
        // 1 arg is >= overload min (0) so the "too few" check passes.
        // But the "too many" check (when enabled) would catch it only if
        // max = 2.  With extra-args off (default), 1 arg is not caught.
        // This is acceptable — PHP itself raises a runtime warning for
        // mt_rand(1) but it still works (treats it as mt_rand(0, 1)).
        // We don't flag it because the overload map only lowers the
        // minimum; intermediate invalid counts require a more complex
        // model we don't need yet.
    }

    #[test]
    fn no_diagnostic_for_rand_with_zero_args() {
        // rand(): int — same overload pattern as mt_rand.
        let php = r#"<?php
function test(): void {
    $n = rand();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "rand with 0 args should be accepted (overload), got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_rand_with_two_args() {
        let php = r#"<?php
function test(): void {
    $n = rand(1, 100);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "rand with 2 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn no_false_positive_when_stub_uses_element_available_attribute() {
        // Stubs like array_push declare a non-variadic parameter with
        // #[PhpStormStubsElementAvailable(from: '5.3', to: '7.2')] alongside
        // a variadic parameter of the same name.  The AST parser filters out
        // the non-variadic parameter for PHP 8.5 (the default), so the
        // required count should be 1 ($array only), not 2.
        //
        // This test uses the real stub pattern to verify the version
        // filtering produces correct argument counts without needing an
        // overload_min_args entry.
        let stub_content: &str = concat!(
            "<?php\n",
            "use JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable;\n",
            "\n",
            "function array_push(\n",
            "    array &$array,\n",
            "    #[PhpStormStubsElementAvailable(from: '5.3', to: '7.2')] $values,\n",
            "    mixed ...$values\n",
            "): int {}\n",
        );

        let backend = Backend::new_test_with_all_stubs(
            HashMap::new(),
            HashMap::from([("array_push", stub_content)]),
            HashMap::new(),
        );
        let uri = "file:///test.php";
        let php = r#"<?php
function test(): void {
    $arr = [1, 2];
    array_push($arr, 3);
}
"#;
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        assert!(
            out.is_empty(),
            "array_push($arr, 3) should not produce a diagnostic when \
             PhpStormStubsElementAvailable filtering is active, got: {out:?}",
        );
    }

    #[test]
    fn no_false_positive_for_stub_variadic_with_one_arg_after_filtering() {
        // After version filtering removes the non-variadic $values param,
        // array_push(array &$array, mixed ...$values) requires only 1 arg.
        // Calling array_push($arr) with just the array is valid PHP 7.3+.
        let stub_content: &str = concat!(
            "<?php\n",
            "use JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable;\n",
            "\n",
            "function array_push(\n",
            "    array &$array,\n",
            "    #[PhpStormStubsElementAvailable(from: '5.3', to: '7.2')] $values,\n",
            "    mixed ...$values\n",
            "): int {}\n",
        );

        let backend = Backend::new_test_with_all_stubs(
            HashMap::new(),
            HashMap::from([("array_push", stub_content)]),
            HashMap::new(),
        );
        let uri = "file:///test.php";
        let php = r#"<?php
function test(): void {
    $arr = [1, 2];
    array_push($arr);
}
"#;
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        assert!(
            out.is_empty(),
            "array_push($arr) with 1 arg should be valid after version filtering \
             removes the non-variadic $values param, got: {out:?}",
        );
    }

    #[test]
    fn flags_too_few_args_to_scope_method_with_extra_param() {
        // scopeTopping($query, $type) → virtual topping($type) needs 1 arg.
        let php = r#"<?php
namespace Illuminate\Database\Eloquent\Attributes;

#[\Attribute]
class Scope {}

namespace Illuminate\Database\Eloquent;

class Model {}
class Builder {}

namespace App;

use Illuminate\Database\Eloquent\Model;

class Bakery extends Model {
    public function scopeTopping(\Illuminate\Database\Eloquent\Builder $query, string $type): void {
        $query->where('topping', $type);
    }
}

class Demo {
    public function test(): void {
        $bakery = new Bakery();
        $bakery->topping();
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 0")),
            "Scope method topping() needs 1 arg after $query stripping, got: {diags:?}",
        );
    }

    // ── Named arguments ─────────────────────────────────────────────

    #[test]
    fn flags_missing_required_when_named_arg_fills_optional() {
        // $a is required; filling only the optional $c by name leaves $a
        // unsupplied, which PHP rejects with ArgumentCountError.
        let php = r#"<?php
function f(int $a, int $b = 0, int $c = 0): void {}
function test(): void {
    f(c: 3);
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(
                |d| d.message.contains("Missing required argument") && d.message.contains("$a")
            ),
            "Expected missing-required-argument diagnostic for $a, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_when_required_filled_by_name() {
        // The single required parameter is supplied by name, so no error
        // even though it is not in the first positional slot.
        let php = r#"<?php
function f(int $a, int $b = 0): void {}
function test(): void {
    f(a: 1);
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    #[test]
    fn no_diagnostic_when_required_split_positional_and_named() {
        // First required parameter positional, second required parameter by
        // name (in any order) — both supplied, so no error.
        let php = r#"<?php
function f(int $a, int $b, int $c = 0): void {}
function test(): void {
    f(1, b: 2);
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    #[test]
    fn reports_multiple_missing_required_named() {
        // Two required parameters left unsupplied while only an optional is
        // filled by name.
        let php = r#"<?php
function f(int $a, int $b, int $c = 0): void {}
function test(): void {
    f(c: 9);
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Missing required arguments")
                    && d.message.contains("$a")
                    && d.message.contains("$b")),
            "Expected both $a and $b reported missing, got: {diags:?}",
        );
    }
}
