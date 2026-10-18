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

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Name resolution

No outstanding items.

## Miscellaneous

### B618. "Update return type" on a closure writes the enclosing method's type

**Impact: Medium · Complexity: Medium-High**

```php
public function run(): array {
    $f = function (): string {
        return 42;   // PHPStan: Anonymous function should return string but returns int.
    };
    return [];
}
```

The quick fix rewrites the closure to `function (): array` and adds a
`@return array{}` docblock above the statement: the type it writes is the
method's, not the closure's. The return-type quick fixes
(`code_actions/phpstan/fix_return_type/`, `remove_unused_return_type.rs`,
`add_iterable_type.rs`) find their function by scanning text (braces back
from the diagnostic line, the `)` before the `{`, the line holding
`function`), and the inference behind them (`declared_body` in
`fix_return_type/inference.rs`) then looks for a named function or method
around the body brace, so a closure resolves to the method it sits in. An
arrow function has no brace at all, and the text scan lands on the
method's. Locate the function through the parsed AST instead
(`find_enclosing_function` in `code_actions/cursor_context.rs`, with
`FunctionKinds::All`) and read the native return type, the body to infer
from and the docblock (already `paren_function_docblock`) from that one
node.

### B630. An unclosed call inside a braced `namespace` block drops the block

**Impact: Low · Complexity: Medium**

```php
<?php
namespace Lib {
    function greet(string $name): void {}
}
namespace App {
    use function Lib\greet;
    greet(   // until the `)` is typed, this line reads as `namespace Lib`
}
```

While a call inside a braced `namespace App { … }` block is missing its
`)`, mago's parser leaves the whole block out of `program.statements`, so
`update_ast` (`parser/ast_update.rs`) records no `NamespaceSpan` for it and
indexes none of its imports, classes or functions. An offset in it falls
into the last block the parse kept (`NamespaceSpan::containing`), and
every name typed there resolves against that block's namespace and imports
until the call is closed. Signature help runs on exactly this text, so it
shows the wrong function or none. A file that declares its blocks as
`namespace App;` keeps every block. The block survives only if the
parser's recovery ends the argument list at the block's `}` instead of
dropping the statement: upstream in `mago-syntax`, or by re-parsing with
the call closed, as signature help's text fallback already does for the
classes (`patch_content_for_signature`).

### B633. Diagnostics forget top-level variables at a braced `namespace` block

**Impact: Low · Complexity: Low**

```php
<?php
namespace A {
    $x = new \DateTime();
    $x->nope();   // reported: no `nope()` on DateTime
}
namespace B {
    $x->nope();   // not reported, though `$x` is still the DateTime
}
```

PHP keeps one global variable scope across a file's `namespace` blocks, and
hover follows it: `walk_body_forward` (`forward_walk/statements.rs`) walks
each block's statements on the same scope, so hovering `$x` in block B shows
`DateTime`. The diagnostic walk does not: `walk_top_level_statements`
(`forward_walk/diagnostic_walk.rs`) recurses into each `Statement::Namespace`
and starts it on a fresh `ScopeState`, so a variable assigned in one block
is untyped in the next and nothing checks the members, arguments or types
it is used with there. Carry the top-level scope from block to block,
switching only the loaders and imports, as the recursion's `block_ctx`
already does.

### B634. Edits outside the PHPStan quick fixes write `\n` line endings into `\r\n` files

**Impact: Low · Complexity: Medium**

In a file saved with `\r\n` line endings, most edits other than the PHPStan
quick fixes end the lines they add with `\n`: the `use` line an import adds
(from completion, "Import class", or the `use Override;` of "Add
`#[Override]`"), a generated constructor, getter, setter or property hook,
implemented methods, an extracted function, constant or interface, "Update
docblock", the docblock generated on `/**`, and an override completion,
among others. The file is left with mixed line endings. Each builder writes a literal
`\n` (`build_constructor` in `generate_constructor.rs`,
`extract_function/codegen.rs`, `update_docblock/rewrite.rs`, …) where it
should write `line_ending(content)` from `text_position.rs`, as the PHPStan
quick fixes and `sort_use_statements` do. Imports need the terminator
carried in: `UseBlockInfo::import_text` (`completion/use_edit.rs`) and the
template's `@use` writer (`blade/use_block.rs`) build their line without
the file's content, so `analyze_use_block_in` and
`analyze_template_use_block` would record it in the `UseBlockInfo`, and
`drop_repeated_separator` would strip that terminator instead of `\n`.
