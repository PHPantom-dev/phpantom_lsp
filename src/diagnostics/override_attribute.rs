//! `#[\Override]` with nothing to override.
//!
//! PHP refuses to compile a method (since 8.3) or a property (since 8.5)
//! marked `#[\Override]` when no parent class or interface declares a
//! member of that name. A member that only replaces one from the class's
//! own traits, or a private one up the chain, overrides nothing; neither
//! does a constructor that replaces a concrete parent constructor.
//!
//! A trait's members are checked where the trait is used, against the
//! using class's ancestors, so a trait's own declarations are skipped.

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::Node;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::types::{
    ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, MAX_TRAIT_DEPTH, PhpVersion, Visibility,
};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

pub(crate) const INVALID_OVERRIDE_ATTRIBUTE_CODE: &str = "invalid_override_attribute";

/// The member an `#[\Override]` attribute is attached to.
enum Target {
    /// A method, by the offset of its name.
    Method(u32),
    /// The properties a declaration (or a promoted parameter) declares,
    /// by the offset of each variable.
    Properties(Vec<u32>),
    /// Anything else. PHP rejects the attribute there for its target,
    /// which is not this diagnostic's concern.
    Other,
}

#[derive(Clone, Copy)]
enum MemberKind {
    Method,
    Property,
}

impl Backend {
    /// Collect `#[\Override]` diagnostics for a single file.
    pub fn collect_override_attribute_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_override_attribute_diagnostics_with_context(&ctx, uri, content, out);
    }

    pub(crate) fn collect_override_attribute_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let php_version = self.php_version();
        if php_version < PhpVersion::new(8, 3) {
            return;
        }
        let Some(resolved_names) = ctx.file.resolved_names.as_deref() else {
            return;
        };
        // Attribute names are class names, so `#[Override]` inside a
        // namespace without an import names that namespace's `Override`.
        let mut attribute_offsets: Vec<u32> = resolved_names
            .iter()
            .filter(|(_, fqn, _)| {
                fqn.trim_start_matches('\\')
                    .eq_ignore_ascii_case("Override")
            })
            .map(|(offset, _, _)| offset)
            .collect();
        if attribute_offsets.is_empty() {
            return;
        }
        attribute_offsets.sort_unstable();

        let targets: Vec<Target> =
            crate::parser::with_parsed_program(content, "override_attribute", |program, _| {
                attribute_offsets
                    .iter()
                    .map(|&offset| attribute_target(program, offset))
                    .collect()
            });

        let mut methods: HashSet<u32> = HashSet::new();
        let mut properties: HashSet<u32> = HashSet::new();
        for target in targets {
            match target {
                Target::Method(offset) => {
                    methods.insert(offset);
                }
                Target::Properties(offsets) => properties.extend(offsets),
                Target::Other => {}
            }
        }
        if php_version < PhpVersion::new(8, 5) {
            properties.clear();
        }
        if methods.is_empty() && properties.is_empty() {
            return;
        }

        let class_loaders = self.resolved_class_loaders(&ctx.file);
        for class in &ctx.file.classes {
            if class.kind == ClassLikeKind::Trait {
                continue;
            }
            let marked_methods = class
                .methods
                .iter()
                .filter(|m| !m.is_virtual && methods.contains(&m.name_offset))
                .map(|m| (m.name, m.name_offset, MemberKind::Method));
            let marked_properties = class
                .properties
                .iter()
                .filter(|p| !p.is_virtual && properties.contains(&p.name_offset))
                .map(|p| (p.name, p.name_offset, MemberKind::Property));

            for (name, offset, kind) in marked_methods.chain(marked_properties) {
                let class_loader = class_loaders.at(offset);
                if overrides_something(class, name.as_str(), kind, class_loader) {
                    continue;
                }
                let (end, message) = match kind {
                    MemberKind::Method => (
                        offset as usize + name.len(),
                        format!(
                            "{}::{}() has #[\\Override] attribute, but no matching parent method exists",
                            class.name, name
                        ),
                    ),
                    MemberKind::Property => (
                        // The span covers the `$`.
                        offset as usize + name.len() + 1,
                        format!(
                            "{}::${} has #[\\Override] attribute, but no matching parent property exists",
                            class.name, name
                        ),
                    ),
                };
                let Some(range) =
                    self.offset_range_to_lsp_range(uri, content, offset as usize, end)
                else {
                    continue;
                };
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Error,
                    INVALID_OVERRIDE_ATTRIBUTE_CODE,
                    message,
                ));
            }
        }
    }
}

/// The member the attribute whose name starts at `offset` belongs to.
fn attribute_target(program: &mago_syntax::cst::Program<'_>, offset: u32) -> Target {
    crate::parser::innermost_node_at(Node::Program(program), offset, |node| match node {
        Node::Method(method) => Some(Target::Method(method.name.span.start.offset)),
        Node::Property(property) => Some(Target::Properties(
            property
                .variables()
                .iter()
                .map(|var| var.span().start.offset)
                .collect(),
        )),
        Node::FunctionLikeParameter(param) => Some(if param.is_promoted_property() {
            Target::Properties(vec![param.variable.span.start.offset])
        } else {
            Target::Other
        }),
        Node::Class(_)
        | Node::Interface(_)
        | Node::Trait(_)
        | Node::Enum(_)
        | Node::AnonymousClass(_)
        | Node::Function(_)
        | Node::Closure(_)
        | Node::ArrowFunction(_)
        | Node::ClassLikeConstant(_)
        | Node::EnumCase(_)
        | Node::PropertyHook(_) => Some(Target::Other),
        _ => None,
    })
    .unwrap_or(Target::Other)
}

/// Whether a parent class or interface of `class` declares the member
/// `name`, as PHP decides it for `#[\Override]`.
///
/// Answers `true` whenever part of the hierarchy cannot be loaded: the
/// missing ancestor may well be the one that declares the member.
fn overrides_something(
    class: &ClassInfo,
    name: &str,
    kind: MemberKind,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    let is_constructor =
        matches!(kind, MemberKind::Method) && name.eq_ignore_ascii_case("__construct");
    // What a parent class (or a trait it uses) has to declare.
    let parent_declares = |candidate: &ClassInfo| match kind {
        MemberKind::Method => candidate.get_method(name).is_some_and(|m| {
            !m.is_virtual
                && m.visibility != Visibility::Private
                // A constructor has no prototype unless it is abstract
                // (or an interface declares it, which is checked below).
                && (!is_constructor || m.is_abstract)
        }),
        MemberKind::Property => candidate
            .get_property(name)
            .is_some_and(|p| !p.is_virtual && p.visibility != Visibility::Private),
    };
    let interface_declares = |candidate: &ClassInfo| match kind {
        MemberKind::Method => candidate.get_method(name).is_some_and(|m| !m.is_virtual),
        MemberKind::Property => candidate.get_property(name).is_some_and(|p| !p.is_virtual),
    };

    let mut interfaces: Vec<Atom> = class.interfaces.clone();
    if class.kind == ClassLikeKind::Interface {
        interfaces.extend(class.parent_class);
    } else {
        // The class's own traits count only for an abstract method they
        // require; a concrete trait member is replaced, not overridden.
        if matches!(kind, MemberKind::Method) {
            let own_trait_declares = |candidate: &ClassInfo| {
                candidate
                    .get_method(name)
                    .is_some_and(|m| !m.is_virtual && m.is_abstract)
            };
            match any_trait_declares(&class.used_traits, class_loader, &own_trait_declares, 0) {
                Some(false) => {}
                _ => return true,
            }
        }

        let mut next = class.parent_class;
        let mut depth = 0u32;
        while let Some(parent_name) = next {
            depth += 1;
            if depth > MAX_INHERITANCE_DEPTH {
                return true;
            }
            let Some(parent) = class_loader(Fqn::new(&parent_name)) else {
                return true;
            };
            if parent_declares(&parent) {
                return true;
            }
            match any_trait_declares(&parent.used_traits, class_loader, &parent_declares, 0) {
                Some(false) => {}
                _ => return true,
            }
            for iface in &parent.interfaces {
                if !interfaces.contains(iface) {
                    interfaces.push(*iface);
                }
            }
            next = parent.parent_class;
        }
    }

    let mut visited: HashSet<Atom> = HashSet::new();
    interfaces.iter().any(|iface| {
        interface_declares_member(*iface, class_loader, &interface_declares, &mut visited, 0)
    })
}

/// Whether one of `trait_names`, or a trait they use, satisfies
/// `declares`. `None` when a trait cannot be loaded.
fn any_trait_declares(
    trait_names: &[Atom],
    class_loader: &ResolvedClassLoader<'_>,
    declares: &dyn Fn(&ClassInfo) -> bool,
    depth: u32,
) -> Option<bool> {
    if depth > MAX_TRAIT_DEPTH {
        return None;
    }
    for name in trait_names {
        let used = class_loader(Fqn::new(name))?;
        if declares(&used) {
            return Some(true);
        }
        if any_trait_declares(&used.used_traits, class_loader, declares, depth + 1)? {
            return Some(true);
        }
    }
    Some(false)
}

/// Whether `iface_name` or an interface it extends satisfies `declares`,
/// answering `true` for one that cannot be loaded.
fn interface_declares_member(
    iface_name: Atom,
    class_loader: &ResolvedClassLoader<'_>,
    declares: &dyn Fn(&ClassInfo) -> bool,
    visited: &mut HashSet<Atom>,
    depth: u32,
) -> bool {
    if depth > MAX_INHERITANCE_DEPTH {
        return true;
    }
    if !visited.insert(iface_name) {
        return false;
    }
    let Some(iface) = class_loader(Fqn::new(&iface_name)) else {
        return true;
    };
    if declares(&iface) {
        return true;
    }
    iface
        .interfaces
        .iter()
        .chain(iface.parent_class.as_ref())
        .any(|parent| {
            interface_declares_member(*parent, class_loader, declares, visited, depth + 1)
        })
}
