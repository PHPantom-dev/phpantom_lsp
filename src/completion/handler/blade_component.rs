//! Strategy: Blade component tag and attribute completion.
//!
//! Always short-circuits once the cursor is confirmed to sit inside a
//! `<x-…>` or `<livewire:…>` tag: a component name and an attribute name
//! are both HTML, so nothing the rest of completion offers (class names,
//! variables, members) belongs there, and an empty list beats falling
//! through to them.
//!
//! Runs on the raw Blade buffer rather than the virtual PHP the rest of
//! completion works from, for the same reason directive-name completion
//! does (`src/blade/directive_completion.rs`): the preprocessor emits a
//! tag's call where the tag *closes*, and a half-typed tag name resolves
//! to no component at all, so neither the name nor a plain attribute ever
//! appears in the virtual PHP anywhere near where it is written.

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit, InsertTextFormat,
    Position, Range, TextEdit,
};

use crate::Backend;
use crate::blade::component_names::view_names_for_component_tag;
use crate::blade::component_tags::{TagContext, TagCursor, TagKind, kebab_case, tag_context_at};
use crate::blade::signature;
use crate::text_position::{offset_to_position, position_to_byte_offset};
use crate::types::Visibility;

/// One attribute a component tag may carry.
struct ComponentAttribute {
    /// The kebab-case name the attribute is written under.
    name: String,
    /// What filling it means, shown beside the name: the declared type of
    /// the parameter or property it fills, or a prop's default value.
    detail: String,
    /// Whether the component has no value of its own for it, so a tag that
    /// leaves it out is short an argument.
    required: bool,
}

impl Backend {
    /// Complete the component name or attribute name the cursor is writing
    /// in `uri`'s raw Blade source.
    pub(super) fn blade_component_completion(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        let ctx = tag_context_at(&content, offset)?;
        let prefix = content.get(ctx.token_start..offset)?.to_lowercase();
        // The edit is in the template's own coordinates, and it replaces
        // what is already typed: a component name holds dots and an
        // attribute name holds hyphens, neither of which an editor treats
        // as part of the word it would otherwise replace.
        let range = Range {
            start: offset_to_position(&content, ctx.token_start),
            end: position,
        };

        let items = match ctx.cursor {
            TagCursor::Name => self.blade_component_name_items(ctx.kind, &prefix, range),
            TagCursor::Attribute => self.blade_component_attribute_items(&ctx, &prefix, range),
        };
        Some(CompletionResponse::Array(items))
    }

    /// The component names a tag of `kind` can be written with.
    fn blade_component_name_items(
        &self,
        kind: TagKind,
        prefix: &str,
        range: Range,
    ) -> Vec<CompletionItem> {
        let discovery = self.blade_discovery();
        // `(name, the class backing it)`. A name with no class is an
        // anonymous component: a template Laravel renders through
        // `AnonymousComponent`, with no declaration to point at.
        let mut names: Vec<(String, Option<&String>)> = match kind {
            TagKind::Livewire => discovery
                .livewire
                .iter()
                .map(|(name, fqn)| (name.clone(), Some(fqn)))
                .collect(),
            TagKind::Blade => {
                let mut names: Vec<(String, Option<&String>)> = discovery
                    .components
                    .iter()
                    .map(|(name, fqn)| (name.clone(), Some(fqn)))
                    .collect();
                let views: Vec<String> = discovery.views.keys().cloned().collect();
                let anonymous = self.anonymous_component_namespaces();
                for name in crate::blade::component_names::component_tag_names(&views, &anonymous) {
                    // A template whose name a class already answers to is
                    // that class's view, not a component of its own.
                    if !discovery.components.contains_key(&name) {
                        names.push((name, None));
                    }
                }
                names
            }
        };
        names.sort_by(|a, b| a.0.cmp(&b.0));

        names
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .enumerate()
            .map(|(index, (name, fqn))| CompletionItem {
                label: name.clone(),
                kind: Some(match fqn {
                    Some(_) => CompletionItemKind::CLASS,
                    None => CompletionItemKind::MODULE,
                }),
                detail: Some(match fqn {
                    Some(fqn) => format!("\\{}", fqn.trim_matches('\\')),
                    None => "anonymous component".to_string(),
                }),
                sort_text: Some(format!("{index:05}")),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: name.clone(),
                })),
                ..CompletionItem::default()
            })
            .collect()
    }

    /// The attributes the component `ctx` names accepts, offered both
    /// plain (a string literal) and `:` prefixed (a PHP expression).
    fn blade_component_attribute_items(
        &self,
        ctx: &TagContext,
        prefix: &str,
        range: Range,
    ) -> Vec<CompletionItem> {
        let attributes = match ctx.kind {
            TagKind::Livewire => self
                .livewire_component_fqn(&ctx.name)
                // Livewire hands the tag's attributes to `mount()` and
                // binds whatever is left to the public properties of the
                // same name, so both are attributes a tag may carry.
                .map(|fqn| self.class_component_attributes(&fqn, "mount", true))
                .unwrap_or_default(),
            TagKind::Blade => match self.blade_component_fqn(&ctx.name) {
                Some(fqn) => self.class_component_attributes(&fqn, "__construct", false),
                None => self.anonymous_component_attributes(&ctx.name),
            },
        };

        let mut items = Vec::with_capacity(attributes.len() * 2);
        for (index, attribute) in attributes.iter().enumerate() {
            for bound in [false, true] {
                let label = if bound {
                    format!(":{}", attribute.name)
                } else {
                    attribute.name.clone()
                };
                if !label.starts_with(prefix) {
                    continue;
                }
                items.push(CompletionItem {
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(attribute.detail.clone()),
                    // Required attributes first, in declaration order,
                    // since a tag missing one is reported as a missing
                    // argument.
                    sort_text: Some(format!(
                        "{}{index:05}{}",
                        u8::from(!attribute.required),
                        u8::from(bound)
                    )),
                    filter_text: Some(label.clone()),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range,
                        new_text: format!("{label}=\"$1\""),
                    })),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    label,
                    ..CompletionItem::default()
                });
            }
        }
        items
    }

    /// The attributes a component class accepts: the parameters of the
    /// method that receives them, plus (for Livewire) the public
    /// properties an attribute of the same name binds.
    fn class_component_attributes(
        &self,
        fqn: &str,
        method: &str,
        public_properties: bool,
    ) -> Vec<ComponentAttribute> {
        let Some(class) = self.find_or_load_class(fqn.trim_matches('\\')) else {
            return Vec::new();
        };
        let loader = |name: &str| self.find_or_load_class(name);
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            &class,
            &loader,
            Some(&self.resolved_class_cache),
        );

        let mut attributes: Vec<ComponentAttribute> = Vec::new();
        if let Some(signature) = resolved
            .methods
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(method))
        {
            for param in signature.parameters.iter() {
                // A variadic parameter collects what is left over
                // positionally, so no attribute names it.
                if param.is_variadic {
                    continue;
                }
                push_attribute(
                    &mut attributes,
                    ComponentAttribute {
                        name: kebab_case(param.name.trim_start_matches('$')),
                        detail: param
                            .type_hint
                            .as_ref()
                            .map(|ty| ty.to_string())
                            .unwrap_or_default(),
                        required: param.is_required,
                    },
                );
            }
        }
        if public_properties {
            for property in resolved.properties.iter() {
                if property.is_static || property.visibility != Visibility::Public {
                    continue;
                }
                push_attribute(
                    &mut attributes,
                    ComponentAttribute {
                        name: kebab_case(&property.name),
                        detail: property
                            .type_hint
                            .as_ref()
                            .map(|ty| ty.to_string())
                            .unwrap_or_default(),
                        // A property always holds whatever it was
                        // declared with, so no tag is short of it.
                        required: false,
                    },
                );
            }
        }
        attributes
    }

    /// The attributes an anonymous component accepts: what its template's
    /// `@props` declares, then the names its body reads that nothing in the
    /// template defines.
    ///
    /// `@aware` is deliberately left out: those entries are pulled from
    /// the surrounding component's data rather than named by the tag.
    fn anonymous_component_attributes(&self, tag: &str) -> Vec<ComponentAttribute> {
        let anonymous = self.anonymous_component_namespaces();
        let discovery = self.blade_discovery();
        let Some(view) = view_names_for_component_tag(tag, &anonymous)
            .into_iter()
            .find(|name| discovery.views.contains_key(name))
        else {
            return Vec::new();
        };
        let Some(source) = self.blade_view_source(&view) else {
            return Vec::new();
        };
        let mut attributes = Vec::new();
        for entry in signature::extract_props(&source).unwrap_or_default() {
            push_attribute(
                &mut attributes,
                ComponentAttribute {
                    name: kebab_case(&entry.name),
                    detail: entry
                        .default
                        .as_ref()
                        .map(|default| format!("= {default}"))
                        .unwrap_or_default(),
                    required: entry.default.is_none(),
                },
            );
        }
        // A small partial usually has no `@props` line at all and reads the
        // tag's attributes straight off its own scope. Nothing but the tag
        // can fill a name the template neither defines nor is handed, so
        // reading one is the template declaring an attribute implicitly
        // (see [`crate::blade::implicit_props`]).
        let custom_directives = self.blade_custom_directives.read();
        let implicit = crate::blade::implicit_props::implicit_props(&source, &custom_directives);
        if implicit.is_empty() {
            return attributes;
        }
        // Only worth asking once the template turns out to read something
        // it does not declare: a template whose `@props` covers its whole
        // body pays for neither lookup.
        let names = std::slice::from_ref(&view);
        let (backing, _) = self.blade_backing_class_vars(names);
        let shared = self.blade_provider_vars(names);
        for name in implicit {
            // A member of the class backing the view, or a variable a
            // provider shares or composes into it, reaches the template
            // whatever the tag writes, so neither is the tag's to pass.
            if backing
                .iter()
                .chain(shared.iter())
                .any(|(supplied, _)| supplied == &name)
            {
                continue;
            }
            push_attribute(
                &mut attributes,
                ComponentAttribute {
                    name: kebab_case(&name),
                    detail: "implicit prop".to_string(),
                    // The template has no value of its own for it, so a tag
                    // that leaves it out renders an undefined variable.
                    required: true,
                },
            );
        }
        attributes
    }
}

/// Record an attribute, leaving an earlier entry of the same name alone.
///
/// A Livewire component usually declares a `mount()` parameter and the
/// public property it assigns under one name; the parameter is the one
/// that describes what the tag passes.
fn push_attribute(attributes: &mut Vec<ComponentAttribute>, attribute: ComponentAttribute) {
    if attributes
        .iter()
        .any(|existing| existing.name == attribute.name)
    {
        return;
    }
    attributes.push(attribute);
}
