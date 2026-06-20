# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.


## B16. PDOStatement fetch mode-dependent return types

**Blocked on:** [phpstorm-stubs#1882](https://github.com/JetBrains/phpstorm-stubs/pull/1882)

`PDOStatement::fetch()` and `PDOStatement::fetchAll()` return
different types depending on the fetch mode constant passed as
the first argument. Once the upstream PR is merged and we update
our stubs, the existing conditional return type support should
handle this automatically.

**Tests:** Assertion lines were removed from
`tests/psalm_assertions/method_call.php` (out of scope until
upstream stubs land).


## B42. Conditionally-defined classes are not resolved

**Severity: Low.** A class, trait, or enum defined inside a top-level
`if`/`else` block (a common polyfill/compat pattern, e.g.
`if (PHP_VERSION_ID < 80000) { class Foo {…} } else { class Foo {…} }`)
is not picked up by current-class detection, variable scope detection,
or member completion. Inside such a class's methods, `$this->`
completion, variable resolution, and `$this->prop` assignment narrowing
all fail because the surrounding pipeline never associates the cursor
with the conditionally-defined class. (The assignment-scan walker itself
now descends into `else`/`elseif` branches, but the gap upstream means
the fix is not yet observable for this pattern.)

**Fix:** Teach the class indexer / current-class and scope detection to
descend into control-flow blocks when locating the class-like declaration
that encloses the cursor.

