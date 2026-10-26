//! Tests for the return type of `__()` / `trans()` / `Lang::get()`.
//!
//! The framework declares `string|array|null` because a key may name a
//! whole group and the keyless form hands the key straight back.  The key
//! at the call site settles which branch applies.

use crate::common::create_psr4_workspace;
use crate::common::{hover_response, open_php};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/"
        }
    }
}"#;

const AUTOLOAD_FILES: &str = "\
<?php
$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);
return array(
    'helpers' => $vendorDir . '/illuminate/Foundation/helpers.php',
);
";

const HELPERS: &str = "\
<?php

/**
 * @return string|array|null
 */
function __($key = null, $replace = [], $locale = null) { return null; }
";

const LANG_FACADE: &str = "\
<?php
namespace Illuminate\\Support\\Facades;

class Lang
{
    /** @return string|array|null */
    public static function get($key, array $replace = [], $locale = null) { return null; }
}
";

const LANG_MESSAGES: &str = "\
<?php
return [
    'welcome' => 'Welcome',
    'checkout' => [
        'headline' => 'Check out',
    ],
];
";

const CONSUMER: &str = "\
<?php
namespace App;

class Greeting
{
    public function demo(string $dynamic): void
    {
        $leaf = __('messages.welcome');
        $group = __('messages.checkout');
        $runtime = __($dynamic);
        $keyless = __();
        $viaLang = \\Illuminate\\Support\\Facades\\Lang::get('messages.welcome');
    }
}
";

async fn hover_after(needle: &str) -> String {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("lang/en/messages.php", LANG_MESSAGES),
            ("vendor/illuminate/Support/Facades/Lang.php", LANG_FACADE),
            ("src/Greeting.php", CONSUMER),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Uri::from_file_path(dir.path().join("src/Greeting.php")).unwrap();
    open_php(&backend, &uri, CONSUMER).await;

    let idx = CONSUMER.find(needle).expect("needle not found") + needle.len();
    let mut line = 0u32;
    let mut character = 0u32;
    for (i, ch) in CONSUMER.char_indices() {
        if i == idx {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    let hover = hover_response(&backend, &uri, line, character).await;

    match hover.map(|h| h.contents) {
        Some(Contents::MarkupContent(markup)) => markup.value,
        _ => String::new(),
    }
}

#[tokio::test]
async fn a_leaf_key_is_the_line_itself() {
    let text = hover_after("$leaf").await;
    assert!(text.contains("$leaf = string"), "got: {text}");
}

#[tokio::test]
async fn a_group_key_is_the_lines_beneath_it() {
    let text = hover_after("$group").await;
    assert!(
        text.contains("$group = array<string, mixed>"),
        "got: {text}"
    );
}

#[tokio::test]
async fn a_runtime_key_keeps_both_branches_but_drops_null() {
    let text = hover_after("$runtime").await;
    assert!(text.contains("string"), "got: {text}");
    assert!(text.contains("array"), "got: {text}");
    assert!(
        !text.contains("null"),
        "a call that names a key never returns null, got: {text}"
    );
}

#[tokio::test]
async fn the_keyless_form_hands_its_own_null_back() {
    let text = hover_after("$keyless").await;
    assert!(text.contains("$keyless = null"), "got: {text}");
}

#[tokio::test]
async fn the_lang_facade_resolves_the_same_way() {
    let text = hover_after("$viaLang").await;
    assert!(text.contains("$viaLang = string"), "got: {text}");
}

#[tokio::test]
async fn a_translation_result_passes_as_a_string_argument() {
    const CALLER: &str = "\
<?php
namespace App;

class Banner
{
    public function show(string $line): void {}

    public function demo(string $dynamic): void
    {
        $this->show(__('messages.welcome'));
        $this->show(__($dynamic));
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("lang/en/messages.php", LANG_MESSAGES),
            ("vendor/illuminate/Support/Facades/Lang.php", LANG_FACADE),
            ("vendor/illuminate/Foundation/helpers.php", HELPERS),
            ("vendor/composer/autoload_files.php", AUTOLOAD_FILES),
            ("src/Banner.php", CALLER),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Uri::from_file_path(dir.path().join("src/Banner.php")).unwrap();
    open_php(&backend, &uri, CALLER).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), CALLER, &mut diags);

    assert!(
        diags.is_empty(),
        "a translated line is a string wherever the key came from, got: {diags:?}"
    );
}
