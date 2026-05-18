//! Code Lens (`textDocument/codeLens`) support.
//!
//! Shows clickable annotations above methods that override a parent
//! method or implement an interface method, linking to the prototype
//! declaration.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::definition::member::MemberKind;
use crate::types::{ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH};
use crate::util::offset_to_position;

/// Information about a prototype (ancestor) method that a local method
/// overrides or implements.
struct Prototype {
    /// Display name of the ancestor class (short name).
    ancestor_name: String,
    /// Whether the ancestor is an interface.
    is_interface: bool,
    /// URI of the file containing the ancestor class.
    file_uri: String,
    /// Position of the method declaration in the ancestor's file.
    position: Position,
}

impl Backend {
    /// Handle a `textDocument/codeLens` request.
    ///
    /// Returns a code lens for each method in the file that overrides
    /// a parent class method or implements an interface method.
    pub fn handle_code_lens(&self, uri: &str, content: &str) -> Option<Vec<CodeLens>> {
        let classes = {
            let map = self.uri_classes_index.read();
            map.get(uri)?.clone()
        };

        let mut lenses = Vec::new();

        for class in &classes {
            let class_fqn = class.fqn();

            for method in &class.methods {
                // Skip synthetic/stub methods with no real source position.
                if method.name_offset == 0 {
                    continue;
                }

                // Skip virtual methods (injected via @method tags, not
                // actually declared in source).
                if method.is_virtual {
                    continue;
                }

                if let Some(proto) =
                    self.find_prototype(class, &class_fqn, &method.name, uri, content)
                {
                    let pos = offset_to_position(content, method.name_offset as usize);
                    let range = Range {
                        start: Position {
                            line: pos.line,
                            character: 0,
                        },
                        end: Position {
                            line: pos.line,
                            character: 0,
                        },
                    };

                    let icon = if proto.is_interface { "◆" } else { "↑" };
                    let title = format!("{} {}::{}", icon, proto.ancestor_name, method.name);

                    let target_uri: Url = match proto.file_uri.parse() {
                        Ok(u) => u,
                        Err(_) => continue,
                    };

                    let command = self.build_code_lens_command(
                        title,
                        target_uri,
                        proto.position,
                    );

                    lenses.push(CodeLens {
                        range,
                        command: Some(command),
                        data: None,
                    });
                }
            }
        }

        if lenses.is_empty() {
            None
        } else {
            Some(lenses)
        }
    }

    /// Search the inheritance hierarchy for the closest ancestor that
    /// declares a method with the given name.
    ///
    /// Priority order: parent class chain, then used traits, then
    /// implemented interfaces. Returns `None` when no ancestor
    /// declares the method.
    fn find_prototype(
        &self,
        class: &ClassInfo,
        _class_fqn: &str,
        method_name: &str,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Prototype> {
        // ── 1. Walk the parent class chain ──────────────────────────────
        let mut current = class.clone();
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let parent_name = match current.parent_class {
                Some(name) => name,
                None => break,
            };
            let parent = match self.find_or_load_class(&parent_name) {
                Some(p) => ClassInfo::clone(&p),
                None => break,
            };
            // Check methods declared directly on this parent (not
            // inherited) so we find the actual declaration site.
            if parent
                .methods
                .iter()
                .any(|m| m.name == method_name && !m.is_virtual)
                && let Some(proto) = self.build_prototype(
                    &parent_name,
                    &parent,
                    method_name,
                    false,
                    current_uri,
                    current_content,
                )
            {
                return Some(proto);
            }
            current = parent;
        }

        // ── 2. Check used traits ────────────────────────────────────────
        if let Some(proto) = self.find_prototype_in_traits(
            &class.used_traits,
            method_name,
            current_uri,
            current_content,
            0,
        ) {
            return Some(proto);
        }

        // ── 3. Check implemented interfaces ─────────────────────────────
        if let Some(proto) =
            self.find_prototype_in_interfaces(class, method_name, current_uri, current_content)
        {
            return Some(proto);
        }

        None
    }

    /// Search a list of traits for a method declaration.
    ///
    /// Recursively checks traits used by each trait, up to a depth limit.
    fn find_prototype_in_traits(
        &self,
        trait_names: &[crate::atom::Atom],
        method_name: &str,
        current_uri: &str,
        current_content: &str,
        depth: usize,
    ) -> Option<Prototype> {
        if depth > MAX_INHERITANCE_DEPTH as usize {
            return None;
        }

        for trait_name in trait_names {
            let trait_info = match self.find_or_load_class(trait_name) {
                Some(t) => t,
                None => continue,
            };
            if trait_info
                .methods
                .iter()
                .any(|m| m.name == method_name && !m.is_virtual)
                && let Some(proto) = self.build_prototype(
                    trait_name,
                    &trait_info,
                    method_name,
                    false,
                    current_uri,
                    current_content,
                )
            {
                return Some(proto);
            }
            // Recurse into traits used by this trait.
            if let Some(proto) = self.find_prototype_in_traits(
                &trait_info.used_traits,
                method_name,
                current_uri,
                current_content,
                depth + 1,
            ) {
                return Some(proto);
            }
        }

        None
    }

    /// Search implemented interfaces (including those inherited from
    /// parents) for a method declaration.
    fn find_prototype_in_interfaces(
        &self,
        class: &ClassInfo,
        method_name: &str,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Prototype> {
        // Collect all interface names from the class and its parent chain.
        let mut all_iface_names: Vec<crate::atom::Atom> = class.interfaces.clone();
        let mut current = class.clone();
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let parent_name = match current.parent_class {
                Some(name) => name,
                None => break,
            };
            let parent = match self.find_or_load_class(&parent_name) {
                Some(p) => ClassInfo::clone(&p),
                None => break,
            };
            for iface in &parent.interfaces {
                if !all_iface_names.contains(iface) {
                    all_iface_names.push(*iface);
                }
            }
            current = parent;
        }

        for iface_name in &all_iface_names {
            if let Some(proto) = self.find_prototype_in_interface(
                iface_name,
                method_name,
                current_uri,
                current_content,
            ) {
                return Some(proto);
            }
        }

        None
    }

    /// Check a single interface (and its own extends chain) for the
    /// method declaration.
    fn find_prototype_in_interface(
        &self,
        iface_name: &str,
        method_name: &str,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Prototype> {
        let iface = self.find_or_load_class(iface_name)?;
        if iface
            .methods
            .iter()
            .any(|m| m.name == method_name && !m.is_virtual)
            && let Some(proto) = self.build_prototype(
                iface_name,
                &iface,
                method_name,
                true,
                current_uri,
                current_content,
            )
        {
            return Some(proto);
        }

        // Walk the interface's own extends chain (interfaces can extend
        // other interfaces via `parent_class` and `interfaces`).
        for parent_iface in &iface.interfaces {
            if let Some(proto) = self.find_prototype_in_interface(
                parent_iface,
                method_name,
                current_uri,
                current_content,
            ) {
                return Some(proto);
            }
        }
        if let Some(parent_name) = iface.parent_class
            && let Some(proto) = self.find_prototype_in_interface(
                &parent_name,
                method_name,
                current_uri,
                current_content,
            )
        {
            return Some(proto);
        }

        None
    }

    /// Build the LSP `Command` for a code lens that navigates to a target
    /// location.
    ///
    /// Uses `editor.action.showReferences` (widely supported) by default,
    /// and `vscode.open` when connected to a VS Code client.
    fn build_code_lens_command(&self, title: String, uri: Url, position: Position) -> Command {
        let client = self.client_name.lock();
        if client.contains("Visual Studio Code") {
            // VS Code: use vscode.open with a fragment for direct navigation.
            let fragment = format!("L{},{}", position.line + 1, position.character + 1);
            let mut target_uri = uri;
            target_uri.set_fragment(Some(&fragment));
            Command {
                title,
                command: "vscode.open".to_string(),
                arguments: Some(vec![serde_json::json!(target_uri)]),
            }
        } else {
            // All other editors: use editor.action.showReferences which is
            // handled by most LSP clients (Zed, Neovim, Emacs, etc.).
            let location = Location {
                uri: uri.clone(),
                range: Range {
                    start: position,
                    end: position,
                },
            };
            Command {
                title,
                command: "editor.action.showReferences".to_string(),
                arguments: Some(vec![
                    serde_json::json!(uri),
                    serde_json::json!(position),
                    serde_json::json!([location]),
                ]),
            }
        }
    }

    /// Build a `Prototype` by locating the method's position in the
    /// ancestor's source file.
    fn build_prototype(
        &self,
        ancestor_fqn: &str,
        ancestor_class: &ClassInfo,
        method_name: &str,
        is_interface: bool,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Prototype> {
        let (file_uri, file_content) =
            self.find_class_file_content(ancestor_fqn, current_uri, current_content)?;

        let name_offset = ancestor_class.member_name_offset(method_name, "method");

        let position = Self::find_member_position(
            &file_content,
            method_name,
            MemberKind::Method,
            name_offset,
        )?;

        // Determine whether to treat this as an interface based on the
        // ancestor's kind (the caller's hint is a fallback).
        let is_iface = ancestor_class.kind == ClassLikeKind::Interface || is_interface;

        Some(Prototype {
            ancestor_name: ancestor_class.name.to_string(),
            is_interface: is_iface,
            file_uri,
            position,
        })
    }
}
