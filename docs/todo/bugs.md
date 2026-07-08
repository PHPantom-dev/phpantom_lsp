# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.


## B23. Deleted functions and stale `define()` data survive edits for the whole session

**Severity: High (stale completions, wrong hover, wrong navigation) · Confirmed**

`update_ast_inner` only ever **inserts** into the two global
symbol maps; it never evicts entries that the edit removed:

- `global_functions` (`src/parser/ast_update.rs:376-406`):
  deleting or renaming a standalone `function foo()` leaves the
  old entry keyed by its FQN. Completion keeps offering `foo()`,
  hover shows the old signature, and go-to-definition jumps to a
  stale byte offset, indefinitely.
- `global_defines` (`src/parser/ast_update.rs:419-429`): uses
  `dmap.entry(name).or_insert_with(...)`, so an existing entry is
  **never updated at all** — not on deletion, and not even when
  the value or position changes. Editing `define('X', 1)` to
  `define('X', 2)` keeps showing `1` on hover; inserting a line
  above the `define` leaves `name_offset` stale, so
  go-to-definition lands on the wrong position.

Nothing else repairs this while the file is open:
`apply_watched_file_changes` explicitly skips open files
(`src/server.rs:2255`), and `did_close` keeps both maps
(intentionally, for cross-file resolution). Classes handle this
correctly via `old_fqns` eviction (`ast_update.rs:526-531,
583-584`); functions and defines need the same treatment.

**Fix:** track which function FQNs / define names the previous
parse of this URI contributed (analogous to `old_fqns`), evict
those that disappeared, and make `global_defines` overwrite
instead of `or_insert_with` so value/offset changes propagate.
`reindex_files_batch` (`src/lib.rs:1254-1259`) already shows the
retain-by-URI pattern for the watched-file path.


## B24. `parse_and_cache_content_versioned` leaves stale index entries on re-parse

**Severity: Medium (ghost classes in resolution and hierarchy) · Confirmed**

The lazy-load parse path (`src/resolution.rs:428-524`) — used for
vendor files, stubs, and any file re-loaded after `did_close` —
does not evict the previous version's state when it re-parses a
URI:

- `fqn_class_index`: only inserts new FQNs (line 463). A class
  deleted or renamed in the file keeps resolving from the stale
  `ClassInfo`.
- `fqn_uri_index`: uses `.entry(fqn).or_insert_with(...)` (line
  464) — never even repoints, let alone removes.
- `gti_index`: `populate_gti_index` (line 478) only adds edges;
  there is **no** `evict_gti_for_fqns` call, so
  `find_implementors` / type hierarchy keep serving children that
  no longer extend the parent.
- `evict_methods_for_fqns` (line 475) is called with the **new**
  FQN set, so `method_store` entries of removed classes linger.

Contrast with `update_ast_inner`, which computes `old_fqns` and
evicts all four correctly (`src/parser/ast_update.rs:526-531,
583-584`). The watched-file path (`reindex_files_batch`) also
evicts correctly — but only fires for files that produce watcher
events; a re-parse through this code path (e.g. re-opening after
`did_close` cleared `uri_classes_index`, phar refresh, or a
vendor change the client doesn't watch) leaves ghosts.

**Fix:** when `was_already_parsed` is true, diff against the
previous `uri_classes_index` entry (it is still available at line
436 before the overwrite) and evict removed FQNs from
`fqn_class_index`, `fqn_uri_index`, `gti_index`, and
`method_store`, mirroring `update_ast_inner`.


## B26. A panic during parse/extraction permanently poisons the URI via `parse_inflight`

**Severity: Medium (file never resolvable again + 200 ms stall per lookup) · Confirmed paths, low-probability trigger**

`parse_and_cache_file` (`src/resolution.rs:259-294`) inserts the
URI into `parse_inflight`, does the work, then removes it — with
no drop guard. If the work unwinds, the `remove` at line 280/293
never runs. From then on, **every** `parse_and_cache_file` call
for that URI takes the `wait_for_cached_result` path
(`resolution.rs:299-310`): a 200 × 1 ms spin that then returns
stale-or-`None` — the file can never be (re)parsed until server
restart, and each attempt burns 200 ms on a blocking thread.

The panic surface is real: `with_parsed_program`
(`src/parser/mod.rs:855-919`) wraps only the **slow path** in
`catch_unwind` (line 913). The thread-local parse-cache fast path
runs both the mago parse (lines 877-894) and the extraction
closure (lines 896-909) **outside** any `catch_unwind` — so with
a warm parse cache, a parser or extraction panic escapes,
contradicting the function's own "a parser panic doesn't crash
the LSP server" contract. Outer layers like the completion
handler's `catch_unwind` (`src/completion/handler.rs:1010`) then
swallow the panic, so the server keeps running with the URI stuck
in `parse_inflight` and nothing in the log but one panic line.

**Fix:** two independent hardenings, both worth doing:

1. Hold the `parse_inflight` entry in an RAII guard whose `Drop`
   removes the URI, so unwinding cleans up.
2. Wrap the fast path of `with_parsed_program` in `catch_unwind`
   like the slow path (evicting the poisoned parse-cache entry on
   panic so the next call re-parses).


## B27. String literal type comparison is quote-style sensitive

**Severity: Low (false-positive argument diagnostic) · Confirmed**

`literal_is_subtype_of` (`src/php_type.rs:3297-3369`) compares two
`PhpType::Literal` string values with plain `lit == other_lit`. Both
the argument-narrowing path
(`src/diagnostics/type_errors.rs`, argument literal narrowing) and
the docblock literal-type parser
(`src/php_type.rs:4095`, `ast::Type::LiteralString`) build the
`Literal` payload from the **raw source text including quote
characters**, so a double-quoted argument literal never equals a
single-quoted docblock literal even when their unquoted contents are
identical.

**Trigger:**

```php
/** @param 'asc'|'desc' $direction */
function orderBy(string $column, string $direction): void {}

function test(): void {
    orderBy('id', "desc"); // flagged: "desc" (double-quoted) != 'desc' (single-quoted)
}
```

Single-quoted usage (`orderBy('id', 'desc')`) is unaffected, since
both sides happen to agree on quote style in that case.

**Fix:** compare the two literals by their unquoted content instead
of raw source text. Both `LiteralString` (source) and
`LiteralStringType` (docblock) already carry a parsed, unquoted
`value` field alongside `raw` — normalise through that (or strip
quotes consistently) before the equality check in
`literal_is_subtype_of`, and make sure the `PhpType::Literal`
constructors that currently pass through `raw` for strings do the
same.
