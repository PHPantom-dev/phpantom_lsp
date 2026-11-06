# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md), but a bug's row lives
**here only** — do not add or link a bug entry to `docs/todo.md`'s sprint
or backlog tables. This file is its own list, not a domain document
sprint items draw from: whenever it holds anything, that is actively
addressed, independently of sprint planning.

Bugs land here from wherever they surface: found while working on another
task, or sweeps of the sample projects under `projects/`. Entries are
grouped by the mechanism that has to change, not by the symptom that
surfaced: one entry is one root cause, however many shapes it shows up in.

## Crashes

No outstanding items.

## Type comparison

### B709. A docblock member the native type cannot hold survives when the native type is not all classes

**Impact: Medium · Complexity: Low-Medium**

```php
/** @return resource|Socket|false|null */      // phpstorm-stubs' socket_import_stream()
function socket_import_stream($stream): Socket|false {}

$socket = socket_import_stream($stream);
socket_get_option($socket, …);   // expects Socket, got resource|Socket|null
```

PHP guarantees the native type, so a docblock can only narrow it: the
effective type is the docblock's members the native type can hold. With
`Socket|false` that is `Socket|false`; `resource` and `null` cannot come
back. `doc_members_native_can_hold` in `src/docblock/type_override.rs`
drops such members only when every native member names a class, so a
native union with a scalar member (`false`, `int`, `string`) keeps them
all. Judge each docblock member against the native members whatever they
are, keeping the members some native member can hold (a class member holds
any object, `false` holds `false`, `bool` holds `true` and `false`, and
so on), and fall back to the native type when none is left.

Found by `analyze` on azjezz/psl (14 false positives from
`socket_import_stream()`).

## Conditional return types

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

### B692. `(int)` of a `ctype_digit()`-checked string is not non-negative

**Impact: Low · Complexity: Low-Medium**

```php
function wait(string $delay): void {
    if (!ctype_digit($delay)) {
        return;
    }
    sleep((int) $delay);   // expects int<0, max>, got int
}
```

A string that passes `ctype_digit()` holds only the digits 0-9, so casting
it to `int` cannot give a negative number, and `sleep()`'s
`int<0, max>` parameter accepts it. The narrowing leaves `$delay` a plain
`string` and the cast gives `int`. PHPStan reports nothing here only
because its function map declares `sleep()` and `usleep()` as taking a
plain `int`. phpstorm-stubs and Mago both declare `int<0, max>`, which
matches PHP 8 throwing a `ValueError` on a negative.

Found in a Laravel project's API client, which sleeps for the number of
seconds a rate-limit message names.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Framework names in strings

No outstanding items.

## Name resolution

### B704. A function declared in two files resolves to whichever is found first

**Impact: Medium · Complexity: Medium**

```php
// wp-admin/includes/noop.php, loaded only by load-scripts.php
function __() {}

// wp-includes/l10n.php, loaded everywhere else
function __( $text, $domain = 'default' ) { … }

__( 'Hello' );   // Expected 0 arguments, got 1
```

WordPress core declares nineteen of its functions a second time as
no-ops for its script loader. Calls across the project resolve to the
no-op, so with `extra-arguments` on WordPress core gets 17,435
argument-count reports (13 once `noop.php` is excluded, the same 13
PhpStorm reports), and hover, signature help, and return types all
describe the wrong declaration. PHP only ever loads one of them, and
nothing in the call says which.

When a name has several declarations, a call is valid if any of them
accepts it, and its return type is the join of theirs. Where one
declaration is plainly the real one (the other has an empty body and no
parameters, or sits in a file only one entry point includes), prefer it
for hover and go-to-definition, and offer the others alongside.

### B705. A method body in a file not yet opened is read without its imports

**Impact: Medium · Complexity: Medium**

```php
// src/Repository/Queries.php, loaded through PSR-4
namespace App\Repository;
use App\Entity\User;

final class Queries {
    public function make() { return new User(); }
    public function users(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('u')->from(User::class, 'u');
    }
}

// src/Service/Users.php, the open file
$queries->make();    // App\Repository\User, expected App\Entity\User
$queries->users()->getQuery()->getResult();   // mixed, expected list<User>
```

When a method declares no return type, or returns a Doctrine query
builder, its type is read from its body. A class name the body writes
resolves against the declaring file's namespace but not its `use`
imports, when that file was loaded on demand rather than opened or
indexed: `User` becomes `App\Repository\User`. A fully-qualified name
resolves, and so does every name once the whole project is indexed (as
`analyze` does), so it shows in the editor rather than the CLI.

**Where to look:** `Backend::infer_return_type_at`
(`src/code_actions/phpstan/fix_return_type/inference.rs`), which takes
the imports from `import_block_at`, and how a file loaded on demand
records its `use` map in its `FileState`.

### B707. A docblock annotation does not name its class

**Impact: Medium · Complexity: Low-Medium**

```php
use Doctrine\ORM\Mapping as ORM;   // Unused import 'Doctrine\ORM\Mapping'
use Symfony\Component\Routing\Annotation\Route;   // Unused import

/** @ORM\Entity */
final class User
{
    /** @Route("/users") */
    public function list(): Response { … }
}
```

Doctrine-style annotations (Doctrine ORM, older Symfony, JMS, Drupal
plugins) name a class with their tag, resolved through the file's `use`
imports like any other class name. The tag is read as an unknown docblock
tag instead, so an import only an annotation uses is reported as unused,
and the quick fix and `phpantom_lsp fix` remove it, which breaks the
annotation at run time. Go-to-definition, hover, references and rename
miss the class too.

Record an annotation tag (`@` then a name, at the start of a docblock
line or inside another annotation's arguments) as a class reference when
its name resolves to a class, the way a docblock type is. A tag PHPDoc
or a tool defines (`@param`, `@phpstan-*`, `@psalm-*`) is never one, and
a lowercase tag with no class behind it stays silent.

**Where to look:** `extract_docblock_symbols` (`src/symbol_map/docblock.rs`),
which emits the class references `unused_imports.rs` and the navigation
features read.

## Code actions

No outstanding items.

## Miscellaneous

### B706. A Doctrine mapping file is not read until the workspace index ends

**Impact: Low-Medium · Complexity: Low-Medium**

```php
// config/doctrine/User.orm.xml maps User, with a `team` association
$em->createQuery('SELECT t.name FROM App\Entity\User u JOIN u.team t');
// Query<mixed, mixed> right after the editor starts,
// Query<null, array{name: string}> a few seconds later
```

The XML and YAML mapping files are indexed with the other resource files,
in the last step of the background workspace index
(`index_resource_paths_batch` from `indexing/preload.rs`). Until then an
entity mapped only by a file is no entity: a hover, completion, or
diagnostic asked for before then gives a mixed query, no magic finders,
and no DQL field names, and nothing recomputes it when the files arrive.
The resolved-class cache is cleared as each file is indexed, but a result
already shown stays wrong until it is asked for again.

**Where to look:** where `index_framework_uri_content` is called during
startup; the mapping files could be indexed before the PHP files are
parsed, or the open files' diagnostics refreshed once they are.

