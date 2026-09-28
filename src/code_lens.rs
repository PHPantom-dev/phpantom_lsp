//! Code Lens (`textDocument/codeLens`) support.
//!
//! Shows reference and implementation counts plus override/implement,
//! Symfony, and Doctrine relationship annotations.

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::find_class_at_offset;
use crate::definition::implementation::member_implementation_providers;
use crate::definition::member::MemberKind;
use crate::framework::{FrameworkReferenceKind, SymfonySymbolKind};
use crate::inheritance::find_declaring_ancestor;
use crate::reference_index::ReferenceIndexKey;
use crate::references::{
    doctrine_repository_matches_entity_convention, looks_like_doctrine_repository,
};
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::text_position::{LineIndex, offset_to_position};
use crate::types::{ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, MethodInfo, Visibility};
use crate::util::short_name;

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
    /// Returns reference lenses for PHP declarations, implementation
    /// lenses for interfaces and abstract classes and their methods,
    /// navigation lenses for methods that override or implement an
    /// ancestor declaration, and indexed Symfony and Doctrine relationship
    /// lenses.
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
        let class_loader = |name: &str| self.find_or_load_class(name);
        let mut seen = HashSet::new();

        for class in &classes {
            let class_fqn = class.fqn();
            let name_offset = class_declaration_name_offset(symbol_map.as_deref(), class);

            if let Some(lens) = self.build_declaration_reference_lens(
                uri,
                &index,
                name_offset,
                &ReferenceIndexKey::class(&class_fqn),
            ) {
                lenses.push(lens);
            }

            let descendants = self.implementation_lens_descendants(class, &class_loader);
            if let Some(descendants) = &descendants {
                let count = descendants.iter().filter(|d| !d.is_abstract).count();
                lenses.extend(Self::build_implementation_lens(
                    uri,
                    &index,
                    name_offset,
                    count,
                    class_fqn,
                    None,
                ));
            }

            if let Some(lens) = self.build_covers_lens(class, uri, &index) {
                lenses.push(lens);
            }
            self.push_framework_class_lenses(
                uri,
                content,
                &index,
                class,
                &class_loader,
                (&mut lenses, &mut seen),
            );
            // The ancestors a framework route may name a method through, built
            // on the first method some framework resource mentions.
            let framework_scope = std::cell::OnceCell::new();
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
                if let Some(descendants) = &descendants {
                    let count = member_implementation_providers(
                        &class_fqn,
                        &method.name,
                        MemberKind::Method,
                        descendants,
                        &class_loader,
                    )
                    .len();
                    if count > 0 {
                        lenses.extend(Self::build_implementation_lens(
                            uri,
                            &index,
                            method.name_offset,
                            count,
                            class_fqn,
                            Some(method.name),
                        ));
                    }
                }
                if let Some(proto) = proto {
                    let icon = if proto.is_interface { "◆" } else { "↑" };
                    let title = format!("{} {}::{}", icon, proto.ancestor_name, method.name);

                    let target_uri: Url = match proto.file_uri.parse() {
                        Ok(u) => u,
                        Err(_) => continue,
                    };

                    let command = self.build_code_lens_command(title, target_uri, proto.position);

                    push_unique_lens(
                        &mut lenses,
                        &mut seen,
                        CodeLens {
                            range,
                            command: Some(command),
                            data: None,
                        },
                    );
                }

                self.push_framework_method_lenses(
                    uri,
                    &index,
                    class,
                    &framework_scope,
                    method,
                    (&mut lenses, &mut seen),
                );
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

        self.push_symfony_route_attribute_lenses(
            uri,
            content,
            &index,
            &classes,
            &mut lenses,
            &mut seen,
        );
        self.push_doctrine_get_repository_lenses(
            uri,
            content,
            &index,
            &class_loader,
            &mut lenses,
            &mut seen,
        );
        self.push_symfony_resource_lenses(uri, content, &index, &mut lenses, &mut seen);

        lenses.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
                .then(lens_title(a).cmp(&lens_title(b)))
        });

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
        let mut candidate_count = self.indexed_member_reference_count(&member)?;
        // A scope or an accessor is used under a name of its own, which
        // the search counts too.
        if let Some(magic) = self.eloquent_magic_member_at(origin_uri, declaration_offset, &member)
        {
            candidate_count += self.indexed_member_reference_count(&magic.use_name)?;
        }
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
        Self::locations_lens_command(
            origin_uri,
            origin_position,
            locations,
            "reference",
            "references",
        )
    }

    fn implementation_lens_command(
        origin_uri: Url,
        origin_position: Position,
        locations: Vec<Location>,
    ) -> Command {
        Self::locations_lens_command(
            origin_uri,
            origin_position,
            locations,
            "implementation",
            "implementations",
        )
    }

    /// A lens command titled with the number of `locations`, which opens
    /// them as a list when clicked.
    fn locations_lens_command(
        origin_uri: Url,
        origin_position: Position,
        locations: Vec<Location>,
        singular: &str,
        plural: &str,
    ) -> Command {
        let count = locations.len();
        Command {
            title: format!("{count} {}", if count == 1 { singular } else { plural }),
            command: "editor.action.showReferences".to_string(),
            arguments: Some(vec![
                serde_json::json!(origin_uri),
                serde_json::json!(origin_position),
                serde_json::json!(locations),
            ]),
        }
    }

    /// The classes the implementation lenses of `class` count, or `None`
    /// when it gets none: only an interface or an abstract class does, and
    /// only once the workspace index can answer.
    fn implementation_lens_descendants(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<Vec<Arc<ClassInfo>>> {
        if !(class.kind == ClassLikeKind::Interface || class.is_abstract)
            || class.keyword_offset == 0
            || !self
                .workspace_indexed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return None;
        }
        Some(self.implementation_descendants(class, &class.fqn(), class_loader, true))
    }

    /// Build the implementation lens for a class, or for one of its
    /// methods when `member` is given.
    ///
    /// `count` is worked out from class metadata alone.  Locating each
    /// implementation means reading the file that declares it, so a
    /// non-zero lens leaves that to the resolve request, which only the
    /// lenses the editor actually shows receive.
    fn build_implementation_lens(
        origin_uri: &str,
        index: &LineIndex,
        declaration_offset: u32,
        count: usize,
        class_fqn: Atom,
        member: Option<Atom>,
    ) -> Option<CodeLens> {
        if declaration_offset == 0 {
            return None;
        }
        let position = index.position(declaration_offset as usize);
        let range = Range::new(
            Position::new(position.line, 0),
            Position::new(position.line, 0),
        );
        if count == 0 {
            return Some(CodeLens {
                range,
                command: Some(Self::implementation_lens_command(
                    Url::parse(origin_uri).ok()?,
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
                "kind": "phpImplementations",
                "uri": origin_uri,
                "position": position,
                "classFqn": class_fqn.as_str(),
                "member": member.as_ref().map(Atom::as_str),
            })),
        })
    }

    /// The locations an implementation lens lists, recomputed from the
    /// class and member it was built for.
    fn implementation_lens_locations(
        &self,
        uri: &str,
        content: &str,
        class_fqn: &str,
        member: Option<&str>,
    ) -> Option<Vec<Location>> {
        let class_loader = |name: &str| self.find_or_load_class(name);
        let class = class_loader(class_fqn)?;
        let descendants = self.implementation_descendants(&class, class_fqn, &class_loader, true);
        Some(match member {
            None => self.class_implementation_locations(uri, content, &class, &descendants),
            Some(member) => {
                let providers = member_implementation_providers(
                    class_fqn,
                    member,
                    MemberKind::Method,
                    &descendants,
                    &class_loader,
                );
                self.member_implementation_locations(
                    uri,
                    content,
                    member,
                    MemberKind::Method,
                    &providers,
                )
            }
        })
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
        // Resolving references needs the type engine, the chain cache and
        // a parse of the file. Going through the shared request helper
        // installs all of them (and the panic guard) once, and hands over
        // the buffer without copying it.
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
                "phpImplementations" => {
                    let class_fqn = data.get("classFqn").and_then(serde_json::Value::as_str)?;
                    let member = data.get("member").and_then(serde_json::Value::as_str);
                    self.implementation_lens_locations(uri, content, class_fqn, member)
                }
                _ => None,
            });
        let Some(locations) = locations.flatten() else {
            return lens;
        };
        let Ok(origin_uri) = Url::parse(uri) else {
            return lens;
        };

        lens.command = Some(if kind == "phpImplementations" {
            Self::implementation_lens_command(origin_uri, position, locations)
        } else {
            Self::reference_lens_command(origin_uri, position, locations)
        });
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

    fn push_symfony_resource_lenses(
        &self,
        uri: &str,
        content: &str,
        index: &LineIndex,
        lenses: &mut Vec<CodeLens>,
        seen: &mut HashSet<String>,
    ) {
        let Some(references) = self.framework_references.read().get(uri).cloned() else {
            return;
        };

        for (idx, declaration) in references.iter().enumerate() {
            let FrameworkReferenceKind::SymfonySymbol {
                kind,
                name,
                declaration: true,
            } = &declaration.kind
            else {
                continue;
            };
            if !matches!(
                kind,
                SymfonySymbolKind::Service
                    | SymfonySymbolKind::Parameter
                    | SymfonySymbolKind::Route
                    | SymfonySymbolKind::Template
            ) {
                continue;
            }

            let pos = index.position(declaration.start as usize);
            let usages = self.framework_symfony_symbol_locations(*kind, name, false, true);
            if !usages.is_empty() {
                let title = format!(
                    "Symfony {}: {} {}",
                    kind.label(),
                    usages.len(),
                    if usages.len() == 1 { "ref" } else { "refs" }
                );
                self.push_locations_lens(uri, pos, title, usages, lenses, seen);
            }

            if matches!(
                kind,
                SymfonySymbolKind::Parameter | SymfonySymbolKind::Template
            ) {
                continue;
            }
            let block_end = references
                .iter()
                .skip(idx + 1)
                .find_map(|candidate| {
                    matches!(
                        &candidate.kind,
                        FrameworkReferenceKind::SymfonySymbol {
                            kind: candidate_kind,
                            declaration: true,
                            ..
                        } if candidate_kind == kind
                    )
                    .then_some(candidate.start)
                })
                .unwrap_or(content.len() as u32);

            if *kind == SymfonySymbolKind::Route {
                if let Some((class_fqn, member_name)) = references.iter().find_map(|candidate| {
                    if candidate.start <= declaration.start || candidate.start >= block_end {
                        return None;
                    }
                    let FrameworkReferenceKind::Method {
                        class_fqn,
                        member_name,
                    } = &candidate.kind
                    else {
                        return None;
                    };
                    Some((class_fqn.as_str(), member_name.as_str()))
                }) && let Some(location) =
                    self.resolve_framework_member_definition(uri, content, class_fqn, member_name)
                {
                    self.push_locations_lens(
                        uri,
                        pos,
                        format!(
                            "Symfony controller: {}::{}",
                            short_name(class_fqn),
                            member_name
                        ),
                        vec![location],
                        lenses,
                        seen,
                    );
                }
                continue;
            }

            if let Some(class_fqn) = references.iter().find_map(|candidate| {
                if candidate.start <= declaration.start || candidate.start >= block_end {
                    return None;
                }
                let FrameworkReferenceKind::Class { fqn } = &candidate.kind else {
                    return None;
                };
                Some(fqn.as_str())
            }) && let Some(location) = self.class_location(class_fqn, uri, content)
            {
                self.push_locations_lens(
                    uri,
                    pos,
                    format!("Symfony service class: {}", short_name(class_fqn)),
                    vec![location],
                    lenses,
                    seen,
                );
            }
        }
    }

    fn push_framework_class_lenses(
        &self,
        uri: &str,
        content: &str,
        index: &LineIndex,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        output: (&mut Vec<CodeLens>, &mut HashSet<String>),
    ) {
        let (lenses, seen) = output;
        let class_fqn = class.fqn();
        let Some(source_pos) = class_lens_position(index, class) else {
            return;
        };

        let config_locations = self.framework_class_reference_locations(class_fqn.as_str());
        if !config_locations.is_empty() {
            let title = if config_locations.len() == 1 {
                "Symfony/Doctrine config: 1 ref".to_string()
            } else {
                format!("Symfony/Doctrine config: {} refs", config_locations.len())
            };
            self.push_locations_lens(
                uri,
                source_pos,
                title,
                config_locations,
                &mut *lenses,
                &mut *seen,
            );
        }

        for repo_fqn in self
            .doctrine_repository_fqns_for_entity(class_fqn.as_str(), class_loader)
            .into_iter()
            .filter(|fqn| !is_builtin_doctrine_repository_fqn(fqn))
        {
            if let Some(location) = self.class_location(&repo_fqn, uri, content) {
                let title = format!("Doctrine repository: {}", short_name(&repo_fqn));
                self.push_locations_lens(
                    uri,
                    source_pos,
                    title,
                    vec![location],
                    &mut *lenses,
                    &mut *seen,
                );
            }
        }

        for entity_fqn in self.doctrine_entities_for_repository(class_fqn.as_str(), class_loader) {
            if let Some(location) = self.class_location(&entity_fqn, uri, content) {
                let title = format!("Doctrine entity: {}", short_name(&entity_fqn));
                self.push_locations_lens(
                    uri,
                    source_pos,
                    title,
                    vec![location],
                    &mut *lenses,
                    &mut *seen,
                );
            }
        }
    }

    fn push_framework_method_lenses(
        &self,
        uri: &str,
        index: &LineIndex,
        class: &ClassInfo,
        scope: &std::cell::OnceCell<crate::references::MemberScope>,
        method: &MethodInfo,
        output: (&mut Vec<CodeLens>, &mut HashSet<String>),
    ) {
        let (lenses, seen) = output;
        let method_name = method.name.as_str();
        if !self.has_framework_member_references(method_name) {
            return;
        }
        let scope = scope.get_or_init(|| {
            let mut hierarchy = HashSet::new();
            hierarchy.insert(class.fqn().to_string());
            hierarchy.extend(self.class_hierarchy_names(class));
            crate::references::MemberScope::exact(hierarchy)
        });

        let route_locations = self.framework_member_reference_locations(method_name, Some(scope));
        if route_locations.is_empty() {
            return;
        }
        let pos = index.position(method.name_offset as usize);

        let title = if route_locations.len() == 1 {
            "Symfony route config: 1 ref".to_string()
        } else {
            format!("Symfony route config: {} refs", route_locations.len())
        };
        self.push_locations_lens(uri, pos, title, route_locations, lenses, seen);
    }

    fn push_symfony_route_attribute_lenses(
        &self,
        uri: &str,
        content: &str,
        index: &LineIndex,
        classes: &[Arc<ClassInfo>],
        lenses: &mut Vec<CodeLens>,
        seen: &mut HashSet<String>,
    ) {
        let attributes = route_attributes(content);
        if attributes.is_empty() {
            return;
        }
        let declarations = code_lens_declarations(index, classes);
        if declarations.is_empty() {
            return;
        }

        for attr in attributes {
            // `declarations` is sorted by offset, so the first one past the
            // attribute is the declaration it decorates.
            let next = declarations.partition_point(|decl| decl.offset <= attr.end);
            let Some(decl) = declarations.get(next) else {
                continue;
            };
            if decl.offset.saturating_sub(attr.end) > 1024 {
                continue;
            }
            let between = &content[attr.end..decl.offset];
            if between.contains(';') || between.contains('{') || between.contains('}') {
                continue;
            }
            let Some(title) = route_attribute_lens_title(&attr, decl.kind) else {
                continue;
            };

            let source_pos = decl.position;
            let Ok(parsed_uri) = Url::parse(uri) else {
                continue;
            };
            let location = Location {
                uri: parsed_uri,
                range: Range {
                    start: source_pos,
                    end: source_pos,
                },
            };
            self.push_locations_lens(uri, source_pos, title, vec![location], lenses, seen);
        }
    }

    fn push_doctrine_get_repository_lenses(
        &self,
        uri: &str,
        content: &str,
        index: &LineIndex,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        lenses: &mut Vec<CodeLens>,
        seen: &mut HashSet<String>,
    ) {
        let calls = get_repository_calls(content);
        if calls.is_empty() {
            return;
        }
        let ctx = self.file_context(uri);
        for call in calls {
            let offset = call.offset as u32;
            let Some(entity_fqn) = class_expr_arg_to_fqn(
                &call.first_arg,
                ctx.use_map_at(offset),
                ctx.namespace_at(offset),
                &ctx.classes,
                offset,
            ) else {
                continue;
            };
            let mut locations = Vec::new();
            let mut title = None;

            for repo_fqn in self
                .doctrine_repository_fqns_for_entity(&entity_fqn, class_loader)
                .into_iter()
                .filter(|fqn| !is_builtin_doctrine_repository_fqn(fqn))
            {
                if let Some(location) = self.class_location(&repo_fqn, uri, content) {
                    title = Some(format!("Doctrine repository: {}", short_name(&repo_fqn)));
                    locations.push(location);
                    break;
                }
            }

            if locations.is_empty()
                && let Some(location) = self.class_location(&entity_fqn, uri, content)
            {
                title = Some(format!("Doctrine entity: {}", short_name(&entity_fqn)));
                locations.push(location);
            }

            let Some(title) = title else {
                continue;
            };
            let pos = index.position(call.offset);
            self.push_locations_lens(uri, pos, title, locations, lenses, seen);
        }
    }

    fn push_locations_lens(
        &self,
        origin_uri: &str,
        source_pos: Position,
        title: String,
        locations: Vec<Location>,
        lenses: &mut Vec<CodeLens>,
        seen: &mut HashSet<String>,
    ) {
        if locations.is_empty() {
            return;
        }
        let Ok(origin_url) = Url::parse(origin_uri) else {
            return;
        };
        let range = Range {
            start: Position {
                line: source_pos.line,
                character: 0,
            },
            end: Position {
                line: source_pos.line,
                character: 0,
            },
        };
        let command =
            self.build_code_lens_locations_command(title, origin_url, source_pos, locations);
        push_unique_lens(
            lenses,
            seen,
            CodeLens {
                range,
                command: Some(command),
                data: None,
            },
        );
    }

    fn class_location(
        &self,
        fqn: &str,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Location> {
        let class_info = self.find_or_load_class(fqn)?;
        let class_fqn = class_info.fqn();
        let (file_uri, file_content) =
            self.find_class_file_content(&class_fqn, current_uri, current_content)?;
        let offset = class_info
            .keyword_offset
            .max(class_info.decl_start_offset)
            .min(file_content.len() as u32);
        if offset == 0 {
            return None;
        }
        let uri = Url::parse(&file_uri).ok()?;
        let pos = offset_to_position(&file_content, offset as usize);
        Some(Location {
            uri,
            range: Range {
                start: pos,
                end: pos,
            },
        })
    }

    fn doctrine_entities_for_repository(
        &self,
        repository_fqn: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<String> {
        let mut out = self.framework_doctrine_entity_fqns_for_repository(repository_fqn);
        let repository = normalize_class_name(repository_fqn);
        if !out.is_empty() {
            return out;
        }
        let Some(repository_class) = class_loader(&repository) else {
            return out;
        };
        if !looks_like_doctrine_repository(&repository_class) {
            return out;
        }

        let mut candidates: Vec<String> = Vec::new();
        {
            let index = self.symbols.fqn_class_index.read();
            candidates.extend(index.keys().map(|key| key.to_string()));
        }
        {
            let uri_index = self.symbols.uri_classes_index.read();
            for classes in uri_index.values() {
                for class in classes {
                    candidates.push(class.fqn().to_string());
                }
            }
        }
        candidates.sort();
        candidates.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        for entity_fqn in candidates {
            if entity_fqn.eq_ignore_ascii_case(&repository) {
                continue;
            }
            if !looks_like_doctrine_entity_name(&entity_fqn) {
                continue;
            }
            if doctrine_repository_matches_entity_convention(&entity_fqn, &repository)
                && !out
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(&entity_fqn))
            {
                out.push(entity_fqn);
            }
        }

        out
    }

    fn class_hierarchy_names(&self, class: &ClassInfo) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut parent_name = class.parent_class;
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let Some(name) = parent_name else {
                break;
            };
            let parent_fqn = name.to_string();
            if out
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&parent_fqn))
            {
                break;
            }
            out.push(parent_fqn);
            let Some(parent) = self.find_or_load_class(&name) else {
                break;
            };
            parent_name = parent.parent_class;
        }
        out
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

    fn build_code_lens_locations_command(
        &self,
        title: String,
        origin_uri: Url,
        origin_position: Position,
        locations: Vec<Location>,
    ) -> Command {
        Command {
            title,
            command: "editor.action.showReferences".to_string(),
            arguments: Some(vec![
                serde_json::json!(origin_uri),
                serde_json::json!(origin_position),
                serde_json::json!(locations),
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

#[derive(Clone, Copy)]
struct CodeLensDeclaration {
    offset: usize,
    position: Position,
    kind: RouteDeclarationKind,
}

#[derive(Clone, Copy)]
enum RouteDeclarationKind {
    Class,
    Method,
}

struct RouteAttribute {
    end: usize,
    path: Option<String>,
    name: Option<String>,
    methods: Vec<String>,
}

struct GetRepositoryCall {
    offset: usize,
    first_arg: String,
}

fn class_lens_position(index: &LineIndex, class: &ClassInfo) -> Option<Position> {
    let offset = if class.keyword_offset > 0 {
        class.keyword_offset
    } else {
        class.decl_start_offset
    };
    if offset == 0 || offset as usize > index.content().len() {
        None
    } else {
        Some(index.position(offset as usize))
    }
}

fn code_lens_declarations(
    index: &LineIndex,
    classes: &[Arc<ClassInfo>],
) -> Vec<CodeLensDeclaration> {
    let mut declarations = Vec::new();
    for class in classes {
        if let Some(position) = class_lens_position(index, class) {
            let offset = if class.keyword_offset > 0 {
                class.keyword_offset
            } else {
                class.decl_start_offset
            };
            declarations.push(CodeLensDeclaration {
                offset: offset as usize,
                position,
                kind: RouteDeclarationKind::Class,
            });
        }
        for method in &class.methods {
            if method.name_offset == 0 || method.is_virtual {
                continue;
            }
            declarations.push(CodeLensDeclaration {
                offset: method.name_offset as usize,
                position: index.position(method.name_offset as usize),
                kind: RouteDeclarationKind::Method,
            });
        }
    }
    declarations.sort_by_key(|decl| decl.offset);
    declarations
}

fn route_attributes(content: &str) -> Vec<RouteAttribute> {
    let mut attributes = Vec::new();
    let mut search = 0usize;
    while let Some(rel) = content[search..].find("#[") {
        let start = search + rel;
        let Some(end) = find_attribute_end(content, start) else {
            break;
        };
        let attr = &content[start..end];
        if !is_route_attribute(attr) {
            search = end;
            continue;
        }
        let args = attr
            .find('(')
            .zip(attr.rfind(')'))
            .and_then(|(open, close)| (close > open).then_some(&attr[open + 1..close]))
            .unwrap_or("");
        let path = find_named_string_arg(args, "path").or_else(|| first_string_literal(args));
        let name = find_named_string_arg(args, "name");
        let methods = find_methods_arg(args);
        attributes.push(RouteAttribute {
            end,
            path,
            name,
            methods,
        });
        search = end;
    }
    attributes
}

fn route_attribute_lens_title(
    attr: &RouteAttribute,
    decl_kind: RouteDeclarationKind,
) -> Option<String> {
    let mut title = String::new();
    match decl_kind {
        RouteDeclarationKind::Class => title.push_str("Symfony route prefix"),
        RouteDeclarationKind::Method => title.push_str("Symfony route"),
    }

    let mut parts = Vec::new();
    if !attr.methods.is_empty() {
        parts.push(attr.methods.join("|"));
    }
    if let Some(path) = &attr.path
        && !path.is_empty()
    {
        parts.push(path.clone());
    }
    if let Some(name) = &attr.name
        && !name.is_empty()
    {
        parts.push(format!("({name})"));
    }

    if parts.is_empty() {
        None
    } else {
        title.push_str(": ");
        title.push_str(&parts.join(" "));
        Some(title)
    }
}

fn find_attribute_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut i = start + 2;
    let mut depth = 1usize;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let byte = bytes[i];
        if let Some(q) = quote {
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'[' => depth += 1,
            b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_route_attribute(attr: &str) -> bool {
    let lower = attr.to_ascii_lowercase();
    lower.starts_with("#[route")
        || lower.starts_with("#[\\symfony\\component\\routing\\attribute\\route")
        || lower.starts_with("#[symfony\\component\\routing\\attribute\\route")
        || lower.starts_with("#[\\symfony\\component\\routing\\annotation\\route")
        || lower.starts_with("#[symfony\\component\\routing\\annotation\\route")
}

fn find_named_string_arg(args: &str, name: &str) -> Option<String> {
    let pattern = format!("{name}:");
    let mut search = 0usize;
    while let Some(rel) = args[search..].find(&pattern) {
        let start = search + rel;
        if start > 0 {
            let prev = args.as_bytes()[start - 1];
            if prev == b'_' || prev.is_ascii_alphanumeric() {
                search = start + pattern.len();
                continue;
            }
        }
        let value_start = start + pattern.len();
        return first_string_literal(&args[value_start..]);
    }
    None
}

fn first_string_literal(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote != b'\'' && quote != b'"' {
            i += 1;
            continue;
        }
        let mut value = String::new();
        i += 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                value.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if bytes[i] == quote {
                return Some(value);
            }
            value.push(bytes[i] as char);
            i += 1;
        }
        return None;
    }
    None
}

fn find_methods_arg(args: &str) -> Vec<String> {
    let Some(start) = args.find("methods:") else {
        return Vec::new();
    };
    let tail = &args[start + "methods:".len()..];
    let end = tail.find("]").map(|idx| idx + 1).unwrap_or_else(|| {
        tail.find(',')
            .or_else(|| tail.find(')'))
            .unwrap_or(tail.len())
    });
    let segment = &tail[..end];
    let mut out = Vec::new();
    let mut search = 0usize;
    while let Some(method) = first_string_literal(&segment[search..]) {
        let Some(pos) = segment[search..].find(&method) else {
            break;
        };
        let method_len = method.len();
        if !out.iter().any(|known: &String| known == &method) {
            out.push(method);
        }
        search += pos + method_len + 1;
        if search >= segment.len() {
            break;
        }
    }
    out
}

fn get_repository_calls(content: &str) -> Vec<GetRepositoryCall> {
    let mut calls = Vec::new();
    let mut search = 0usize;
    while let Some(rel) = content[search..].find("getRepository") {
        let name_start = search + rel;
        let name_end = name_start + "getRepository".len();
        if name_start > 0 && is_ident_byte(content.as_bytes()[name_start - 1]) {
            search = name_end;
            continue;
        }
        if content
            .as_bytes()
            .get(name_end)
            .is_some_and(|byte| is_ident_byte(*byte))
        {
            search = name_end;
            continue;
        }
        let Some(open) = content[name_end..].find('(').map(|open| name_end + open) else {
            break;
        };
        if !content[name_end..open].trim().is_empty() {
            search = name_end;
            continue;
        }
        let Some(close) = find_matching_paren(content, open) else {
            break;
        };
        let args = &content[open + 1..close];
        if let Some(first_arg) = split_first_arg(args) {
            calls.push(GetRepositoryCall {
                offset: name_start,
                first_arg: first_arg.to_string(),
            });
        }
        search = close + 1;
    }
    calls
}

fn class_expr_arg_to_fqn(
    first_arg: &str,
    use_map: &std::collections::HashMap<String, String>,
    namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    access_offset: u32,
) -> Option<String> {
    let class_expr = first_arg.trim().strip_suffix("::class")?.trim();
    let class_expr = class_expr.trim_start_matches('\\');
    if class_expr.is_empty() {
        return None;
    }
    match class_expr {
        "self" | "static" => {
            find_class_at_offset(local_classes, access_offset).map(|class| class.fqn().to_string())
        }
        "parent" => find_class_at_offset(local_classes, access_offset)
            .and_then(|class| class.parent_class.map(|parent| parent.to_string())),
        _ => Some(Backend::resolve_to_fqn(class_expr, use_map, namespace)),
    }
}

fn find_matching_paren(content: &str, open: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = open;
    while i < bytes.len() {
        let byte = bytes[i];
        if let Some(q) = quote {
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn split_first_arg(args: &str) -> Option<&str> {
    let bytes = args.as_bytes();
    let mut quote: Option<u8> = None;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    for (i, byte) in bytes.iter().enumerate() {
        if let Some(q) = quote {
            if *byte == b'\\' {
                continue;
            }
            if *byte == q {
                quote = None;
            }
            continue;
        }
        match *byte {
            b'\'' | b'"' => quote = Some(*byte),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b',' if paren_depth == 0 && bracket_depth == 0 => return Some(args[..i].trim()),
            _ => {}
        }
    }
    let trimmed = args.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn is_ident_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn is_builtin_doctrine_repository_fqn(fqn: &str) -> bool {
    let normalized = normalize_class_name(fqn);
    normalized.starts_with("Doctrine\\")
        || matches!(
            short_name(&normalized),
            "ServiceEntityRepository" | "EntityRepository" | "ObjectRepository"
        )
}

fn looks_like_doctrine_entity_name(fqn: &str) -> bool {
    let normalized = normalize_class_name(fqn);
    normalized.contains("\\Entity\\")
        || normalized.contains("\\Entities\\")
        || short_name(&normalized).ends_with("Entity")
}

fn normalize_class_name(name: &str) -> String {
    name.trim().trim_start_matches('\\').to_string()
}

fn push_unique_lens(lenses: &mut Vec<CodeLens>, seen: &mut HashSet<String>, lens: CodeLens) {
    let title = lens_title(&lens);
    let key = format!(
        "{}:{}:{}",
        lens.range.start.line, lens.range.start.character, title
    );
    if seen.insert(key) {
        lenses.push(lens);
    }
}

fn lens_title(lens: &CodeLens) -> String {
    lens.command
        .as_ref()
        .map(|command| command.title.clone())
        .unwrap_or_default()
}
