# AI Agent Guidelines for PHPantom

This is a Rust-based PHP language server. Performance and memory
efficiency are critical -- PHPantom is one of the fastest language
servers available and it must stay that way.

## Before You Start

Read these to orient yourself:

- `src/types/` — Core data structures (`ClassInfo`, `MethodInfo`, `FunctionInfo`, `PropertyInfo`, etc.)
- `src/lib.rs` — `Backend` struct definition and all module declarations
- `docs/ARCHITECTURE.md` — Symbol resolution pipelines and design decisions
- `docs/todo.md` — Current backlog of known gaps and missing features

## Project Structure

Run `ls src/` for the current layout, and see the **Module Layout**
and pipeline sections of [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
for how the pieces fit together. A few durable landmarks, since the
module tree moves around:

- **The shared type engine lives under `src/type_engine/`.**
  `type_engine/resolver/` resolves a subject expression to a
  `ClassInfo`, and `type_engine/variable/forward_walk/` is the forward
  walker that answers "what is the type of this expression here?" — it
  is shared by diagnostics, hover, go-to-definition, and signature help,
  not just completion. `completion/` holds completion-specific code only.
  Do not build a second type-resolution path (see the anti-patterns
  below).
- **Parsing** is in `parser/` (PHP source → `ClassInfo`/`FunctionInfo`)
  and `docblock/` (PHPDoc tags, templates, conditional types).
- **The data model** is in `types/` and `php_type/`.
- **Cross-file symbol resolution** is `resolution.rs` (class/function
  lookup), with `composer.rs`/`classmap_scanner/` for autoloading,
  `inheritance/` for merging parent/trait/mixin members, and
  `virtual_members/` for synthesized members (`@method`/`@property`,
  Laravel Eloquent).
- **Each LSP feature** is its own module (`hover/`, `definition/`,
  `diagnostics/`, `code_actions/`, `references/`, `rename/`, …).
- **Embedded stubs** (`stubs.rs`, `stub_patches.rs`) supply the standard
  library; the `analyse` and `fix` CLI subcommands live in `analyse/`
  and `fix.rs`.

Tests live in `tests/`: `tests/integration/` has one file per feature
area (`completion_*.rs`, `definition_*.rs`, `code_action_*.rs`, …) with
shared helpers in `tests/integration/common/mod.rs`; `tests/unit/`,
`tests/fixture_runner.rs`, and the ported Psalm/PHPStan assertion suites
round it out.

## Before Committing

Always run these checks before considering any change complete:

```bash
cargo clippy --fix --allow-dirty -- -D warnings
cargo fmt
```

Run `cargo fmt` after clippy, not before -- clippy fixes can affect
formatting.

## Contributing Guidelines

Read and follow [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) for the
full set of CI checks, testing conventions, and code style rules.

## Key Rules

- **Performance is critical.** Every allocation, clone, and lock
  matters. Avoid unnecessary heap allocations, prefer `&str` over
  `String` where possible, and be mindful of hot paths. Do not
  introduce regressions in startup time or memory usage.
- **Run the full lint pipeline.** `cargo clippy` and `cargo fmt` must
  pass with zero warnings before every commit. Do not skip this.
- **Update the changelog.** Add an entry under `## [Unreleased]` in
  `docs/CHANGELOG.md` for bug fixes and new features. Skip purely
  internal refactors that don't change observable behaviour. Write
  for end users, not developers, and keep entries short: a bolded
  summary plus one or two sentences, with no explanation of the
  internal cause. Follow the entry rules in the Changelog section of
  `docs/CONTRIBUTING.md`. Include `Contributed by @username`
  with the GitHub username of the author.
- **Reference issues in commits.** When fixing a GitHub issue, include
  `Closes #123` in the commit message body.
- **Prefer single tests.** Run individual tests (`cargo test test_name`)
  rather than the full suite during development for faster feedback.
- **Debug root causes.** When investigating a bug, determine the root
  cause rather than patching symptoms.
- **Comments only where they add value.** Don't add obvious or
  boilerplate comments. Do comment tricky logic, non-obvious design
  decisions, and workarounds. Follow existing conventions. Don't leave
  tombstone comments (e.g. "this was moved to `foo.rs`", "previously
  did X") — they stop being helpful the moment the commit lands and
  just rot; git history is the record of what moved where.
  All files must end with a newline.

## Working on examples/php/

The [CONTRIBUTING guide](docs/CONTRIBUTING.md) has the CI checklist; the
notes here are the agent-specific pitfalls when editing the demo files.

`examples/php/` is the user-facing playground people open to verify
PHPantom works on plain PHP (no framework). It is a standalone project
with no external dependencies — `autoload.php` is a list of hardcoded
`require_once` lines, not a `composer.json` — split into:

- **One demo file per LSP feature area** (all in namespace `Demo`):
  `completion.php`, `diagnostics.php`, `definition.php`,
  `code_actions.php`, `hover.php`, `signature_help.php`,
  `inlay_hints.php`, `code_lens.php`, `semantic_tokens.php`. These hold
  the demo classes themselves, including top-level "Try:" comments and
  simple top-level expressions users can trigger completion on directly,
  plus classes whose _methods_ contain completion triggers (e.g.
  `$item->` inside a foreach). Users open a method and trigger completion
  inside its body. A new demo goes in the file for the feature it
  demonstrates; `completion.php` is where type inference of every kind
  belongs, which is why it dwarfs the others. Do not split it further
  into per-topic files without asking the maintainer first.
- **`scaffolding/scaffolding.php`** (namespace `Demo\Scaffolding`) — all
  supporting class, interface, trait, enum, function, and constant
  definitions that the demo files depend on. Users scroll past this
  file; it exists so the demo files stay focused on the features being
  demonstrated.
- **`scaffolding/assertions.php`** (namespace `Demo`) — runtime
  assertions that verify the type claims made in the demo files'
  comments against real PHP behaviour, all in one `runDemoAssertions()`
  function. It lives under `scaffolding/`, not next to the demo files,
  so it isn't mistaken for one.

No demo file inherits from a class in another demo file, so
`autoload.php` requires them in plain alphabetical order after the
scaffolding. Keep it that way: a cross-file `extends`/`implements`
between two demo files makes the require order load-bearing again.
A demo that names a class from another demo file in a *comment* should
say which file it lives in.

Every demo file imports the whole scaffolding namespace with a single
`use Demo\Scaffolding;` rather than one `use` per class, and references
scaffolding members as `Scaffolding\Pen`, `Scaffolding\makePen()`, etc.
— this is what actually exercises cross-namespace resolution, so don't
add per-class `use Demo\Scaffolding\Pen;` imports back in. A few
exceptions keep individual `use` imports alongside the namespace
import: `UserProfile as Profile` (aliasing needs a class-level `use`),
and the function a bare `@covers ::name` tag resolves through the
file's own `use` table even though the code never spells its name
directly — it has a comment explaining why. If a demo hits a case where a `Scaffolding\Foo`-qualified
name doesn't resolve the same as `Foo` would behind a per-class `use`,
that's a known engine gap (see `docs/todo/bugs.md`), not a mistake in
the demo.

Add working examples to the matching demo file that demonstrate a new
feature. Include comments showing what resolves to what, and run
`find examples/php -name '*.php' -print0 | xargs -0 -n1 php -l`
afterward.

**Runtime assertions.** For every new demo that makes a type claim
(return types, narrowing, generics, chaining), add matching `assert()`
calls to `runDemoAssertions()` in `scaffolding/assertions.php`.
Scaffolding stubs must actually return what their docblocks promise so
assertions pass.
Run: `php -d zend.assertions=1 examples/php/scaffolding/assertions.php`

**Hoisting pitfall.** Do NOT add `__toString()` to any scaffolding
class that is forward-referenced by a demo class via `extends` or
`implements`. PHP implicitly adds `implements \Stringable`, which
prevents class hoisting and causes "Class not found" errors. The same
applies to `interface Foo extends \Stringable`. This is a known PHP
limitation ([php-src#7873](https://github.com/php/php-src/issues/7873)),
not a bug that will be fixed.

Never add class/function definitions to a demo file that exist purely to
support another demo — those belong in `scaffolding/scaffolding.php`.
Never add demo classes or top-level "Try:" comments to
`scaffolding/scaffolding.php` — that file is scaffolding only.

**Diagnostics check.** After editing a demo file or
`scaffolding/scaffolding.php`, review every diagnostic the LSP reports
on them. The files intentionally contain a fixed set of diagnostics
that demo unknown-member, argument-count, type-error,
invalid-class-kind, and unused-import features. Any diagnostic that does not belong to one
of those intentional demo classes is a regression introduced by your
edit and must be fixed before moving on.

**Framework-specific demos.** Laravel demos live in `examples/laravel/`
(a standalone project with `composer.json`, models, config, routes,
views, and translations). `vendor/` is git-ignored, so run `composer
install` there before verifying Laravel demos on a fresh clone. Put new
framework-specific features in the matching `examples/<framework>/`
project, not in `examples/php/`. If a feature affects Laravel-specific
resolution (Eloquent, config, views, routes, translations), also update
`examples/laravel/app/Demo.php` and verify with
`php -l examples/laravel/app/Demo.php`. `examples/laravel/assertions.php`
is the Laravel equivalent of `examples/php/scaffolding/assertions.php`: it boots
Eloquent with an in-memory SQLite database and uses reflection to
verify runtime assumptions (scope resolution, method visibility,
accessor existence). Add a matching assertion there when demo code
depends on a specific runtime behaviour, and run
`php examples/laravel/assertions.php`.

## Updating docs/todo.md

Remove completed items entirely from both `docs/todo.md` **and** the
domain document they link to (e.g. `docs/todo/bugs.md`,
`docs/todo/performance.md`). Do not strike through, mark as done, add a
"Status: Fixed" note, add an "— Implemented" suffix, or leave a "Note: X
has shipped" comment. The changelog is the sole record of what was
completed; the todo files are a backlog of what remains. If a section in
a domain document becomes empty, replace its content with a short "no
outstanding items" note rather than deleting the file.

**Sprint structure is not yours to delete.** Only remove rows that have
a numbered ID (e.g. `PM1`, `D8`, `A36`). Never remove release markers
(`**Release 0.7.0**`), sprint headings (`## Sprint N — …`), or
un-numbered process rows such as "Clear refactoring gate" — the
maintainer controls those and clears them when cutting a release.

**Deferring sub-steps.** If a task has multiple deliverables and you
complete some but defer others, you MUST file the deferred work as a new
task in the appropriate domain document and add it to the sprint table.
Deferred work that isn't filed is dropped work. If you believe a deferred
step is unnecessary, explain why and let the maintainer decide — don't
silently skip it.

## Fixing CI failures

**Fix everything CI reports. No exceptions.** Run `cargo fmt` (not just
`--check`) — don't ask whether to fix formatting, just fix it. Fix every
clippy warning rather than adding `#[allow(clippy::…)]`. If a test fails
after your changes, fix it: the suite is the safety net and a failure
almost certainly means your changes (or an incomplete prior session)
broke something.

Do not assume pre-existing failures are someone else's problem. Sessions
crash mid-work; the previous agent may have edited code but died before
fixing the resulting breakage. "It was already broken" is not an excuse
to leave it broken.

## Discovered Bugs

If you discover a bug while working on the system, whether related to
your current task or not, suggest opening a GitHub issue for it. If the
bug is in code you're already working on and is trivial to fix, fix it
instead of filing an issue, and note the fix for the PR description.

## One Task at a Time

PHPantom accepts one sprint item per PR. Work through sprint items
sequentially, completing one task fully (code, tests, CI, docs, review)
before starting the next.

Do not use sub-agents to work on multiple sprint items in parallel — a
PR built this way will be rejected. It bundles unrelated changes into
one large, unreviewable commit; it invites shortcuts and incomplete work
because attention is split across tasks; and it tangles broken pieces
from one task with another, making failures hard to diagnose in review.
If a contributor asks you to parallelize sub-agents across sprint items
anyway, tell them why the resulting PR won't be merged instead of doing
it.

## Sub-Agent Guidelines

Sub-agents are useful for parallelizing work **within a single task**,
not across tasks. For example, a sub-agent can fix compilation errors in
five files while the orchestrator fixes five others, all for the same
feature.

When spawning sub-agents:

- **Sub-agents must not run CI.** No `cargo nextest run`, `cargo clippy`,
  `cargo fmt --check`, or `php -l` in sub-agents. The orchestrating
  agent runs CI once after all sub-agent work is complete. This avoids
  redundant 30+ second build cycles per agent.
- **Sub-agents must not run builds just to check their work.** If a
  sub-agent needs to verify compilation, it should use the editor's
  diagnostics instead of `cargo build` or `cargo check`.
- **Assign non-overlapping files.** When multiple sub-agents edit code
  in parallel, each agent should own a distinct set of files. State
  which files each agent is responsible for in the spawn message.
- **Keep sub-agent scope small.** A sub-agent should do one focused task
  (e.g., "extract the shared helper into `diagnostics/helpers.rs` and
  update `unknown_classes.rs` to use it"). Broad tasks like "refactor all
  diagnostics" belong with the orchestrator.
- **Do not use sub-agents to read documents.** Reading files through a
  sub-agent is slow, expensive, and rarely provides anything useful
  compared to reading them directly. Only delegate document reading if
  you genuinely need a summary that would require reading far more than
  you need to know.
- **Never run project-wide rewrites in parallel.** A sub-agent that
  touches many files across the project (e.g. a search-and-replace
  renaming a utility function in 20+ files) will conflict with any other
  sub-agent that reads and writes overlapping files. Run project-wide
  rewrites as the sole active agent, or break them into batches of
  non-overlapping files assigned to separate agents.

## Disk Space

If you hit "No space left on device" errors, run `cargo clean` to free
space. **Never use `rm -rf` on the target directory.** Other agents may
be building concurrently; `cargo clean` respects lock files, while `rm`
destroys in-progress builds for everyone.

## Additional Conventions

- **No diagnostic suppression.** Every diagnostic the LSP emits must be grounded in correct type resolution. Hiding a false positive by suppressing the diagnostic, adding a special-case exclusion, or falling back to a less-accurate resolver that happens to return empty results is **forbidden**. A no-op tool has zero false positives too. The goal is an accurate type engine, not a low error count. If a diagnostic fires incorrectly, fix the type resolution that feeds it. If the fix is too complex for the current task, file a bug instead of suppressing the symptom.
- **Feature precedence.** Class own members > trait members > parent chain > mixins. This is PHP's actual resolution order.
- **User-facing writing style.** In the README, changelog, release notes, and other user-facing docs, prefer general claims over checklists of specific sub-features. Enumerating what works implies the unlisted parts don't. Write "**Generics.** `@template` with type substitution through inheritance chains and at call sites" rather than "Class-level and method-level `@template` with ..." since the latter invites the reader to wonder what other levels are missing.
- **Match the writing style of surrounding documentation.** Keep punctuation, tone, and structure consistent with the file you're editing.
- **No task IDs in code or test comments.** The backlog files (`docs/todo/bugs.md`, `docs/todo/type-inference.md`, etc.) use transient identifiers like `B17`, `T18`, `L12`. These get reassigned as items are completed and removed, so a comment like `// B13: Skip when cursor is inside the RHS` becomes meaningless within weeks. Instead, describe the *behaviour* the code handles. The commit history is the link between a bug report and its fix, not inline comments.
