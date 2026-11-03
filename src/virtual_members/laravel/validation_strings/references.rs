//! References embedded in rule parameters and form-request message keys.

use std::borrow::Cow;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{CompletionItemKind, Location};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::{Fqn, find_class_at_offset};
use crate::types::{ClassInfo, ClassLikeKind, FileContext, PropertySource};

use super::super::validation_rules::{RulesSource, form_request_rules, rules_from_source_range};
use super::catalog::RuleCatalog;
use super::context::{decoded_literal, literal_value};
use super::{SiteKind, ValidationOwner, ValidationSite, location, token_at};

/// A declaration a validation string may name.
pub(crate) struct Candidate {
    /// The text inserted for the declaration.
    pub name: String,
    /// Its type or documentation.
    pub detail: String,
    /// The declaration site, when already available without additional reads.
    pub location: Option<Location>,
    /// The kind shown in completion.
    pub kind: CompletionItemKind,
}

/// The part of a literal the cursor edits or navigates.
pub(crate) struct Selection {
    /// Start of the text to replace.
    pub start: usize,
    /// End of the text to replace.
    pub end: usize,
    /// The cursor, so candidate enumeration can discard unrelated prefixes.
    pub prefix_end: usize,
    /// The meaning of the selected text.
    pub kind: SelectionKind,
}

/// The declarations meaningful at a validation-string position.
pub(crate) enum SelectionKind {
    Rule,
    Field,
    Table,
    Column(String),
    Enum,
    Message,
    Attribute,
}

/// Identify the token without consuming delimiters or another rule's text.
pub(crate) fn selection_at(
    site: &ValidationSite,
    content: &str,
    cursor: usize,
    catalog: &RuleCatalog,
) -> Option<Selection> {
    let whole = |kind| {
        Some(Selection {
            start: site.start,
            end: site.end,
            prefix_end: cursor,
            kind,
        })
    };
    match site.kind {
        SiteKind::MessageKey => return whole(SelectionKind::Message),
        SiteKind::AttributeKey => return whole(SelectionKind::Attribute),
        SiteKind::InValue => return whole(SelectionKind::Enum),
        SiteKind::Rule => {}
    }
    let token = token_at(site, content, cursor)?;
    if cursor <= token.name_end {
        return Some(Selection {
            start: token.start,
            end: token.name_end,
            prefix_end: cursor,
            kind: SelectionKind::Rule,
        });
    }
    let params = token.parameters_start?;
    let decoded = decoded_literal(site, content, params, token.end)?;
    let ranges = csv_ranges(&decoded.value);
    let (index, &(start, end)) = ranges.iter().enumerate().find(|(_, (start, end))| {
        decoded.source_offset(*start) <= cursor && cursor <= decoded.source_offset(*end)
    })?;
    let name = literal_value(site, content, token.start, token.name_end)?;
    let kind = if super::rule_matches(&name, "exists") || super::rule_matches(&name, "unique") {
        match index {
            0 => SelectionKind::Table,
            1 => SelectionKind::Column(
                csv_value(&decoded.value[ranges[0].0..ranges[0].1]).into_owned(),
            ),
            _ => return None,
        }
    } else if super::rule_matches(&name, "in") || super::rule_matches(&name, "not_in") {
        SelectionKind::Enum
    } else {
        let rule = catalog
            .dependent_rules
            .iter()
            .find(|rule| super::rule_matches(rule, &name))?;
        // Conditional rules alternate a field with comparison values;
        // those values are not field references.
        if (rule.ends_with("_if") || rule.ends_with("_unless")) && index > 0 {
            return None;
        }
        SelectionKind::Field
    };
    Some(Selection {
        start: decoded.source_offset(start),
        end: decoded.source_offset(end),
        prefix_end: cursor,
        kind,
    })
}

/// Read the PHP value, and CSV parameter value where applicable, of a selection.
pub(crate) fn selection_value<'a>(
    site: &ValidationSite,
    selection: &Selection,
    content: &'a str,
    end: usize,
) -> Option<Cow<'a, str>> {
    let value = literal_value(site, content, selection.start, end)?;
    if site.kind != SiteKind::Rule || matches!(selection.kind, SelectionKind::Rule) {
        return Some(value);
    }
    Some(match value {
        Cow::Borrowed(value) => csv_value(value),
        Cow::Owned(value) => Cow::Owned(csv_value(&value).into_owned()),
    })
}

// Laravel parses parameters with str_getcsv, including doubled quotes and a
// backslash escape that preserves the backslash in the resulting value.
fn csv_ranges(value: &str) -> Vec<(usize, usize)> {
    let bytes = value.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    loop {
        let leading = value[start..].len() - value[start..].trim_start().len();
        let mut index = start + leading;
        if bytes.get(index) == Some(&b'"') {
            index += 1;
            while index < bytes.len() {
                match bytes[index] {
                    b'\\' if index + 1 < bytes.len() => index += 2,
                    b'"' if bytes.get(index + 1) == Some(&b'"') => index += 2,
                    b'"' => {
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
        }
        while index < bytes.len() && bytes[index] != b',' {
            index += 1;
        }
        ranges.push((start, index));
        if index == bytes.len() {
            return ranges;
        }
        start = index + 1;
    }
}

fn csv_value(value: &str) -> Cow<'_, str> {
    let trimmed = value.trim_start();
    let Some(inner) = trimmed.strip_prefix('"') else {
        return Cow::Borrowed(value);
    };
    let bytes = inner.as_bytes();
    let mut decoded = String::with_capacity(inner.len());
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if index + 1 < bytes.len() => index += 2,
            b'"' => {
                decoded.push_str(&inner[start..index]);
                if bytes.get(index + 1) == Some(&b'"') {
                    decoded.push('"');
                    index += 2;
                    start = index;
                } else {
                    decoded.push_str(&inner[index + 1..]);
                    return Cow::Owned(decoded);
                }
            }
            _ => index += 1,
        }
    }
    decoded.push_str(&inner[start..]);
    Cow::Owned(decoded)
}

/// Encode a CSV parameter when Laravel can read it back without changing it.
pub(crate) fn csv_parameter(value: &str) -> Option<Cow<'_, str>> {
    if !value.is_empty()
        && !value.contains([',', '\r', '\n'])
        && !value.trim_start().starts_with('"')
    {
        return Some(Cow::Borrowed(value));
    }
    // With PHP's default CSV escape, these combinations consume an enclosure.
    // Rule::in([...]) remains available for values that cannot use this spelling.
    if value.ends_with('\\') || value.contains("\\\"") {
        return None;
    }
    Some(Cow::Owned(format!("\"{}\"", value.replace('"', "\"\""))))
}

/// Declarations appropriate for one confirmed validation literal.
pub(crate) fn candidates_at(
    backend: &Backend,
    uri: &str,
    content: &str,
    ctx: &FileContext,
    site: &ValidationSite,
    selection: &Selection,
) -> Vec<Candidate> {
    let catalog = backend.cached_validation_rules();
    let mut candidates = match &selection.kind {
        SelectionKind::Rule => catalog
            .rules
            .iter()
            .map(|rule| Candidate {
                name: rule.name.clone(),
                detail: rule.description.clone(),
                location: rule.location.clone(),
                kind: CompletionItemKind::Function,
            })
            .collect(),
        SelectionKind::Field | SelectionKind::Attribute => {
            let mut fields = field_candidates(backend, uri, content, ctx, site).0;
            let prefix =
                selection_value(site, selection, content, selection.prefix_end).unwrap_or_default();
            for field in &mut fields {
                field.name = concrete_field_name(&field.name, &prefix);
            }
            fields
        }
        SelectionKind::Message => {
            let (fields, _) = field_candidates(backend, uri, content, ctx, site);
            let mut candidates = Vec::new();
            let prefix =
                selection_value(site, selection, content, selection.prefix_end).unwrap_or_default();
            for field in fields {
                let name = message_field_name(&field.name, &prefix);
                if !name.starts_with(prefix.as_ref())
                    && !prefix
                        .strip_prefix(&name)
                        .is_some_and(|suffix| suffix.starts_with('.'))
                {
                    continue;
                }
                for rule in &catalog.rules {
                    let candidate_name = format!("{name}.{}", rule.name);
                    if !candidate_name.starts_with(prefix.as_ref()) {
                        continue;
                    }
                    candidates.push(Candidate {
                        name: candidate_name,
                        detail: rule.description.clone(),
                        location: field.location.clone(),
                        kind: CompletionItemKind::Field,
                    });
                }
                candidates.push(Candidate { name, ..field });
            }
            candidates
        }
        SelectionKind::Table => table_candidates(backend, &catalog),
        SelectionKind::Column(table) => column_candidates(backend, &catalog, table),
        SelectionKind::Enum => {
            enum_candidates(backend, content, ctx, site, &catalog).unwrap_or_default()
        }
    };
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    candidates.dedup_by(|left, right| left.name == right.name);
    candidates
}

/// Field declarations, including inherited request rules and their source files.
pub(crate) fn field_candidates(
    backend: &Backend,
    uri: &str,
    content: &str,
    ctx: &FileContext,
    site: &ValidationSite,
) -> (Vec<Candidate>, bool) {
    if let ValidationOwner::RulesMethod { offset, .. } = site.owner.as_ref()
        && let Some(rules) = find_class_at_offset(&ctx.classes, *offset)
            .and_then(|class| form_request_rules(backend, class, uri, content))
    {
        let fields = rules
            .rules
            .entries
            .iter()
            .map(|field| {
                let (uri, text) = match (&field.origin, &rules.source) {
                    (Some(file), _) | (None, RulesSource::OtherFile(file)) => {
                        (file.uri.as_str(), file.content.as_str())
                    }
                    (None, RulesSource::CurrentFile) => (uri, content),
                };
                Candidate {
                    name: field.key.clone(),
                    detail: field.rules.clone(),
                    location: location(uri, text, field.key_start, field.key_end),
                    kind: CompletionItemKind::Field,
                }
            })
            .collect();
        return (fields, rules.rules.keys_complete);
    }
    if matches!(site.kind, SiteKind::MessageKey | SiteKind::AttributeKey) {
        return (Vec::new(), false);
    }
    if let ValidationOwner::Method {
        receiver_start,
        receiver_end,
        method,
        ..
    } = site.owner.as_ref()
        && method.eq_ignore_ascii_case("sometimes")
        && let Some((start, end)) =
            validator_rules_range(content, *receiver_start, *receiver_end, ctx)
        && let Some(rules) = rules_from_source_range(content, start, end)
    {
        let mut fields: Vec<_> = rules
            .entries
            .iter()
            .map(|field| Candidate {
                name: field.key.clone(),
                detail: field.rules.clone(),
                location: location(uri, content, field.key_start, field.key_end),
                kind: CompletionItemKind::Field,
            })
            .collect();
        for field in site.fields.iter().filter(|field| {
            !rules
                .entries
                .iter()
                .any(|existing| existing.key == field.name)
        }) {
            fields.push(Candidate {
                name: field.name.clone(),
                detail: content
                    .get(field.value_start..field.value_end)
                    .unwrap_or_default()
                    .to_string(),
                location: location(uri, content, field.start, field.end),
                kind: CompletionItemKind::Field,
            });
        }
        return (fields, false);
    }
    (
        site.fields
            .iter()
            .map(|field| Candidate {
                name: field.name.clone(),
                detail: content
                    .get(field.value_start..field.value_end)
                    .unwrap_or_default()
                    .to_string(),
                location: location(uri, content, field.start, field.end),
                kind: CompletionItemKind::Field,
            })
            .collect(),
        site.fields_complete,
    )
}

/// Whether two field patterns can name the same concrete input field.
pub(crate) fn field_matches(pattern: &str, field: &str) -> bool {
    let mut fields = field.split('.');
    for pattern in pattern.split('.') {
        let Some(field) = fields.next() else {
            return false;
        };
        if !segment_matches(pattern, field) {
            return false;
        }
    }
    fields.next().is_none()
}

fn segment_matches(left: &str, right: &str) -> bool {
    match (left.split_once('*'), right.split_once('*')) {
        (None, None) => left == right,
        (Some((prefix, tail)), None) => wildcard_matches(prefix, tail, right),
        (None, Some((prefix, tail))) => wildcard_matches(prefix, tail, left),
        (Some((left_prefix, _)), Some((right_prefix, _))) => {
            // Their middle fragments can be interleaved inside the stars;
            // only the anchored prefix and suffix constrain both patterns.
            let left_suffix = left.rsplit_once('*').unwrap().1;
            let right_suffix = right.rsplit_once('*').unwrap().1;
            (left_prefix.starts_with(right_prefix) || right_prefix.starts_with(left_prefix))
                && (left_suffix.ends_with(right_suffix) || right_suffix.ends_with(left_suffix))
        }
    }
}

fn wildcard_matches(prefix: &str, tail: &str, text: &str) -> bool {
    let Some(mut rest) = text.strip_prefix(prefix) else {
        return false;
    };
    let mut fragments = tail.split('*');
    let suffix = fragments.next_back().unwrap_or_default();
    for fragment in fragments {
        let Some(offset) = rest.find(fragment) else {
            return false;
        };
        rest = &rest[offset + fragment.len()..];
    }
    rest.ends_with(suffix)
}

/// Whether a custom message or attribute key addresses a declared field/rule.
pub(crate) fn message_key_known(
    key: &str,
    kind: SiteKind,
    fields: &[Candidate],
    catalog: &RuleCatalog,
) -> bool {
    if fields.iter().any(|field| field_matches(key, &field.name)) {
        return true;
    }
    if kind != SiteKind::MessageKey {
        return false;
    }
    if !catalog.complete && !key.contains('.') {
        return true;
    }
    let known_rule = |name: &str| {
        catalog
            .rules
            .iter()
            .any(|rule| field_matches(name, &rule.name))
            || catalog.classes.iter().any(|class| {
                field_matches(name, &class.fqn)
                    || field_matches(
                        name,
                        &super::super::helpers::camel_to_snake(crate::util::short_name(&class.fqn)),
                    )
            })
    };
    known_rule(key)
        || key.rsplit_once('.').is_some_and(|(field, rule)| {
            fields
                .iter()
                .any(|candidate| field_matches(field, &candidate.name))
                && (!catalog.complete || known_rule(rule))
        })
}

fn concrete_field_name(field: &str, prefix: &str) -> String {
    if !field.contains('*') {
        return field.to_string();
    }
    let mut typed = prefix.split('.');
    field
        .split('.')
        .map(|segment| {
            let typed = typed.next().unwrap_or_default();
            if segment == "*" && !typed.is_empty() {
                typed
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn message_field_name(field: &str, prefix: &str) -> String {
    let dots = field.bytes().filter(|byte| *byte == b'.').count();
    if let Some((end, _)) = prefix.match_indices('.').nth(dots)
        && field_matches(&prefix[..end], field)
    {
        return prefix[..end].to_string();
    }
    concrete_field_name(field, prefix)
}

/// Find the `make()` whose result the selected validator variable still holds.
fn validator_rules_range(
    content: &str,
    start: usize,
    end: usize,
    ctx: &FileContext,
) -> Option<(usize, usize)> {
    crate::parser::with_parsed_program(content, "validator_rule_source", |program, _| {
        let root = Node::Program(program);
        let expression = expression_at(root, start as u32, end as u32)?;
        let expression = if let Expression::Variable(Variable::Direct(variable)) = expression {
            let scope = innermost_scope(root, start as u32)?;
            let mut latest = None;
            last_assignment(scope, start as u32, variable.name, false, &mut latest);
            latest?.1?
        } else {
            expression
        };
        make_rules_range(expression, ctx)
    })
}

fn expression_at<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    start: u32,
    end: u32,
) -> Option<&'ast Expression<'arena>> {
    if let Node::Expression(expression) = node
        && expression.span().start.offset == start
        && expression.span().end.offset == end
    {
        return Some(expression);
    }
    let mut found = None;
    node.visit_children(|child| {
        if found.is_none() && child.span().start.offset <= start && child.span().end.offset >= end {
            found = expression_at(child, start, end);
        }
    });
    found
}

fn innermost_scope<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    cursor: u32,
) -> Option<Node<'ast, 'arena>> {
    let mut found = None;
    node.visit_children(|child| {
        if child.span().start.offset <= cursor && child.span().end.offset >= cursor {
            found = innermost_scope(child, cursor).or(found);
        }
    });
    found.or(match node {
        Node::Program(_) => Some(node),
        Node::Method(Method {
            body: MethodBody::Concrete(block),
            ..
        }) => Some(Node::Block(block)),
        Node::Function(function) => Some(Node::Block(&function.body)),
        Node::Closure(closure) => Some(Node::Block(&closure.body)),
        Node::ArrowFunction(function) => Some(Node::Expression(function.expression)),
        _ => None,
    })
}

fn last_assignment<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    cursor: u32,
    variable: &[u8],
    conditional: bool,
    latest: &mut Option<(u32, Option<&'ast Expression<'arena>>)>,
) {
    let conditional = conditional
        || matches!(
            node,
            Node::If(_)
                | Node::Switch(_)
                | Node::Match(_)
                | Node::Conditional(_)
                | Node::While(_)
                | Node::DoWhile(_)
                | Node::For(_)
                | Node::Foreach(_)
                | Node::Try(_)
        );
    if let Node::Assignment(assignment) = node
        && matches!(assignment.lhs, Expression::Variable(Variable::Direct(target)) if target.name == variable)
        && node.span().end.offset <= cursor
        && latest
            .as_ref()
            .is_none_or(|(end, _)| node.span().end.offset >= *end)
    {
        *latest = Some((
            node.span().end.offset,
            (!conditional).then_some(assignment.rhs),
        ));
    }
    node.visit_children(|child| {
        if child.span().start.offset < cursor
            && !matches!(
                child,
                Node::Closure(_)
                    | Node::ArrowFunction(_)
                    | Node::Function(_)
                    | Node::Method(_)
                    | Node::PropertyHook(_)
            )
        {
            last_assignment(child, cursor, variable, conditional, latest);
        }
    });
}

fn make_rules_range(expression: &Expression<'_>, ctx: &FileContext) -> Option<(usize, usize)> {
    let Expression::Call(Call::StaticMethod(call)) = expression else {
        return None;
    };
    let ClassLikeMemberSelector::Identifier(method) = &call.method else {
        return None;
    };
    if !method.value.eq_ignore_ascii_case(b"make") {
        return None;
    }
    let Expression::Identifier(class) = call.class else {
        return None;
    };
    let fqn = ctx.resolve_name_at(bytes_to_str(class.value()), class.span().start.offset);
    if !matches!(
        fqn.as_str(),
        "Illuminate\\Support\\Facades\\Validator" | "Illuminate\\Validation\\Factory"
    ) {
        return None;
    }
    let rules =
        call.argument_list.arguments.iter().enumerate().find_map(
            |(index, argument)| match argument {
                Argument::Named(named) if named.name.value == b"rules" => Some(named.value),
                Argument::Positional(positional) if index == 1 && positional.ellipsis.is_none() => {
                    Some(positional.value)
                }
                _ => None,
            },
        )?;
    Some((
        rules.span().start.offset as usize,
        rules.span().end.offset as usize,
    ))
}

fn class_location(
    backend: &Backend,
    class: &ClassInfo,
    offset: usize,
    length: usize,
) -> Option<Location> {
    let uri = backend.class_declaration_uri(&class.fqn())?;
    let content = backend.get_file_content_arc(&uri)?;
    location(&uri, &content, offset, offset + length)
}

fn table_candidates(backend: &Backend, catalog: &RuleCatalog) -> Vec<Candidate> {
    let mut out = Vec::new();
    let index = backend.laravel.schema_index.read();
    let default_connection = index.default_connection.as_deref().unwrap_or("default");
    for model in &catalog.models {
        if let Some(name) = super::super::table_names::model_table_name(model)
            && let Some(connection) = model_connection(model, default_connection)
        {
            if connection == default_connection {
                out.push(Candidate {
                    name: name.clone(),
                    detail: model.fqn().to_string(),
                    location: None,
                    kind: CompletionItemKind::Class,
                });
            }
            if index.default_connection.is_some()
                || model
                    .laravel()
                    .is_some_and(|metadata| metadata.connection_name.is_some())
            {
                out.push(Candidate {
                    name: format!("{connection}.{name}"),
                    detail: model.fqn().to_string(),
                    location: None,
                    kind: CompletionItemKind::Class,
                });
            }
        }
    }
    for table in index.tables() {
        if table.connection == default_connection {
            out.push(Candidate {
                name: table.name.clone(),
                detail: format!("Database table ({})", table.connection),
                location: None,
                kind: CompletionItemKind::Class,
            });
        }
        if index.default_connection.is_some() || table.connection != default_connection {
            out.push(Candidate {
                name: format!("{}.{}", table.connection, table.name),
                detail: format!("Database table ({})", table.connection),
                location: None,
                kind: CompletionItemKind::Class,
            });
        }
    }
    out
}

fn model_connection<'a>(model: &'a ClassInfo, default: &'a str) -> Option<&'a str> {
    let metadata = model.laravel()?;
    metadata
        .connection_name
        .as_deref()
        .or_else(|| (!metadata.has_get_connection_name_method).then_some(default))
}

fn matches_table(model: &ClassInfo, table: &str, default_connection: &str) -> bool {
    let (connection, table) = table
        .split_once('.')
        .map_or((None, table), |(connection, table)| {
            (Some(connection), table)
        });
    if model.fqn().as_str() == table.trim_start_matches('\\') {
        return true;
    }
    model_connection(model, default_connection) == Some(connection.unwrap_or(default_connection))
        && super::super::table_names::model_table_name(model).as_deref() == Some(table)
}

/// Resolve only the selected database name, avoiding source reads for every
/// completion candidate.
pub(crate) fn database_definition(
    backend: &Backend,
    selection: &Selection,
    name: &str,
) -> Option<Location> {
    let (table, column) = match &selection.kind {
        SelectionKind::Table => (name, None),
        SelectionKind::Column(table) => (table.as_str(), Some(name)),
        _ => return None,
    };
    let catalog = backend.cached_validation_rules();
    let default_connection = backend
        .laravel
        .schema_index
        .read()
        .default_connection
        .clone()
        .unwrap_or_else(|| "default".to_string());
    for model in catalog
        .models
        .iter()
        .filter(|model| matches_table(model, table, &default_connection))
    {
        let location = match column {
            Some(column) => backend.model_column_declaration_location(model, column),
            None => class_location(backend, model, model.keyword_offset as usize, 5),
        };
        if location.is_some() {
            return location;
        }
    }
    super::schema::definition(backend, table, column)
}

fn column_candidates(backend: &Backend, catalog: &RuleCatalog, table: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    let index = backend.laravel.schema_index.read();
    let default_connection = index.default_connection.as_deref().unwrap_or("default");
    for model in catalog
        .models
        .iter()
        .filter(|model| matches_table(model, table, default_connection))
    {
        for name in super::super::where_property::collect_column_names(model) {
            if !is_model_column(model, &name) {
                continue;
            }
            let property = model
                .properties
                .iter()
                .find(|property| property.name == name);
            out.push(Candidate {
                location: None,
                detail: property
                    .and_then(|property| property.type_hint.as_ref())
                    .map_or_else(|| model.fqn().to_string(), ToString::to_string),
                name,
                kind: CompletionItemKind::Field,
            });
        }
    }
    let (connection, name) = table.split_once('.').unwrap_or((default_connection, table));
    for table in index
        .tables()
        .filter(|table| table.name == name && connection == table.connection)
    {
        out.extend(table.columns.iter().map(|column| Candidate {
            name: column.name.clone(),
            detail: column.php_type.to_string(),
            location: None,
            kind: CompletionItemKind::Field,
        }));
    }
    out
}

fn is_model_column(model: &ClassInfo, name: &str) -> bool {
    if name == "*" {
        return false;
    }
    let is_relation = |source: Option<&PropertySource>| {
        matches!(
            source,
            Some(
                PropertySource::Relationship { .. }
                    | PropertySource::RelationshipCount { .. }
                    | PropertySource::Pivot
            )
        )
    };
    if !model
        .properties
        .iter()
        .any(|property| property.name == name && is_relation(property.source.as_ref()))
    {
        return true;
    }
    model
        .properties
        .iter()
        .any(|property| property.name == name && !is_relation(property.source.as_ref()))
        || model
            .doc_properties()
            .iter()
            .any(|(column, _)| column.as_str() == name)
        || model.laravel().is_some_and(|metadata| {
            metadata
                .casts_definitions
                .iter()
                .any(|(column, _)| column == name)
                || metadata
                    .attributes_definitions
                    .iter()
                    .any(|(column, _)| column == name)
                || metadata
                    .dates_definitions
                    .iter()
                    .any(|column| column == name)
        })
}

fn enum_candidates(
    backend: &Backend,
    content: &str,
    ctx: &FileContext,
    site: &ValidationSite,
    catalog: &RuleCatalog,
) -> Option<Vec<Candidate>> {
    let field = site.field.and_then(|index| site.fields.get(index))?;
    let array = match site.recovered_rules.as_deref() {
        Some(array) => Cow::Borrowed(array),
        None => Cow::Owned(rules_from_source_range(
            content,
            site.array_start,
            site.array_end,
        )?),
    };
    let rule = array.entries.iter().find(|rule| rule.key == field.name)?;
    let resolved = backend.resolved_class_loader_with(&ctx.classes);
    let mut enums = Vec::new();
    if let Some(name) = &rule.enum_class {
        enums.extend(resolved(Fqn::new(name)));
    }
    let default_connection = backend
        .laravel
        .schema_index
        .read()
        .default_connection
        .clone()
        .unwrap_or_else(|| "default".to_string());
    for part in rule.rules.split('|') {
        let Some((name, args)) = part.split_once(':') else {
            continue;
        };
        if !super::rule_matches(name, "exists") && !super::rule_matches(name, "unique") {
            continue;
        }
        let mut args = args.split(',');
        let table = args.next().unwrap_or_default();
        let column = args.next().unwrap_or(&field.name);
        for model in catalog
            .models
            .iter()
            .filter(|model| matches_table(model, table, &default_connection))
        {
            if let Some(property) = model
                .properties
                .iter()
                .find(|property| property.name == column)
                && let Some(name) = property.type_hint.as_ref().and_then(|ty| ty.base_name())
            {
                enums.extend(resolved(Fqn::new(name)));
            }
            if let Some(metadata) = model.laravel()
                && let Some((_, name)) = metadata
                    .casts_definitions
                    .iter()
                    .find(|(name, _)| name == column)
            {
                enums.extend(resolved(Fqn::new(name)));
            }
        }
    }
    let mut out = Vec::new();
    for class in enums
        .iter()
        .filter(|class| class.kind == ClassLikeKind::Enum)
    {
        for case in class.constants.iter().filter(|case| case.is_enum_case) {
            let Some(value) = case.enum_value.as_deref() else {
                continue;
            };
            let Some(value) =
                crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(value)
            else {
                continue;
            };
            let Some(value) = value.as_literal() else {
                continue;
            };
            let name = match value {
                crate::php_type::LiteralValue::String(_) => {
                    value.string_content().map(|value| value.into_owned())
                }
                crate::php_type::LiteralValue::Int(_) => {
                    value.parse_i64().map(|value| value.to_string())
                }
                crate::php_type::LiteralValue::Float(_) => None,
            };
            let Some(name) = name else {
                continue;
            };
            out.push(Candidate {
                name,
                detail: format!("{}::{}", class.fqn(), case.name),
                location: class_location(
                    backend,
                    class,
                    case.name_offset as usize,
                    case.name.len(),
                ),
                kind: CompletionItemKind::EnumMember,
            });
        }
    }
    Some(out)
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod tests;
