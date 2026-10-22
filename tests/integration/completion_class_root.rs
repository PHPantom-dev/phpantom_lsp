//! Completion at the class-body root (issue #277): typing a bare
//! identifier directly inside a class body offers overridable parent /
//! interface / trait members with their full declarations, plus member
//! keywords — and suppresses classes, functions, and constants, which
//! are invalid at that position.

use crate::common::{completion_response, create_test_backend, open_php};
use tower_lsp::lsp_types::*;

async fn completion_items(text: &str, uri: &str, line: u32, character: u32) -> Vec<CompletionItem> {
    let backend = create_test_backend();
    let uri = Url::parse(uri).unwrap();

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, line, character, None).await;

    match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

fn insert_text_of(item: &CompletionItem) -> &str {
    item.insert_text
        .as_deref()
        .or_else(|| {
            item.text_edit.as_ref().map(|te| match te {
                CompletionTextEdit::Edit(e) => e.new_text.as_str(),
                CompletionTextEdit::InsertAndReplace(e) => e.new_text.as_str(),
            })
        })
        .unwrap_or("")
}

#[tokio::test]
async fn class_root_offers_parent_members_with_full_declarations() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public string $title = 'untitled';\n",
        "    protected ?string $subtitle;\n",
        "    private string $secret;\n",
        "    public const STATUS_OK = 1;\n",
        "    public function getContent(): string { return ''; }\n",
        "    protected static function getCache(): array { return []; }\n",
        "    private function hidden(): void {}\n",
        "}\n",
        "class Post extends Article {\n",
        "    ge\n",
        "}\n",
    );
    let items = completion_items(text, "file:///class_root.php", 11, 6).await;

    let content = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("getContent"))
        .expect("getContent should be offered");
    let insert = insert_text_of(content);
    assert!(
        insert.starts_with("public function getContent(): string"),
        "method insert must carry the full declaration, got: {insert}"
    );

    let cache = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("getCache"))
        .expect("getCache should be offered");
    let insert = insert_text_of(cache);
    assert!(
        insert.starts_with("protected static function getCache(): array"),
        "static method insert must keep visibility and static, got: {insert}"
    );

    assert!(
        !items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("hidden")),
        "private parent methods must not be offered"
    );
}

#[tokio::test]
async fn class_root_offers_properties_and_constants_with_declarations() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public string $title = 'untitled';\n",
        "    protected ?string $subtitle;\n",
        "    public const STATUS_OK = 1;\n",
        "    protected const LIMIT = 10;\n",
        "}\n",
        "class Post extends Article {\n",
        "    \n",
        "}\n",
    );
    let items = completion_items(text, "file:///class_root_props.php", 8, 4).await;

    let title = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("title"))
        .expect("parent property $title should be offered");
    assert_eq!(
        insert_text_of(title),
        "public string $title = 'untitled';",
        "property insert must carry the full declaration"
    );

    let subtitle = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("subtitle"))
        .expect("parent property $subtitle should be offered");
    assert_eq!(
        insert_text_of(subtitle),
        "protected ?string $subtitle;",
        "property insert must keep visibility and nullable type"
    );

    let status = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("STATUS_OK"))
        .expect("parent constant STATUS_OK should be offered");
    assert_eq!(
        insert_text_of(status),
        "public const STATUS_OK = 1;",
        "constant insert must carry the full declaration"
    );

    let limit = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("LIMIT"))
        .expect("parent constant LIMIT should be offered");
    assert_eq!(
        insert_text_of(limit),
        "protected const LIMIT = 10;",
        "constant insert must keep protected visibility"
    );
}

#[tokio::test]
async fn class_root_suppresses_classes_functions_and_constants() {
    let text = concat!(
        "<?php\n",
        "class ArticleHelper {}\n",
        "function article_format(): string { return ''; }\n",
        "class Post {\n",
        "    ar\n",
        "}\n",
    );
    let items = completion_items(text, "file:///class_root_suppress.php", 4, 6).await;

    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::CLASS)),
        "class names are invalid at the class-body root, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FUNCTION)),
        "function names are invalid at the class-body root, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn class_root_offers_member_keywords() {
    let text = concat!("<?php\n", "class Post {\n", "    pu\n", "}\n",);
    let items = completion_items(text, "file:///class_root_keywords.php", 2, 6).await;

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"public"),
        "member keywords should be offered at the class-body root, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"print"),
        "statement keywords must not leak into the class body, got: {labels:?}"
    );
}

#[tokio::test]
async fn class_root_dollar_offers_only_properties() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public string $title = 'untitled';\n",
        "    public function getContent(): string { return ''; }\n",
        "    public const STATUS_OK = 1;\n",
        "}\n",
        "class Post extends Article {\n",
        "    $ti\n",
        "}\n",
    );
    let items = completion_items(text, "file:///class_root_dollar.php", 7, 7).await;

    let title = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("$title"))
        .expect("parent property $title should be offered after `$`");
    assert_eq!(
        insert_text_of(title),
        "public string $title = 'untitled';",
        "property insert must carry the full declaration"
    );
    // The replace range must cover the typed `$` so the declaration's own
    // `$` doesn't double up.
    match title.text_edit.as_ref().expect("text edit") {
        CompletionTextEdit::Edit(e) => {
            assert_eq!(e.range.start.character, 4, "range must start at the `$`");
        }
        CompletionTextEdit::InsertAndReplace(_) => panic!("expected plain edit"),
    }
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::METHOD)),
        "methods must not be offered after `$`"
    );
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::CONSTANT)),
        "constants must not be offered after `$`"
    );
}

#[tokio::test]
async fn class_root_offers_trait_members() {
    let text = concat!(
        "<?php\n",
        "trait Timestamps {\n",
        "    protected ?int $updatedAt = null;\n",
        "    public function touch(): void {}\n",
        "}\n",
        "class Post {\n",
        "    use Timestamps;\n",
        "    to\n",
        "}\n",
    );
    let items = completion_items(text, "file:///class_root_trait.php", 7, 6).await;

    let touch = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("touch"))
        .expect("trait method touch should be offered");
    let insert = insert_text_of(touch);
    assert!(
        insert.starts_with("public function touch(): void"),
        "trait method insert must carry the full declaration, got: {insert}"
    );
    assert!(
        touch.additional_text_edits.is_none(),
        "trait methods must not insert #[\\Override]"
    );
}

#[tokio::test]
async fn class_root_offers_interface_methods() {
    let text = concat!(
        "<?php\n",
        "interface Formatter {\n",
        "    public function format(string $value): string;\n",
        "}\n",
        "class JsonFormatter implements Formatter {\n",
        "    fo\n",
        "}\n",
    );
    let items = completion_items(text, "file:///class_root_iface.php", 5, 6).await;

    let format = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("format"))
        .expect("interface method format should be offered");
    let insert = insert_text_of(format);
    assert!(
        insert.starts_with("public function format(string \\$value): string"),
        "interface method insert must carry the full declaration, got: {insert}"
    );
}

#[tokio::test]
async fn method_body_statement_start_is_not_class_root() {
    // A statement start inside a method body also follows `{`/`;`, but
    // must keep normal statement completion (functions, classes, ...).
    let text = concat!(
        "<?php\n",
        "function article_format(): string { return ''; }\n",
        "class Post extends Article {\n",
        "    public function render(): string {\n",
        "        arti\n",
        "    }\n",
        "}\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "}\n",
    );
    let items = completion_items(text, "file:///method_body.php", 4, 12).await;

    assert!(
        items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FUNCTION)),
        "function completion must still work inside method bodies, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
    assert!(
        !items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getContent")),
        "override suggestions must not appear inside method bodies"
    );
}

#[tokio::test]
async fn top_level_code_is_not_class_root() {
    let text = concat!("<?php\n", "class ArticleHelper {}\n", "arti\n",);
    let items = completion_items(text, "file:///top_level.php", 2, 4).await;

    assert!(
        items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::CLASS)),
        "top-level completion must still offer classes, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn class_root_members_already_declared_are_omitted() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "    public function getTitle(): string { return ''; }\n",
        "}\n",
        "class Post extends Article {\n",
        "    public function getContent(): string { return 'x'; }\n",
        "    ge\n",
        "}\n",
    );
    let items = completion_items(text, "file:///class_root_declared.php", 7, 6).await;

    assert!(
        items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getTitle")),
        "getTitle is still overridable"
    );
    assert!(
        !items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getContent")),
        "members the class already declares must not be offered again"
    );
}

#[tokio::test]
async fn enum_root_offers_no_properties() {
    let text = concat!(
        "<?php\n",
        "interface HasLabel {\n",
        "    public function label(): string;\n",
        "}\n",
        "enum Status: string implements HasLabel {\n",
        "    case Active = 'active';\n",
        "    la\n",
        "}\n",
    );
    let items = completion_items(text, "file:///enum_root.php", 6, 6).await;

    let label = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("label"))
        .expect("interface method label should be offered in enum body");
    let insert = insert_text_of(label);
    assert!(
        insert.starts_with("public function label(): string"),
        "enum interface method insert must carry the declaration, got: {insert}"
    );
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::PROPERTY)),
        "enums cannot declare properties"
    );
}

/// Trivia before the cursor must not hide the member position: a
/// backwards scan for the previous significant byte lands on `t` of a
/// comment, `/` of a docblock, or `]` of an attribute.
#[tokio::test]
async fn class_root_fires_after_comments_docblocks_and_attributes() {
    for (name, trivia) in [
        ("line comment", "    // a note with no trailing semicolon\n"),
        ("hash comment", "    # a note\n"),
        ("docblock", "    /** Handles content. */\n"),
        ("block comment", "    /* closes with } and ; */\n"),
        ("attribute", "    #[Deprecated]\n"),
        ("attribute with args", "    #[Route('/a]b', name: 'x')]\n"),
        (
            "docblock then attribute",
            "    /** @var int */\n    #[Deprecated]\n",
        ),
    ] {
        let text = format!(
            concat!(
                "<?php\n",
                "class Article {{\n",
                "    public function getContent(): string {{ return ''; }}\n",
                "}}\n",
                "class Post extends Article {{\n",
                "{trivia}",
                "    ge\n",
                "}}\n",
            ),
            trivia = trivia
        );
        let line = 5 + trivia.matches('\n').count() as u32;
        let items = completion_items(&text, "file:///trivia.php", line, 6).await;
        assert!(
            items
                .iter()
                .any(|i| i.filter_text.as_deref() == Some("getContent")),
            "{name}: overrides should still be offered, got: {:?}",
            items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
        );
        assert!(
            !items
                .iter()
                .any(|i| i.kind == Some(CompletionItemKind::CLASS)),
            "{name}: class names must stay suppressed, got: {:?}",
            items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
        );
    }
}

/// A `}` or `;` inside a comment or string used to unbalance the
/// backwards brace scan and lose the class body entirely.
#[tokio::test]
async fn class_root_fires_after_braces_inside_comments_and_strings() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "}\n",
        "class Post extends Article {\n",
        "    // this comment closes with }\n",
        "    public string $marker = 'a } b ; c';\n",
        "    ge\n",
        "}\n",
    );
    let items = completion_items(text, "file:///braces_in_trivia.php", 7, 6).await;
    assert!(
        items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getContent")),
        "got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn inside_property_default_is_not_class_root() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "}\n",
        "class Post extends Article {\n",
        "    public string $marker = 'a; ge\n",
        "}\n",
    );
    let items = completion_items(text, "file:///in_string.php", 5, 33).await;
    assert!(
        !items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getContent")),
        "a `;` inside a string literal is not a member boundary, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn inside_heredoc_default_is_not_class_root() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "}\n",
        "class Post extends Article {\n",
        "    public string $doc = <<<TXT\n",
        "        a; b\n",
        "        ge\n",
        "        TXT;\n",
        "}\n",
    );
    let items = completion_items(text, "file:///in_heredoc.php", 7, 10).await;
    assert!(
        !items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getContent")),
        "heredoc body is not a member position, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn class_root_fires_after_a_heredoc_property() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "}\n",
        "class Post extends Article {\n",
        "    public string $doc = <<<TXT\n",
        "        a; b\n",
        "        TXT;\n",
        "    ge\n",
        "}\n",
    );
    let items = completion_items(text, "file:///after_heredoc.php", 8, 6).await;
    assert!(
        items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getContent")),
        "got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn inside_property_hook_body_is_not_class_root() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "}\n",
        "class Post extends Article {\n",
        "    public string $name {\n",
        "        ge\n",
        "    }\n",
        "}\n",
    );
    let items = completion_items(text, "file:///hook.php", 6, 10).await;
    assert!(
        !items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("getContent")),
        "a property hook body is not the class root, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn inside_trait_use_adaptation_block_is_not_class_root() {
    let text = concat!(
        "<?php\n",
        "trait A { public function getContent(): string { return 'a'; } }\n",
        "trait B { public function getContent(): string { return 'b'; } }\n",
        "class Post {\n",
        "    use A, B {\n",
        "        ge\n",
        "    }\n",
        "}\n",
    );
    let items = completion_items(text, "file:///adaptation.php", 5, 10).await;
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::METHOD)),
        "a trait-use adaptation block is not the class root, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn anonymous_class_body_inside_a_method_is_a_class_root() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public function getContent(): string { return ''; }\n",
        "}\n",
        "class Factory {\n",
        "    public function make(): object {\n",
        "        return new class extends Article {\n",
        "            ge\n",
        "        };\n",
        "    }\n",
        "}\n",
    );
    let items = completion_items(text, "file:///anon.php", 7, 14).await;
    let content = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("getContent"))
        .unwrap_or_else(|| {
            panic!(
                "anonymous class body is a class root, got: {:?}",
                items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
            )
        });
    assert!(
        insert_text_of(content).starts_with("public function getContent(): string"),
        "got: {}",
        insert_text_of(content)
    );
}

#[tokio::test]
async fn class_root_keeps_native_constant_type() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public const string ORIGIN = 'article';\n",
        "    public const LIMIT = 10;\n",
        "}\n",
        "class Post extends Article {\n",
        "    \n",
        "}\n",
    );
    let items = completion_items(text, "file:///const_type.php", 6, 4).await;

    let origin = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("ORIGIN"))
        .expect("ORIGIN should be offered");
    assert_eq!(
        insert_text_of(origin),
        "public const string ORIGIN = 'article';",
        "a typed constant must keep its native type"
    );

    let limit = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("LIMIT"))
        .expect("LIMIT should be offered");
    assert_eq!(
        insert_text_of(limit),
        "public const LIMIT = 10;",
        "an untyped constant must not gain an inferred type"
    );
}

#[tokio::test]
async fn class_root_offers_interface_and_trait_constants() {
    let text = concat!(
        "<?php\n",
        "interface HasOrigin {\n",
        "    const ORIGIN = 'iface';\n",
        "}\n",
        "trait Limited {\n",
        "    const LIMIT = 10;\n",
        "}\n",
        "class Post implements HasOrigin {\n",
        "    use Limited;\n",
        "    \n",
        "}\n",
    );
    let items = completion_items(text, "file:///iface_const.php", 9, 4).await;
    let labels = || items.iter().map(|i| i.label.clone()).collect::<Vec<_>>();

    let origin = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("ORIGIN"))
        .unwrap_or_else(|| panic!("interface constant should be offered, got: {:?}", labels()));
    assert_eq!(insert_text_of(origin), "public const ORIGIN = 'iface';");

    let limit = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("LIMIT"))
        .unwrap_or_else(|| panic!("trait constant should be offered, got: {:?}", labels()));
    assert_eq!(insert_text_of(limit), "public const LIMIT = 10;");
}

/// The demo file's own "Try:" instruction must actually work.  Its
/// trigger line sits under the explanatory comments inside the class
/// body, which is exactly the shape a backwards scan for the previous
/// significant byte gets wrong.
#[tokio::test]
async fn demo_php_class_root_trigger_works() {
    // ClassRootCompletionDemo extends/uses scaffolding classes, so the
    // scaffolding file must be opened in the same backend for its inherited
    // members to resolve.
    let scaffolding = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/php/scaffolding/scaffolding.php"
    ))
    .expect("examples/php/scaffolding/scaffolding.php");
    let demo = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/php/completion.php"
    ))
    .expect("examples/php/completion.php");
    let anchor = demo
        .find("class ClassRootCompletionDemo")
        .expect("demo class present");
    // Insert the trigger on the last line of the class body, after every
    // "Try:" comment, without depending on their wording.
    let close = anchor
        + demo[anchor..]
            .find("\n}\n")
            .expect("demo class closing brace")
        + 1;
    let line = demo[..close].matches('\n').count() as u32;

    let mut text = demo.clone();
    text.insert_str(close, "    o\n");

    let backend = create_test_backend();
    open_php(
        &backend,
        &Url::parse("file:///scaffolding.php").unwrap(),
        &scaffolding,
    )
    .await;
    let demo_uri = Url::parse("file:///demo_trigger.php").unwrap();
    open_php(&backend, &demo_uri, &text).await;
    let completion_at = |line: u32, character: u32| {
        let backend = &backend;
        let uri = demo_uri.clone();
        async move {
            let result = completion_response(backend, &uri, line, character, None).await;
            match result {
                Some(CompletionResponse::Array(items)) => items,
                Some(CompletionResponse::List(list)) => list.items,
                None => Vec::new(),
            }
        }
    };

    let items = completion_at(line, 5).await;
    let labels = || items.iter().map(|i| i.label.clone()).collect::<Vec<_>>();

    assert!(
        items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("onChange")),
        "demo promises onChange(), got: {:?}",
        labels()
    );
    assert!(
        items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("oneTimeToken")),
        "demo promises $oneTimeToken, got: {:?}",
        labels()
    );
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::CLASS)),
        "demo promises no class names, got: {:?}",
        labels()
    );

    // The second "Try:" line promises the inherited typed constant.
    let items = completion_at(line, 5).await;
    let ttl = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("ONE_TIME_TTL"))
        .unwrap_or_else(|| panic!("demo promises ONE_TIME_TTL, got: {:?}", labels()));
    assert_eq!(
        insert_text_of(ttl),
        "public const string ONE_TIME_TTL = '15m';"
    );
}

/// A docblock or comment that merely mentions `const` or `function` above
/// the cursor must not hand the position to the modifier-anchored
/// override path, which scans backwards without skipping comments.
#[tokio::test]
async fn comment_mentioning_const_does_not_claim_the_class_root() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public const string ORIGIN = 'article';\n",
        "    public function onChange(): void {}\n",
        "}\n",
        "class Post extends Article {\n",
        "    // offered as `public const string ORIGIN = 'article';`\n",
        "    o\n",
        "}\n",
    );
    let items = completion_items(text, "file:///comment_const.php", 7, 5).await;
    let labels = || items.iter().map(|i| i.label.clone()).collect::<Vec<_>>();

    let change = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("onChange"))
        .unwrap_or_else(|| panic!("methods should still be offered, got: {:?}", labels()));
    assert!(
        insert_text_of(change).starts_with("public function onChange(): void"),
        "got: {}",
        insert_text_of(change)
    );
    let origin = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("ORIGIN"))
        .unwrap_or_else(|| panic!("constants should still be offered, got: {:?}", labels()));
    assert_eq!(
        insert_text_of(origin),
        "public const string ORIGIN = 'article';",
        "the class-root path must own this position, not the `const` path"
    );
}

/// The modifier-anchored path still owns positions where a modifier has
/// actually been typed, so those keep inserting a bare name.
#[tokio::test]
async fn typed_modifier_still_uses_the_name_only_override_path() {
    let text = concat!(
        "<?php\n",
        "class Article {\n",
        "    public const LIMIT = 10;\n",
        "    public function getContent(): string { return ''; }\n",
        "    public string $title = 'untitled';\n",
        "}\n",
        "class Post extends Article {\n",
        "    public function get\n",
        "    const LI\n",
        "    protected $tit\n",
        "}\n",
    );

    let items = completion_items(text, "file:///typed_modifier.php", 7, 23).await;
    let content = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("getContent"))
        .expect("getContent after `function`");
    assert!(
        insert_text_of(content).starts_with("getContent("),
        "a typed `function` keyword must not be duplicated, got: {}",
        insert_text_of(content)
    );

    let items = completion_items(text, "file:///typed_modifier.php", 8, 12).await;
    let limit = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("LIMIT"))
        .expect("LIMIT after `const`");
    assert_eq!(
        insert_text_of(limit),
        "LIMIT = 10",
        "a typed `const` keyword must not be duplicated"
    );

    let items = completion_items(text, "file:///typed_modifier.php", 9, 18).await;
    let title = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("title"))
        .expect("title after `protected $`");
    assert_eq!(
        insert_text_of(title),
        "title = 'untitled'",
        "a typed visibility modifier must not be duplicated"
    );
}

#[tokio::test]
async fn trait_name_completion_after_use_keyword_still_works() {
    let text = concat!(
        "<?php\n",
        "trait Greets { public function greet(): void {} }\n",
        "class Post {\n",
        "    use Gre\n",
        "}\n",
    );
    let items = completion_items(text, "file:///probe_use.php", 3, 11).await;
    assert!(
        items.iter().any(|i| i.label == "Greets"),
        "the class-root path must not claim `use Tra|`, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn enum_case_name_position_offers_no_classes() {
    let text = concat!(
        "<?php\n",
        "enum Suit: string {\n",
        "    case Hearts = 'H';\n",
        "    case Spa\n",
        "}\n",
    );
    let items = completion_items(text, "file:///probe_case.php", 3, 12).await;
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::CLASS)),
        "an enum case name is not a class-name position, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn attribute_name_completion_in_class_body_still_works() {
    let text = concat!(
        "<?php\n",
        "#[\\Attribute]\n",
        "class MyRoute {}\n",
        "class Post {\n",
        "    #[MyRo\n",
        "    public function go(): void {}\n",
        "}\n",
    );
    let items = completion_items(text, "file:///probe_attr_name.php", 4, 10).await;
    assert!(
        items.iter().any(|i| i.label == "MyRoute"),
        "the class-root path must not claim `#[MyRo|`, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn type_hint_completion_after_visibility_still_works() {
    let text = concat!(
        "<?php\n",
        "class Article {}\n",
        "class Post {\n",
        "    public Arti\n",
        "}\n",
    );
    let items = completion_items(text, "file:///probe_typehint.php", 3, 15).await;
    assert!(
        items.iter().any(|i| i.label == "Article"),
        "the class-root path must not claim `public Arti|`, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

/// PHP rejects redeclaring a `final` inherited method outright, so it must
/// not be offered as an override candidate on either entry point.  A
/// `final` method reached through a trait the editing class uses directly
/// is a different story: the class's own declaration simply wins, so it
/// stays on offer.
#[tokio::test]
async fn final_parent_methods_are_not_offered_as_overrides() {
    let text = concat!(
        "<?php\n",
        "trait Lockable {\n",
        "    final public function onTrait(): void {}\n",
        "}\n",
        "class Base {\n",
        "    final public function onLock(): void {}\n",
        "    public function onOpen(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    use Lockable;\n",
        "    on\n",
        "    public function on\n",
        "}\n",
    );

    for (line, character, what) in [(10, 6, "class-body root"), (11, 22, "after `function`")] {
        let items = completion_items(text, "file:///final_override.php", line, character).await;
        let names = || {
            items
                .iter()
                .filter_map(|i| i.filter_text.clone())
                .collect::<Vec<_>>()
        };
        assert!(
            !items
                .iter()
                .any(|i| i.filter_text.as_deref() == Some("onLock")),
            "{what}: a final parent method cannot be overridden, got: {:?}",
            names()
        );
        assert!(
            items
                .iter()
                .any(|i| i.filter_text.as_deref() == Some("onOpen")),
            "{what}: non-final parent methods must still be offered, got: {:?}",
            names()
        );
        assert!(
            items
                .iter()
                .any(|i| i.filter_text.as_deref() == Some("onTrait")),
            "{what}: a directly-used trait's final method is redeclarable, got: {:?}",
            names()
        );
    }
}

/// The same `final` trait method is *not* redeclarable once it arrives
/// through a parent that used the trait: PHP reports "Cannot override
/// final method Mid::onTrait()".
#[tokio::test]
async fn final_methods_from_a_parents_trait_are_not_offered() {
    let text = concat!(
        "<?php\n",
        "trait Lockable {\n",
        "    final public function onTrait(): void {}\n",
        "    public function onPlain(): void {}\n",
        "}\n",
        "class Mid {\n",
        "    use Lockable;\n",
        "}\n",
        "class Child extends Mid {\n",
        "    on\n",
        "}\n",
    );
    let items = completion_items(text, "file:///final_parent_trait.php", 9, 6).await;
    let names = || {
        items
            .iter()
            .filter_map(|i| i.filter_text.clone())
            .collect::<Vec<_>>()
    };
    assert!(
        !items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("onTrait")),
        "a final method inherited via the parent's trait is not overridable, got: {:?}",
        names()
    );
    assert!(
        items
            .iter()
            .any(|i| i.filter_text.as_deref() == Some("onPlain")),
        "non-final trait methods must still be offered, got: {:?}",
        names()
    );
}

/// Member-access completion is unaffected: a `final` method is perfectly
/// callable, it just cannot be redeclared.
#[tokio::test]
async fn final_methods_still_appear_in_member_access_completion() {
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    final public function onLock(): void {}\n",
        "}\n",
        "class Child extends Base {}\n",
        "$c = new Child();\n",
        "$c->onL\n",
    );
    let items = completion_items(text, "file:///final_member_access.php", 6, 7).await;
    assert!(
        items.iter().any(|i| i.label.starts_with("onLock")),
        "final methods are still callable, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

/// Redeclaring an inherited `readonly` property as non-readonly is a fatal
/// error ("Cannot redeclare readonly property"), so the generated
/// declaration has to keep the modifier.  Redeclaring it *as* readonly is
/// legal, so the property stays on offer.
#[tokio::test]
async fn class_root_keeps_readonly_on_a_redeclared_property() {
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public readonly string $onName;\n",
        "    protected readonly int $onCount;\n",
        "    public string $onPlain = 'x';\n",
        "}\n",
        "class Child extends Base {\n",
        "    \n",
        "}\n",
    );
    let items = completion_items(text, "file:///readonly_override.php", 7, 4).await;

    let name = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("onName"))
        .expect("parent readonly property $onName should be offered");
    assert_eq!(
        insert_text_of(name),
        "public readonly string $onName;",
        "a readonly property must be redeclared as readonly"
    );

    let count = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("onCount"))
        .expect("parent readonly property $onCount should be offered");
    assert_eq!(insert_text_of(count), "protected readonly int $onCount;");

    let plain = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("onPlain"))
        .expect("parent property $onPlain should be offered");
    assert_eq!(
        insert_text_of(plain),
        "public string $onPlain = 'x';",
        "a non-readonly property must not gain the modifier"
    );
}

/// A promoted constructor parameter is a real property, so its `readonly`
/// modifier has to survive the same round trip.
#[tokio::test]
async fn class_root_keeps_readonly_on_a_promoted_property() {
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function __construct(\n",
        "        public readonly string $onLabel,\n",
        "        protected int $onSize,\n",
        "    ) {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    \n",
        "}\n",
    );
    let items = completion_items(text, "file:///readonly_promoted.php", 8, 4).await;

    let label = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("onLabel"))
        .expect("promoted readonly property $onLabel should be offered");
    assert_eq!(insert_text_of(label), "public readonly string $onLabel;");

    let size = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("onSize"))
        .expect("promoted property $onSize should be offered");
    assert_eq!(insert_text_of(size), "protected int $onSize;");
}
