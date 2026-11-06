//! Code Lens (`textDocument/codeLens`) support.
//!
//! Shows reference and implementation counts, override/implement
//! annotations, the links between a Doctrine entity and its repository,
//! the links between a Messenger message and its handlers, the links
//! between a class and its tests, the routes that dispatch to a Laravel
//! controller method, the classes and files a Laravel model, event or
//! listener is paired with, and the renders of a Yii template.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::definition::implementation::member_implementation_providers;
use crate::definition::member::MemberKind;
use crate::inheritance::find_declaring_ancestor;
use crate::reference_counts::MemberSlot;
use crate::reference_index::ReferenceIndexKey;
use crate::references::MemberRefKind;
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::text_position::{LineIndex, offset_to_position};
use crate::types::{ClassInfo, ClassLikeKind, Visibility};

/// A location at the top of the file at `uri`.
fn file_top_location(uri: &str) -> Option<Location> {
    Some(Location {
        uri: uri.parse().ok()?,
        range: Range::default(),
    })
}

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

/// The title of the lens for a route that dispatches to a controller
/// method: its verbs, its URI and its name, as far as they are known
/// (`GET /orders/{order} · orders.show`).
fn route_lens_title(action: &crate::virtual_members::laravel::RouteAction) -> String {
    let mut title = action.verbs.clone();
    if !action.uri.is_empty() {
        if !title.is_empty() {
            title.push(' ');
        }
        if !action.uri.starts_with('/') {
            title.push('/');
        }
        title.push_str(&action.uri);
    }
    if let Some(name) = &action.name {
        if !title.is_empty() {
            title.push_str(" · ");
        }
        title.push_str(name);
    }
    if title.is_empty() {
        title.push_str("Route");
    }
    title
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
    /// ancestor declaration, in a Doctrine project the lenses that link an
    /// entity and its repository, in a Symfony project the ones that
    /// link a Messenger message and its handlers, the ones that link a
    /// class and its tests, and in a Laravel project the routes that
    /// dispatch to each controller method and the links between a model
    /// and its factory, policy, observers, migrations and resources, and
    /// between an event and its listeners.
    pub fn handle_code_lens(&self, uri: &str, content: &str) -> Option<Vec<CodeLens>> {
        let classes = {
            let map = self.symbols.uri_classes_index.read();
            map.get(uri).cloned().unwrap_or_default()
        };
        let symbol_map = self.symbol_map_for(uri);

        // One line table for the whole request: every lens converts at
        // least one offset from this content, and `offset_to_position` is
        // O(offset) on each call.
        let index = LineIndex::new(content);

        let mut lenses = Vec::new();
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let is_doctrine = self.is_doctrine_project();
        let is_symfony = self.is_symfony_project();
        // Discovering the routes reads every user file's symbol map, which
        // waits for the workspace index, so like the implementation lenses
        // they appear once indexing is done and the client re-pulls.
        let routes = (!classes.is_empty()
            && self.resolved_class_cache.read().laravel().is_enabled()
            && self
                .workspace_index
                .indexed
                .load(std::sync::atomic::Ordering::Acquire))
        .then(|| self.cached_routes());

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

            if is_doctrine && class.kind == ClassLikeKind::Class {
                self.push_doctrine_lenses(class, &index, &mut lenses);
            }

            if is_symfony {
                self.push_messenger_lenses(uri, class, &index, &mut lenses);
            }

            self.push_laravel_related_lenses(uri, class, &index, &mut lenses);

            let route_actions = routes
                .as_deref()
                .map_or(&[][..], |routes| routes.controller_actions(&class_fqn));

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

                for action in route_actions
                    .iter()
                    .filter(|action| action.method.eq_ignore_ascii_case(&method.name))
                {
                    lenses.push(CodeLens {
                        range,
                        command: Some(self.build_code_lens_command(
                            route_lens_title(action),
                            (*action.declaration.file).clone(),
                            action.declaration.position,
                        )),
                        data: None,
                    });
                }

                let proto = self.find_prototype(class, &method.name, uri, content);
                if !method.name.starts_with("__")
                    && let Some(lens) = self.build_member_reference_lens(
                        uri,
                        &index,
                        method.name_offset,
                        class_fqn,
                        method.name,
                        MemberSlot {
                            is_static: method.is_static,
                            kind: MemberRefKind::Method,
                        },
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

                    let target_uri: Uri = match proto.file_uri.parse() {
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
                    MemberSlot {
                        is_static: property.is_static,
                        kind: MemberRefKind::Property,
                    },
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
                    MemberSlot {
                        is_static: true,
                        kind: MemberRefKind::Constant,
                    },
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

        if let Some(lens) = self.build_test_subject_lens(uri, &classes, &index) {
            lenses.push(lens);
        }

        // A template declares no class, so only such a file is looked up
        // among the renders.
        if classes.is_empty()
            && self.is_yii_project()
            && let Some(lens) = self.build_yii_view_render_lens(uri)
        {
            lenses.push(lens);
        }

        if lenses.is_empty() {
            None
        } else {
            Some(lenses)
        }
    }

    /// The lens at the top of a Yii template listing the calls that render
    /// it, or `None` when nothing does.
    fn build_yii_view_render_lens(&self, uri: &str) -> Option<CodeLens> {
        let path = crate::util::uri_to_path(uri)?;
        let renders = self.yii_view_renders_of(&path);
        if renders.is_empty() {
            return None;
        }
        let position = Position::new(0, 0);
        Some(CodeLens {
            range: Range {
                start: position,
                end: position,
            },
            command: Some(Self::locations_lens_command(
                crate::util::parse_url(uri)?,
                position,
                renders,
                "render",
                "renders",
            )),
            data: None,
        })
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
        let origin_url = crate::util::parse_url(origin_uri)?;
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
        slot: MemberSlot,
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
        let origin_url = crate::util::parse_url(origin_uri)?;
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

        let supports_refresh = self.caps().code_lens_refresh;
        let cached_locations = if supports_refresh {
            self.member_ref_locations_cached(
                origin_uri,
                declaration_offset,
                class_fqn,
                member,
                slot,
            )
        } else {
            self.member_ref_locations_ready(origin_uri, declaration_offset, class_fqn, member, slot)
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
                    tooltip: None,
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
                "isStatic": slot.is_static,
                "isMethod": slot.kind == MemberRefKind::Method,
                "isConstant": slot.kind == MemberRefKind::Constant,
            })),
        })
    }

    fn reference_lens_command(
        origin_uri: Uri,
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
        origin_uri: Uri,
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
        origin_uri: Uri,
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
            tooltip: None,
        }
    }

    /// The classes the implementation lenses of `class` count, or `None`
    /// when it gets none: only an interface or an abstract class does, and
    /// only once the workspace index can answer.
    fn implementation_lens_descendants(
        &self,
        class: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<Vec<Arc<ClassInfo>>> {
        if !(class.kind == ClassLikeKind::Interface || class.is_abstract)
            || class.keyword_offset == 0
            || !self
                .workspace_index
                .indexed
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
                    crate::util::parse_url(origin_uri)?,
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
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let class = class_loader(Fqn::new(class_fqn))?;
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
                    let flag = |key: &str| data.get(key).and_then(serde_json::Value::as_bool);
                    let slot = MemberSlot {
                        is_static: flag("isStatic")?,
                        kind: if flag("isConstant")? {
                            MemberRefKind::Constant
                        } else if flag("isMethod")? {
                            MemberRefKind::Method
                        } else {
                            MemberRefKind::Property
                        },
                    };
                    Some(self.resolve_member_ref_locations(
                        uri,
                        offset,
                        crate::atom::atom(class_fqn),
                        crate::atom::atom(member),
                        slot,
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
        let Some(origin_uri) = crate::util::parse_url(uri) else {
            return lens;
        };

        lens.command = Some(if kind == "phpImplementations" {
            Self::implementation_lens_command(origin_uri, position, locations)
        } else {
            Self::reference_lens_command(origin_uri, position, locations)
        });
        lens
    }

    /// The lenses linking a Doctrine entity to its repository, or a
    /// repository back to its entity, on the class declaration line.
    fn push_doctrine_lenses(
        &self,
        class: &ClassInfo,
        index: &LineIndex,
        lenses: &mut Vec<CodeLens>,
    ) {
        if class.keyword_offset == 0 {
            return;
        }
        let entities = self.doctrine_entity_fqns_for_repository(class);
        let (label, targets) = if entities.is_empty() {
            (
                "Doctrine repository",
                self.doctrine_repository_fqns_for_entity(&class.fqn()),
            )
        } else {
            ("Doctrine entity", entities)
        };
        if targets.is_empty() {
            return;
        }

        let line = index.line_of(class.keyword_offset as usize);
        let position = Position {
            line: line as u32,
            character: line_indent(index, line),
        };
        for target in targets {
            let Some(location) = self.class_declaration_location(&target) else {
                continue;
            };
            let title = format!("{label}: {}", crate::util::short_name(&target));
            lenses.push(CodeLens {
                range: Range {
                    start: position,
                    end: position,
                },
                command: Some(self.build_code_lens_command(
                    title,
                    location.uri,
                    location.range.start,
                )),
                data: None,
            });
        }
    }

    /// The lenses linking a Messenger message to the handlers that receive
    /// it, on the class declaration line, and a handler method to the
    /// message it handles, on the method's.
    fn push_messenger_lenses(
        &self,
        uri: &str,
        class: &ClassInfo,
        index: &LineIndex,
        lenses: &mut Vec<CodeLens>,
    ) {
        if class.keyword_offset == 0 {
            return;
        }
        let mut push = |offset: u32, title: String, location: Location| {
            let line = index.line_of(offset as usize);
            let position = Position {
                line: line as u32,
                character: line_indent(index, line),
            };
            lenses.push(CodeLens {
                range: Range {
                    start: position,
                    end: position,
                },
                command: Some(self.build_code_lens_command(
                    title,
                    location.uri,
                    location.range.start,
                )),
                data: None,
            });
        };

        for (message, method) in self.symfony_handled_messages(uri, class.fqn()) {
            let Some(location) = self.class_declaration_location(&message) else {
                continue;
            };
            let offset = class
                .methods
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(&method))
                .map_or(class.keyword_offset, |method| method.name_offset);
            let title = format!("Messenger message: {}", crate::util::short_name(&message));
            push(offset, title, location);
        }

        if class.kind == ClassLikeKind::Interface || class.kind == ClassLikeKind::Class {
            for handler in self.symfony_message_handlers(class) {
                let Some(location) = self
                    .class_member_declaration_location(&handler.handler, &handler.method, false)
                    .or_else(|| self.class_declaration_location(&handler.handler))
                else {
                    continue;
                };
                let title = format!(
                    "Messenger handler: {}",
                    crate::util::short_name(&handler.handler)
                );
                push(class.keyword_offset, title, location);
            }
        }
    }

    /// The lenses linking a Laravel class to the classes and files it is
    /// paired with (a model's factory, policy, observers, migrations and
    /// resources, an event's listeners, and the way back), one per kind of
    /// link.  A link that belongs to a method sits on the method's line.
    fn push_laravel_related_lenses(
        &self,
        uri: &str,
        class: &ClassInfo,
        index: &LineIndex,
        lenses: &mut Vec<CodeLens>,
    ) {
        let links = self.laravel_related_links(class);
        if links.is_empty() {
            return;
        }
        // Group by line and kind, keeping the order the links came in.
        type Targets = Vec<(String, Location)>;
        let mut groups: Vec<(u32, (&str, &str), Targets)> = Vec::new();
        for link in links {
            let offset = link
                .anchor
                .and_then(|anchor| class.methods.iter().find(|method| method.name == anchor))
                .map_or(class.keyword_offset, |method| method.name_offset);
            match groups
                .iter_mut()
                .find(|(known, kind, _)| *known == offset && *kind == link.kind)
            {
                Some((_, _, targets)) => {
                    if !targets.iter().any(|(_, known)| *known == link.location) {
                        targets.push((link.label, link.location));
                    }
                }
                None => groups.push((offset, link.kind, vec![(link.label, link.location)])),
            }
        }
        for (offset, (singular, plural), targets) in groups {
            let line = index.line_of(offset as usize);
            let position = Position {
                line: line as u32,
                character: line_indent(index, line),
            };
            let title = match targets.as_slice() {
                [(label, _)] => format!("{singular}: {label}"),
                _ => format!("{} {plural}", targets.len()),
            };
            lenses.extend(self.targets_lens(uri, position, title, targets));
        }
    }

    /// Build the "which tests cover this class" lens for a class
    /// declaration: the test classes whose PHPUnit coverage metadata
    /// (`@covers` / `@uses` / `#[CoversClass]` and friends) names it, and
    /// the tests its name and path pair it with by convention.
    ///
    /// `None` when the class has no keyword position (synthetic/anonymous
    /// classes) or no test can be located.  The coverage half stays empty
    /// while the workspace is still indexing, since
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

        let mut targets: Vec<(String, Location)> = self
            .find_covering_test_classes(&class.fqn())
            .into_iter()
            .filter_map(|(test_fqn, test_uri)| {
                let location = self.covers_lens_target(test_fqn.as_str(), &test_uri, uri, index)?;
                Some((crate::util::short_name(&test_fqn).to_string(), location))
            })
            .collect();
        for (label, location) in self.conventional_test_locations(class, uri) {
            if !targets.iter().any(|(_, known)| known.uri == location.uri) {
                targets.push((label, location));
            }
        }

        let line = index.line_of(class.keyword_offset as usize);
        let position = Position {
            line: line as u32,
            character: line_indent(index, line),
        };
        let title = match targets.as_slice() {
            [(label, _)] => format!("Tests: {label}"),
            _ => format!("Tests: {} tests", targets.len()),
        };
        self.targets_lens(uri, position, title, targets)
    }

    /// The tests the naming convention pairs with `class`, declared in
    /// `uri`, each labelled with its file's name.  A PHPUnit test is
    /// located at its class declaration, a Pest file at its top.
    fn conventional_test_locations(&self, class: &ClassInfo, uri: &str) -> Vec<(String, Location)> {
        // Expected class names are worked out under the mappings' lock, but
        // looked up only once it is released: loading a class reads the
        // mappings again.
        let files: Vec<(std::path::PathBuf, Option<String>)> = {
            let mappings = self.psr4_mappings().read();
            if !mappings.iter().any(|mapping| mapping.is_dev) {
                return Vec::new();
            }
            let Some(root) = self.workspace_root().read().clone() else {
                return Vec::new();
            };
            let Some(path) = crate::util::uri_to_path(uri) else {
                return Vec::new();
            };
            crate::composer::conventional_test_files(&mappings, &root, &class.fqn(), &path)
                .into_iter()
                .map(|test_path| {
                    let fqn =
                        crate::composer::resolve_namespace_from_path(&mappings, &root, &test_path)
                            .map(|(namespace, name)| match namespace {
                                Some(namespace) => format!("{namespace}\\{name}"),
                                None => name,
                            });
                    (test_path, fqn)
                })
                .collect()
        };

        files
            .into_iter()
            .filter_map(|(path, fqn)| {
                let label = path.file_stem()?.to_str()?.to_string();
                let test_uri = crate::util::path_to_uri(&path);
                let location = fqn
                    .and_then(|fqn| self.class_declaration_location(&fqn))
                    .filter(|location| location.uri.as_str() == test_uri)
                    .or_else(|| file_top_location(&test_uri))?;
                Some((label, location))
            })
            .collect()
    }

    /// The lens on a test linking it to the classes the naming convention
    /// pairs it with: on the test class named after the file, or at the top
    /// of a Pest file, which declares no class.
    ///
    /// Coverage metadata needs no lens on this side: go-to-definition on the
    /// metadata reaches the class it names.
    fn build_test_subject_lens(
        &self,
        uri: &str,
        classes: &[Arc<ClassInfo>],
        index: &LineIndex,
    ) -> Option<CodeLens> {
        let path = crate::util::uri_to_path(uri)?;
        let subjects = {
            let mappings = self.psr4_mappings().read();
            if !mappings.iter().any(|mapping| mapping.is_dev) {
                return None;
            }
            let root = self.workspace_root().read().clone()?;
            crate::composer::conventional_test_subjects(&mappings, &root, &path)
        };
        if subjects.is_empty() {
            return None;
        }

        let position = if classes.is_empty() {
            Position::new(0, 0)
        } else {
            let stem = path.file_stem()?.to_str()?;
            let test_class = classes
                .iter()
                .find(|class| class.keyword_offset != 0 && class.name.eq_ignore_ascii_case(stem))?;
            let line = index.line_of(test_class.keyword_offset as usize);
            Position {
                line: line as u32,
                character: line_indent(index, line),
            }
        };

        let targets: Vec<(String, Location)> = subjects
            .into_iter()
            .filter_map(|(fqn, subject_path)| {
                let location = self
                    .class_declaration_location(&fqn)
                    .or_else(|| file_top_location(&crate::util::path_to_uri(&subject_path)))?;
                Some((crate::util::short_name(&fqn).to_string(), location))
            })
            .collect();
        let title = match targets.as_slice() {
            [(label, _)] => format!("Test subject: {label}"),
            _ => format!("Test subjects: {} classes", targets.len()),
        };
        self.targets_lens(uri, position, title, targets)
    }

    /// A lens at `position` that goes straight to its one target, or lists
    /// several.  `None` when there is no target.
    fn targets_lens(
        &self,
        uri: &str,
        position: Position,
        title: String,
        targets: Vec<(String, Location)>,
    ) -> Option<CodeLens> {
        let command = match targets.as_slice() {
            [] => return None,
            [(_, location)] => {
                self.build_code_lens_command(title, location.uri.clone(), location.range.start)
            }
            _ => {
                let current_uri: Uri = uri.parse().ok()?;
                let locations: Vec<Location> =
                    targets.into_iter().map(|(_, location)| location).collect();
                Command {
                    title,
                    command: "editor.action.showReferences".to_string(),
                    arguments: Some(vec![
                        serde_json::json!(current_uri),
                        serde_json::json!(position),
                        serde_json::json!(locations),
                    ]),
                    tooltip: None,
                }
            }
        };
        Some(CodeLens {
            range: Range {
                start: position,
                end: position,
            },
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
        let uri: Uri = test_uri.parse().ok()?;

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
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
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
        uri: Uri,
        position: Position,
    ) -> Command {
        if self.caps().show_document {
            return Command {
                title,
                command: "phpantom.navigateToPrototype".to_string(),
                arguments: Some(vec![serde_json::json!(uri), serde_json::json!(position)]),
                tooltip: None,
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
            tooltip: None,
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

        let position = Self::find_member_position(
            &file_content,
            method_name,
            MemberKind::Method,
            ancestor_class,
        )?;

        Some(Prototype {
            ancestor_name: ancestor_class.name.to_string(),
            is_interface: ancestor_class.kind == ClassLikeKind::Interface,
            file_uri,
            position,
        })
    }
}
