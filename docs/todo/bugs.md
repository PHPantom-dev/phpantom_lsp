# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## By-reference closure capture propagation ignores named arguments

When a closure is passed as a *named* argument to a call
(`c(callback: function () use (&$foo) { ... })`), the by-reference
capture propagation matches the argument to a parameter by its
position in the argument list rather than by the argument's name.
If the named argument is not in its natural position, the wrong
parameter's `@param-immediately-invoked-callable` /
`@param-later-invoked-callable` tag is consulted, so the outer
variable's type is either propagated when it should not be or left
stale when it should update. Match named arguments to parameters by
name before deciding whether the callable is immediately invoked.
