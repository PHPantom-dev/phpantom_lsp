use crate::common::{block_on, complete_labels_at, create_psr4_workspace, create_test_backend};
use tower_lsp::lsp_types::*;

// ─── Multi-line method chain completion tests ───────────────────────────────
//
// These tests verify that fluent method chains spanning multiple lines produce
// completions. The cursor is on a continuation line (one that starts with
// `->` or `?->` after optional whitespace), and the resolver must join the
// preceding lines to reconstruct the full chain expression.

// ─── Basic multi-line chain ─────────────────────────────────────────────────

/// A two-line chain: `$this->getRepo()\n    ->` should resolve the return
/// type of `getRepo()` and offer its members.
#[tokio::test]
async fn test_multiline_chain_basic() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Repo {\n",
        "    public function findAll(): array { return []; }\n",
        "}\n",
        "class Service {\n",
        "    public function getRepo(): Repo { return new Repo(); }\n",
        "    public function run(): void {\n",
        "        $this->getRepo()\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 8 (`            ->`) right after `->`
    let names = complete_labels_at(&backend, &uri, text, 8, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("findAll(")),
        "Should offer Repo::findAll(), got: {names:?}"
    );
}

// ─── Three-line chain ───────────────────────────────────────────────────────

/// A chain spanning three continuation lines.
#[tokio::test]
async fn test_multiline_chain_three_lines() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_three.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where(): static { return $this; }\n",
        "    public function orderBy(): static { return $this; }\n",
        "    public function limit(): static { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "}\n",
        "class QueryService {\n",
        "    public function query(): Builder { return new Builder(); }\n",
        "    public function run(): void {\n",
        "        $this->query()\n",
        "            ->where()\n",
        "            ->orderBy()\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 13 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 13, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("limit(")),
        "Should offer Builder::limit(), got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("get(")),
        "Should offer Builder::get(), got: {names:?}"
    );
}

// ─── Nullsafe continuation ──────────────────────────────────────────────────

/// A chain that uses `?->` on a continuation line.
#[tokio::test]
async fn test_multiline_chain_nullsafe_continuation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_nullsafe.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class City { public function getName(): string { return ''; } }\n",
        "class Address { public function getCity(): ?City { return null; } }\n",
        "class User { public function getAddress(): ?Address { return null; } }\n",
        "class App {\n",
        "    public function run(User $user): void {\n",
        "        $user->getAddress()\n",
        "            ?->getCity()\n",
        "            ?->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 8 (`            ?->`) after `?->`
    let names = complete_labels_at(&backend, &uri, text, 8, 15).await;
    assert!(
        names.iter().any(|n| n.starts_with("getName(")),
        "Should offer City::getName() through nullsafe chain, got: {names:?}"
    );
}

// ─── Static method call base ────────────────────────────────────────────────

/// A chain starting with a static method call on the base line.
#[tokio::test]
async fn test_multiline_chain_static_base() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class QBuilder {\n",
        "    public static function create(): static { return new static(); }\n",
        "    public function select(): static { return $this; }\n",
        "    public function execute(): array { return []; }\n",
        "}\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        QBuilder::create()\n",
        "            ->select()\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 10 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 10, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("execute(")),
        "Should offer QBuilder::execute(), got: {names:?}"
    );
}

// ─── Variable assignment chain ──────────────────────────────────────────────

/// A multi-line chain assigned to a variable: `$result = $this->foo()\n    ->bar()`.
/// Cursor is after `->` on the continuation, not at the end.
#[tokio::test]
async fn test_multiline_chain_with_variable_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Paginator { public function total(): int { return 0; } }\n",
        "class QueryResult {\n",
        "    public function paginate(): Paginator { return new Paginator(); }\n",
        "}\n",
        "class DataService {\n",
        "    public function getData(): QueryResult { return new QueryResult(); }\n",
        "    public function run(): void {\n",
        "        $result = $this->getData()\n",
        "            ->paginate()\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 10 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 10, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("total(")),
        "Should offer Paginator::total(), got: {names:?}"
    );
}

// ─── Chain with arguments ───────────────────────────────────────────────────

/// Continuation lines include method calls with arguments.
#[tokio::test]
async fn test_multiline_chain_with_arguments() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_args.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Fluent {\n",
        "    public function set(string $key, mixed $value): static { return $this; }\n",
        "    public function build(): string { return ''; }\n",
        "}\n",
        "class Maker {\n",
        "    public function create(): Fluent { return new Fluent(); }\n",
        "    public function run(): void {\n",
        "        $this->create()\n",
        "            ->set('name', 'foo')\n",
        "            ->set('value', 42)\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 11 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 11, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("build(")),
        "Should offer Fluent::build(), got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("set(")),
        "Should offer Fluent::set(), got: {names:?}"
    );
}

// ─── Single-line chain still works ──────────────────────────────────────────

/// Ensure single-line chains are not broken by the multi-line logic.
#[tokio::test]
async fn test_single_line_chain_still_works() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///single_line.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item { public function getName(): string { return ''; } }\n",
        "class Container { public function first(): Item { return new Item(); } }\n",
        "class App {\n",
        "    public function getContainer(): Container { return new Container(); }\n",
        "    public function run(): void {\n",
        "        $this->getContainer()->first()->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 6 after `->`
    let names = complete_labels_at(&backend, &uri, text, 6, 43).await;
    assert!(
        names.iter().any(|n| n.starts_with("getName(")),
        "Single-line chain should still offer Item::getName(), got: {names:?}"
    );
}

// ─── Cross-file multi-line chain ────────────────────────────────────────────

/// Multi-line chain where the return type comes from a different file
/// resolved via PSR-4.
#[tokio::test]
async fn test_multiline_chain_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Builder.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Builder {\n",
                    "    public function where(): static { return $this; }\n",
                    "    public function first(): ?Item { return null; }\n",
                    "}\n",
                ),
            ),
            (
                "src/Item.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Item {\n",
                    "    public function getId(): int { return 0; }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///test_multiline_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Builder;\n",
        "class Service {\n",
        "    public function getBuilder(): Builder { return new Builder(); }\n",
        "    public function run(): void {\n",
        "        $this->getBuilder()\n",
        "            ->where()\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    let names = complete_labels_at(&backend, &uri, text, 7, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("where(")),
        "Should offer Builder::where() through multi-line PSR-4 chain, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("first(")),
        "Should offer Builder::first() through multi-line PSR-4 chain, got: {names:?}"
    );
}

// ─── Partial identifier on continuation line ────────────────────────────────

/// When the user has started typing an identifier on the continuation line,
/// completion should still work (the partial text is filtered by the editor).
#[tokio::test]
async fn test_multiline_chain_partial_identifier() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_partial.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pipeline {\n",
        "    public function through(): static { return $this; }\n",
        "    public function then(): mixed { return null; }\n",
        "    public function thenReturn(): mixed { return null; }\n",
        "}\n",
        "class App {\n",
        "    public function pipe(): Pipeline { return new Pipeline(); }\n",
        "    public function run(): void {\n",
        "        $this->pipe()\n",
        "            ->through()\n",
        "            ->the\n",
        "    }\n",
        "}\n",
    );

    // Cursor after `the` on line 11
    let names = complete_labels_at(&backend, &uri, text, 11, 17).await;
    assert!(
        names.iter().any(|n| n.starts_with("then(")),
        "Should offer Pipeline::then() with partial identifier, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("thenReturn(")),
        "Should offer Pipeline::thenReturn() with partial identifier, got: {names:?}"
    );
}

// ─── $this on its own line ──────────────────────────────────────────────────

/// When `$this` is on the line above and the continuation starts with `->`.
#[tokio::test]
async fn test_multiline_chain_this_on_own_line() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return ''; }\n",
        "    public function show(): void {\n",
        "        $this\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 5 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 5, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("render(")),
        "Should offer Widget::render() when $this is on preceding line, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("show(")),
        "Should offer Widget::show() when $this is on preceding line, got: {names:?}"
    );
}

// ─── Chain with closure argument ────────────────────────────────────────────

/// Continuation lines that include closure arguments (contains `->` inside
/// the closure that should not confuse the extraction).
#[tokio::test]
async fn test_multiline_chain_with_blank_line() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_blank.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Repo {\n",
        "    public function findAll(): array { return []; }\n",
        "}\n",
        "class Service {\n",
        "    public function getRepo(): Repo { return new Repo(); }\n",
        "    public function run(): void {\n",
        "        $this->getRepo()\n",
        "\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 9 (`            ->`) right after `->`
    // A blank line separates the base expression from the continuation.
    let names = complete_labels_at(&backend, &uri, text, 9, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("findAll(")),
        "Should offer Repo::findAll() even with a blank line in the chain, got: {names:?}"
    );
}

/// A chain with multiple blank lines between segments should still resolve.
#[tokio::test]
async fn test_multiline_chain_with_multiple_blank_lines() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_multi_blank.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where(): static { return $this; }\n",
        "    public function orderBy(): static { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "}\n",
        "class Model {\n",
        "    public function query(): Builder { return new Builder(); }\n",
        "    public function run(): void {\n",
        "        $this->query()\n",
        "\n",
        "            ->where()\n",
        "\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 13 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 13, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("orderBy(")),
        "Should offer Builder::orderBy() with multiple blank lines in chain, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("get(")),
        "Should offer Builder::get() with multiple blank lines in chain, got: {names:?}"
    );
}

/// A whitespace-only line (spaces/tabs but no code) should be treated as blank.
#[tokio::test]
async fn test_multiline_chain_with_whitespace_only_line() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_ws.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Repo {\n",
        "    public function findAll(): array { return []; }\n",
        "}\n",
        "class Service {\n",
        "    public function getRepo(): Repo { return new Repo(); }\n",
        "    public function run(): void {\n",
        "        $this->getRepo()\n",
        "        \n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 9 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 9, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("findAll(")),
        "Should offer Repo::findAll() with a whitespace-only line in the chain, got: {names:?}"
    );
}

/// Chain with a closure argument AND a blank line should still resolve.
#[tokio::test]
async fn test_multiline_chain_with_closure_arg() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_closure.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Collection {\n",
        "    public function filter(callable $fn): static { return $this; }\n",
        "    public function map(callable $fn): static { return $this; }\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "class Processor {\n",
        "    public function items(): Collection { return new Collection(); }\n",
        "    public function run(): void {\n",
        "        $this->items()\n",
        "            ->filter(fn($x) => $x > 0)\n",
        "            ->map(fn($x) => $x * 2)\n",
        "            ->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on line 12 (`            ->`) after `->`
    let names = complete_labels_at(&backend, &uri, text, 12, 14).await;
    assert!(
        names.iter().any(|n| n.starts_with("count(")),
        "Should offer Collection::count() after chain with closure args, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("filter(")),
        "Should offer Collection::filter() after chain with closure args, got: {names:?}"
    );
}

#[tokio::test]
async fn test_multiline_chain_same_line_after_closure_arg_close() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_closure_same_line.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Collection {\n",
        "    public function map(callable $fn): static { return $this; }\n",
        "    public function all(): array { return []; }\n",
        "}\n",
        "class Processor {\n",
        "    public function items(): Collection { return new Collection(); }\n",
        "    public function run(): void {\n",
        "        $this->items()\n",
        "            ->map(function ($x) {\n",
        "                return $x;\n",
        "            })->a\n",
        "    }\n",
        "}\n",
    );

    let names = complete_labels_at(&backend, &uri, text, 11, 17).await;
    assert!(
        names.iter().any(|n| n.starts_with("all(")),
        "Should offer Collection::all() after same-line closure close, got: {names:?}"
    );
}

#[tokio::test]
async fn test_multiline_chain_same_line_after_bracket_close_does_not_misresolve() {
    // Bracket depth (`[`/`]`) is not tracked by the backward balance scan
    // used to reconstruct the base expression, so a chain operator that
    // follows a closing `]` on its own line must not be treated as a
    // same-line continuation: doing so would drop everything before the
    // bracket close (e.g. the `$this->items[` base) and hand the resolver
    // a malformed expression instead of the real receiver. Falling back to
    // "no completions" is correct here; resolving to the wrong receiver's
    // members would not be.
    let backend = create_test_backend();
    let uri = Url::parse("file:///multiline_bracket_same_line.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Box {\n",
        "    public function open(): string { return ''; }\n",
        "}\n",
        "class Container {\n",
        "    /** @var array<string, Box> */\n",
        "    public array $items = [];\n",
        "    public function run(): void {\n",
        "        $this->items[\n",
        "            'key'\n",
        "        ]->o\n",
        "    }\n",
        "}\n",
    );

    let names = complete_labels_at(&backend, &uri, text, 10, 12).await;
    assert!(
        !names.iter().any(|n| n.starts_with("open(")),
        "Should not resolve Box::open() from a malformed collapsed expression, got: {names:?}"
    );
}

#[test]
fn test_long_union_return_chain_completes_quickly() {
    // Each link of a fluent chain whose methods return a union
    // (`A|B`, both declaring the same method) used to double the
    // receiver set: every member of the union resolved the same
    // union again and the results were concatenated without dedup,
    // so a chain of n calls built 2^n receivers. Real-world Pest
    // test files chain 20+ `->and(...)->toBe(...)` expectations,
    // which hung the analyzer and exhausted memory (issue #320).
    // With per-link dedup the receiver set stays at 2 and a long
    // chain completes instantly.
    //
    // `A` and `B` each carry a method the other lacks, so the
    // assertions below distinguish "the union was deduped" from
    // "the union collapsed to its first member".
    //
    // The completion runs on its own thread and is collected through a
    // `recv_timeout`, because a regression here does not return a wrong
    // answer: it runs for 2^38 steps while allocating. Nothing inside
    // the completion path yields, so `tokio::time::timeout` cannot
    // interrupt it (verified: it reports the deadline only after the
    // work finishes). Without a hard bound the whole test binary hangs
    // until CI gives up, or the OOM killer takes every other test in it
    // down as collateral.
    let uri = Url::parse("file:///union_chain_blowup.php").unwrap();
    let mut text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function m(): A|B { return new A(); }\n",
        "    public function onlyA(): string { return ''; }\n",
        "}\n",
        "class B {\n",
        "    public function m(): A|B { return new B(); }\n",
        "    public function onlyB(): string { return ''; }\n",
        "}\n",
        "class Chained {\n",
        "    public function run(A $a): void {\n",
        "        $a->m()\n",
    )
    .to_string();
    for _ in 0..38 {
        text.push_str("            ->m()\n");
    }
    text.push_str("            ->\n    }\n}\n");

    let cursor_line = 12 + 38;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(phpantom_lsp::PARSE_WORKER_STACK_SIZE)
        .spawn(move || {
            let backend = create_test_backend();
            let names = block_on(complete_labels_at(&backend, &uri, &text, cursor_line, 14));
            let _ = tx.send(names);
        })
        .expect("spawn union-chain completion thread");

    // Generous next to the ~2 ms this takes with the dedup in place, and
    // nowhere near enough for the blowup: even a 20-link chain took two
    // and a half minutes before the fix.
    let names = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("38-link union-return chain should complete, not blow up exponentially");
    assert!(
        names.iter().any(|n| n.starts_with("m(")),
        "Union-return chain should offer m() at every depth, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("onlyA(")),
        "Union member A should survive the chain walk, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("onlyB(")),
        "Union member B should survive the chain walk, got: {names:?}"
    );
}
