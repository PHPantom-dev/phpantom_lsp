//! Extraction of anonymous classes (`new class { ... }`).
//!
//! Anonymous classes are given synthetic names of the form
//! `__anonymous@<offset>` so that
//! [`find_class_at_offset`](crate::class_lookup::find_class_at_offset) can resolve
//! `$this` inside their bodies. This module walks statement and expression
//! trees looking for `Expression::AnonymousClass` nodes, recursing into
//! method bodies (including nested anonymous classes) along the way.

use std::sync::Arc;

use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom, atom_bytes};
use crate::types::*;

use super::DocblockCtx;

/// Walker that appends a [`ClassInfo`] for every `new class { … }`
/// expression it encounters. The generated traversal reaches anonymous
/// classes at any depth (control flow, closures, arguments, array
/// elements, property defaults, string interpolations); nested anonymous
/// classes are found because the default `walk_anonymous_class` continues
/// into the class body after the hook fires.
struct AnonymousClassWalker<'a, 'd> {
    doc_ctx: Option<&'d DocblockCtx<'a>>,
}

impl<'a, 'd> Walker<'a, 'a, Vec<ClassInfo>> for AnonymousClassWalker<'a, 'd> {
    fn walk_in_anonymous_class(&self, node: &'a AnonymousClass<'a>, classes: &mut Vec<ClassInfo>) {
        classes.push(Backend::extract_anonymous_class_info(node, self.doc_ctx));
    }
}

impl Backend {
    /// Build a [`ClassInfo`] for an anonymous class expression.
    fn extract_anonymous_class_info<'a>(
        anon: &AnonymousClass<'a>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) -> ClassInfo {
        let parent_class = anon
            .extends
            .as_ref()
            .and_then(|ext| ext.types.first().map(|ident| atom_bytes(ident.value())));

        let interfaces: Vec<Atom> = anon
            .implements
            .as_ref()
            .map(|imp| {
                imp.types
                    .iter()
                    .map(|ident| atom_bytes(ident.value()))
                    .collect()
            })
            .unwrap_or_default();

        let ExtractedMembers {
            methods,
            properties,
            constants,
            used_traits,
            trait_precedences,
            trait_aliases,
            ..
        } = Self::extract_class_like_members(anon.members.iter(), doc_ctx, &[]);

        let start_offset = anon.left_brace.start.offset;
        let end_offset = anon.right_brace.end.offset;
        // Anonymous classes don't have a meaningful keyword_offset for
        // go-to-definition purposes — use 0 ("not available").
        let keyword_offset = 0;
        let name = atom(&format!("{ANONYMOUS_CLASS_PREFIX}{start_offset}"));

        ClassInfo {
            kind: ClassLikeKind::Class,
            name,
            methods: methods.into_iter().map(Arc::new).collect::<Vec<_>>().into(),
            properties: properties
                .into_iter()
                .map(Arc::new)
                .collect::<Vec<_>>()
                .into(),
            constants: constants
                .into_iter()
                .map(Arc::new)
                .collect::<Vec<_>>()
                .into(),
            start_offset,
            end_offset,
            keyword_offset,
            decl_start_offset: start_offset,
            parent_class,
            interfaces,
            used_traits,
            mixins: vec![],
            mixin_generics: vec![],
            require_extends: None,
            require_implements: Vec::new(),
            is_final: false,
            is_abstract: false,
            is_readonly: anon.modifiers.contains_readonly(),
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: vec![],
            template_param_bounds: AtomMap::default(),
            template_param_defaults: AtomMap::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: AtomMap::default(),
            trait_precedences,
            trait_aliases,
            links: Vec::new(),
            see_refs: Vec::new(),
            class_docblock: None,
            doc_members: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            method_index: Default::default(),
            indexed_method_count: 0,
            laravel: None,
            fqn: None,
        }
    }

    /// Walk a statement subtree, appending a [`ClassInfo`] for every
    /// anonymous class found anywhere within it.
    pub(crate) fn find_anonymous_classes_in_statement<'a>(
        statement: &'a Statement<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        AnonymousClassWalker { doc_ctx }.walk_statement(statement, classes);
    }

    /// Walk class-like members, appending a [`ClassInfo`] for every
    /// anonymous class found in method bodies, property defaults, constant
    /// values, and property hook bodies.
    pub(super) fn find_anonymous_classes_in_members<'a>(
        members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        let walker = AnonymousClassWalker { doc_ctx };
        for member in members {
            walker.walk_class_like_member(member, classes);
        }
    }
}
