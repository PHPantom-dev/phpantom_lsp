# zed-laravel

Laravel and Blade support for the [Zed](https://zed.dev) editor,
powered by [PHPantom](https://github.com/PHPantom-dev/phpantom_lsp).

This is an incubating project intended to graduate into its own
repository (`phpantom-devs/zed-laravel`). It is deliberately separate
from plain PHP language-server wiring, which has merged into the
official `zed-extensions/php` (the PHPantom repo's own
`zed-extension/` has been retired), so this project must not grow
plain-PHP features. Everything framework-flavoured lives here
instead.

## Why

PHPantom already does the hard part (Blade preprocessing, Eloquent,
string-key intelligence) server-side. The Blade *language* side is
covered upstream: the `zed-laravel-blade` extension owns the Blade
language definition and already registers PHPantom as a Blade
language server with the `blade` language id. What remains for this
extension is the Laravel workflow layer Zed can express through
capabilities: runnables and snippets.

## Scope

See [todo.md](todo.md) for the work items, including the ecosystem
context (two existing extensions, `zed-laravel-blade` and
`mike-bronner/zed-laravel`, bound our scope). In one paragraph:
verify and upstream-improve the existing Blade wiring rather than
shipping a competing language definition, add Laravel-aware runnables
(artisan commands, run-this-test), Blade/Laravel snippets, and
coordinate with the mike-bronner Laravel extension on coexistence.
Zed's extension API is capability-based (languages, grammars,
language servers, snippets, runnables via task templates) — no
arbitrary panels or webviews — so the scope is honest about that:
anything needing custom UI stays in the language server as code
lenses, hovers, and completions, which Zed renders natively.
