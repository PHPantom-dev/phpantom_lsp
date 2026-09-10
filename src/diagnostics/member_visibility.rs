//! Access checks for `private` and `protected` class members.
//!
//! A member that exists is not automatically a member you may touch.
//! PHP resolves `$obj->name` to a declaration first and enforces the
//! declaration's visibility second, so a private property reached from
//! outside its class is a fatal `Error` at runtime:
//!
//! ```php
//! class Account {
//!     private string $pin = '0000';
//! }
//!
//! $account = new Account();
//! echo $account->pin; // Cannot access private property Account::$pin
//! ```
//!
//! The check runs inside the unknown-member pass rather than in a pass
//! of its own: that walk has already resolved the subject expression to
//! a class, which is the expensive half of the work.
//!
//! ## Where the effective member comes from
//!
//! The visibility that counts is the one the member has *after* the
//! class is assembled, not the one the original declaration was written
//! with.  A trait `use` clause can rewrite it — `use T { run as private; }`
//! makes a public trait method private in its host, an alias gives one
//! method two visibilities, and `insteadof` decides which of two
//! competing declarations survives at all.  The inheritance merge in
//! [`crate::inheritance`] already applies every one of those rules, so
//! this check reads the merged class and inherits that correctness
//! rather than re-deriving it.
//!
//! ## Why the merged class is not enough on its own
//!
//! The merged class records what a class *has*.  It does not record who
//! declared it, and the two come apart further than they look: the merge
//! folds an *ancestor's* traits straight into the descendant, so even a
//! private member sitting on the merged class may have been written in a
//! trait a parent uses.  Reasoning backwards from "the member is on this
//! class, so it belongs to this class" is therefore wrong, and wrong in
//! both directions — it lets a scope in that should be kept out and
//! keeps one out that PHP lets in.
//!
//! So the declaring class is *computed*, by [`declaring_class`], and
//! only when it is actually needed.  It is not needed for a public
//! member, and not for any member reached from outside every class:
//! there no scope could qualify, whoever wrote the declaration.  What is
//! left — a non-public member reached from inside some class — is the
//! rare case that pays for a walk over the raw hierarchy.
//!
//! ## What that buys on the hot path
//!
//! A public member, which is the overwhelming majority of accesses, is
//! answered by the merged lookup the surrounding pass has already paid
//! for, with no hierarchy walk at all.
//!
//! ## Silence is the default
//!
//! Nothing is reported unless a declaration is positively found and
//! positively out of reach.  An ancestor the loader cannot produce, a
//! provenance walk that comes up empty, a virtual member, a trait body
//! whose host class is unknown, a trait as the subject, and a union with
//! one branch that permits the access all end in `None`.
//!
//! One promise this module cannot make is silence over a union whose
//! branches did not all resolve: the resolver hands over the branches it
//! could load and drops the rest, so a branch that names an unindexed
//! class is not visible here at all.

use std::sync::Arc;

use crate::class_lookup::is_subtype_of;
use crate::types::{ClassInfo, ClassLikeKind, Visibility};

/// Diagnostic code for an access to a member the calling scope may not see.
pub(crate) const INVALID_MEMBER_ACCESS_CODE: &str = "invalid_member_access";

/// Guard against a cycle in a malformed or mid-edit hierarchy.  Matches
/// the depth cap the other hierarchy walks in the codebase use.
const MAX_HIERARCHY_DEPTH: u32 = 20;

/// Which kind of member a lookup turned out to be, so the message can
/// name it without re-deriving it from the access syntax.
#[derive(Clone, Copy)]
enum MemberKind {
    Method,
    Property,
    StaticProperty,
    Constant,
}

impl MemberKind {
    fn label(self) -> &'static str {
        match self {
            MemberKind::Method => "method",
            MemberKind::Property => "property",
            MemberKind::StaticProperty => "static property",
            MemberKind::Constant => "constant",
        }
    }

    /// Spell the member the way PHP's own error message does.
    fn qualify(self, owner: &str, member_name: &str) -> String {
        match self {
            MemberKind::Method => format!("{}::{}()", owner, member_name),
            MemberKind::Property => format!("{}::${}", owner, member_name),
            // Extraction strips the `$` from `Foo::$bar`, so it is put
            // back here rather than being taken from the member name.
            MemberKind::StaticProperty => format!("{}::${}", owner, member_name),
            MemberKind::Constant => format!("{}::{}", owner, member_name),
        }
    }
}

/// What one branch of a union type says about the access.
enum BranchVerdict {
    /// The scope may reach the member, or nothing here can be judged —
    /// either way the access must not be reported.
    Permitted,
    /// The member exists on this branch and the scope may not reach it.
    Rejected(Rejection),
    /// This branch declares no such member.  The unknown-member check
    /// owns that case.
    Missing,
}

/// A rejected access, carrying what the message needs to name it.
struct Rejection {
    /// The class whose scope the reader has to enter for the access to
    /// become legal.
    owner: Arc<ClassInfo>,
    visibility: Visibility,
    kind: MemberKind,
}

/// Build the diagnostic message for an access PHP would reject, or
/// `None` when the access is fine — or cannot be judged.
///
/// `merged_classes` are the fully-resolved branches of the subject's
/// type, as the surrounding pass produced them.
///
/// The union verdict follows the conservative rule the neighbouring
/// checks already use: a message is produced only when every branch
/// rejects the access.  One permitted branch and one branch with a magic
/// handler each silence it, because the value may legally be that branch
/// at runtime.  A branch that could not be resolved cannot silence it,
/// because the resolver has already dropped it by the time the branches
/// arrive here.
///
/// A union that mixes a rejected branch with a branch that declares no
/// such member is silent too, even though PHP would fail on either.
/// That is a known hole rather than a decision: closing it means
/// reporting across two checks that each stay silent on their own.
///
/// `subject_binds_scope` says the subject is written as `$this`, `self`,
/// or `static` — the forms that name whatever class the code is *bound*
/// to.  Usually that is the class the code sits in, but a closure can be
/// bound elsewhere: a callback registered with Laravel's `Macroable`, or
/// any `Closure::bind`, runs with the target class as its scope even
/// though it is written inside another class.  The receiver is that
/// class, so it counts as a scope alongside the enclosing declaration.
/// `parent::` is deliberately not included: it names a class to look the
/// member up in, while the scope stays the class doing the looking.
#[allow(clippy::too_many_arguments)]
pub(crate) fn inaccessible_member_message(
    merged_classes: &[Arc<ClassInfo>],
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    current_class: Option<&ClassInfo>,
    subject_binds_scope: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    // Inside a trait the host class that supplies `$this` is unknown,
    // so every visibility answer would be a guess.
    if current_class.is_some_and(|c| c.kind == ClassLikeKind::Trait) {
        return None;
    }

    let mut rejected: Option<Rejection> = None;
    for class in merged_classes {
        match judge_branch(
            class,
            member_name,
            is_static,
            is_method_call,
            current_class,
            subject_binds_scope,
            class_loader,
        ) {
            BranchVerdict::Permitted => return None,
            BranchVerdict::Missing => return None,
            BranchVerdict::Rejected(rejection) => {
                if rejected.is_none() {
                    rejected = Some(rejection);
                }
            }
        }
    }

    let rejection = rejected?;
    Some(build_message(&rejection, member_name))
}

/// Judge the access against one branch of the union.
fn judge_branch(
    merged: &Arc<ClassInfo>,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    current_class: Option<&ClassInfo>,
    subject_binds_scope: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> BranchVerdict {
    // A trait naming the member is never a call PHP resolves.  The
    // spans that carry one are the identifiers in a `use` adaptation
    // (`use T { run as public; }`, `A::run insteadof B;`), which symbol
    // extraction records as `Trait::run()` so go-to-definition and
    // rename land on them — they select and re-scope a declaration
    // rather than reaching for one.  The only other way a trait becomes
    // the subject is a literal `T::run()`, which PHP deprecated in 8.1
    // and removed in 8.2, so it is not a visibility question either.
    if merged.kind == ClassLikeKind::Trait {
        return BranchVerdict::Permitted;
    }

    let Some((visibility, kind)) = declared_member(merged, member_name, is_static, is_method_call)
    else {
        // A class that answers for members the caller cannot see
        // directly turns the access into a magic-method call rather
        // than an error.
        if handles_inaccessible_access(merged, is_static, is_method_call) {
            return BranchVerdict::Permitted;
        }
        // Not on the assembled class.  The one member PHP has and the
        // merge does not is a parent's private one.
        return match private_ancestor_declaration(
            merged,
            member_name,
            is_static,
            is_method_call,
            class_loader,
        ) {
            Some(rejection) if !is_accessible(&rejection, current_class, class_loader) => {
                BranchVerdict::Rejected(rejection)
            }
            Some(_) => BranchVerdict::Permitted,
            None => BranchVerdict::Missing,
        };
    };

    // A public member is the overwhelming majority of accesses, and
    // nothing about the class can make one unreachable — so it is
    // settled before the scan for a magic handler, which is the only
    // work here proportional to the class's size.
    if visibility == Visibility::Public {
        return BranchVerdict::Permitted;
    }

    // A class that answers for members the caller cannot see directly
    // turns the access into a magic-method call rather than an error.
    if handles_inaccessible_access(merged, is_static, is_method_call) {
        return BranchVerdict::Permitted;
    }

    // Every class the access counts as being written inside.  Normally
    // that is just the enclosing declaration, but a subject that names
    // its own binding adds the class it binds to — see
    // `subject_binds_scope`.
    let mut scopes: Vec<&ClassInfo> = Vec::new();
    if let Some(current) = current_class {
        scopes.push(current);
    }
    if subject_binds_scope {
        scopes.push(merged);
    }

    // With no scope at all nothing non-public can be reached, whichever
    // class declared it — the verdict needs no provenance here.  The
    // message does: `protected` is spelled in terms of the class that
    // introduced the member, and naming the receiver instead would state
    // a rule PHP does not follow, sending a reader looking for a subclass
    // of the wrong class.  A receiver is used only when the declaration
    // cannot be traced at all.
    if scopes.is_empty() {
        let owner = declaring_class(
            merged,
            member_name,
            is_static,
            is_method_call,
            visibility,
            class_loader,
        )
        .unwrap_or_else(|| Arc::clone(merged));
        return BranchVerdict::Rejected(Rejection {
            owner,
            visibility,
            kind,
        });
    }

    // Anything at or below the receiver is at or below whatever declared
    // a protected member, so this settles the common inside-the-hierarchy
    // case without looking for the declaring class.
    if visibility == Visibility::Protected
        && scopes.iter().any(|scope| {
            scope.fqn() == merged.fqn() || is_subtype_of(scope, merged.fqn().as_str(), class_loader)
        })
    {
        return BranchVerdict::Permitted;
    }

    // Everything left needs to know which class wrote the declaration.
    let Some(owner) = declaring_class(
        merged,
        member_name,
        is_static,
        is_method_call,
        visibility,
        class_loader,
    ) else {
        // The provenance walk came up empty, which means something in
        // the hierarchy could not be read.  Guessing here is what
        // produced wrong answers before; stay quiet instead.
        return BranchVerdict::Permitted;
    };

    let rejection = Rejection {
        owner,
        visibility,
        kind,
    };
    if scopes
        .iter()
        .any(|scope| is_accessible(&rejection, Some(scope), class_loader))
    {
        BranchVerdict::Permitted
    } else {
        BranchVerdict::Rejected(rejection)
    }
}

/// Find the class that wrote the declaration of a non-public member that
/// is present on the assembled class.
///
/// The assembled class records what a class *has*, never who declared
/// it, and the two differ more than they look: the inheritance merge
/// folds an ancestor's traits straight into the descendant, so even a
/// private member sitting on the merged class may have been written in a
/// trait used by a parent.  Every rule below is therefore about where
/// the raw declaration lives, not about which merged class carries it.
///
/// Returns `None` when the hierarchy cannot be read far enough to be
/// sure, which the caller treats as "say nothing".
fn declaring_class(
    merged: &Arc<ClassInfo>,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    visibility: Visibility,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Arc<ClassInfo>> {
    let raw = class_loader(merged.fqn().as_str())?;

    match visibility {
        Visibility::Public => None,
        // A private member is never inherited, so the merge can only
        // have taken it from the class itself or from a trait used
        // somewhere up the chain.  The nearest level that supplies it
        // is the scope it belongs to.
        Visibility::Private => {
            if declared_member(&raw, member_name, is_static, is_method_call).is_some()
                || declares_through_own_traits(
                    &raw,
                    member_name,
                    is_static,
                    is_method_call,
                    class_loader,
                )
            {
                return Some(raw);
            }
            ancestors(&raw, class_loader).find(|ancestor| {
                declares_through_own_traits(
                    ancestor,
                    member_name,
                    is_static,
                    is_method_call,
                    class_loader,
                )
            })
        }
        // A protected member is visible to everything below the class
        // that introduced it, so the owner is the *furthest* level that
        // declares it non-privately.  A private namesake further up is
        // a different member that the nearer declaration shadows, and
        // must not be mistaken for the introducer.
        Visibility::Protected => {
            let mut owner = None;
            if declares_non_privately(&raw, member_name, is_static, is_method_call, class_loader) {
                owner = Some(Arc::clone(&raw));
            }
            for ancestor in ancestors(&raw, class_loader) {
                if declares_non_privately(
                    &ancestor,
                    member_name,
                    is_static,
                    is_method_call,
                    class_loader,
                ) {
                    owner = Some(ancestor);
                }
            }
            owner
        }
    }
}

/// Whether one level of the hierarchy supplies the member through a
/// trait it uses.
fn declares_through_own_traits(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    class.used_traits.iter().any(|trait_name| {
        trait_declares(
            trait_name,
            member_name,
            is_static,
            is_method_call,
            class_loader,
            0,
        )
        .is_some()
    })
}

/// Whether one level of the hierarchy declares the member — itself or
/// through a trait — with something other than `private`.
fn declares_non_privately(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if let Some((visibility, _)) = declared_member(class, member_name, is_static, is_method_call) {
        return visibility != Visibility::Private;
    }
    class.used_traits.iter().any(|trait_name| {
        trait_declares(
            trait_name,
            member_name,
            is_static,
            is_method_call,
            class_loader,
            0,
        )
        .is_some_and(|visibility| visibility != Visibility::Private)
    })
}

/// The visibility a trait supplies the member with, following the `use`
/// clauses of the trait itself.
///
/// A trait composes other traits the same way a class does, and PHP
/// counts a member reached that way as introduced by whichever class
/// uses the outermost trait.  Stopping at the first level would leave a
/// protected member of a nested trait untraceable, and the introducer
/// would then be read off a *nearer* redeclaration — which reports an
/// access PHP allows.
///
/// Adaptations are not followed: a `use` clause that renames or
/// re-scopes what it imports leaves no trace in the trait it imports
/// from, so such a member is simply not found here — which costs a
/// report rather than inventing one.
fn trait_declares(
    trait_name: &str,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u32,
) -> Option<Visibility> {
    if depth > MAX_HIERARCHY_DEPTH {
        return None;
    }
    let used = class_loader(trait_name)?;
    if let Some((visibility, _)) = declared_member(&used, member_name, is_static, is_method_call) {
        return Some(visibility);
    }
    used.used_traits.iter().find_map(|nested| {
        trait_declares(
            nested,
            member_name,
            is_static,
            is_method_call,
            class_loader,
            depth + 1,
        )
    })
}

/// Find a private member on an ancestor, which the inheritance merge
/// drops and which is therefore invisible on the assembled class.
///
/// Only ancestors are walked: the receiver's own declarations and its
/// traits' are already on the merged class, adaptations applied.
fn private_ancestor_declaration(
    merged: &Arc<ClassInfo>,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Rejection> {
    for ancestor in ancestors(merged, class_loader) {
        let Some((visibility, kind)) =
            declared_member(&ancestor, member_name, is_static, is_method_call)
        else {
            continue;
        };
        // A public or protected ancestor member would have been merged
        // into the assembled class, so finding one here means the two
        // views disagree and nothing can be concluded.
        if visibility != Visibility::Private {
            return None;
        }
        return Some(Rejection {
            owner: ancestor,
            visibility,
            kind,
        });
    }
    None
}

/// The raw ancestors of a class, nearest first.
///
/// Lazy on purpose.  The caller that looks for a private ancestor member
/// stops at the first level that has one, and this runs on every member
/// the assembled class does not carry — the ordinary state of code being
/// typed — so materialising the whole chain would allocate a vector per
/// access to read one entry of it.
fn ancestors<'a>(
    class: &Arc<ClassInfo>,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> impl Iterator<Item = Arc<ClassInfo>> + 'a {
    let mut next = class.parent_class;
    let mut depth = 0u32;
    std::iter::from_fn(move || {
        let name = next?;
        depth += 1;
        if depth > MAX_HIERARCHY_DEPTH {
            return None;
        }
        let ancestor = class_loader(&name)?;
        next = ancestor.parent_class;
        Some(ancestor)
    })
}

/// The visibility and kind `class` declares `member_name` with, matching
/// the member kind the access syntax asks for.
///
/// Method names are compared case-insensitively and property and
/// constant names case-sensitively, which is how PHP compares them.
fn declared_member(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
) -> Option<(Visibility, MemberKind)> {
    if is_method_call {
        return class
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(member_name))
            .map(|m| (m.visibility, MemberKind::Method));
    }

    if is_static {
        if let Some(constant) = class.constants.iter().find(|c| c.name == member_name) {
            return Some((constant.visibility, MemberKind::Constant));
        }
        // A static property is written `Foo::$bar`, and the stored name
        // may or may not carry the `$`.
        return class
            .properties
            .iter()
            .find(|p| {
                p.is_static && (p.name == member_name || format!("${}", p.name) == member_name)
            })
            .map(|p| (p.visibility, MemberKind::StaticProperty));
    }

    class
        .properties
        .iter()
        .find(|p| p.name == member_name)
        .map(|p| (p.visibility, MemberKind::Property))
}

/// Whether the scope the access is written in may see the declaration.
///
/// - `private` — only from the declaring class itself.  Privacy in PHP
///   is per class, not per object, so reaching into another instance of
///   your own class is allowed.
/// - `protected` — from the declaring class or any class descending
///   from it.  That covers sibling access: a member declared on a shared
///   parent is visible to every branch below it.
///
/// Scopes are compared by FQN, so two classes that share a short name
/// across namespaces are not mistaken for each other.
fn is_accessible(
    rejection: &Rejection,
    current_class: Option<&ClassInfo>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if rejection.visibility == Visibility::Public {
        return true;
    }
    let Some(current) = current_class else {
        return false;
    };
    if current.fqn() == rejection.owner.fqn() {
        return true;
    }
    rejection.visibility == Visibility::Protected
        && is_subtype_of(current, rejection.owner.fqn().as_str(), class_loader)
}

/// Whether the class turns an inaccessible member into a magic-method
/// call instead of an error.
///
/// PHP consults `__get`, `__set`, `__isset`, and `__unset` for a
/// property the caller cannot reach and `__call` / `__callStatic` for a
/// method, and only fails when the handler is absent.  A class declaring
/// any of them has opted into answering for such members.
///
/// This is deliberately not `has_magic_method_for_access` in the
/// unknown-member module, which answers a different question — whether a
/// member that exists *nowhere* is still handled — and gates `__get`
/// behind the
/// `report-magic-properties` setting.  That setting is about which
/// virtual property surface to trust; it has no bearing on whether PHP
/// dispatches to `__get` for a property that demonstrably exists.
///
/// The four property handlers are treated as one set because the span
/// this check runs on records that a property was accessed, not whether
/// it was read, written, or unset.  Distinguishing them would let the
/// exact handler be required; until then the union of them is the
/// conservative choice, trading a missed report for never inventing one.
fn handles_inaccessible_access(class: &ClassInfo, is_static: bool, is_method_call: bool) -> bool {
    let handlers: &[&str] = match (is_method_call, is_static) {
        (true, true) => &["__callStatic"],
        (true, false) => &["__call"],
        // A `::` access that is not a call reaches a constant or a
        // static property, neither of which any magic method covers.
        (false, true) => return false,
        (false, false) => &["__get", "__set", "__isset", "__unset"],
    };
    class.methods.iter().any(|m| {
        handlers
            .iter()
            .any(|handler| m.name.eq_ignore_ascii_case(handler))
    })
}

/// Word the message after the shape of the access, naming the class that
/// declares the member rather than the one the access went through —
/// that is the class whose scope the reader has to enter to fix it.
fn build_message(rejection: &Rejection, member_name: &str) -> String {
    let modifier = match rejection.visibility {
        Visibility::Private => "private",
        Visibility::Protected => "protected",
        Visibility::Public => "public",
    };

    let owner = display_class_name(&rejection.owner);
    let verb = match rejection.kind {
        MemberKind::Method => "call",
        _ => "access",
    };
    let scope = match rejection.visibility {
        Visibility::Protected => format!("outside {} or its subclasses", owner),
        _ => "outside its declaring class".to_string(),
    };

    format!(
        "Cannot {} {} {} {} from {}",
        verb,
        modifier,
        rejection.kind.label(),
        rejection.kind.qualify(&owner, member_name),
        scope,
    )
}

/// Name the class for the message, preferring the FQN.
fn display_class_name(owner: &ClassInfo) -> String {
    if owner.name.starts_with("__anonymous@") {
        return "anonymous class".to_string();
    }
    owner.fqn().to_string()
}
