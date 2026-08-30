//! Code Lens (`textDocument/codeLens`) support.
//!
//! Shows reference counts plus override/implement annotations.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::definition::member::MemberKind;
use crate::reference_index::ReferenceIndexKey;
use crate::symbol_map::SymbolKind;
use crate::text_position::offset_to_position;
use crate::types::{ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, Visibility};

fn line_indent(content: &str, byte_offset: usize) -> u32 {
    let line_start = content[..byte_offset]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    content[line_start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count() as u32
}

/// Information about a prototype (ancestor) method that a local method
/// overrides or implements.
struct Prototype {
    /// Display name of the ancestor class (short name).
    ancestor_name: String,
    is_interface: bool,
    /// URI of the file containing the ancestor class.
    file_uri: String,
    /// Position of the method declaration in the ancestor's file.
    position: Position,
}

impl Backend {
    /// Handle a `textDocument/codeLens` request.
    ///
    /// Returns reference lenses for PHP declarations and navigation lenses
    /// for methods that override or implement an ancestor declaration.
    pub fn handle_code_lens(&self, uri: &str, content: &str) -> Option<Vec<CodeLens>> {
        let classes = {
            let map = self.symbols.uri_classes_index.read();
            map.get(uri).cloned().unwrap_or_default()
        };

        let mut lenses = Vec::new();

        for class in &classes {
            let class_fqn = class.fqn();

            if let Some(lens) = self.build_declaration_reference_lens(
                uri,
                content,
                self.class_declaration_name_offset(uri, class),
                &ReferenceIndexKey::class(&class_fqn),
            ) {
                lenses.push(lens);
            }

            if let Some(lens) = self.build_covers_lens(class, uri, content) {
                lenses.push(lens);
            }

            for method in &class.methods {
                if method.name_offset == 0
                    || method.is_virtual
                    || method.visibility == crate::types::Visibility::Private
                {
                    continue;
                }

                let pos = offset_to_position(content, method.name_offset as usize);
                let indent = line_indent(content, method.name_offset as usize);
                let range = Range {
                    start: Position {
                        line: pos.line,
                        character: indent,
                    },
                    end: Position {
                        line: pos.line,
                        character: indent,
                    },
                };

                let proto = self.find_prototype(class, &class_fqn, &method.name, uri, content);
                if !method.name.starts_with("__")
                    && proto.is_none()
                    && let Some(lens) = self.build_member_reference_lens(
                        uri,
                        content,
                        method.name_offset,
                        class_fqn,
                        method.name,
                        method.is_static,
                    )
                {
                    lenses.push(lens);
                }
                if let Some(proto) = proto {
                    let icon = if proto.is_interface { "◆" } else { "↑" };
                    let title = format!("{} {}::{}", icon, proto.ancestor_name, method.name);

                    let target_uri: Url = match proto.file_uri.parse() {
                        Ok(u) => u,
                        Err(_) => continue,
                    };

                    let command = self.build_code_lens_command(title, target_uri, proto.position);

                    lenses.push(CodeLens {
                        range,
                        command: Some(command),
                        data: None,
                    });
                }
            }

            for property in &class.properties {
                if property.name_offset == 0
                    || property.is_virtual
                    || property.visibility == Visibility::Private
                {
                    continue;
                }
                let member_name = property.name.strip_prefix('$').unwrap_or(&property.name);
                if let Some(lens) = self.build_member_reference_lens(
                    uri,
                    content,
                    property.name_offset,
                    class_fqn,
                    crate::atom::atom(member_name),
                    property.is_static,
                ) {
                    lenses.push(lens);
                }
            }

            for constant in &class.constants {
                if constant.name_offset == 0 || constant.visibility == Visibility::Private {
                    continue;
                }
                if let Some(lens) = self.build_member_reference_lens(
                    uri,
                    content,
                    constant.name_offset,
                    class_fqn,
                    constant.name,
                    true,
                ) {
                    lenses.push(lens);
                }
            }
        }

        if let Some(symbol_map) = self.symbol_maps.read().get(uri).cloned() {
            for span in &symbol_map.spans {
                let key = match &span.kind {
                    SymbolKind::FunctionCall {
                        name,
                        is_definition: true,
                        ..
                    } => self.function_reference_key(uri, span.start, name),
                    SymbolKind::ConstantReference {
                        name,
                        is_definition: true,
                    } => ReferenceIndexKey::Constant(self.constant_fqn_at(uri, span.start, name)),
                    _ => continue,
                };
                if let Some(lens) =
                    self.build_declaration_reference_lens(uri, content, span.start, &key)
                {
                    lenses.push(lens);
                }
            }
        }

        if lenses.is_empty() {
            None
        } else {
            Some(lenses)
        }
    }

    /// Build a declaration reference lens from the candidate index.
    ///
    /// A zero count is returned fully resolved because semantic filtering can
    /// only remove candidates.  Non-zero declarations take the LSP's lazy
    /// resolve path, which computes exact locations only when the client asks.
    fn build_declaration_reference_lens(
        &self,
        origin_uri: &str,
        content: &str,
        declaration_offset: u32,
        key: &ReferenceIndexKey,
    ) -> Option<CodeLens> {
        if declaration_offset == 0 {
            return None;
        }

        let candidate_count = self.indexed_reference_count(key)?;
        let origin_url = Url::parse(origin_uri).ok()?;
        let position = offset_to_position(content, declaration_offset as usize);
        let range = Range::new(
            Position::new(position.line, 0),
            Position::new(position.line, 0),
        );
        if candidate_count == 0 {
            return Some(CodeLens {
                range,
                command: Some(Self::reference_lens_command(
                    origin_url,
                    position,
                    Vec::new(),
                )),
                data: None,
            });
        }

        Some(CodeLens {
            range,
            command: None,
            data: Some(serde_json::json!({
                "kind": "phpReferences",
                "uri": origin_uri,
                "position": position,
            })),
        })
    }

    fn build_member_reference_lens(
        &self,
        origin_uri: &str,
        content: &str,
        declaration_offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) -> Option<CodeLens> {
        if declaration_offset == 0 {
            return None;
        }
        let key = ReferenceIndexKey::Member {
            name: member.to_string(),
            is_static,
        };
        let candidate_count = self.indexed_reference_count(&key)?;
        let origin_url = Url::parse(origin_uri).ok()?;
        let position = offset_to_position(content, declaration_offset as usize);
        let range = Range::new(
            Position::new(position.line, 0),
            Position::new(position.line, 0),
        );
        if candidate_count == 0 {
            return Some(CodeLens {
                range,
                command: Some(Self::reference_lens_command(
                    origin_url,
                    position,
                    Vec::new(),
                )),
                data: None,
            });
        }

        let supports_refresh = self
            .supports_code_lens_refresh
            .load(std::sync::atomic::Ordering::Acquire);
        let cached_locations = if supports_refresh {
            self.member_ref_locations_cached(
                origin_uri,
                declaration_offset,
                class_fqn,
                member,
                is_static,
            )
        } else {
            self.member_ref_locations_ready(
                origin_uri,
                declaration_offset,
                class_fqn,
                member,
                is_static,
            )
        };
        if let Some(locations) = cached_locations {
            return Some(CodeLens {
                range,
                command: Some(Self::reference_lens_command(
                    origin_url, position, locations,
                )),
                data: None,
            });
        }

        // Clients with refresh support can re-pull once the shared background
        // worker fills the exact cache.  Omitting the cold lens avoids an
        // eager resolve burst merely to obtain titles for the viewport.
        if supports_refresh {
            return None;
        }

        Some(CodeLens {
            range,
            command: None,
            data: Some(serde_json::json!({
                "kind": "phpMemberReferences",
                "uri": origin_uri,
                "position": position,
                "offset": declaration_offset,
                "classFqn": class_fqn.as_str(),
                "member": member.as_str(),
                "isStatic": is_static,
            })),
        })
    }

    fn class_declaration_name_offset(&self, uri: &str, class: &ClassInfo) -> u32 {
        let maps = self.symbol_maps.read();
        let Some(map) = maps.get(uri) else {
            return class.keyword_offset;
        };
        map.spans
            .iter()
            .find(|span| {
                matches!(
                    &span.kind,
                    SymbolKind::ClassDeclaration { name } if *name == class.name
                ) && span.start >= class.decl_start_offset
                    && span.start <= class.start_offset
            })
            .map(|span| span.start)
            .unwrap_or(class.keyword_offset)
    }

    fn reference_lens_command(
        origin_uri: Url,
        origin_position: Position,
        locations: Vec<Location>,
    ) -> Command {
        let count = locations.len();
        Command {
            title: format!(
                "{count} {}",
                if count == 1 {
                    "reference"
                } else {
                    "references"
                }
            ),
            command: "editor.action.showReferences".to_string(),
            arguments: Some(vec![
                serde_json::json!(origin_uri),
                serde_json::json!(origin_position),
                serde_json::json!(locations),
            ]),
        }
    }

    pub(crate) fn resolve_code_lens_item(&self, mut lens: CodeLens) -> CodeLens {
        if lens.command.is_some() {
            return lens;
        }
        let Some(data) = lens.data.as_ref() else {
            return lens;
        };
        let Some(kind) = data.get("kind").and_then(serde_json::Value::as_str) else {
            return lens;
        };
        let Some(uri) = data.get("uri").and_then(serde_json::Value::as_str) else {
            return lens;
        };
        let Some(position) = data
            .get("position")
            .cloned()
            .and_then(|value| serde_json::from_value::<Position>(value).ok())
        else {
            return lens;
        };
        let locations = match kind {
            "phpReferences" => {
                let Some(content) = self.get_file_content(uri) else {
                    return lens;
                };
                let Some(locations) =
                    self.find_references_from_workspace_index(uri, &content, position, false)
                else {
                    return lens;
                };
                locations
            }
            "phpMemberReferences" => {
                let Some(offset) = data
                    .get("offset")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|offset| u32::try_from(offset).ok())
                else {
                    return lens;
                };
                let Some(class_fqn) = data.get("classFqn").and_then(serde_json::Value::as_str)
                else {
                    return lens;
                };
                let Some(member) = data.get("member").and_then(serde_json::Value::as_str) else {
                    return lens;
                };
                let Some(is_static) = data.get("isStatic").and_then(serde_json::Value::as_bool)
                else {
                    return lens;
                };
                self.resolve_member_ref_locations(
                    uri,
                    offset,
                    crate::atom::atom(class_fqn),
                    crate::atom::atom(member),
                    is_static,
                )
            }
            _ => return lens,
        };
        let Ok(origin_uri) = Url::parse(uri) else {
            return lens;
        };

        lens.command = Some(Self::reference_lens_command(
            origin_uri, position, locations,
        ));
        lens
    }

    /// Build the "which tests cover this class" lens for a class
    /// declaration, from the test classes whose PHPUnit coverage metadata
    /// (`@covers` / `@uses` / `#[CoversClass]` and friends) names it.
    ///
    /// `None` when the class has no keyword position (synthetic/anonymous
    /// classes), no test declares coverage for it, or none of the test
    /// classes that do can be located on disk any more.  Also `None` while
    /// the workspace is still indexing, since
    /// [`Backend::find_covering_test_classes`] cannot answer until then.
    fn build_covers_lens(&self, class: &ClassInfo, uri: &str, content: &str) -> Option<CodeLens> {
        if class.keyword_offset == 0 {
            return None;
        }

        let locations: Vec<(Atom, Location)> = self
            .find_covering_test_classes(&class.fqn())
            .into_iter()
            .filter_map(|(test_fqn, test_uri)| {
                let location =
                    self.covers_lens_target(test_fqn.as_str(), &test_uri, uri, content)?;
                Some((test_fqn, location))
            })
            .collect();
        if locations.is_empty() {
            return None;
        }

        let pos = offset_to_position(content, class.keyword_offset as usize);
        let indent = line_indent(content, class.keyword_offset as usize);
        let range = Range {
            start: Position {
                line: pos.line,
                character: indent,
            },
            end: Position {
                line: pos.line,
                character: indent,
            },
        };

        let command = if let [(test_fqn, location)] = locations.as_slice() {
            let title = format!("Tests: {}", crate::util::short_name(test_fqn));
            self.build_code_lens_command(title, location.uri.clone(), location.range.start)
        } else {
            let title = format!("Tests: {} tests", locations.len());
            let current_uri: Url = match uri.parse() {
                Ok(u) => u,
                Err(_) => return None,
            };
            Command {
                title,
                command: "editor.action.showReferences".to_string(),
                arguments: Some(vec![
                    serde_json::json!(current_uri),
                    serde_json::json!(range.start),
                    serde_json::json!(locations.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>()),
                ]),
            }
        };

        Some(CodeLens {
            range,
            command: Some(command),
            data: None,
        })
    }

    /// The location of a covering test class's own declaration, for the
    /// covers lens to navigate to.
    fn covers_lens_target(
        &self,
        test_fqn: &str,
        test_uri: &str,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Location> {
        let test_class = self.find_or_load_class(test_fqn)?;
        if test_class.keyword_offset == 0 {
            return None;
        }

        // The search already told us which file declares it, so read that
        // rather than looking the class up by name a second time.
        let file_content = if test_uri == current_uri {
            current_content.to_string()
        } else {
            self.get_file_content(test_uri)?
        };
        let position = offset_to_position(&file_content, test_class.keyword_offset as usize);
        let uri: Url = test_uri.parse().ok()?;

        Some(Location {
            uri,
            range: Range {
                start: position,
                end: position,
            },
        })
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
    /// LSP has no standard "go to this location" command, so the client
    /// decides which of the two shapes it can act on:
    ///
    /// * A client that advertises `window.showDocument` gets the custom
    ///   `phpantom.navigateToPrototype` command.  It comes back as a
    ///   `workspace/executeCommand`, and the server answers it with a
    ///   `window/showDocument` request.
    /// * A client that does not gets `editor.action.showReferences` with
    ///   the `(uri, position, locations)` argument triple VS Code
    ///   established, which such clients resolve on their own without a
    ///   round trip.  Zed is the notable one: it recognises the command
    ///   name but never answers `window/showDocument`.
    fn build_code_lens_command(&self, title: String, uri: Url, position: Position) -> Command {
        if self
            .supports_show_document
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Command {
                title,
                command: "phpantom.navigateToPrototype".to_string(),
                arguments: Some(vec![serde_json::json!(uri), serde_json::json!(position)]),
            };
        }

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
