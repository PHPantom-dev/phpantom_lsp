use crate::common::{complete_at, create_test_backend};
use tower_lsp::lsp_types::*;

/// Returns `true` when the item carries the `DEPRECATED` tag.
fn is_tagged_deprecated(item: &CompletionItem) -> bool {
    item.tags
        .as_ref()
        .is_some_and(|t| t.contains(&CompletionItemTag::DEPRECATED))
}

// ─── Deprecated method ──────────────────────────────────────────────────────

/// A method marked `@deprecated` in its PHPDoc should carry the
/// `DEPRECATED` tag in its completion item.
#[tokio::test]
async fn test_deprecated_method_is_marked() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    /**\n",
        "     * @deprecated Use sendAsync() instead.\n",
        "     */\n",
        "    public function sendLegacy(): void {}\n",
        "\n",
        "    public function sendAsync(): void {}\n",
        "\n",
        "    public function run(): void {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    // Cursor after `$this->` on line 10
    let items = complete_at(&backend, &uri, text, 10, 15).await;

    let legacy = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("sendLegacy"));
    assert!(legacy.is_some(), "Should find sendLegacy in completions");
    assert!(
        is_tagged_deprecated(legacy.unwrap()),
        "sendLegacy should be tagged deprecated"
    );

    let async_method = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("sendAsync"));
    assert!(
        async_method.is_some(),
        "Should find sendAsync in completions"
    );
    assert!(
        !is_tagged_deprecated(async_method.unwrap()),
        "sendAsync should NOT be tagged deprecated"
    );
}

// ─── Non-deprecated method ──────────────────────────────────────────────────

/// A method without `@deprecated` should NOT be tagged deprecated.
#[tokio::test]
async fn test_non_deprecated_method_is_not_marked() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///non_deprecated_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @return void\n",
        "     */\n",
        "    public function doWork(): void {}\n",
        "\n",
        "    public function run(): void {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 15).await;

    let work = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("doWork"));
    assert!(work.is_some(), "Should find doWork in completions");
    assert!(
        !is_tagged_deprecated(work.unwrap()),
        "doWork should NOT be tagged deprecated"
    );
}

// ─── Deprecated property ────────────────────────────────────────────────────

/// A property marked `@deprecated` in its PHPDoc should carry the
/// `DEPRECATED` tag in its completion item.
#[tokio::test]
async fn test_deprecated_property_is_marked() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_property.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    /**\n",
        "     * @deprecated Use getDebugMode() instead.\n",
        "     */\n",
        "    public bool $debug = false;\n",
        "\n",
        "    public string $name = 'app';\n",
        "\n",
        "    public function test(): void {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 15).await;

    let debug = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("debug"));
    assert!(debug.is_some(), "Should find debug in completions");
    assert!(
        is_tagged_deprecated(debug.unwrap()),
        "debug property should be tagged deprecated"
    );

    let name = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("name"));
    assert!(name.is_some(), "Should find name in completions");
    assert!(
        !is_tagged_deprecated(name.unwrap()),
        "name property should NOT be tagged deprecated"
    );
}

// ─── Deprecated constant ────────────────────────────────────────────────────

/// A class constant marked `@deprecated` in its PHPDoc should carry the
/// `DEPRECATED` tag in its completion item.
#[tokio::test]
async fn test_deprecated_constant_is_marked() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_constant.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class HttpStatus {\n",
        "    /**\n",
        "     * @deprecated Use OK instead.\n",
        "     */\n",
        "    const SUCCESS = 200;\n",
        "\n",
        "    const OK = 200;\n",
        "}\n",
        "$x = HttpStatus::\n",
    );

    // Cursor after `HttpStatus::` on line 9
    let items = complete_at(&backend, &uri, text, 9, 17).await;

    let success = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("SUCCESS"));
    assert!(success.is_some(), "Should find SUCCESS in completions");
    assert!(
        is_tagged_deprecated(success.unwrap()),
        "SUCCESS constant should be tagged deprecated"
    );

    let ok = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("OK"));
    assert!(ok.is_some(), "Should find OK in completions");
    assert!(
        !is_tagged_deprecated(ok.unwrap()),
        "OK constant should NOT be tagged deprecated"
    );
}

// ─── Deprecated function ────────────────────────────────────────────────────

/// A standalone function marked `@deprecated` in its PHPDoc should carry
/// the `DEPRECATED` tag in its completion item.
#[tokio::test]
async fn test_deprecated_function_is_marked() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_function.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @deprecated Use newHelper() instead.\n",
        " */\n",
        "function oldHelper(): void {}\n",
        "\n",
        "function newHelper(): void {}\n",
        "\n",
        "Helper\n",
    );

    // Cursor at end of `Helper` on line 8 — matches both oldHelper and newHelper
    let items = complete_at(&backend, &uri, text, 8, 6).await;

    let old = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("oldHelper"));
    assert!(
        old.is_some(),
        "Should find oldHelper in completions. Got: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    assert!(
        is_tagged_deprecated(old.unwrap()),
        "oldHelper should be tagged deprecated"
    );

    let new = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("newHelper"));
    assert!(
        new.is_some(),
        "Should find newHelper in completions. Got: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    assert!(
        !is_tagged_deprecated(new.unwrap()),
        "newHelper should NOT be tagged deprecated"
    );
}

// ─── Deprecated method with bare @deprecated ────────────────────────────────

/// A method with a bare `@deprecated` tag (no description) should still
/// be tagged deprecated.
#[tokio::test]
async fn test_deprecated_method_bare_tag() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_bare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @deprecated\n",
        "     */\n",
        "    public function oldMethod(): void {}\n",
        "\n",
        "    public function test(): void {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 15).await;

    let old = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("oldMethod"));
    assert!(old.is_some(), "Should find oldMethod in completions");
    assert!(
        is_tagged_deprecated(old.unwrap()),
        "oldMethod should be tagged deprecated even with bare @deprecated tag"
    );
}

// ─── Multiple deprecated members ────────────────────────────────────────────

/// When a class has multiple deprecated and non-deprecated members,
/// only the deprecated ones should be tagged.
#[tokio::test]
async fn test_multiple_deprecated_and_non_deprecated_members() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multi_deprecated.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Api {\n",
        "    /**\n",
        "     * @deprecated Use fetchV2() instead.\n",
        "     */\n",
        "    public function fetchV1(): array { return []; }\n",
        "\n",
        "    public function fetchV2(): array { return []; }\n",
        "\n",
        "    /**\n",
        "     * @deprecated\n",
        "     */\n",
        "    public string $legacyUrl = '';\n",
        "\n",
        "    public string $baseUrl = '';\n",
        "\n",
        "    /**\n",
        "     * @deprecated Use VERSION_2 instead.\n",
        "     */\n",
        "    const VERSION_1 = 1;\n",
        "\n",
        "    const VERSION_2 = 2;\n",
        "}\n",
        "class Client {\n",
        "    public function run(Api $api): void {\n",
        "        $api->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 25, 14).await;

    // Deprecated items
    let fetch_v1 = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("fetchV1"));
    assert!(fetch_v1.is_some(), "Should find fetchV1");
    assert!(
        is_tagged_deprecated(fetch_v1.unwrap()),
        "fetchV1 should be tagged deprecated"
    );

    let legacy_url = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("legacyUrl"));
    assert!(legacy_url.is_some(), "Should find legacyUrl");
    assert!(
        is_tagged_deprecated(legacy_url.unwrap()),
        "legacyUrl should be tagged deprecated"
    );

    // Non-deprecated items
    let fetch_v2 = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("fetchV2"));
    assert!(fetch_v2.is_some(), "Should find fetchV2");
    assert!(
        !is_tagged_deprecated(fetch_v2.unwrap()),
        "fetchV2 should NOT be tagged deprecated"
    );

    let base_url = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("baseUrl"));
    assert!(base_url.is_some(), "Should find baseUrl");
    assert!(
        !is_tagged_deprecated(base_url.unwrap()),
        "baseUrl should NOT be tagged deprecated"
    );
}

/// Constant completion via `::` should also respect deprecated tags.
#[tokio::test]
async fn test_deprecated_constant_via_double_colon() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_const_dc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Api {\n",
        "    /**\n",
        "     * @deprecated Use VERSION_2 instead.\n",
        "     */\n",
        "    const VERSION_1 = 1;\n",
        "\n",
        "    const VERSION_2 = 2;\n",
        "}\n",
        "$v = Api::\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 10).await;

    let v1 = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("VERSION_1"));
    assert!(v1.is_some(), "Should find VERSION_1");
    assert!(
        is_tagged_deprecated(v1.unwrap()),
        "VERSION_1 should be tagged deprecated"
    );

    let v2 = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("VERSION_2"));
    assert!(v2.is_some(), "Should find VERSION_2");
    assert!(
        !is_tagged_deprecated(v2.unwrap()),
        "VERSION_2 should NOT be tagged deprecated"
    );
}

// ─── Deprecated with other docblock tags ─────────────────────────────────────

/// `@deprecated` mixed with `@param`, `@return`, etc. should still be detected.
#[tokio::test]
async fn test_deprecated_mixed_with_other_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_mixed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Processor {\n",
        "    /**\n",
        "     * Process the given data.\n",
        "     *\n",
        "     * @param array $data The data to process.\n",
        "     * @deprecated since 3.0, use processV2() instead.\n",
        "     * @return bool\n",
        "     */\n",
        "    public function process(array $data): bool { return true; }\n",
        "\n",
        "    public function test(): void {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 12, 15).await;

    let process = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("process"));
    assert!(process.is_some(), "Should find process in completions");
    assert!(
        is_tagged_deprecated(process.unwrap()),
        "process should be tagged deprecated even with mixed tags"
    );
}

// ─── Static deprecated method ───────────────────────────────────────────────

/// Deprecated static methods should be tagged when accessed via `::`.
#[tokio::test]
async fn test_deprecated_static_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    /**\n",
        "     * @deprecated Use createNew() instead.\n",
        "     */\n",
        "    public static function createLegacy(): self { return new self(); }\n",
        "\n",
        "    public static function createNew(): self { return new self(); }\n",
        "}\n",
        "Factory::\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 10).await;

    let legacy = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("createLegacy"));
    assert!(legacy.is_some(), "Should find createLegacy in completions");
    assert!(
        is_tagged_deprecated(legacy.unwrap()),
        "createLegacy should be tagged deprecated"
    );

    let new_method = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("createNew"));
    assert!(new_method.is_some(), "Should find createNew in completions");
    assert!(
        !is_tagged_deprecated(new_method.unwrap()),
        "createNew should NOT be tagged deprecated"
    );
}

// ─── Deprecated static property ─────────────────────────────────────────────

/// Deprecated static properties should be tagged when accessed via `::`.
#[tokio::test]
async fn test_deprecated_static_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_static_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Settings {\n",
        "    /**\n",
        "     * @deprecated Use $newDefault instead.\n",
        "     */\n",
        "    public static string $oldDefault = 'legacy';\n",
        "\n",
        "    public static string $newDefault = 'modern';\n",
        "}\n",
        "Settings::\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 11).await;

    let old = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("$oldDefault"));
    assert!(
        old.is_some(),
        "Should find $oldDefault in completions. Got: {:?}",
        items
            .iter()
            .map(|i| (&i.label, &i.filter_text))
            .collect::<Vec<_>>()
    );
    assert!(
        is_tagged_deprecated(old.unwrap()),
        "$oldDefault should be tagged deprecated"
    );

    let new = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("$newDefault"));
    assert!(new.is_some(), "Should find $newDefault in completions");
    assert!(
        !is_tagged_deprecated(new.unwrap()),
        "$newDefault should NOT be tagged deprecated"
    );
}

// ─── No false positive on similar words ─────────────────────────────────────

/// A docblock mentioning "deprecated" in prose (not as a tag) should NOT
/// cause the member to be flagged.
#[tokio::test]
async fn test_deprecated_word_in_prose_not_flagged() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///deprecated_prose.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Docs {\n",
        "    /**\n",
        "     * This method replaces the deprecated v1 API.\n",
        "     */\n",
        "    public function replaceOld(): void {}\n",
        "\n",
        "    public function test(): void {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 15).await;

    let method = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("replaceOld"));
    assert!(method.is_some(), "Should find replaceOld in completions");
    assert!(
        !is_tagged_deprecated(method.unwrap()),
        "replaceOld should NOT be tagged deprecated — 'deprecated' in prose is not a tag"
    );
}
