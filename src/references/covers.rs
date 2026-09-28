//! Reverse PHPUnit coverage lookup.
//!
//! [`Backend::find_covering_test_classes`] answers "which test classes
//! declare coverage for this class?", the subject-side direction of the
//! `@covers` relationship.  The test side needs nothing of its own: a
//! coverage target is an ordinary symbol-map reference, so go-to-definition
//! and rename already reach the subject from the test.
//!
//! The search is the same shape as [`super::classes`]' class-reference
//! search, narrowed twice: the reference index picks the files that name the
//! class at all, and the [`ClassRefContext::CoversTarget`] tag the symbol map
//! puts on coverage metadata picks the references that are coverage
//! declarations rather than ordinary uses.

use super::*;

use crate::atom::Atom;
use crate::class_lookup::find_documented_class_at_offset;
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::types::ClassInfo;

impl Backend {
    /// The classes whose PHPUnit coverage metadata names `target_fqn`,
    /// as `(covering class FQN, URI of the file that declares it)`.
    ///
    /// Empty while the workspace is still being indexed, since the reference
    /// index cannot narrow the candidate files until then.
    pub(crate) fn find_covering_test_classes(&self, target_fqn: &str) -> Vec<(Atom, String)> {
        let target = strip_fqn_prefix(target_fqn);
        let target_short = crate::util::short_name(target);

        let candidate_keys = class_candidate_keys(target, target_short);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);

        let mut found: Vec<(Atom, String)> = Vec::new();

        for (file_uri, symbol_map) in &snapshot {
            // Only files that carry coverage metadata at all are worth
            // resolving names in, and almost none do.
            if !symbol_map.spans.iter().any(|span| {
                matches!(
                    &span.kind,
                    SymbolKind::ClassReference {
                        context: ClassRefContext::CoversTarget,
                        ..
                    }
                )
            }) {
                continue;
            }

            // Same resolution ladder as the class-reference search: prefer
            // mago-names' offset-keyed resolution, fall back to the file's
            // imports for names it does not track (docblock references, which
            // is exactly what `@covers` is).
            let fqn_resolver = super::SpanFqnResolver::new(self, file_uri);
            // Only a tag on a class docblock needs the source text, to tell a
            // block that documents the class from one that documents whatever
            // sits between it and the declaration.
            let file_content = std::cell::OnceCell::new();

            let classes = match self.symbols.uri_classes_index.read().get(file_uri) {
                Some(classes) => classes.clone(),
                None => continue,
            };

            for span in &symbol_map.spans {
                let SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context: ClassRefContext::CoversTarget,
                } = &span.kind
                else {
                    continue;
                };

                // `class_ref_span` strips the leading `\` but records that it
                // was there, so a written-out target is already the FQN.
                // Re-resolving it would prefix the test's own namespace and
                // produce `App\Tests\App\Calculator`.
                let resolved = fqn_resolver.fqn(name, *is_fqn, span.start);
                if !class_names_match(strip_fqn_prefix(&resolved), target, target_short) {
                    continue;
                }

                // The class the metadata belongs to is the covering test,
                // whether the tag sat on the class or on one of its methods.
                let content = file_content
                    .get_or_init(|| self.get_file_content_arc(file_uri).unwrap_or_default());
                let Some(covering_fqn) = covering_class_fqn(&classes, content, span.start) else {
                    continue;
                };
                if found.iter().any(|(fqn, _)| *fqn == covering_fqn) {
                    continue;
                }
                found.push((covering_fqn, file_uri.clone()));
            }
        }

        // The reference index hands back candidate files in hash order, so
        // sort for a stable lens label and test expectations.
        found.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
        found
    }
}

/// The class a coverage tag at `offset` belongs to, whether the tag sat on
/// the class docblock or on one of its methods.
fn covering_class_fqn(classes: &[Arc<ClassInfo>], content: &str, offset: u32) -> Option<Atom> {
    find_documented_class_at_offset(classes, content, offset).map(|class| class.fqn())
}
