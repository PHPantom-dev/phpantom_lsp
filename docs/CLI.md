# CLI Reference

PHPantom is a language server, but it also ships CLI tools for batch
analysis and automated fixing. These run the same engine that powers the
editor, so results are consistent between what you see in your editor
and what CI reports.

## Modes

| Command                  | Purpose                                              |
| ------------------------ | ---------------------------------------------------- |
| `phpantom_lsp`           | Start the LSP server over stdin/stdout (the default) |
| `phpantom_lsp --tcp 9257`| Start the LSP server listening on a TCP port         |
| `phpantom_lsp analyze`   | Report diagnostics across the project                |
| `phpantom_lsp fix`       | Apply automated code fixes across the project        |
| `phpantom_lsp init`      | Generate a default `.phpantom.toml` config file      |

Running with no subcommand starts the language server. Editors launch
this automatically.

---

## TCP Transport

By default PHPantom communicates over stdin/stdout, which is what most
editors expect. The `--tcp` flag switches to TCP transport instead,
which is useful when you want to attach a debugger to the server
process, connect from an IDE plugin that prefers a network socket, or
just poke at the JSON-RPC stream with `nc` or `socat`.

The server binds to the given address, accepts a single client
connection, serves it, and exits when the client disconnects.

```sh
phpantom_lsp --tcp 9257                  # listen on 127.0.0.1:9257
phpantom_lsp --tcp 127.0.0.1:9257       # same, explicit host
phpantom_lsp --tcp 0.0.0.0:9257         # listen on all interfaces
phpantom_lsp --tcp 0                    # OS picks a free port
```

When the server starts it prints the bound address to stderr:

```
PHPantom LSP listening on tcp://127.0.0.1:9257
```

This is especially handy with port `0`: the OS assigns an available
port and the server tells you which one it picked.

### Connecting

Any LSP client that supports TCP can connect. For quick manual testing:

```sh
# In one terminal, start the server:
phpantom_lsp --tcp 9257

# In another terminal, connect and send JSON-RPC:
nc 127.0.0.1 9257
```

### Options

| Flag              | Description                                                              |
| ----------------- | ------------------------------------------------------------------------ |
| `--tcp <ADDR>`    | Address to listen on. Full `HOST:PORT` or just `PORT` (defaults to `127.0.0.1`). |

---

## `analyze`

Scans PHP files and reports PHPantom diagnostics in a PHPStan-style
table format. The goal is full symbol resolution: every class, member,
and function call in your codebase should be resolvable. When that
holds, completion and hover work everywhere, and PHPStan gets the type
information it needs at every level.

Use it to find and fix the spots where the editor can't resolve a
symbol, so you can achieve and maintain full type coverage across
the project.

### What it checks

It doesn't try to find every possible bug. Its main focus is symbol
resolution: every class, member, and function call should point to
something real. It also catches basic correctness issues like wrong
argument counts and missing interface implementations. When a codebase
passes cleanly, completions work everywhere for every developer on the
team.

That makes it useful in a few situations:

- **Teams that just want completions to work in their editor.** If
  your goal is "every `->` and `::` resolves to something" rather
  than "catch every possible runtime error," PHPantom's analysis
  covers exactly that.
- **As a companion to PHPStan at a moderate level.** A team running
  PHPStan at level 4 (catching dead code and type mismatches) can add
  PHPantom's analysis to enforce that every class, member, and function
  is resolvable across the full codebase. PHPStan catches logic errors,
  PHPantom catches structural gaps. Together they cover a useful quality
  surface without the effort of configuring PHPStan at max level.
- **As a quick sanity check.** Point it at a Composer project and it
  reports what it finds. No baselines, no ignore files, no level to
  choose. The only configuration worth knowing about is
  `unresolved-member-access`: enable it in `.phpantom.toml` to also
  flag member access on variables whose type could not be resolved
  (off by default because it is noisy on untyped codebases).

> [!NOTE]
> There are still occasional false positives, though they are getting
> fewer with each release. If you hit one, please
> [report it](https://github.com/PHPantom-dev/phpantom_lsp/issues).

### Usage

```sh
phpantom_lsp analyze                             # scan entire project
phpantom_lsp analyze src/                        # scan a subdirectory
phpantom_lsp analyze src/Foo.php                 # scan a single file
phpantom_lsp analyze --severity warning          # errors and warnings only
phpantom_lsp analyze --severity error            # errors only
phpantom_lsp analyze --project-root /path/to/app # explicit project root
phpantom_lsp analyze --no-colour                 # plain text output
```

### Options

| Flag                       | Description                                                      |
| -------------------------- | ---------------------------------------------------------------- |
| `[PATH]`                   | File or directory to analyze. Defaults to the entire project.    |
| `--severity <LEVEL>`       | Minimum severity: `all` (default), `warning`, or `error`.        |
| `--project-root <DIR>`     | Project root directory. Defaults to the current working directory.|
| `--no-colour`              | Disable ANSI colour output.                                      |

### Exit codes

| Code | Meaning                 |
| ---- | ----------------------- |
| 0    | No diagnostics found    |
| 1    | Diagnostics were found  |

### Example output

```
 ------ -------------------------------------------
   Line   src/Service/UserService.php
 ------ -------------------------------------------
   15     Unknown class 'App\Models\LegacyUser'.
          🪪  unknown_class
   42     Call to undefined method Post::archive().
          🪪  unknown_member
 ------ -------------------------------------------
```

### Reported diagnostics

The analyze command reports the same diagnostics you see in your editor.
Each has a rule identifier shown below the message.

| Identifier               | Severity | Description                                          |
| ------------------------ | -------- | ---------------------------------------------------- |
| `syntax_error`           | Error    | PHP parse errors                                     |
| `unknown_class`          | Warning  | Class, interface, trait, or enum not resolvable       |
| `unknown_member`         | Warning  | Property or method not found on the resolved class    |
| `unknown_function`       | Error    | Function call not resolvable                          |
| `argument_count`         | Error    | Wrong number of arguments to a function or method     |
| `implementation_error`   | Error    | Missing required interface or abstract methods        |
| `scalar_member_access`   | Error    | Member access on a scalar type (int, string, etc.)    |
| `unused_import`          | Hint     | `use` statement with no references in the file        |
| `deprecated`             | Hint     | Reference to a `@deprecated` symbol                   |

---

## `fix`

Applies code fixes across the project. Specify which rules to run, or
omit `--rule` to run all preferred native fixers.

This is useful for cleaning up an entire codebase in one pass. For
example, a project with hundreds of unused `use` statements can be
cleaned up in seconds rather than file by file.

```sh
phpantom_lsp fix                                  # apply all preferred fixers
phpantom_lsp fix --rule unused_import             # only remove unused imports
phpantom_lsp fix --rule unused_import --rule deprecated  # multiple rules
phpantom_lsp fix --dry-run                        # preview without writing
phpantom_lsp fix src/                             # restrict to a subdirectory
phpantom_lsp fix src/Foo.php                      # fix a single file
phpantom_lsp fix --project-root /path/to/app      # explicit project root
```

### Options

| Flag                       | Description                                                          |
| -------------------------- | -------------------------------------------------------------------- |
| `[PATH]`                   | File or directory to fix. Defaults to the entire project.            |
| `--rule <RULE>`            | Rule to apply (repeatable). Omit to run all preferred native rules.  |
| `--dry-run`                | Report what would change without writing files.                      |
| `--with-phpstan`           | Enable PHPStan-based fixers (future feature).                        |
| `--project-root <DIR>`     | Project root directory. Defaults to the current working directory.    |
| `--no-colour`              | Disable ANSI colour output.                                         |

### Exit codes

| Code | Meaning                                          |
| ---- | ------------------------------------------------ |
| 0    | Fixes applied successfully (or nothing to fix)   |
| 1    | Error (bad arguments, write failure, etc.)       |
| 2    | Dry-run found fixable issues (nothing written)   |

### Available rules

Rules correspond to diagnostic identifiers.

| Rule               | Description                    |
| ------------------ | ------------------------------ |
| `unused_import`    | Remove unused `use` statements |

### Example output

```
 ------ -------------------------------------------
   Line   src/Service/UserService.php
 ------ -------------------------------------------
    5     Unused import 'App\Models\LegacyUser'
          🔧  unused_import
    6     Unused import 'App\Support\OldHelper'
          🔧  unused_import
 ------ -------------------------------------------

 [FIXED] Applied 2 fixes across 1 file
```

### Dry-run example

```sh
phpantom_lsp fix --dry-run --project-root /path/to/app
```

```
 ------ -------------------------------------------
   Line   src/Service/UserService.php
 ------ -------------------------------------------
    5     Unused import 'App\Models\LegacyUser'
          🔧  unused_import
 ------ -------------------------------------------

 [DRY RUN] 1 fix in 1 file (not applied)
```

### Idempotency

Running `fix` twice produces the same result as running it once. If
all issues are already resolved, the command exits with code 0 and
writes nothing.

---

## `init`

Creates a default `.phpantom.toml` in the current directory with all
options documented and commented out. Safe to run if the file already
exists (it will not overwrite).

```sh
phpantom_lsp init
```

See [Project Configuration](SETUP.md#project-configuration) for details
on available settings.

---

## CI Integration

PHPantom works well as a lightweight CI gate. It's a single static
binary with no runtime dependencies (no PHP, no Composer, no Node). Drop
it into a pipeline and point it at your project root.

### Why use it in CI?

Editor diagnostics only help the developer who has the editor open.
CI analysis protects the whole team: no PR can merge if it introduces
an unresolvable symbol, regardless of which editor each developer uses.
Over time this keeps the codebase fully navigable, so completions,
hover, and go-to-definition work everywhere for everyone.

It also complements PHPStan rather than replacing it. PHPStan is better
at catching logical errors, type mismatches, and dead code. PHPantom is
better at catching structural gaps: unknown classes, unresolvable
members, missing implementations. Running both gives you broad coverage
without needing PHPStan at max level to get full symbol resolution.

### GitHub Actions example

```yaml
name: Type Coverage
on: [push, pull_request]

jobs:
  phpantom:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # Use --no-dev to catch production code that depends on dev-only
      # packages (e.g. calling PHPUnit classes from application code).
      - name: Install Composer dependencies
        run: composer install --no-interaction --prefer-dist --no-dev

      - name: Install PHPantom
        run: |
          curl -sL https://github.com/PHPantom-dev/phpantom_lsp/releases/download/0.6.0/phpantom_lsp-x86_64-unknown-linux-gnu.tar.gz | tar xz
          chmod +x phpantom_lsp

      - name: Check type coverage
        run: ./phpantom_lsp analyze --severity warning --no-colour src/
```

The `analyze` step fails the build if any class, member, or function
cannot be resolved (including unused imports). The output is clean and
readable in the CI log.

### Common patterns

**Diagnostics gate.** Fail the build when PHPantom finds unresolvable
symbols:

```sh
phpantom_lsp analyze --severity warning --project-root . --no-colour
```

**Enforce clean imports.** Fail the build when unused imports exist:

```sh
phpantom_lsp fix --dry-run --rule unused_import --project-root . --no-colour
```

**Pre-commit hook.** Clean up imports before every commit:

```sh
phpantom_lsp fix --rule unused_import --project-root .
```

**Combine analyze and fix.** Run fixes first, then check what remains:

```sh
phpantom_lsp fix --project-root .
phpantom_lsp analyze --project-root .
```
