/// Request-scoped memos for call resolution, plus body-return-type
/// inference.
///
/// Bundles the callable-target cache and the body-return-inference memo
/// behind [`activate_type_engine_caches`], which every request entry
/// point activates so the two memos live exactly as long as one request
/// (or one file's diagnostic pass).
///
/// The facilities that need project-wide state — inferring a return type
/// from a method body, the model behind an auth guard, the validation
/// rules describing a request — read the `Backend` off the resolution
/// context instead, so a caller cannot forget to install them.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::php_type::PhpType;
use crate::types::*;

// ─── Thread-local memos ─────────────────────────────────────────────────────

/// What a body-return inference was asked: the method, plus the argument
/// types the call site decided its parameters with.
///
/// The arguments are part of the key because they change the answer — a
/// method whose return type is decided by what it was handed gives a
/// different type to every call site that hands it something different.
type BodyInferKey = (Atom, Atom, Box<[PhpType]>);

/// Memoized body-return-inference results.
type BodyInferMemo = HashMap<BodyInferKey, Option<PhpType>>;

/// The body-return inferences currently walking a body, each with the
/// argument types its call site decided, keyed by `(declaring FQN,
/// method)`.
type CallSiteArgFrames = Vec<((Atom, Atom), Box<[PhpType]>)>;

thread_local! {
    /// When `Some`, `resolve_instance_method_callable` caches results
    /// by `"FQN::method_lower"`.  Activated by
    /// [`activate_type_engine_caches`], cleared on guard drop.
    pub(super) static CALLABLE_TARGET_CACHE: RefCell<Option<HashMap<String, Option<ResolvedCallableTarget>>>> =
        const { RefCell::new(None) };

    /// Re-entry guard for body return inference.  Tracks
    /// `(FQN, method)` keys currently being inferred to prevent
    /// infinite recursion when a method body references another
    /// method that also lacks a return type.
    static BODY_INFER_VISITED: RefCell<HashSet<(Atom, Atom)>> =
        RefCell::new(HashSet::new());

    /// When `Some`, memoizes completed body return inference results by
    /// `(FQN, method)`.  Cleared when the owning guard drops, so the memo
    /// lives exactly as long as one request / one file's diagnostic pass.
    ///
    /// Without this memo, every call site that needs a method's inferred
    /// return type re-walks the entire method body.  On large legacy
    /// files where most methods lack declared return types, the repeated
    /// walks compound into a multi-minute blowup (each walk itself
    /// triggers inference for the callees it contains).
    static BODY_INFER_MEMO: RefCell<Option<BodyInferMemo>> =
        const { RefCell::new(None) };

    /// Current nesting depth of body return inference.  Caps the
    /// chain length so that A→B→C→D… doesn't trigger unbounded
    /// sequential body scans.  Each scan runs `resolve_variable_types`
    /// (forward walker + full resolution), so even non-recursive
    /// chains are expensive.
    static BODY_INFER_DEPTH: Cell<u8> = const { Cell::new(0) };

    /// The call-site argument types of every body-return inference
    /// currently walking a body, so the walker can seed that body's
    /// parameters with what the call actually passed rather than with
    /// what the signature declares.
    ///
    /// Keyed by the class that *declares* the method, which is the class
    /// the walker reports as its own: a method inherited from a trait is
    /// read out of the trait's file, so keying by the receiver would
    /// never match.  Bounded by [`MAX_BODY_INFER_DEPTH`], so a linear
    /// scan is cheaper than hashing.
    static BODY_INFER_ARGS: RefCell<CallSiteArgFrames> =
        const { RefCell::new(Vec::new()) };
}

/// Activate the thread-local callable target cache.
///
/// While the returned guard is alive, `resolve_instance_method_callable`
/// caches callable target resolutions by `"FQN::method_lower"` so
/// that the same method on the same class is resolved at most once per
/// diagnostic pass, regardless of how many different chain expressions
/// lead to it.
/// The guard [`with_callable_target_cache`] hands back.
type CallableTargetCacheGuard =
    crate::type_engine::MemoGuard<HashMap<String, Option<ResolvedCallableTarget>>>;

fn with_callable_target_cache() -> CallableTargetCacheGuard {
    crate::type_engine::activate_memo(&CALLABLE_TARGET_CACHE)
}

// ── Body return type inference ──────────────────────────────────────────────

/// Activate the body-return-inference memo for the current thread.
/// The guard [`with_body_infer_memo`] hands back.
type BodyInferMemoGuard = crate::type_engine::MemoGuard<BodyInferMemo>;

fn with_body_infer_memo() -> BodyInferMemoGuard {
    crate::type_engine::activate_memo(&BODY_INFER_MEMO)
}

// ── Call-site argument frames ───────────────────────────────────────────────

/// Pops the frame it pushed, so an inference that unwinds cannot leave a
/// call's argument types seeding an unrelated body.
struct CallSiteArgsGuard;

impl Drop for CallSiteArgsGuard {
    fn drop(&mut self) {
        BODY_INFER_ARGS.with(|cell| {
            cell.borrow_mut().pop();
        });
    }
}

/// Publish `args` as the argument types for the body of `class_fqn::method`
/// until the returned guard drops.
fn push_call_site_args(class_fqn: &str, method: Atom, args: &[PhpType]) -> CallSiteArgsGuard {
    BODY_INFER_ARGS.with(|cell| {
        cell.borrow_mut()
            .push(((atom(class_fqn), method), args.into()));
    });
    CallSiteArgsGuard
}

/// The argument types the call site passed to `class_fqn::method_name`,
/// when its body is the one currently being walked for a return type.
///
/// Indexed by declared parameter, so entry `i` is what parameter `i`
/// received; a parameter no argument decided reads back as `mixed`.
pub(crate) fn call_site_param_types(class_fqn: &str, method_name: &str) -> Option<Box<[PhpType]>> {
    let class_fqn = class_fqn.trim_start_matches('\\');
    BODY_INFER_ARGS.with(|cell| {
        cell.borrow()
            .iter()
            .find(|((fqn, method), _)| {
                fqn.trim_start_matches('\\').eq_ignore_ascii_case(class_fqn)
                    && method.eq_ignore_ascii_case(method_name)
            })
            .map(|(_, args)| args.clone())
    })
}

/// Whether a body-return inference is already walking a body.
///
/// Reading a method body where a return type is *declared* only pays for
/// itself while an outer inference is running, because that is the only
/// time a refinement the arguments decide can still reach a caller.
pub(crate) fn body_inference_in_progress() -> bool {
    BODY_INFER_DEPTH.with(|cell| cell.get()) > 0
}

/// Maximum nesting depth for body return inference chains.
///
/// A→B→C is 3 levels deep.  Real PHP code rarely has long chains of
/// untyped methods calling each other, and each level runs a full
/// forward-walk body scan, so keeping this low avoids expensive
/// sequential scans on pathological code.
const MAX_BODY_INFER_DEPTH: u8 = 3;

/// Infer a method's return type by scanning its body.
///
/// Called when `resolve_method_return_types_with_args` encounters a real
/// (non-virtual, non-stub) method that has no declared return type and
/// no `@return` docblock.  Returns `None` when the method is already
/// being inferred (re-entry), when the chain is too deep, or when
/// inference itself produces no useful result.
///
/// `call_args` are the types the call site decided the method's
/// parameters with, indexed by declared parameter (`mixed` where the
/// call decided nothing).  They seed the body's scope, so a method whose
/// result depends on what it was handed answers per call site rather
/// than once for the whole project.  Pass `&[]` from a caller that has
/// no particular call site in mind.
pub(crate) fn try_infer_body_return_type(
    backend: &Backend,
    class_fqn: &str,
    method: &MethodInfo,
    call_args: &[PhpType],
) -> Option<PhpType> {
    // Build the memo / re-entry key from an interned `(FQN, method)`
    // pair.  Both halves come from bounded symbol-name spaces and are
    // already interned, so the key adds no new entries to the global
    // interner (a joined `"FQN::method"` string would leak one entry per
    // distinct pair for the process lifetime).  The memo keeps the
    // argument types too, since they are what the answer depends on; the
    // re-entry guard is keyed without them on purpose, so a method that
    // calls itself with a different argument type on every hop still
    // re-enters the same key and stops at the depth cap.
    let visited_key = (atom(class_fqn), method.name);
    let memo_key = (
        visited_key.0,
        visited_key.1,
        Box::<[PhpType]>::from(call_args),
    );

    crate::type_engine::memoized_bounded_inference(
        &BODY_INFER_MEMO,
        &BODY_INFER_VISITED,
        &BODY_INFER_DEPTH,
        MAX_BODY_INFER_DEPTH,
        memo_key,
        visited_key,
        || {
            // Filter out `mixed` and `void` — these are not useful as
            // inferred return types for completion/hover.
            infer_body_return_type(backend, class_fqn, method, call_args)
                .filter(|t| !t.is_mixed() && !t.is_void())
        },
    )
}

/// Scan a method body for its return type.
///
/// Delegates to [`Backend::infer_return_type_for_function`], which has
/// the full resolution infrastructure (use maps, namespace resolution,
/// function loader, class loader with stubs/class index/PSR-4).
fn infer_body_return_type(
    backend: &Backend,
    class_fqn: &str,
    method: &MethodInfo,
    call_args: &[PhpType],
) -> Option<PhpType> {
    // The method may have been inherited from a trait or parent class
    // declared in a *different* file.  `method.name_offset` is relative
    // to that declaring file, so reading the receiver's own file at
    // that offset would land on the wrong location.  Resolve the class
    // that actually declares the method and read *its* file.
    let declaring_class = backend.find_or_load_class(class_fqn).map(|receiver| {
        let loader = |name: &str| backend.find_or_load_class(name);
        crate::hover::find_declaring_class(
            &receiver,
            &method.name,
            &crate::hover::MemberKindForOrigin::Method,
            &loader,
        )
    });
    let file_uri = declaring_class
        .as_ref()
        .and_then(|decl| {
            backend
                .symbols
                .fqn_uri_index
                .read()
                .get(&decl.fqn())
                .cloned()
        })
        // Fall back to the receiver's own file when the declaring
        // class could not be located (e.g. only known via the AST).
        .or_else(|| backend.symbols.fqn_uri_index.read().get(class_fqn).cloned())?;

    let content = backend.get_file_content(&file_uri)?;

    // Convert method name_offset to a 0-based line number.  The offset
    // was recorded against the file as it was parsed, which need not be
    // the content read back here, so count over the bytes rather than
    // slicing the string: a `\n` byte never appears inside a multi-byte
    // character, but an offset landing mid-character would panic a
    // string slice.
    let offset = method.name_offset as usize;
    let bytes = content.as_bytes();
    if offset >= bytes.len() {
        return None;
    }
    let func_line = bytes[..offset].iter().filter(|&&b| b == b'\n').count();

    // Walk backwards from the method name to find the function
    // keyword line (the declaration may start on an earlier line).
    // infer_return_type_for_function expects the line of the
    // `function` keyword.
    let lines: Vec<&str> = content.lines().collect();
    let mut decl_line = func_line;
    for i in (0..=func_line).rev() {
        let trimmed = lines.get(i).map(|l| l.trim()).unwrap_or("");
        if trimmed.contains("function ")
            || trimmed.contains("function(")
            || trimmed.starts_with("function")
        {
            decl_line = i;
            break;
        }
        if trimmed.ends_with('}') || trimmed.ends_with(';') {
            break;
        }
    }

    // Publish the call site's argument types against the class the
    // walker will report as its own while it reads this body, so the
    // parameters seed from what was passed instead of from the
    // signature.  The two caches that answer "what does this expression
    // resolve to at this offset" step aside for the same span: seeded
    // parameters make a different scope out of the very same offsets,
    // and neither cache keys on which call site asked.
    let _seeded = (!call_args.is_empty()).then(|| {
        let declaring_fqn = declaring_class
            .as_ref()
            .map_or_else(|| class_fqn.to_string(), |decl| decl.fqn().to_string());
        (
            push_call_site_args(&declaring_fqn, method.name, call_args),
            crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
            crate::type_engine::resolver::with_isolated_chain_cache(),
        )
    });

    let result = backend.infer_return_type_for_function(&file_uri, &content, decl_line, true)?;

    // A declaration the caller can fall back on is only worth replacing
    // with a reading the body actually agrees on. `mixed` is the one
    // declaration this is reached with (it says nothing, so the body is
    // read for something better), and a body whose `return` statements
    // disagree has not said anything better: the union is this walk's
    // reconstruction of the control flow, complete only as far as the
    // branch analysis behind it goes. `processArgument()` in PHPStan's own
    // source returns a schema, an array, or its own `mixed` argument, and
    // reading it as `Schema|Statement` claimed the array could not happen
    // — which then rejected the caller that hands the result straight to a
    // `Schema` parameter. `mixed` is the sound answer there.
    //
    // Returns that agree leave nothing to reconstruct: one `return` (or
    // several with the same type) resolves to whatever that expression is,
    // nullable or generic included, and that is a real narrowing of a
    // declaration that promised nothing.
    if method.return_type.is_some() && !result.returns_agree {
        return None;
    }

    // Prefer the effective type (richer, e.g. `list<string>`)
    // over the native type (e.g. `array`).
    let inferred = result.effective.unwrap_or(result.native);

    // A method whose declaring trait/class has `@template` parameters can
    // have its body infer a bare, unsubstituted parameter name (e.g.
    // `@param T $t` / `return $t;` infers literal `T`) when the caller's
    // merge-time substitution — which resolves `T` to a concrete type or
    // erases it to `mixed` for the *using* class — never touches the
    // trait's own source file that this re-reads.  A raw template name is
    // no more informative than `mixed` and would otherwise leak to the
    // user as if it were a real type, so reject it the same way.
    if let Some(decl) = declaring_class.as_ref() {
        let mut template_params: Vec<String> = method
            .template_params
            .iter()
            .map(|p| p.to_string())
            .collect();
        template_params.extend(decl.template_params.iter().map(|p| p.to_string()));
        if inferred.references_any_template_param(&template_params) {
            return None;
        }
    }

    Some(inferred)
}

// ── Bundled request-scope activation ────────────────────────────────────────

/// The request-scoped memos the type engine keeps, activated as a unit.
///
/// All four are pure memos: without them the type engine reaches the
/// same answers, just by re-doing work it has already done for this
/// file.
/// They are activated at the chokepoints every request passes through so
/// no feature pays for a cold cache the one next to it already warmed.
pub(crate) struct TypeEngineCaches {
    _callable_target: CallableTargetCacheGuard,
    _body_infer: BodyInferMemoGuard,
    _out_type: super::out_param::OutTypeMemoGuard,
    _var_type: crate::type_engine::variable::resolution::VarTypeMemoGuard,
}

/// Activate every request-scoped type-engine memo for the current
/// thread, returning one RAII guard that clears them all on drop.
///
/// Called from [`Backend::with_file_content`], which every LSP handler
/// goes through, and from the few entry points that fetch their own file
/// content (completion, completion/code-action resolve, the diagnostic
/// pass, and the `analyse` CLI).  Nested activation is a no-op, so an
/// inner pass cannot clobber the memos an outer one installed.
///
/// [`Backend::with_file_content`]: Backend::with_file_content
pub(crate) fn activate_type_engine_caches() -> TypeEngineCaches {
    TypeEngineCaches {
        _callable_target: with_callable_target_cache(),
        _body_infer: with_body_infer_memo(),
        _out_type: super::out_param::with_out_type_memo(),
        _var_type: crate::type_engine::variable::resolution::with_var_type_memo(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{make_backend, make_method};

    /// A recorded `name_offset` that no longer lines up with the content
    /// read back (the file changed, or the URI resolved elsewhere) can
    /// land inside a multi-byte character.  Inference must give up
    /// quietly instead of panicking and taking the file's whole
    /// diagnostic pass with it.
    #[test]
    fn offset_inside_multibyte_char_does_not_panic() {
        let backend = make_backend();
        let uri = "file:///app/Box.php";
        let content = "<?php\n// ──────\nclass Box\n{\n    public function size()\n    {\n        return 1;\n    }\n}\n";
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));
        backend
            .symbols
            .fqn_uri_index
            .write()
            .insert("Box".to_string(), uri.to_string());

        // The first `─` occupies bytes 9..12, so byte 10 is not a
        // character boundary.
        let mut method = make_method("size", None);
        method.name_offset = 10;

        assert!(try_infer_body_return_type(&backend, "Box", &method, &[]).is_none());
    }
}
