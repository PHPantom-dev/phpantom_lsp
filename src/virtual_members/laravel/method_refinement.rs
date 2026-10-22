//! Copy-on-change refinement of a loaded class's method signatures.
//!
//! The config-driven patches (auth user model, storage disk type) rewrite
//! the return type of a few named vendor methods, which may be declared as
//! real methods or as `@method` tags in [`ClassInfo::doc_members`].  The
//! class is shared behind an `Arc`, so it is copied only once a method has
//! actually changed; a lookup the refinement leaves alone returns the same
//! `Arc` it was given.

use std::sync::Arc;

use crate::types::{ClassInfo, MethodInfo};

/// Run `refine` over every real and `@method`-tag method of `loaded` whose
/// name satisfies `applies`.
///
/// `refine` returns whether it changed the method.  Returns `loaded`
/// itself when nothing changed, otherwise a patched copy.
pub(super) fn refine_methods(
    loaded: Arc<ClassInfo>,
    applies: impl Fn(&str) -> bool,
    mut refine: impl FnMut(&mut MethodInfo) -> bool,
) -> Arc<ClassInfo> {
    let mut patched: Option<ClassInfo> = None;

    for (i, method) in loaded.methods.iter().enumerate() {
        if let Some(refined) = refined_copy(method, &applies, &mut refine) {
            patched
                .get_or_insert_with(|| (*loaded).clone())
                .methods
                .make_mut()[i] = refined;
        }
    }

    if let Some(doc) = loaded.doc_members.as_ref() {
        let mut patched_doc = None;
        for (i, method) in doc.methods.iter().enumerate() {
            if let Some(refined) = refined_copy(method, &applies, &mut refine) {
                patched_doc.get_or_insert_with(|| (**doc).clone()).methods[i] = refined;
            }
        }
        if let Some(patched_doc) = patched_doc {
            patched.get_or_insert_with(|| (*loaded).clone()).doc_members =
                Some(Arc::new(patched_doc));
        }
    }

    patched.map_or(loaded, Arc::new)
}

fn refined_copy(
    method: &Arc<MethodInfo>,
    applies: &impl Fn(&str) -> bool,
    refine: &mut impl FnMut(&mut MethodInfo) -> bool,
) -> Option<Arc<MethodInfo>> {
    if !applies(method.name.as_str()) {
        return None;
    }
    let mut refined = (**method).clone();
    refine(&mut refined).then(|| Arc::new(refined))
}
