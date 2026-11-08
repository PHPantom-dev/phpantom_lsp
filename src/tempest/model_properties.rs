//! Property names supplied to Tempest's model helpers and typed query builders.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::completion::eloquent_string::{extract_identifier_backwards, extract_receiver_subject};
use crate::completion::named_args::NamedArgContext;
use crate::completion::source::code_context::{CodeContext, code_context_at};
use crate::completion::source::string_argument::{
    StringArgument, StringArgumentPosition, string_argument,
};
use crate::php_type::{PhpType, TypeKind};
use crate::text_position::{byte_range_to_lsp_range, position_to_offset};
use crate::type_engine::resolver::resolve_target_classes_expr;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, ClassInfo, FileContext, MethodInfo, Visibility};

const BUILDERS: &[&str] = &[
    "Tempest\\Database\\Builder\\QueryBuilders\\QueryBuilder",
    "Tempest\\Database\\Builder\\QueryBuilders\\SelectQueryBuilder",
    "Tempest\\Database\\Builder\\QueryBuilders\\CountQueryBuilder",
    "Tempest\\Database\\Builder\\QueryBuilders\\UpdateQueryBuilder",
    "Tempest\\Database\\Builder\\QueryBuilders\\DeleteQueryBuilder",
    "Tempest\\Database\\Builder\\QueryBuilders\\WhereGroupBuilder",
];
const MODEL_TRAIT: &str = "Tempest\\Database\\IsDatabaseModel";

#[derive(Clone, Copy, PartialEq, Eq)]
enum PropertyArgument {
    Field(&'static str),
    Fields,
    Keys,
    Named,
}

fn argument_role(method: &str) -> Option<PropertyArgument> {
    let method = method.to_ascii_lowercase();
    Some(match method.as_str() {
        "where" => PropertyArgument::Field("statement"),
        "orderby" => PropertyArgument::Field("field"),
        "count" | "sum" | "avg" | "min" | "max" => PropertyArgument::Field("column"),
        "select" | "include" => PropertyArgument::Fields,
        "findornew" | "updateorcreate" => PropertyArgument::Keys,
        "new" | "create" | "find" | "update" | "insert" => PropertyArgument::Named,
        name if !name.ends_with("raw")
            && (name.starts_with("where")
                || name.starts_with("andwhere")
                || name.starts_with("orwhere")) =>
        {
            PropertyArgument::Field("field")
        }
        _ => return None,
    })
}

impl PropertyArgument {
    // Tempest's where() tests operator substrings case-sensitively, even
    // inside an identifier. The explicit field APIs do not make that test.
    fn accepts_name(self, name: &str) -> bool {
        bare_property(name)
            && (self != Self::Field("statement")
                || !["LIKE", "IN", "BETWEEN", "EXISTS", "REGEXP"]
                    .iter()
                    .any(|operator| name.contains(operator)))
    }

    fn matches_method(self, method: &MethodInfo) -> bool {
        let Some(first) = method.parameters.first() else {
            return false;
        };
        match self {
            Self::Field(name) => first.name.trim_start_matches('$') == name,
            Self::Fields | Self::Named => first.is_variadic,
            Self::Keys => {
                first.name.trim_start_matches('$') == "find"
                    && method
                        .parameters
                        .get(1)
                        .is_some_and(|p| p.name.trim_start_matches('$') == "update")
            }
        }
    }

    fn matches_string(self, argument: &StringArgument) -> bool {
        match self {
            Self::Field(name) => {
                argument.position == StringArgumentPosition::Direct
                    && argument
                        .name
                        .as_deref()
                        .map_or(argument.index == 0, |actual| actual == name)
            }
            Self::Fields => argument.position == StringArgumentPosition::Direct,
            Self::Keys => {
                matches!(
                    argument.position,
                    StringArgumentPosition::ArrayKey | StringArgumentPosition::ArrayElement
                ) && argument
                    .name
                    .as_deref()
                    .map_or(argument.index < 2, |name| matches!(name, "find" | "update"))
            }
            Self::Named => false,
        }
    }
}

fn bare_property(name: &str) -> bool {
    name.bytes().enumerate().all(|(index, byte)| {
        byte == b'_'
            || byte >= 0x80
            || byte.is_ascii_alphabetic()
            || (index != 0 && byte.is_ascii_digit())
    })
}

fn call_receiver(expression: &str) -> Option<(SubjectExpr, AccessKind, String)> {
    match &mut SubjectExpr::parse(expression) {
        SubjectExpr::PropertyChain { base, property } => Some((
            std::mem::replace(base.as_mut(), SubjectExpr::This),
            AccessKind::Arrow,
            std::mem::take(property),
        )),
        SubjectExpr::StaticAccess { class, member } => Some((
            SubjectExpr::parse(class),
            AccessKind::DoubleColon,
            std::mem::take(member),
        )),
        _ => None,
    }
}

fn model_classes(ty: &PhpType, loader: &ResolvedClassLoader<'_>, out: &mut Vec<Arc<ClassInfo>>) {
    match ty.kind() {
        TypeKind::Union(types) => {
            for ty in types {
                model_classes(ty, loader, out);
            }
        }
        TypeKind::Nullable(inner) | TypeKind::ClassString(Some(inner)) => {
            model_classes(inner, loader, out)
        }
        TypeKind::Literal(value) => {
            if let Some(name) = value.string_content()
                && let Some(class) = loader(Fqn::new(name.trim_start_matches('\\')))
            {
                out.push(class);
            }
        }
        _ => {
            if let Some(name) = ty.base_name()
                && let Some(class) = loader(Fqn::new(name))
            {
                out.push(class);
            }
        }
    }
}

impl Backend {
    fn tempest_property_models(
        &self,
        receiver: &SubjectExpr,
        access: AccessKind,
        method: &str,
        content: &str,
        site: (u32, &FileContext),
    ) -> Option<Vec<Arc<ClassInfo>>> {
        let (offset, ctx) = site;
        let role = argument_role(method)?;
        let loaders = self.file_loaders(ctx);
        let loader = loaders.resolved_class_loader();
        let mut models = Vec::new();
        for target in resolve_target_classes_expr(
            receiver,
            access,
            &loaders.resolution_ctx_at(content, offset),
        ) {
            let Some(class) = target.class_info else {
                continue;
            };
            let resolved = crate::virtual_members::resolve_class_fully_cached(
                &class,
                loader,
                &self.resolved_class_cache,
            );
            let Some(member) = resolved.get_method_ci(method) else {
                continue;
            };
            if member.visibility != Visibility::Public
                || (access == AccessKind::DoubleColon && !member.is_static)
                || !role.matches_method(member)
            {
                continue;
            }
            // These framework builders are final. Read their model directly
            // rather than walking the same ancestors for each builder name.
            if let TypeKind::Generic(generic) = target.type_string.kind()
                && BUILDERS
                    .iter()
                    .any(|builder| generic.name.eq_ignore_ascii_case(builder))
                && let Some(model) = generic.args.first()
            {
                model_classes(model, loader, &mut models);
                continue;
            }
            // An override belongs to the model's own API. Only the method
            // supplied by IsDatabaseModel forwards these names to properties.
            let raw = loader(Fqn::new(&class.fqn())).unwrap_or(class);
            if raw.get_method_ci(method).is_none()
                && crate::inheritance::find_declaring_ancestor(&raw, loader, &|candidate| {
                    candidate.get_method_ci(method).is_some()
                })
                .is_some_and(|(name, _)| name.eq_ignore_ascii_case(MODEL_TRAIT))
            {
                models.push(raw);
            }
        }
        models.sort_by_key(|class| class.fqn());
        models.dedup_by_key(|class| class.fqn());
        for model in &mut models {
            *model = crate::virtual_members::resolve_class_fully_cached(
                model,
                loader,
                &self.resolved_class_cache,
            );
        }
        (!models.is_empty()).then_some(models)
    }

    /// Complete property strings using the receiver's resolved model type.
    pub(crate) fn tempest_model_property_string_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        if !self.is_tempest_database_project() {
            return None;
        }
        let cursor = position_to_offset(content, position) as usize;
        let (quote, quote_char) = code.open_string?;
        let call = code.open_brackets.iter().rev().find(|bracket| {
            bracket.byte == b'('
                && !extract_identifier_backwards(&content[..bracket.code_before])
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("array"))
        })?;
        let (method, _) = extract_identifier_backwards(&content[..call.code_before])?;
        let role = argument_role(method)?;
        let operator = call.callee_operator.as_ref()?;
        let argument = string_argument(content, cursor, code, call)?;
        if !role.matches_string(&argument)
            || !role.accepts_name(content.get(quote + 1..argument.string_end)?)
        {
            return None;
        }
        // The indexed call text comes from the AST, so comments and line
        // breaks inside a fluent chain do not cut off its receiver.
        let map = ctx.uri.as_deref().and_then(|uri| self.symbol_map_for(uri));
        let receiver = map
            .as_ref()
            .filter(|map| map.built_from(content))
            .and_then(|map| map.find_enclosing_call_site(cursor as u32))
            .filter(|site| site.args_start as usize == call.offset + 1)
            .and_then(|site| call_receiver(site.call_expression))
            .map(|(receiver, _, _)| receiver)
            .or_else(|| {
                extract_receiver_subject(content, operator).map(|text| SubjectExpr::parse(&text))
            })?;
        let models = self.tempest_property_models(
            &receiver,
            AccessKind::of_member_access(operator.is_static),
            method,
            content,
            (cursor as u32, ctx),
        )?;
        let prefix = content.get(quote + 1..cursor)?;
        let range = byte_range_to_lsp_range(content, quote + 1, argument.string_end);
        let mut items = Vec::new();
        for model in models {
            for property in model.properties.iter().filter(|p| {
                p.visibility == Visibility::Public
                    && !p.is_static
                    && !p.is_virtual
                    && p.name.starts_with(prefix)
                    && role.accepts_name(&p.name)
                    && !argument.used_keys.iter().any(|key| key == p.name)
            }) {
                items.push(CompletionItem {
                    label: property.name.to_string(),
                    kind: Some(CompletionItemKind::Property),
                    detail: Some(format!("{}::${}", model.fqn(), property.name)),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range,
                        new_text: crate::text_scan::escape_literal_value(
                            &property.name,
                            quote_char,
                        ),
                    })),
                    ..Default::default()
                });
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        Some(CompletionResponse::CompletionItemList(items))
    }

    /// Complete the variadic names model helpers assign or query.
    pub(crate) fn tempest_model_property_named_completions(
        &self,
        context: &NamedArgContext,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<Vec<CompletionItem>> {
        if !self.is_tempest_database_project() {
            return None;
        }
        let (method, _) = extract_identifier_backwards(&context.call_expression)?;
        if argument_role(method) != Some(PropertyArgument::Named) {
            return None;
        }
        let (receiver, access, method) = call_receiver(&context.call_expression)?;
        let models = self.tempest_property_models(
            &receiver,
            access,
            &method,
            content,
            (position_to_offset(content, position), ctx),
        )?;
        let mut items = Vec::new();
        for model in models {
            for property in model.properties.iter().filter(|p| {
                p.visibility == Visibility::Public
                    && !p.is_static
                    && !p.is_virtual
                    && p.name.starts_with(&context.prefix)
                    && !context
                        .existing_named_args
                        .iter()
                        .any(|name| name == p.name)
            }) {
                items.push(CompletionItem {
                    label: format!("{}:", property.name),
                    kind: Some(CompletionItemKind::Property),
                    detail: Some(format!("{}::${}", model.fqn(), property.name)),
                    insert_text: Some(format!("{}: ", property.name)),
                    ..Default::default()
                });
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        Some(items)
    }

    /// Navigate a model property string or a forwarded named argument.
    pub(crate) fn tempest_model_property_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Vec<Location> {
        self.tempest_model_property_locations(uri, content, position)
            .unwrap_or_default()
    }

    fn tempest_model_property_locations(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<Location>> {
        if !self.is_tempest_database_project() {
            return None;
        }
        let offset = position_to_offset(content, position);
        let map = self.symbol_map_for(uri)?;
        let call = map.find_enclosing_call_site(offset)?;
        let (method, _) = extract_identifier_backwards(call.call_expression)?;
        let role = argument_role(method)?;
        if !map.built_from(content) {
            return None;
        }
        let name = if role == PropertyArgument::Named {
            call.named_arg_indices
                .iter()
                .zip(call.named_arg_names.iter())
                .find_map(|(&index, name)| {
                    let start = call.arg_offsets[index as usize];
                    (start <= offset && offset <= start + name.len() as u32).then_some(name)
                })?
        } else {
            let code = code_context_at(content, offset as usize)?;
            let (quote, _) = code.open_string?;
            let paren = code.open_brackets.iter().find(|bracket| {
                bracket.byte == b'(' && bracket.offset + 1 == call.args_start as usize
            })?;
            let argument = string_argument(content, offset as usize, &code, paren)?;
            if !role.matches_string(&argument)
                || argument.position == StringArgumentPosition::ArrayElement
            {
                return None;
            }
            content.get(quote + 1..argument.string_end)?
        };
        if name.is_empty() || !role.accepts_name(name) {
            return None;
        }
        let (receiver, access, method) = call_receiver(call.call_expression)?;
        let ctx = self.file_context(uri);
        let models =
            self.tempest_property_models(&receiver, access, &method, content, (offset, &ctx))?;
        let mut locations = Vec::new();
        for model in models {
            if model.get_property(name).is_some_and(|p| {
                p.visibility == Visibility::Public && !p.is_static && !p.is_virtual
            }) && let Some(location) =
                self.class_member_declaration_location(&model.fqn(), name, true)
                && !locations.contains(&location)
            {
                locations.push(location);
            }
        }
        Some(locations)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_fixtures::make_backend;
    use tower_lsp_server::gen_lsp_types::Position;

    #[test]
    fn property_names_need_the_database_package() {
        let backend = make_backend();
        let dir = tempfile::tempdir().unwrap();
        let open = |name: &str, source: &str| {
            let uri = crate::util::path_to_uri(&dir.path().join(name));
            backend
                .open_files
                .write()
                .insert(uri.clone(), std::sync::Arc::new(source.to_string()));
            backend.update_ast(&uri, source);
            uri
        };
        open(
            "Book.php",
            "<?php namespace Tempest\\Database; trait IsDatabaseModel { public static function create(mixed ...$params): static {} }
namespace App; class Book { use \\Tempest\\Database\\IsDatabaseModel; public string $title; }",
        );
        let source = "<?php \\App\\Book::create(title: 'x');";
        let uri = open("caller.php", source);
        let position = Position::new(0, source.find("title:").unwrap() as u32 + 1);

        backend.set_is_tempest_database(true);
        assert_eq!(
            backend
                .tempest_model_property_definition(&uri, source, position)
                .len(),
            1
        );
        backend.set_is_tempest_database(false);
        assert!(
            backend
                .tempest_model_property_definition(&uri, source, position)
                .is_empty()
        );
    }
}
