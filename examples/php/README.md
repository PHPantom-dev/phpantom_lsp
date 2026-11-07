# PHP Showcase for PHPantom LSP

A playground for every completion, diagnostics, go-to-definition, and code
action feature that doesn't need a framework. Unlike a framework project,
where demo code, models, and templates naturally live in separate files,
a plain PHP playground has to split itself up on purpose — that's what this
directory does: one demo file per feature area (all in namespace `Demo`),
their supporting fixtures in `scaffolding/scaffolding.php` (namespace
`Demo\Scaffolding`), and the runtime assertions that check the type claims
made in the demo comments in `scaffolding/assertions.php`.

## Layout

| File                  | What it demos                                                          |
| --------------------- | ---------------------------------------------------------------------- |
| `completion.php`      | Completion and the type inference behind it. By far the largest file.  |
| `diagnostics.php`     | The errors and warnings PHPantom reports.                              |
| `definition.php`      | Go-to-definition, type definition, implementation, and type hierarchy. |
| `code_actions.php`    | The quick fixes and refactorings on the lightbulb.                     |
| `hover.php`           | What hovering a symbol shows.                                          |
| `signature_help.php`  | The parameter hints shown while typing arguments.                      |
| `inlay_hints.php`     | The inline hints next to arguments and inferred types.                 |
| `code_lens.php`       | The annotations above a declaration.                                   |
| `semantic_tokens.php` | The type-aware highlighting layered over your editor's grammar.        |

Every file is in namespace `Demo` and imports the scaffolding namespace as
a whole (`use Demo\Scaffolding;`), so each reference across the boundary is
a real cross-namespace lookup. `autoload.php` requires the scaffolding first
and then each demo file; no demo file inherits from another, so their order
is only alphabetical.

## What it demos

- **Type narrowing.** `instanceof`, `assert()`, guard clauses,
  `property_exists()`/`method_exists()`, custom type-guard functions, and
  PHPUnit-style assertion re-exports, each narrowing a union down to the
  branch the code actually reached.
- **Generics.** `@template` with type substitution through inheritance
  chains and at call sites, method-level and closure-level templates,
  conditional return types, and template bounds.
- **Cross-file, cross-namespace resolution.** Demo classes live in `Demo\`,
  shared fixtures in `Demo\Scaffolding\` — every reference across that
  boundary is a real cross-namespace lookup (`use Demo\Scaffolding;` plus
  `Scaffolding\Pen`-qualified names), exercising the same namespace
  resolution a real multi-file project relies on.
- **Diagnostics.** Unknown members, argument-count and type mismatches,
  named arguments that bind no parameter, readonly-property violations,
  deprecation warnings, private members nothing uses, and invalid
  class-kind usage (instantiating an abstract class or enum, `instanceof`
  against a trait).
- **Code actions.** Import a class, remove an unused import, sort `use`
  statements, generate a constructor/getter/setter/property hooks, extract
  a function, promote a constructor parameter, and more.
- **Navigation.** Go-to-definition, go-to-type-definition,
  go-to-implementation, signature help, code lens, and inlay hints.
- **Semantic highlighting.** Coloring driven by what a name resolves to
  rather than by its shape: the four class-like kinds told apart, members
  checked against the class they are read from, docblock-declared members
  and template parameters, and the modifiers (static, readonly,
  deprecated) a grammar has no way to infer.

## Getting started

1. Open this directory as a project (or workspace folder) in your editor.
   There's nothing to install — the project has no external dependencies,
   so `autoload.php` is just a list of `require_once` lines instead of a
   `composer.json`.
2. Open the file for the feature you want to see (`completion.php` is the
   place to start) and navigate to any class's `demo()` method.
3. Trigger completion, hover, or go-to-definition inside the method body.
4. Run `php -d zend.assertions=1 scaffolding/assertions.php` to verify the
   type claims made in the demo comments against real PHP runtime
   behaviour.
