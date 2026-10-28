//! Declaration locations for validation rules that name committed database schema.

use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer};
use tower_lsp_server::gen_lsp_types::{Location, Position};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;

use super::super::database_schema::blueprint_declares_column;
use super::super::helpers::extract_string_literal;

const SCHEMA: &str = "Illuminate\\Support\\Facades\\Schema";

/// Find a table or column in the schema sources the project already indexed.
pub(super) fn definition(backend: &Backend, table: &str, column: Option<&str>) -> Option<Location> {
    if table.is_empty() {
        return None;
    }
    let (connection, table, default_connection, migrations, dumps) = {
        let index = backend.laravel.schema_index.read();
        let default_connection = index.default_connection.as_deref().unwrap_or("default");
        let (connection, table) = table
            .split_once('.')
            .filter(|(connection, table)| {
                index.connection_drivers.contains_key(*connection)
                    || index.table(connection, table).is_some()
            })
            .unwrap_or((default_connection, table));
        if table.is_empty() {
            return None;
        }
        let migrations: Vec<_> = index
            .migration_sources()
            .rev()
            .filter(|(_, content)| {
                content
                    .as_bytes()
                    .windows(table.len())
                    .any(|part| part.eq_ignore_ascii_case(table.as_bytes()))
            })
            .map(|(path, content)| (path.to_path_buf(), content.to_string()))
            .collect();
        let dumps: Vec<_> = index
            .dump_paths_for(connection)
            .map(std::path::Path::to_path_buf)
            .collect();
        (
            connection.to_string(),
            table.to_string(),
            default_connection.to_string(),
            migrations,
            dumps,
        )
    };
    for (path, content) in migrations {
        if let Some(offset) =
            migration_definition(&content, &table, column, &connection, &default_connection)
        {
            return Some(crate::definition::point_location(
                crate::util::parse_url(&crate::util::path_to_uri(&path))?,
                crate::text_position::offset_to_position(&content, offset),
            ));
        }
    }
    for path in dumps {
        let uri = crate::util::path_to_uri(&path);
        if let Some(content) = backend.get_file_content(&uri)
            && let Some(position) = sql_definition(&content, &table, column)
        {
            return Some(crate::definition::point_location(
                crate::util::parse_url(&uri)?,
                position,
            ));
        }
    }
    None
}

fn migration_definition(
    content: &str,
    table: &str,
    column: Option<&str>,
    connection: &str,
    default_connection: &str,
) -> Option<usize> {
    crate::parser::with_parsed_program(content, "validation_schema_definition", |program, _| {
        let arena = mago_allocator::LocalArena::new();
        let names = NameResolver::new(&arena).resolve(program);
        let names = OwnedResolvedNames::from_resolved(&names);
        let search = MigrationSearch {
            content,
            table,
            column,
            connection,
            names: &names,
        };
        let mut found = None;
        search.walk(Node::Program(program), default_connection, &mut found);
        found
    })
}

struct MigrationSearch<'a> {
    content: &'a str,
    table: &'a str,
    column: Option<&'a str>,
    connection: &'a str,
    names: &'a OwnedResolvedNames,
}

impl MigrationSearch<'_> {
    fn walk(&self, node: Node<'_, '_>, connection: &str, found: &mut Option<usize>) {
        if matches!(node, Node::Method(method) if method.name.value.eq_ignore_ascii_case(b"down")) {
            return;
        }
        let declared_connection = match node {
            Node::Class(class) => class_connection(class.members.iter(), self.content),
            Node::AnonymousClass(class) => class_connection(class.members.iter(), self.content),
            _ => None,
        };
        let connection = declared_connection.unwrap_or(connection);
        match node {
            Node::StaticMethodCall(call) if self.is_schema(call.class) => {
                self.call(&call.method, &call.argument_list, connection, found);
            }
            Node::MethodCall(call) => {
                if let Expression::Call(Call::StaticMethod(root)) = call.object
                    && self.is_schema(root.class)
                    && matches!(&root.method, ClassLikeMemberSelector::Identifier(method) if method.value.eq_ignore_ascii_case(b"connection"))
                    && let Some((connection, _, _)) = argument(&root.argument_list, 0, "name")
                        .and_then(|arg| extract_string_literal(arg, self.content))
                {
                    self.call(&call.method, &call.argument_list, connection, found);
                }
            }
            _ => {}
        }
        node.visit_children(|child| self.walk(child, connection, found));
    }

    fn is_schema(&self, expression: &Expression<'_>) -> bool {
        matches!(expression, Expression::Identifier(identifier) if self.names.get(identifier.span().start.offset).is_some_and(|name| name.eq_ignore_ascii_case(SCHEMA)))
    }

    fn call(
        &self,
        method: &ClassLikeMemberSelector<'_>,
        args: &ArgumentList<'_>,
        connection: &str,
        found: &mut Option<usize>,
    ) {
        let ClassLikeMemberSelector::Identifier(method) = method else {
            return;
        };
        if !connection.eq_ignore_ascii_case(self.connection) {
            return;
        }
        let method = bytes_to_str(method.value);
        if !(method.eq_ignore_ascii_case("create")
            || (self.column.is_some() && method.eq_ignore_ascii_case("table")))
        {
            return;
        }
        let Some((table, start, _)) =
            argument(args, 0, "table").and_then(|arg| extract_string_literal(arg, self.content))
        else {
            return;
        };
        if !table.eq_ignore_ascii_case(self.table) {
            return;
        }
        let Some(column) = self.column else {
            *found = Some(start);
            return;
        };
        if let Some(Expression::Closure(closure)) = argument(args, 1, "callback")
            && let Some(offset) = closure_column(closure, self.content, column)
        {
            *found = Some(offset);
        }
    }
}

fn class_connection<'a>(
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &'a str,
) -> Option<&'a str> {
    members
        .filter_map(|member| match member {
            ClassLikeMember::Property(Property::Plain(property)) => Some(property),
            _ => None,
        })
        .flat_map(|property| property.items.iter())
        .find_map(|item| {
            let PropertyItem::Concrete(property) = item else {
                return None;
            };
            (property.variable.name == b"$connection")
                .then(|| extract_string_literal(property.value, content))
                .flatten()
                .map(|(connection, _, _)| connection)
        })
}

fn argument<'a, 'arena>(
    arguments: &'a ArgumentList<'arena>,
    index: usize,
    name: &str,
) -> Option<&'a Expression<'arena>> {
    arguments
        .arguments
        .iter()
        .enumerate()
        .find_map(|(position, argument)| match argument {
            Argument::Named(argument) if argument.name.value == name.as_bytes() => {
                Some(argument.value)
            }
            Argument::Positional(argument) if position == index => Some(argument.value),
            _ => None,
        })
}

fn closure_column(
    closure: &function_like::closure::Closure<'_>,
    content: &str,
    column: &str,
) -> Option<usize> {
    let variable = closure.parameter_list.parameters.first()?.variable.name;
    let mut found = None;
    walk_blueprint(
        Node::Block(&closure.body),
        variable,
        content,
        column,
        &mut found,
    );
    found
}

fn walk_blueprint(
    node: Node<'_, '_>,
    variable: &[u8],
    content: &str,
    column: &str,
    found: &mut Option<usize>,
) {
    if matches!(
        node,
        Node::Closure(_)
            | Node::ArrowFunction(_)
            | Node::Function(_)
            | Node::Class(_)
            | Node::AnonymousClass(_)
    ) {
        return;
    }
    if let Node::MethodCall(call) = node
        && matches!(call.object, Expression::Variable(Variable::Direct(receiver)) if receiver.name == variable)
        && let ClassLikeMemberSelector::Identifier(method) = &call.method
    {
        let method_name = bytes_to_str(method.value);
        if method_name.eq_ignore_ascii_case("after") {
            if let Some(Expression::Closure(closure)) = argument(&call.argument_list, 1, "callback")
                && let Some(offset) = closure_column(closure, content, column)
            {
                *found = Some(offset);
            }
        } else if let Some(offset) =
            column_argument(method_name, &call.argument_list, content, column)
        {
            *found = Some(offset.unwrap_or(method.span.start.offset as usize));
        }
    }
    node.visit_children(|child| walk_blueprint(child, variable, content, column, found));
}

fn column_argument(
    method: &str,
    args: &ArgumentList<'_>,
    content: &str,
    wanted: &str,
) -> Option<Option<usize>> {
    let index = usize::from(matches!(method, "addColumn" | "renameColumn"));
    let parameter = if method == "renameColumn" {
        "to"
    } else {
        "column"
    };
    let value =
        argument(args, index, parameter).and_then(|arg| extract_string_literal(arg, content));
    if matches!(
        method,
        "timestamps" | "timestampsTz" | "nullableTimestamps" | "nullableTimestampsTz"
    ) {
        return matches!(wanted, "created_at" | "updated_at").then_some(None);
    }
    if method == "rememberToken" {
        return (wanted == "remember_token").then_some(None);
    }
    if !blueprint_declares_column(method)
        && !matches!(method, "addColumn" | "renameColumn" | "softDeletesDatetime")
    {
        return None;
    }
    if method.to_ascii_lowercase().ends_with("morphs") {
        let (prefix, start, _) = value?;
        return wanted
            .strip_prefix(prefix)
            .is_some_and(|suffix| matches!(suffix, "_id" | "_type"))
            .then_some(Some(start));
    }
    if let Some((name, start, _)) = value {
        return name.eq_ignore_ascii_case(wanted).then_some(Some(start));
    }
    let implicit = match method {
        "id" => "id",
        "uuid" => "uuid",
        "ulid" => "ulid",
        "ipAddress" => "ip_address",
        "macAddress" => "mac_address",
        "softDeletes" | "softDeletesTz" | "softDeletesDatetime" => "deleted_at",
        _ => return None,
    };
    (wanted == implicit).then_some(None)
}

fn sql_definition(content: &str, table: &str, column: Option<&str>) -> Option<Position> {
    let tokens = Tokenizer::new(&GenericDialect {}, content)
        .tokenize_with_location()
        .ok()?;
    let tokens: Vec<_> = tokens
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .collect();
    for (start, token) in tokens.iter().enumerate() {
        if !word_is(token, "create")
            || !tokens
                .get(start + 1)
                .is_some_and(|token| word_is(token, "table"))
        {
            continue;
        }
        let mut index = start + 2;
        if tokens.get(index).is_some_and(|token| word_is(token, "if")) {
            index += 3;
        }
        let mut name = *tokens.get(index)?;
        while tokens
            .get(index + 1)
            .is_some_and(|token| token.token == Token::Period)
        {
            index += 2;
            name = *tokens.get(index)?;
        }
        if !word_is(name, table)
            || !tokens
                .get(index + 1)
                .is_some_and(|token| token.token == Token::LParen)
        {
            continue;
        }
        let Some(column) = column else {
            return sql_position(content, name);
        };
        let mut depth = 1usize;
        let mut declaration = true;
        for token in tokens.iter().skip(index + 2) {
            if depth == 1 && declaration && word_is(token, column) {
                return sql_position(content, token);
            }
            declaration = false;
            match token.token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Token::Comma if depth == 1 => declaration = true,
                _ => {}
            }
        }
    }
    None
}

fn word_is(token: &TokenWithSpan, value: &str) -> bool {
    matches!(&token.token, Token::Word(word) if word.value.eq_ignore_ascii_case(value))
}

fn sql_position(content: &str, token: &TokenWithSpan) -> Option<Position> {
    let line = token.span.start.line.checked_sub(1)? as usize;
    let column = token.span.start.column.checked_sub(1)? as usize;
    let text = content.lines().nth(line)?;
    let character = text
        .chars()
        .take(column)
        .map(char::len_utf16)
        .sum::<usize>();
    Some(Position::new(line as u32, character as u32))
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
