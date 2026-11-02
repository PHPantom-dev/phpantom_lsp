//! Quick-fixes for a component tag nothing resolves
//! (`unknown_blade_component`, `unknown_livewire_component`): "Change to"
//! the closest component names, and create the component.
//!
//! A Blade tag is offered both shapes `make:component` produces: an
//! anonymous component (the template alone, at the view the tag resolves
//! to) and a class-backed one (the class in the component namespace plus
//! its template). A Livewire tag gets the class and template
//! `make:livewire` produces.

use std::path::PathBuf;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::blade::component_tags::{TagKind, TagSpan, tag_spans};
use crate::blade::unknown_components::ComponentIndex;
use crate::diagnostics::blade_component_tags::unknown_component_report;
use crate::diagnostics::helpers::make_diagnostic;
use crate::text_position::{byte_range_to_lsp_range, ranges_overlap};

use super::create_missing_view::close_names;

/// One file a create action writes.
struct NewFile {
    path: PathBuf,
    content: String,
}

impl Backend {
    /// Offer the closest component names for a tag under the cursor that
    /// names no component, and offer to create the component.
    ///
    /// Runs with the request range in the template's own coordinates and
    /// reads the template's own text, since a tag never reaches the PHP
    /// the template lowers to.
    pub(crate) fn collect_create_missing_component_actions(
        &self,
        uri: &str,
        _content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        if !self.is_blade_file(uri) {
            return;
        }
        let Some(source) = self.get_file_content_arc(uri) else {
            return;
        };
        let range_of = |start: usize, end: usize| byte_range_to_lsp_range(&source, start, end);
        let tags: Vec<TagSpan> = tag_spans(&source)
            .into_iter()
            .filter(|tag| {
                ranges_overlap(
                    &range_of(tag.name_span.start, tag.name_span.end),
                    &params.range,
                )
            })
            .collect();
        if tags.is_empty() {
            return;
        }
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return;
        };
        let index = self.component_index(tags.iter().any(|tag| tag.kind == TagKind::Livewire));
        let can_create = self.caps().file_create;

        for tag in tags {
            if !index.is_unknown(self, tag.kind, &tag.name) {
                continue;
            }
            let name_range = range_of(tag.name_span.start, tag.name_span.end);
            let (code, message) = unknown_component_report(tag.kind, &tag.name);
            let diagnostic =
                make_diagnostic(name_range, DiagnosticSeverity::Warning, code, message);

            let known = index.known_names(tag.kind);
            let suggestions = close_names(&tag.name, &known);
            let suggestion_preferred = suggestions.first().is_some_and(|best| {
                best.0 <= 1 && suggestions.get(1).is_none_or(|next| next.0 > best.0)
            });
            let closer = closing_name_span(&source, &tag);
            for (i, (_, name)) in suggestions.iter().enumerate() {
                let mut edits = vec![TextEdit {
                    range: name_range,
                    new_text: name.to_string(),
                }];
                if let Some(closer) = &closer {
                    edits.push(TextEdit {
                        range: range_of(closer.start, closer.end),
                        new_text: name.to_string(),
                    });
                }
                out.push(CodeActionResponse::CodeAction(CodeAction {
                    title: format!("Change to '{name}'"),
                    kind: Some(CodeActionKind::QuickFix),
                    diagnostics: Some(vec![diagnostic.clone()]),
                    edit: Some(super::single_file_edit(doc_uri.clone(), edits)),
                    command: None,
                    is_preferred: (suggestion_preferred && i == 0).then_some(true),
                    disabled: None,
                    data: None,
                    tags: None,
                }));
            }

            if !can_create {
                continue;
            }
            let creations: Vec<(String, Vec<NewFile>)> = match tag.kind {
                TagKind::Blade => [
                    self.anonymous_component_files(&tag.name, &index)
                        .map(|files| (format!("Create anonymous component '{}'", tag.name), files)),
                    self.class_component_files(&tag.name, &index)
                        .map(|files| (format!("Create component class for '{}'", tag.name), files)),
                ]
                .into_iter()
                .flatten()
                .collect(),
                TagKind::Livewire => self
                    .livewire_component_files(&tag.name)
                    .map(|files| (format!("Create Livewire component '{}'", tag.name), files))
                    .into_iter()
                    .collect(),
            };
            for (i, (title, files)) in creations.into_iter().enumerate() {
                if let Some(action) = self.create_files_action(
                    title,
                    files,
                    &diagnostic,
                    !suggestion_preferred && i == 0,
                ) {
                    out.push(CodeActionResponse::CodeAction(action));
                }
            }
        }
    }

    /// The template an anonymous component written as `<x-{name}>` would
    /// be: the first view the tag resolves to that has a directory to be
    /// created in.
    fn anonymous_component_files(
        &self,
        name: &str,
        index: &ComponentIndex,
    ) -> Option<Vec<NewFile>> {
        let path = crate::blade::component_names::view_names_to_create_for_component_tag(
            name,
            index.anonymous(),
        )
        .iter()
        .find_map(|view| self.missing_view_file(view))?;
        Some(vec![NewFile {
            path,
            content: String::new(),
        }])
    }

    /// The class `make:component` would write for `<x-{name}>`, and the
    /// template it renders.
    ///
    /// A tag under a prefix gets its class in the namespace registered for
    /// that prefix; one under a prefix with no class namespace has nowhere
    /// for a class to go.
    fn class_component_files(&self, name: &str, index: &ComponentIndex) -> Option<Vec<NewFile>> {
        let (namespace, path) = match name.split_once("::") {
            Some((prefix, path)) => {
                let namespace = self
                    .laravel
                    .provider_resources
                    .read()
                    .class_component_namespaces
                    .iter()
                    .find(|(registered, _)| registered == prefix)
                    .map(|(_, namespace)| namespace.trim_matches('\\').to_string())?;
                (namespace, path)
            }
            None => (
                format!(
                    "{}{}",
                    self.application_namespace(),
                    crate::blade::discovery::DEFAULT_COMPONENT_NAMESPACE_TAIL
                ),
                name,
            ),
        };
        let view = crate::blade::component_names::view_names_to_create_for_component_tag(
            name,
            index.anonymous(),
        )
        .into_iter()
        .find_map(|view| Some((self.missing_view_file(&view)?, view)));
        let (view_path, view_name) = view?;
        let (class_namespace, class_name) = class_for_path(&namespace, path);
        let class_path = self.class_file_path(&class_namespace, &class_name)?;
        Some(vec![
            NewFile {
                path: class_path,
                content: class_component_source(&class_namespace, &class_name, &view_name),
            },
            NewFile {
                path: view_path,
                content: "<div>\n</div>\n".to_string(),
            },
        ])
    }

    /// The class and template `make:livewire` would write for
    /// `<livewire:{name}>`.
    fn livewire_component_files(&self, name: &str) -> Option<Vec<NewFile>> {
        let namespace = self
            .livewire_class_namespace()
            .trim_matches('\\')
            .to_string();
        let view_name = format!("livewire.{name}");
        let view_path = self.missing_view_file(&view_name)?;
        let (class_namespace, class_name) = class_for_path(&namespace, name);
        let class_path = self.class_file_path(&class_namespace, &class_name)?;
        Some(vec![
            NewFile {
                path: class_path,
                content: livewire_component_source(&class_namespace, &class_name, &view_name),
            },
            NewFile {
                path: view_path,
                // A Livewire component's template needs a single root
                // element.
                content: "<div>\n</div>\n".to_string(),
            },
        ])
    }

    /// Where PSR-4 puts `namespace\class_name`, whether or not it exists.
    fn class_file_path(&self, namespace: &str, class_name: &str) -> Option<PathBuf> {
        let root = self.workspace_root().read().clone()?;
        let mappings = self.psr4_mappings().read();
        crate::composer::psr4_path_for_class(
            &mappings,
            &root,
            &format!("{namespace}\\{class_name}"),
        )
    }

    /// An action creating every file in `files` and opening the first.
    /// `None` when one of them already exists, since creating the rest
    /// would leave a component half made.
    fn create_files_action(
        &self,
        title: String,
        files: Vec<NewFile>,
        diagnostic: &Diagnostic,
        preferred: bool,
    ) -> Option<CodeAction> {
        if files.iter().any(|file| file.path.exists()) {
            return None;
        }
        let mut creates = Vec::with_capacity(files.len());
        let mut edits = Vec::new();
        for file in files {
            let uri = crate::util::path_to_url(&file.path)?;
            creates.push(DocumentChange::CreateFile(CreateFile {
                uri: uri.clone(),
                options: Some(CreateFileOptions {
                    overwrite: Some(false),
                    ignore_if_exists: Some(true),
                }),
                annotation_id: None,
            }));
            if !file.content.is_empty() {
                edits.push((
                    uri,
                    vec![TextEdit {
                        range: Range::default(),
                        new_text: file.content,
                    }],
                ));
            }
        }
        let DocumentChange::CreateFile(first) = creates.first()? else {
            return None;
        };
        let open = first.uri.clone();
        Some(CodeAction {
            title,
            kind: Some(CodeActionKind::QuickFix),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(super::helpers::document_changes_edit(creates, edits)),
            command: Some(self.build_code_lens_command(
                "Open new component".to_string(),
                open,
                Position::new(0, 0),
            )),
            is_preferred: preferred.then_some(true),
            disabled: None,
            data: None,
            tags: None,
        })
    }
}

/// The name bytes of the closing tag that ends `tag`, when it has one and
/// it repeats the tag's name exactly (`</x-alert>`), so a rename can
/// change both.
fn closing_name_span(source: &str, tag: &TagSpan) -> Option<std::ops::Range<usize>> {
    if !tag.closed {
        return None;
    }
    let close = source.get(..tag.span.end)?;
    let before_gt = close.strip_suffix('>')?.trim_end();
    let name_start = before_gt.len().checked_sub(tag.name.len())?;
    let opening = format!("</{}", tag.kind.prefix());
    (before_gt.get(name_start..)? == tag.name && before_gt[..name_start].ends_with(&opening))
        .then_some(name_start..before_gt.len())
}

/// The namespace and short name of the class a dotted component path
/// names below `namespace`.
fn class_for_path(namespace: &str, path: &str) -> (String, String) {
    let class = crate::blade::unknown_components::class_for_tag_path(namespace, path);
    match class.rsplit_once('\\') {
        Some((namespace, name)) => (namespace.to_string(), name.to_string()),
        None => (String::new(), class),
    }
}

/// The class `make:component` writes.
fn class_component_source(namespace: &str, class_name: &str, view: &str) -> String {
    format!(
        "<?php

namespace {namespace};

use Closure;
use Illuminate\\Contracts\\View\\View;
use Illuminate\\View\\Component;

class {class_name} extends Component
{{
    /**
     * Create a new component instance.
     */
    public function __construct()
    {{
        //
    }}

    /**
     * Get the view / contents that represent the component.
     */
    public function render(): View|Closure|string
    {{
        return view('{view}');
    }}
}}
"
    )
}

/// The class `make:livewire` writes.
fn livewire_component_source(namespace: &str, class_name: &str, view: &str) -> String {
    format!(
        "<?php

namespace {namespace};

use Livewire\\Component;

class {class_name} extends Component
{{
    public function render()
    {{
        return view('{view}');
    }}
}}
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dotted_path_becomes_a_nested_studly_class() {
        assert_eq!(
            class_for_path("App\\View\\Components", "forms.date-picker"),
            (
                "App\\View\\Components\\Forms".to_string(),
                "DatePicker".to_string()
            )
        );
        assert_eq!(
            class_for_path("App\\Livewire", "orders.overview"),
            ("App\\Livewire\\Orders".to_string(), "Overview".to_string())
        );
    }

    #[test]
    fn the_closing_tag_name_is_found_only_when_it_repeats_the_name() {
        let source = "<x-alert>hi</x-alert>";
        let tag = tag_spans(source).remove(0);
        assert_eq!(closing_name_span(source, &tag), Some(15..20));

        let slot = "<x-slot:title>hi</x-slot>";
        let tag = tag_spans(slot).remove(0);
        assert_eq!(closing_name_span(slot, &tag), None);
    }
}
