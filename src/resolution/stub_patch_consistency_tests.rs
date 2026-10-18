//! A constant lookup routes its stub source through `update_ast`
//! (under a `phpantom-stub://const/…` URI).  The same stub file
//! often also defines functions — e.g. `ARRAY_FILTER_USE_BOTH`
//! lives in the stub chunk that declares `array_map`.  Functions
//! registered on that path must carry the same stub patches as the
//! dedicated stub-function loader; otherwise the unpatched variant
//! overwrites (or preempts) the patched one and `array_map` loses
//! its `@template TValue`, silently breaking closure parameter
//! inference for the rest of the session.  Which registration runs
//! first depends on file analysis order, so the breakage was
//! nondeterministic in full-project runs.

use crate::Backend;

#[test]
fn constant_lookup_before_function_lookup_keeps_stub_patches() {
    let backend = Backend::new_test_with_full_stubs();

    assert!(
        backend
            .lookup_global_constant("ARRAY_FILTER_USE_BOTH")
            .is_some(),
        "ARRAY_FILTER_USE_BOTH must resolve from the embedded stubs"
    );

    let fi = backend
        .find_or_load_function(&["array_map"])
        .expect("array_map must resolve from the embedded stubs");
    assert!(
        !fi.template_params.is_empty(),
        "array_map must keep its @template stub patch when the \
         constant stub that defines it was parsed first"
    );
}

#[test]
fn constant_lookup_after_function_lookup_keeps_stub_patches() {
    let backend = Backend::new_test_with_full_stubs();

    assert!(
        backend
            .find_or_load_function(&["array_map"])
            .is_some_and(|fi| !fi.template_params.is_empty()),
        "array_map must resolve with its @template stub patch"
    );

    assert!(
        backend
            .lookup_global_constant("ARRAY_FILTER_USE_BOTH")
            .is_some(),
        "ARRAY_FILTER_USE_BOTH must resolve from the embedded stubs"
    );

    let fi = backend
        .find_or_load_function(&["array_map"])
        .expect("array_map must still resolve");
    assert!(
        !fi.template_params.is_empty(),
        "array_map must keep its @template stub patch after a \
         constant lookup re-registered the same stub file's functions"
    );
}
