//! The variables a template inherits from the layout it `@extends`.
//!
//! A child and its layout render from one data array: Laravel compiles
//! `@extends('layouts.app')` into a footer that renders `layouts.app` with
//! the very data the child was given. So whatever the layout declares it
//! needs, the child is handed too, and a `@var` written once in a layout
//! types every template under it without anyone redeclaring it.
//!
//! The child stays in charge of its own names. A name its own signature
//! declares keeps the child's type (the preprocessor never re-declares a
//! signature name), so the child may narrow what the layout declared but
//! cannot have it widened underneath, which is the covariant merge
//! Bladestan performs. The chain is walked all the way up, nearest layout
//! first, so a name a grandparent layout declares reaches the child as
//! well.

use crate::Backend;

use super::call_site_inference::InjectedVars;

impl Backend {
    /// The variables the layouts above a template declare, the nearest
    /// layout's winning for a name more than one of them declares.
    pub(crate) fn blade_layout_vars(&self, content: &str) -> InjectedVars {
        let mut vars: InjectedVars = Vec::new();
        for (_, source) in self.blade_layout_chain(content) {
            for (name, ty) in super::signature::declarations(&source) {
                if vars.iter().any(|(existing, _)| existing == &name) {
                    continue;
                }
                // The child's prologue has no namespace and none of the
                // layout's imports, so a short class name has to be
                // qualified here or it would resolve to the global one.
                vars.push((name, self.docblock_type(Some(&ty))));
            }
        }
        vars
    }

    /// The layouts above a template, nearest first: each extended layout's
    /// view name and source.
    ///
    /// `@extendsFirst` offers several candidates and Blade renders the
    /// first that exists, so the walk follows the first one a view root
    /// actually holds.
    ///
    /// The walk stops at a name the view index does not know, at a file
    /// that cannot be read, and at the first name it has already seen, so
    /// a chain that loops back on itself terminates with what it found
    /// rather than running forever.
    pub(crate) fn blade_layout_chain(&self, content: &str) -> Vec<(String, String)> {
        let mut chain: Vec<(String, String)> = Vec::new();
        let mut next = super::signature::extract_extends(content);
        while let Some((name, source)) = next.into_iter().find_map(|name| {
            let source = self.blade_view_source(&name)?;
            Some((name, source))
        }) {
            if chain.iter().any(|(seen, _)| seen == &name) {
                break;
            }
            next = super::signature::extract_extends(&source);
            chain.push((name, source));
        }
        chain
    }

    /// A view's source, read through an open buffer when there is one so
    /// an unsaved edit to a layout is what its children see.
    pub(crate) fn blade_view_source(&self, view_name: &str) -> Option<String> {
        let path = self.blade_view_path(view_name)?;
        if let Some(uri) = crate::util::path_to_url(&path)
            && let Some(content) = self.get_file_content(uri.as_str())
        {
            return Some(content);
        }
        // A file URI cannot be built from a relative path, and the indexed
        // view paths are relative whenever the project root is (the analyse
        // CLI passes `--project-root` through as given).  Nothing is open in
        // that case either, so reading the file directly loses nothing.
        std::fs::read_to_string(&path).ok()
    }
}
