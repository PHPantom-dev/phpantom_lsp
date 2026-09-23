//! Code Lens (`textDocument/codeLens`) support.
//!
//! Shows reference counts plus override/implement annotations.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::definition::member::MemberKind;
use crate::inheritance::find_declaring_ancestor;
use crate::reference_index::ReferenceIndexKey;
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::text_position::{LineIndex, offset_to_position};
use crate::types::{ClassInfo, ClassLikeKind, Visibility};

/// Shown while a declaration's references are being counted, so the lens
/// keeps its line instead of vanishing and shifting the file, and reads as
/// the count it is about to become.
const PENDING_COUNT_TITLE: &str = "- references";

/// The offset of the class' own name, so the reference lens sits on the
/// declaration line rather than on a preceding attribute or docblock.
fn class_declaration_name_offset(symbol_map: Option<&SymbolMap>, class: &ClassInfo) -> u32 {
    let Some(map) = symbol_map else {
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

/// The column the line containing `byte_offset` starts its content at,
/// so a lens sits above the declaration rather than in the left margin.
fn line_indent(index: &LineIndex, line: usize) -> u32 {
    index.content()[index.line_start(line)..]
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
        let symbol_map = self.symbol_maps.read().get(uri).cloned();

        // One line table for the whole request: every lens converts at
        // least one offset from this content, and `offset_to_position` is
        // O(offset) on each call.
        let index = LineIndex::new(content);

        let mut lenses = Vec::new();

        for class in &classes {
            let class_fqn = class.fqn();

            if let Some(lens) = self.build_declaration_reference_lens(
                uri,
                &index,
                class_declaration_name_offset(symbol_map.as_deref(), class),
                &ReferenceIndexKey::class(&class_fqn),
            ) {
                lenses.push(lens);
            }

            if let Some(lens) = self.build_covers_lens(class, uri, &index) {
                lenses.push(lens);
            }

            for method in &class.methods {
                if method.name_offset == 0
                    || method.is_virtual
                    || method.visibility == crate::types::Visibility::Private
                {
                    continue;
                }

                let line = index.line_of(method.name_offset as usize);
                let indent = line_indent(&index, line);
                let position = Position {
                    line: line as u32,
                    character: indent,
                };
                let range = Range {
                    start: position,
                    end: position,
                };

                let proto = self.find_prototype(class, &method.name, uri, content);
                if !method.name.starts_with("__")
                    && let Some(lens) = self.build_member_reference_lens(
                        uri,
                        &index,
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
                    &index,
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
                    &index,
                    constant.name_offset,
                    class_fqn,
                    constant.name,
                    true,
                ) {
                    lenses.push(lens);
                }
            }
        }

        if let Some(symbol_map) = &symbol_map {
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
                    self.build_declaration_reference_lens(uri, &index, span.start, &key)
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
        index: &LineIndex,
        declaration_offset: u32,
        key: &ReferenceIndexKey,
    ) -> Option<CodeLens> {
        if declaration_offset == 0 {
            return None;
        }

        let candidate_count = self.indexed_reference_count(key)?;
        let origin_url = Url::parse(origin_uri).ok()?;
        let position = index.position(declaration_offset as usize);
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
        index: &LineIndex,
        declaration_offset: u32,
        class_fqn: Atom,
        member: Atom,
        is_static: bool,
    ) -> Option<CodeLens> {
        if declaration_offset == 0 {
            return None;
        }
        let candidate_count = self.indexed_member_reference_count(&member)?;
        let origin_url = Url::parse(origin_uri).ok()?;
        let position = index.position(declaration_offset as usize);
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

        // Clients with refresh support re-pull once the shared background
        // worker fills the exact cache, so the lens holds its line with a
        // placeholder rather than being omitted: a lens that comes and goes
        // moves every line of the file under the reader on each keystroke.
        // Resolving it eagerly instead would put the whole viewport's
        // searches back on the request.
        if supports_refresh {
            return Some(CodeLens {
                range,
                command: Some(Command {
                    title: PENDING_COUNT_TITLE.to_string(),
                    // No handler: the placeholder is text, not an action.
                    command: String::new(),
                    arguments: None,
                }),
                data: None,
            });
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

    pub fn resolve_code_lens_item(&self, mut lens: CodeLens) -> CodeLens {
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
        // Both kinds resolve references, which needs the type engine, the
        // chain cache and a parse of the file. Going through the shared
        // request helper installs all of them (and the panic guard) once,
        // and hands over the buffer without copying it.
        let locations =
            self.with_file_content("codeLens/resolve", uri, None, |content, _| match kind {
                "phpReferences" => self.find_references(uri, content, position, false),
                "phpMemberReferences" => {
                    let offset = data
                        .get("offset")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|offset| u32::try_from(offset).ok())?;
                    let class_fqn = data.get("classFqn").and_then(serde_json::Value::as_str)?;
                    let member = data.get("member").and_then(serde_json::Value::as_str)?;
                    let is_static = data.get("isStatic").and_then(serde_json::Value::as_bool)?;
                    Some(self.resolve_member_ref_locations(
                        uri,
                        offset,
                        crate::atom::atom(class_fqn),
                        crate::atom::atom(member),
                        is_static,
                    ))
                }
                _ => None,
            });
        let Some(locations) = locations.flatten() else {
            return lens;
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
    fn build_covers_lens(
        &self,
        class: &ClassInfo,
        uri: &str,
        index: &LineIndex,
    ) -> Option<CodeLens> {
        if class.keyword_offset == 0 {
            return None;
        }

        let locations: Vec<(Atom, Location)> = self
            .find_covering_test_classes(&class.fqn())
            .into_iter()
            .filter_map(|(test_fqn, test_uri)| {
                let location = self.covers_lens_target(test_fqn.as_str(), &test_uri, uri, index)?;
                Some((test_fqn, location))
            })
            .collect();
        if locations.is_empty() {
            return None;
        }

        let line = index.line_of(class.keyword_offset as usize);
        let position = Position {
            line: line as u32,
            character: line_indent(index, line),
        };
        let range = Range {
            start: position,
            end: position,
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
        current_index: &LineIndex,
    ) -> Option<Location> {
        let test_class = self.find_or_load_class(test_fqn)?;
        if test_class.keyword_offset == 0 {
            return None;
        }

        // The search already told us which file declares it, so read that
        // rather than looking the class up by name a second time.
        let offset = test_class.keyword_offset as usize;
        let position = if test_uri == current_uri {
            current_index.position(offset)
        } else {
            offset_to_position(&self.get_file_content(test_uri)?, offset)
        };
        let uri: Url = test_uri.parse().ok()?;

        Some(Location {
            uri,
            range: Range {
                start: position,
                end: position,
            },
        })
    }

    /// The closest ancestor that declares a method with the given name,
    /// as a navigation target.
    ///
    /// Returns `None` when no ancestor declares the method.
    fn find_prototype(
        &self,
        class: &ClassInfo,
        method_name: &str,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Prototype> {
        let class_loader = |name: &str| self.find_or_load_class(name);
        let declares = |candidate: &ClassInfo| {
            candidate
                .methods
                .iter()
                .any(|m| m.name == method_name && !m.is_virtual)
        };
        let (ancestor_name, ancestor) = find_declaring_ancestor(class, &class_loader, &declares)?;
        self.build_prototype(
            &ancestor_name,
            &ancestor,
            method_name,
            current_uri,
            current_content,
        )
    }

    /// Build the LSP `Command` that navigates to (or opens) a target
    /// location, for a code lens or a resolved code action alike.
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
    pub(crate) fn build_code_lens_command(
        &self,
        title: String,
        uri: Url,
        position: Position,
    ) -> Command {
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

        Some(Prototype {
            ancestor_name: ancestor_class.name.to_string(),
            is_interface: ancestor_class.kind == ClassLikeKind::Interface,
            file_uri,
            position,
        })
    }
}
