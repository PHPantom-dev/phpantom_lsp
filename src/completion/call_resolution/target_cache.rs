/// Thread-local caches and RAII activation guards for call resolution.
///
/// Bundles the callable-target cache, body-return-type inference memo,
/// and guard-aware auth user resolver, plus the `Backend` methods that
/// activate them at request entry points.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::Backend;
use crate::php_type::PhpType;
use crate::types::*;

// ─── Thread-local caches and body return inference ──────────────────────────

/// Closure type for body return type inference.
///
/// Takes `(class_fqn, &MethodInfo)` and returns `Some(PhpType)` when the
/// method body can be scanned for return statements.
type BodyReturnInferrerFn = Box<dyn Fn(&str, &MethodInfo) -> Option<PhpType>>;

/// Closure type for guard-aware auth user model resolution.
///
/// Takes an optional guard name (`None` for the default guard) and
/// returns the model type configured for that guard in
/// `config/auth.php`, or `None` when no concrete model can be pinned
/// down.
type AuthUserResolverFn = Box<dyn Fn(Option<&str>) -> Option<PhpType>>;

thread_local! {
    /// When `Some`, `resolve_instance_method_callable` caches results
    /// by `"FQN::method_lower"`.  Activated by
    /// [`with_callable_target_cache`], cleared on guard drop.
    pub(super) static CALLABLE_TARGET_CACHE: RefCell<Option<HashMap<String, Option<ResolvedCallableTarget>>>> =
        const { RefCell::new(None) };

    /// When `Some`, methods without a declared return type can have
    /// their return type inferred by scanning the method body.
    ///
    /// The closure takes `(class_fqn, &MethodInfo)` and returns
    /// `Some(PhpType)` when inference succeeds.  Set up by
    /// [`with_body_return_inferrer`] at request entry points that
    /// have access to `Backend`.
    static BODY_RETURN_INFERRER: RefCell<Option<BodyReturnInferrerFn>> =
        const { RefCell::new(None) };

    /// Re-entry guard for body return inference.  Tracks
    /// `"FQN::method"` keys currently being inferred to prevent
    /// infinite recursion when a method body references another
    /// method that also lacks a return type.
    static BODY_INFER_VISITED: RefCell<HashSet<String>> =
        RefCell::new(HashSet::new());

    /// When `Some`, memoizes completed body return inference results by
    /// `"FQN::method"`.  Activated together with [`BODY_RETURN_INFERRER`]
    /// and cleared when the owning guard drops, so the memo lives exactly
    /// as long as one request / one file's diagnostic pass.
    ///
    /// Without this memo, every call site that needs a method's inferred
    /// return type re-walks the entire method body.  On large legacy
    /// files where most methods lack declared return types, the repeated
    /// walks compound into a multi-minute blowup (each walk itself
    /// triggers inference for the callees it contains).
    static BODY_INFER_MEMO: RefCell<Option<HashMap<String, Option<PhpType>>>> =
        const { RefCell::new(None) };

    /// Current nesting depth of body return inference.  Caps the
    /// chain length so that A→B→C→D… doesn't trigger unbounded
    /// sequential body scans.  Each scan runs `resolve_variable_types`
    /// (forward walker + full resolution), so even non-recursive
    /// chains are expensive.
    static BODY_INFER_DEPTH: Cell<u8> = const { Cell::new(0) };

    /// When `Some`, `user()` calls on an auth entry point (a `Guard` or
    /// `Request` subtype) resolve the model configured in
    /// `config/auth.php` for the guard named at the call site.
    ///
    /// The closure takes an optional guard name (from `auth('admin')`,
    /// `Auth::guard('admin')`, `->guard('admin')`, or
    /// `$request->user('admin')`; `None` for the default guard) and
    /// returns the resolved model type.  Set up by
    /// [`Backend::activate_auth_user_resolver`] at request entry points
    /// that have access to `Backend` (which holds the config and class
    /// index the traversal needs).
    pub(super) static AUTH_USER_RESOLVER: RefCell<Option<AuthUserResolverFn>> =
        const { RefCell::new(None) };

}

pub(crate) struct CallableTargetCacheGuard {
    owns: bool,
}

impl Drop for CallableTargetCacheGuard {
    fn drop(&mut self) {
        if self.owns {
            CALLABLE_TARGET_CACHE.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// Activate the thread-local callable target cache.
///
/// While the returned guard is alive, `resolve_instance_method_callable`
/// caches callable target resolutions by `"FQN::method_lower"` so
/// that the same method on the same class is resolved at most once per
/// diagnostic pass, regardless of how many different chain expressions
/// lead to it.
pub(crate) fn with_callable_target_cache() -> CallableTargetCacheGuard {
    let already_active = CALLABLE_TARGET_CACHE.with(|cell| cell.borrow().is_some());
    if already_active {
        return CallableTargetCacheGuard { owns: false };
    }
    CALLABLE_TARGET_CACHE.with(|cell| {
        *cell.borrow_mut() = Some(HashMap::new());
    });
    CallableTargetCacheGuard { owns: true }
}

// ── Body return type inference ──────────────────────────────────────────────

/// RAII guard that clears [`BODY_RETURN_INFERRER`] on drop.
pub(crate) struct BodyReturnInferrerGuard {
    owns: bool,
}

impl Drop for BodyReturnInferrerGuard {
    fn drop(&mut self) {
        if self.owns {
            BODY_RETURN_INFERRER.with(|cell| {
                *cell.borrow_mut() = None;
            });
            BODY_INFER_MEMO.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// Activate body return type inference for the current thread.
///
/// The provided closure is called when `resolve_method_return_types_with_args`
/// encounters a real (non-virtual, non-stub) method that has no declared
/// return type and no `@return` docblock.  It receives the owning class's
/// FQN and the `MethodInfo`, and should return `Some(PhpType)` when the
/// method body can be scanned for return statements.
///
/// Returns an RAII guard that clears the inferrer on drop.
pub(crate) fn with_body_return_inferrer(inferrer: BodyReturnInferrerFn) -> BodyReturnInferrerGuard {
    let already_active = BODY_RETURN_INFERRER.with(|cell| cell.borrow().is_some());
    if already_active {
        return BodyReturnInferrerGuard { owns: false };
    }
    BODY_RETURN_INFERRER.with(|cell| {
        *cell.borrow_mut() = Some(inferrer);
    });
    BODY_INFER_MEMO.with(|cell| {
        *cell.borrow_mut() = Some(HashMap::new());
    });
    BodyReturnInferrerGuard { owns: true }
}

/// Try to infer a method's return type from its body using the
/// thread-local [`BODY_RETURN_INFERRER`].
///
/// Returns `None` when no inferrer is active, when the method is
/// already being inferred (re-entry), or when inference itself
/// produces no result.
/// Maximum nesting depth for body return inference chains.
///
/// A→B→C is 3 levels deep.  Real PHP code rarely has long chains of
/// untyped methods calling each other, and each level runs a full
/// forward-walk body scan, so keeping this low avoids expensive
/// sequential scans on pathological code.
const MAX_BODY_INFER_DEPTH: u8 = 3;

pub(crate) fn try_infer_body_return_type(class_fqn: &str, method: &MethodInfo) -> Option<PhpType> {
    // Build the memo / re-entry key.
    let key = format!("{}::{}", class_fqn, method.name);

    // Serve a memoized result from an earlier completed inference in
    // this request.  Checked before the depth cap so that deep call
    // chains still benefit from results computed at shallower depths.
    let memoized =
        BODY_INFER_MEMO.with(|cell| cell.borrow().as_ref().and_then(|m| m.get(&key).cloned()));
    if let Some(cached) = memoized {
        return cached;
    }

    // Depth cap: avoid long chains of sequential body scans.
    let depth = BODY_INFER_DEPTH.with(|cell| cell.get());
    if depth >= MAX_BODY_INFER_DEPTH {
        return None;
    }

    // Check + insert into the visited set (re-entry guard).
    let already_visiting = BODY_INFER_VISITED.with(|cell| {
        let mut set = cell.borrow_mut();
        !set.insert(key.clone())
    });
    if already_visiting {
        return None;
    }

    BODY_INFER_DEPTH.with(|cell| cell.set(depth + 1));

    let result = BODY_RETURN_INFERRER.with(|cell| {
        let borrow = cell.borrow();
        let inferrer = borrow.as_ref()?;
        let inferred = inferrer(class_fqn, method);
        // Filter out `mixed` and `void` — these are not useful as
        // inferred return types for completion/hover.
        inferred.filter(|t| !t.is_mixed() && !t.is_void())
    });

    // Restore depth and remove from visited set so the same method
    // can be inferred again from a different call chain.
    BODY_INFER_DEPTH.with(|cell| cell.set(depth));
    BODY_INFER_VISITED.with(|cell| {
        cell.borrow_mut().remove(&key);
    });

    // Memoize only completed inferrer runs (the depth-cap and re-entry
    // short-circuits above return early and are never stored, so a
    // cut-off `None` cannot shadow a later real result).  A result
    // computed mid-chain may itself have had its nested inference
    // depth-capped; serving it to shallower callers trades a sliver of
    // precision for never walking the same body twice in one request.
    BODY_INFER_MEMO.with(|cell| {
        if let Some(memo) = cell.borrow_mut().as_mut() {
            memo.insert(key, result.clone());
        }
    });

    result
}

// ── Guard-aware auth user model resolution ──────────────────────────────────

/// RAII guard that clears [`AUTH_USER_RESOLVER`] on drop.
pub(crate) struct AuthUserResolverGuard {
    owns: bool,
}

impl Drop for AuthUserResolverGuard {
    fn drop(&mut self) {
        if self.owns {
            AUTH_USER_RESOLVER.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// Activate guard-aware auth user model resolution for the current thread.
///
/// The provided closure maps an optional guard name to the model type
/// configured for that guard in `config/auth.php`.  Returns an RAII
/// guard that clears the resolver on drop.
pub(crate) fn with_auth_user_resolver(resolver: AuthUserResolverFn) -> AuthUserResolverGuard {
    let already_active = AUTH_USER_RESOLVER.with(|cell| cell.borrow().is_some());
    if already_active {
        return AuthUserResolverGuard { owns: false };
    }
    AUTH_USER_RESOLVER.with(|cell| {
        *cell.borrow_mut() = Some(resolver);
    });
    AuthUserResolverGuard { owns: true }
}

impl Backend {
    /// Build and activate the thread-local guard-aware auth user model
    /// resolver.
    ///
    /// Returns an RAII guard that deactivates the resolver on drop.
    /// Call this alongside [`activate_body_return_inferrer`] at request
    /// entry points so that `user()` calls resolve the model configured
    /// for the guard named at the call site.
    ///
    /// [`activate_body_return_inferrer`]: Backend::activate_body_return_inferrer
    pub(crate) fn activate_auth_user_resolver(&self) -> AuthUserResolverGuard {
        let backend = self.clone_for_diagnostic_worker();
        let resolver = move |guard: Option<&str>| -> Option<PhpType> {
            let loader = |name: &str| backend.find_or_load_class(name);
            crate::virtual_members::laravel::resolve_auth_user_type(&backend, guard, &loader)
        };
        with_auth_user_resolver(Box::new(resolver))
    }

    /// Build and activate the thread-local body return type inferrer.
    ///
    /// Returns an RAII guard that deactivates the inferrer on drop.
    /// Call this at the start of completion, hover, and diagnostic
    /// request handlers so that methods without declared return types
    /// can have their return type inferred from the method body.
    ///
    /// Internally clones the `Backend` (all fields are `Arc`-wrapped,
    /// so this is cheap) and delegates to
    /// [`Backend::infer_return_type_for_function`] which has the full
    /// resolution infrastructure (use maps, namespace resolution,
    /// function loader, class loader with stubs/class index/PSR-4).
    pub(crate) fn activate_body_return_inferrer(&self) -> BodyReturnInferrerGuard {
        let backend = self.clone_for_diagnostic_worker();

        let inferrer = move |class_fqn: &str, method: &MethodInfo| -> Option<PhpType> {
            // The method may have been inherited from a trait or parent class
            // declared in a *different* file.  `method.name_offset` is relative
            // to that declaring file, so reading the receiver's own file at
            // that offset would land on the wrong location.  Resolve the class
            // that actually declares the method and read *its* file.
            let file_uri = backend
                .find_or_load_class(class_fqn)
                .map(|receiver| {
                    let loader = |name: &str| backend.find_or_load_class(name);
                    crate::hover::find_declaring_class(
                        &receiver,
                        &method.name,
                        &crate::hover::MemberKindForOrigin::Method,
                        &loader,
                    )
                })
                .and_then(|decl| backend.fqn_uri_index.read().get(&decl.fqn()).cloned())
                // Fall back to the receiver's own file when the declaring
                // class could not be located (e.g. only known via the AST).
                .or_else(|| backend.fqn_uri_index.read().get(class_fqn).cloned())?;

            // Read the file content.
            let content = backend.get_file_content(&file_uri)?;

            // Convert method name_offset to a 0-based line number.
            let offset = method.name_offset as usize;
            if offset >= content.len() {
                return None;
            }
            let func_line = content[..offset].matches('\n').count();

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

            let result =
                backend.infer_return_type_for_function(&file_uri, &content, decl_line, true)?;

            // Prefer the effective type (richer, e.g. `list<string>`)
            // over the native type (e.g. `array`).
            Some(result.effective.unwrap_or(result.native))
        };

        with_body_return_inferrer(Box::new(inferrer))
    }
}
