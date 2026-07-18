# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## `ReflectionClass::newInstanceArgs()` returns the class-string, not the instance

`new ReflectionClass($class)` where `$class` is
`class-string<AbstractASTNode|ASTAnonymousClass>` binds the
`ReflectionClass<T>` template parameter to the *class-string* rather than
unwrapping it to the object type `T`. As a result `$reflection->newInstanceArgs(...)`
(and `newInstance()`) resolve to `class-string<...>|null` instead of the
instantiated object type. Surfaced by the return-type mismatch diagnostic
on `pdepend`'s `ASTNodeTestCase::createNodeInstance()`, which returns
`$reflection->newInstanceArgs(...)` from a method declared
`: AbstractASTNode|ASTAnonymousClass`. The fix is to unwrap `class-string<X>`
to `X` when binding the `ReflectionClass` constructor argument to the class
template parameter.
