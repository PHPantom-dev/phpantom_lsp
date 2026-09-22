//! Call records and the member-target resolution they share with
//! occurrence export.

use crate::Backend;
use crate::inheritance::ancestry::find_declaring_ancestor;
use crate::symbol_map::SymbolMap;
use crate::types::{ClassInfo, FileContext};

use super::ranges::{argument_expression_range, function_fqn};
use super::{ByteRange, CallKind, ExportCall, MemberTargetMemo, OccurrenceKind, SourceDocument};

pub(super) fn export_calls(
    backend: &Backend,
    source: &SourceDocument,
    context: &FileContext,
    map: &SymbolMap,
    member_targets: &mut MemberTargetMemo,
) -> Vec<ExportCall> {
    map.call_sites
        .iter()
        .map(|call| {
            let (kind, resolved_symbol) = resolve_call(
                backend,
                context,
                &source.source,
                &call.call_expression,
                call.args_start,
                member_targets,
            );
            let arguments = call
                .arg_offsets
                .iter()
                .enumerate()
                .filter_map(|(index, start)| {
                    let end = call
                        .comma_offsets
                        .get(index)
                        .copied()
                        .unwrap_or(call.args_end);
                    let is_named = call
                        .named_arg_indices
                        .binary_search(&(index as u32))
                        .is_ok();
                    argument_expression_range(&source.source, *start, end, is_named)
                })
                .collect();
            ExportCall {
                kind,
                expression: call.call_expression.clone(),
                resolved_symbol,
                arguments_range: ByteRange {
                    start: call.args_start,
                    end: call.args_end,
                },
                arguments,
            }
        })
        .collect()
}

fn resolve_call(
    backend: &Backend,
    context: &FileContext,
    source: &str,
    expression: &str,
    offset: u32,
    member_targets: &mut MemberTargetMemo,
) -> (CallKind, Option<String>) {
    if let Some(class_name) = expression.strip_prefix("new ") {
        let loader = backend.class_loader(context);
        return (
            CallKind::Constructor,
            loader(class_name).map(|class| class.fqn().to_string()),
        );
    }
    if let Some((subject, method)) = expression.rsplit_once("->") {
        return (
            CallKind::Method,
            MemberTargetResolver {
                backend,
                context,
                source,
                memo: member_targets,
            }
            .resolve(subject, false, offset, method, OccurrenceKind::Method),
        );
    }
    if let Some((subject, method)) = expression.rsplit_once("::") {
        return (
            CallKind::StaticMethod,
            MemberTargetResolver {
                backend,
                context,
                source,
                memo: member_targets,
            }
            .resolve(subject, true, offset, method, OccurrenceKind::Method),
        );
    }
    let loader = backend.function_loader(context);
    (
        CallKind::Function,
        loader(expression, offset).as_ref().map(function_fqn),
    )
}

pub(super) struct MemberTargetResolver<'a> {
    pub(super) backend: &'a Backend,
    pub(super) context: &'a FileContext,
    pub(super) source: &'a str,
    pub(super) memo: &'a mut MemberTargetMemo,
}

impl MemberTargetResolver<'_> {
    pub(super) fn resolve(
        &mut self,
        subject: &str,
        is_static: bool,
        offset: u32,
        member_name: &str,
        kind: OccurrenceKind,
    ) -> Option<String> {
        let owner = resolve_member_owner(
            self.backend,
            self.context,
            self.source,
            subject,
            is_static,
            offset,
        )?;
        let key = (owner.clone(), member_name.to_string(), kind);
        let declaring_owner = self
            .memo
            .entry(key)
            .or_insert_with(|| {
                resolve_declaring_member_owner(
                    self.backend,
                    self.context,
                    &owner,
                    member_name,
                    kind,
                )
            })
            .clone();
        if let Some(declaring_owner) = declaring_owner {
            return Some(format!("{declaring_owner}::{member_name}"));
        }

        let loader = self.backend.class_loader(self.context);
        let class = loader(&owner)?;
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            &class,
            &loader,
            Some(&self.backend.resolved_class_cache),
        );
        member_exists(&resolved, member_name, kind).then(|| format!("{owner}::{member_name}"))
    }
}

fn resolve_member_owner(
    backend: &Backend,
    context: &FileContext,
    source: &str,
    subject: &str,
    is_static: bool,
    offset: u32,
) -> Option<String> {
    let class_loader = backend.class_loader(context);
    let function_loader = backend.function_loader(context);
    let resolution_context = crate::type_engine::subject_resolution::SubjectResolutionCtx {
        local_classes: &context.classes,
        use_map: &context.use_map,
        namespace: &context.namespace,
        content: source,
        class_loader: &class_loader,
        backend: Some(backend),
        function_loader: &function_loader,
    };
    let resolved = crate::type_engine::subject_resolution::resolve_subject_type(
        subject,
        is_static,
        offset,
        &resolution_context,
    )?;
    let names = resolved.top_level_class_names();
    (names.len() == 1).then(|| names[0].trim_start_matches('\\').to_string())
}

/// The class that actually declares `member_name`, so an occurrence on a
/// subclass resolves to the prototype rather than to the receiver.
///
/// The walk is the shared one every other feature uses, so the precedence
/// it applies (own members, then traits, then the parent chain, then
/// interfaces) stays in step with go-to-definition and hover.
fn resolve_declaring_member_owner(
    backend: &Backend,
    context: &FileContext,
    owner: &str,
    member_name: &str,
    kind: OccurrenceKind,
) -> Option<String> {
    let class_loader = backend.class_loader(context);
    let class = class_loader(owner)?;
    let declares = |candidate: &ClassInfo| member_exists(candidate, member_name, kind);
    if declares(&class) {
        return Some(class.fqn().to_string());
    }
    find_declaring_ancestor(&class, &class_loader, &declares)
        .map(|(_, declaring)| declaring.fqn().to_string())
}

fn member_exists(class: &ClassInfo, member_name: &str, kind: OccurrenceKind) -> bool {
    match kind {
        OccurrenceKind::Method => class
            .methods
            .iter()
            .any(|member| member.name.eq_ignore_ascii_case(member_name)),
        OccurrenceKind::Property => class
            .properties
            .iter()
            .any(|member| member.name.as_str() == member_name),
        OccurrenceKind::Constant => class
            .constants
            .iter()
            .any(|member| member.name.as_str() == member_name),
        _ => false,
    }
}

pub(super) fn member_occurrence_kind(
    source: &str,
    range: ByteRange,
    is_static: bool,
    is_method_call: bool,
) -> OccurrenceKind {
    if is_method_call {
        OccurrenceKind::Method
    } else if is_static
        && source
            .get(range.start as usize..range.end as usize)
            .is_some_and(|text| !text.starts_with('$'))
    {
        OccurrenceKind::Constant
    } else {
        OccurrenceKind::Property
    }
}

pub(super) fn declared_member_kind(
    class: &ClassInfo,
    name: &str,
    is_static: bool,
) -> OccurrenceKind {
    if class
        .methods
        .iter()
        .any(|member| member.name.eq_ignore_ascii_case(name))
    {
        OccurrenceKind::Method
    } else if class
        .properties
        .iter()
        .any(|member| member.name.as_str() == name)
    {
        OccurrenceKind::Property
    } else if is_static {
        OccurrenceKind::Constant
    } else {
        OccurrenceKind::Property
    }
}

pub(super) fn declared_constant_symbol(
    source: &str,
    context: &FileContext,
    range: ByteRange,
    name: &str,
) -> String {
    let is_define_string = source
        .as_bytes()
        .get(range.end as usize)
        .is_some_and(|byte| matches!(byte, b'\'' | b'"'));
    if is_define_string {
        name.trim_start_matches('\\').to_string()
    } else {
        context.resolve_name_at(name, range.start)
    }
}
