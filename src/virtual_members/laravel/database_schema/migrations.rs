use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_span::HasSpan;

use super::*;

pub(super) fn apply_migration_file(index: &mut SchemaIndex, content: &str) {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"migration.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    for stmt in program.statements.iter() {
        apply_migration_stmt(index, stmt, content, None);
    }
}

fn apply_migration_stmt(
    index: &mut SchemaIndex,
    stmt: &Statement<'_>,
    content: &str,
    inherited_connection: Option<&str>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            for stmt in ns.statements().iter() {
                apply_migration_stmt(index, stmt, content, inherited_connection);
            }
        }
        Statement::Class(class) => {
            apply_migration_class(index, class, content, inherited_connection)
        }
        Statement::Return(ret) => {
            if let Some(Expression::AnonymousClass(class)) = ret.value {
                apply_anonymous_migration_class(index, class, content, inherited_connection);
            }
        }
        _ => {}
    }
}

fn apply_migration_class(
    index: &mut SchemaIndex,
    class: &class_like::Class<'_>,
    content: &str,
    inherited_connection: Option<&str>,
) {
    let connection = migration_class_connection(class, content)
        .or_else(|| inherited_connection.map(str::to_string))
        .or_else(|| index.default_connection.clone())
        .unwrap_or_else(|| "default".to_string());
    apply_migration_members(index, class.members.iter(), content, &connection);
}

fn apply_anonymous_migration_class(
    index: &mut SchemaIndex,
    class: &AnonymousClass<'_>,
    content: &str,
    inherited_connection: Option<&str>,
) {
    let connection = migration_anonymous_class_connection(class, content)
        .or_else(|| inherited_connection.map(str::to_string))
        .or_else(|| index.default_connection.clone())
        .unwrap_or_else(|| "default".to_string());
    apply_migration_members(index, class.members.iter(), content, &connection);
}

fn apply_migration_members<'a>(
    index: &mut SchemaIndex,
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &str,
    connection: &str,
) {
    for member in members {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let method_name = crate::atom::bytes_to_str(method.name.value);
        if method_name.eq_ignore_ascii_case("down") {
            continue;
        }
        if let MethodBody::Concrete(body) = &method.body {
            for stmt in body.statements.iter() {
                apply_migration_method_stmt(index, stmt, content, connection);
            }
        }
    }
}

fn migration_anonymous_class_connection(
    class: &AnonymousClass<'_>,
    content: &str,
) -> Option<String> {
    for member in class.members.iter() {
        if let Some(connection) = migration_connection_property(member, content) {
            return Some(connection);
        }
    }
    None
}

fn migration_class_connection(class: &class_like::Class<'_>, content: &str) -> Option<String> {
    for member in class.members.iter() {
        if let Some(connection) = migration_connection_property(member, content) {
            return Some(connection);
        }
    }
    None
}

fn migration_connection_property(member: &ClassLikeMember<'_>, content: &str) -> Option<String> {
    let ClassLikeMember::Property(Property::Plain(prop)) = member else {
        return None;
    };
    for item in prop.items.iter() {
        let PropertyItem::Concrete(concrete) = item else {
            continue;
        };
        if crate::atom::bytes_to_str(concrete.variable.name).trim_start_matches('$') != "connection"
        {
            continue;
        }
        if let Some(value) = string_literal_value(concrete.value, content) {
            return Some(value);
        }
    }
    None
}

fn apply_migration_method_stmt(
    index: &mut SchemaIndex,
    stmt: &Statement<'_>,
    content: &str,
    connection: &str,
) {
    match stmt {
        Statement::Expression(expr) => {
            apply_migration_expr(index, expr.expression, content, connection)
        }
        Statement::If(if_stmt) => {
            crate::parser::for_each_if_branch(if_stmt, |statements| {
                for stmt in statements {
                    apply_migration_method_stmt(index, stmt, content, connection);
                }
            });
        }
        Statement::Block(block) => {
            for stmt in block.statements.iter() {
                apply_migration_method_stmt(index, stmt, content, connection);
            }
        }
        _ => {}
    }
}

fn apply_migration_expr(
    index: &mut SchemaIndex,
    expr: &Expression<'_>,
    content: &str,
    connection: &str,
) {
    match expr {
        Expression::Call(Call::StaticMethod(call)) => {
            if let Some((method, args, call_connection)) =
                schema_static_call(call, content, connection)
            {
                apply_schema_call(index, &call_connection, method, args, content);
            }
        }
        Expression::Call(Call::Method(call)) => {
            if let Some((method, args, call_connection)) =
                schema_method_call(call, content, connection)
            {
                apply_schema_call(index, &call_connection, method, args, content);
            } else {
                apply_migration_expr(index, call.object, content, connection);
            }
        }
        _ => {}
    }
}

fn schema_static_call<'a>(
    call: &'a call::StaticMethodCall<'a>,
    _content: &str,
    connection: &str,
) -> Option<(&'a str, &'a ArgumentList<'a>, String)> {
    if !is_schema_class(call.class) {
        return None;
    }
    let ClassLikeMemberSelector::Identifier(method) = &call.method else {
        return None;
    };
    let method = crate::atom::bytes_to_str(method.value);
    if matches!(method, "connection" | "setConnection") {
        return None;
    }
    Some((method, &call.argument_list, connection.to_string()))
}

fn schema_method_call<'a>(
    call: &'a call::MethodCall<'a>,
    content: &str,
    fallback_connection: &str,
) -> Option<(&'a str, &'a ArgumentList<'a>, String)> {
    let ClassLikeMemberSelector::Identifier(method) = &call.method else {
        return None;
    };
    let Expression::Call(Call::StaticMethod(static_call)) = call.object else {
        return None;
    };
    if !is_schema_class(static_call.class) {
        return None;
    }
    let ClassLikeMemberSelector::Identifier(connection_method) = &static_call.method else {
        return None;
    };
    let connection_method = crate::atom::bytes_to_str(connection_method.value);
    if !matches!(connection_method, "connection" | "setConnection") {
        return None;
    }
    let connection = static_call
        .argument_list
        .arguments
        .iter()
        .next()
        .and_then(|arg| string_literal_value(arg.value(), content))
        .unwrap_or_else(|| fallback_connection.to_string());
    Some((
        crate::atom::bytes_to_str(method.value),
        &call.argument_list,
        connection,
    ))
}

fn is_schema_class(expr: &Expression<'_>) -> bool {
    let Expression::Identifier(identifier) = expr else {
        return false;
    };
    let name = crate::atom::bytes_to_str(identifier.value());
    matches!(
        name.trim_start_matches('\\'),
        "Schema" | "Illuminate\\Support\\Facades\\Schema"
    )
}

fn apply_schema_call(
    index: &mut SchemaIndex,
    connection: &str,
    method: &str,
    args: &ArgumentList<'_>,
    content: &str,
) {
    match method {
        "create" | "table" => {
            let Some(table_name) = args
                .arguments
                .iter()
                .next()
                .and_then(|arg| string_literal_value(arg.value(), content))
            else {
                return;
            };
            if method == "create" {
                index.insert_table(SchemaTable::new(
                    connection.to_string(),
                    table_name.clone(),
                    Vec::new(),
                ));
            }
            for arg in args.arguments.iter().skip(1) {
                if let Expression::Closure(closure) = arg.value() {
                    process_blueprint_closure(index, connection, &table_name, closure, content);
                    break;
                }
            }
        }
        "drop" | "dropIfExists" => {
            if let Some(table_name) = args
                .arguments
                .iter()
                .next()
                .and_then(|arg| string_literal_value(arg.value(), content))
            {
                index.drop_table(connection, &table_name);
            }
        }
        "rename" => {
            let strings = string_args_from_arguments(args, content);
            if strings.len() >= 2 {
                index.rename_table(connection, &strings[0], &strings[1]);
            }
        }
        _ => {}
    }
}

fn process_blueprint_closure(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    closure: &function_like::closure::Closure<'_>,
    content: &str,
) {
    for stmt in closure.body.statements.iter() {
        process_blueprint_stmt(index, connection, table_name, stmt, content);
    }
}

fn process_blueprint_stmt(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    stmt: &Statement<'_>,
    content: &str,
) {
    match stmt {
        Statement::Expression(expr) => {
            if let Expression::Call(Call::Method(call)) = expr.expression {
                process_blueprint_call(index, connection, table_name, call, content);
            }
        }
        Statement::If(if_stmt) => {
            crate::parser::for_each_if_branch(if_stmt, |statements| {
                for stmt in statements {
                    process_blueprint_stmt(index, connection, table_name, stmt, content);
                }
            });
        }
        _ => {}
    }
}

fn process_blueprint_call(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    call: &call::MethodCall<'_>,
    content: &str,
) {
    let root = root_method_call(call);
    let ClassLikeMemberSelector::Identifier(method) = &root.method else {
        return;
    };
    let method = crate::atom::bytes_to_str(method.value);
    let chain = node_text(call, content).unwrap_or_default();
    let args = node_text(&root.argument_list, content).unwrap_or_default();
    if method == "after" {
        for arg in root.argument_list.arguments.iter() {
            if let Expression::Closure(closure) = arg.value() {
                process_blueprint_closure(index, connection, table_name, closure, content);
            }
        }
        return;
    }
    if let Some(closure_text) = index.blueprint_macros.get(method).cloned() {
        expand_blueprint_macro(index, connection, table_name, &closure_text);
        return;
    }
    apply_blueprint_statement(index, connection, table_name, method, args, chain);
}

fn expand_blueprint_macro(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    closure_text: &str,
) {
    let synthetic = format!("<?php $__fn = {closure_text};");
    let arena = LocalArena::new();
    let file_id = FileId::new(b"macro.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, synthetic.as_bytes());

    for stmt in program.statements.iter() {
        if let Statement::Expression(expr) = stmt {
            let assign_rhs = match expr.expression {
                Expression::Assignment(op) => Some(op.rhs),
                _ => None,
            };
            if let Some(Expression::Closure(closure)) = assign_rhs {
                let saved = std::mem::take(&mut index.blueprint_macros);
                for body_stmt in closure.body.statements.iter() {
                    process_blueprint_stmt(index, connection, table_name, body_stmt, &synthetic);
                }
                index.blueprint_macros = saved;
                return;
            }
        }
    }
}

fn root_method_call<'a>(call: &'a call::MethodCall<'a>) -> &'a call::MethodCall<'a> {
    let mut root = call;
    while let Expression::Call(Call::Method(parent)) = root.object {
        root = parent;
    }
    root
}

fn string_literal_value(expr: &Expression<'_>, content: &str) -> Option<String> {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return None;
    };
    if let Some(value) = s.value {
        return crate::atom::literal_bytes_to_str(value).map(str::to_string);
    }
    let start = s.span.start.offset as usize + 1;
    let end = s.span.end.offset as usize - 1;
    if start <= end && end <= content.len() {
        Some(content[start..end].to_string())
    } else {
        None
    }
}

fn string_args_from_arguments(args: &ArgumentList<'_>, content: &str) -> Vec<String> {
    args.arguments
        .iter()
        .filter_map(|arg| string_literal_value(arg.value(), content))
        .collect()
}

fn node_text<'a>(node: &impl HasSpan, content: &'a str) -> Option<&'a str> {
    let span = node.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    content.get(start..end)
}

fn apply_blueprint_statement(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    method: &str,
    args: &str,
    chain: &str,
) {
    let nullable = chain_contains_nullable(chain);
    match method {
        "dropColumn" | "removeColumn" => {
            let table = index.get_or_create_table(connection, table_name);
            for column in string_args(args) {
                table.drop_column(&column);
            }
        }
        "renameColumn" => {
            let strings = string_args(args);
            if strings.len() >= 2 {
                let table = index.get_or_create_table(connection, table_name);
                table.rename_column(&strings[0], &strings[1]);
            }
        }
        "rename" => {
            if let Some(new_name) = first_string_arg(args) {
                index.rename_table(connection, table_name, &new_name);
            }
        }
        "timestamps" | "timestampsTz" | "nullableTimestamps" | "nullableTimestampsTz" => {
            let table = index.get_or_create_table(connection, table_name);
            table.set_column(migration_column("created_at", "TIMESTAMP", true));
            table.set_column(migration_column("updated_at", "TIMESTAMP", true));
        }
        "dropTimestamps" | "dropTimestampsTz" => {
            let table = index.get_or_create_table(connection, table_name);
            table.drop_column("created_at");
            table.drop_column("updated_at");
        }
        "softDeletes" | "softDeletesTz" | "softDeletesDatetime" => {
            let column = first_string_arg(args).unwrap_or_else(|| "deleted_at".to_string());
            let table = index.get_or_create_table(connection, table_name);
            table.set_column(migration_column(&column, "TIMESTAMP", true));
        }
        "dropSoftDeletes" | "dropSoftDeletesTz" => {
            let column = first_string_arg(args).unwrap_or_else(|| "deleted_at".to_string());
            let table = index.get_or_create_table(connection, table_name);
            table.drop_column(&column);
        }
        "rememberToken" => {
            let table = index.get_or_create_table(connection, table_name);
            table.set_column(migration_column_with_chain(
                "remember_token",
                "VARCHAR",
                nullable,
                chain,
            ))
        }
        "dropRememberToken" => {
            let table = index.get_or_create_table(connection, table_name);
            table.drop_column("remember_token")
        }
        "morphs" | "nullableMorphs" => {
            if let Some(prefix) = first_string_arg(args) {
                let nullable = nullable || method == "nullableMorphs";
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column(
                    &format!("{prefix}_type"),
                    "VARCHAR",
                    nullable,
                ));
                table.set_column(migration_column(
                    &format!("{prefix}_id"),
                    "BIGINT",
                    nullable,
                ));
            }
        }
        "uuidMorphs" | "nullableUuidMorphs" => {
            if let Some(prefix) = first_string_arg(args) {
                let nullable = nullable || method == "nullableUuidMorphs";
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column(
                    &format!("{prefix}_type"),
                    "VARCHAR",
                    nullable,
                ));
                table.set_column(migration_column(&format!("{prefix}_id"), "UUID", nullable));
            }
        }
        "dropMorphs" => {
            if let Some(prefix) = first_string_arg(args) {
                let table = index.get_or_create_table(connection, table_name);
                table.drop_column(&format!("{prefix}_type"));
                table.drop_column(&format!("{prefix}_id"));
            }
        }
        "addColumn" => {
            let strings = string_args(args);
            if strings.len() >= 2 {
                // A type the column-method table does not know is still a
                // column: `addColumn()` accepts any grammar type name.
                let database_type = migration_database_type(&strings[0]).unwrap_or("TEXT");
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column_with_chain(
                    &strings[1],
                    database_type,
                    nullable,
                    chain,
                ));
            }
        }
        _ => {
            if let Some((column, database_type)) =
                migration_column_from_blueprint_method(method, args)
            {
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column_with_chain(
                    &column,
                    database_type,
                    nullable,
                    chain,
                ));
            }
        }
    }
}

fn migration_column_from_blueprint_method<'a>(
    method: &'a str,
    args: &str,
) -> Option<(String, &'a str)> {
    let database_type = migration_database_type(method)?;
    let column = first_string_arg(args).or_else(|| default_column_name(method))?;
    Some((column, database_type))
}

fn default_column_name(method: &str) -> Option<String> {
    match method {
        "id" => Some("id".to_string()),
        "uuid" => Some("uuid".to_string()),
        "ulid" => Some("ulid".to_string()),
        "ipAddress" => Some("ip_address".to_string()),
        "macAddress" => Some("mac_address".to_string()),
        _ => None,
    }
}

/// The database type a Blueprint column method defines, or `None` for a method
/// that defines no column.
///
/// Every Blueprint method that takes a column name is not a column definition:
/// `index('email')`, `foreign('author_id')`, and `comment('…')` name a column or
/// the table without adding one, so only the methods that reach
/// `Blueprint::addColumn()` are listed here.
fn migration_database_type(method: &str) -> Option<&'static str> {
    match method.to_ascii_lowercase().as_str() {
        "bigincrements" | "biginteger" | "foreignid" | "foreignidfor" | "id"
        | "unsignedbiginteger" => Some("BIGINT"),
        "increments" | "integer" | "integerincrements" | "unsignedinteger" => Some("INTEGER"),
        "mediumincrements" | "mediuminteger" | "unsignedmediuminteger" => Some("MEDIUMINT"),
        "smallincrements" | "smallinteger" | "unsignedsmallinteger" => Some("SMALLINT"),
        "tinyincrements" | "tinyinteger" | "unsignedtinyinteger" => Some("TINYINT"),
        "boolean" => Some("BOOLEAN"),
        "decimal" | "unsigneddecimal" => Some("DECIMAL"),
        "double" => Some("DOUBLE"),
        "float" => Some("FLOAT"),
        "uuid" | "foreignuuid" => Some("UUID"),
        "ulid" | "foreignulid" => Some("CHAR(26)"),
        "ipaddress" | "macaddress" => Some("VARCHAR"),
        "json" => Some("JSON"),
        "jsonb" => Some("JSONB"),
        "date" => Some("DATE"),
        "datetime" | "datetimetz" => Some("DATETIME"),
        "time" | "timetz" => Some("TIME"),
        "timestamp" | "timestamptz" | "timestampstz" | "softdeletes" | "softdeletestz" => {
            Some("TIMESTAMP")
        }
        "year" => Some("YEAR"),
        "binary" | "mediumbinary" | "largebinary" => Some("BLOB"),
        "computed" => Some("MIXED"),
        "enum" | "set" => Some("VARCHAR"),
        "morphs"
        | "nullablemorphs"
        | "uuidmorphs"
        | "nullableuuidmorphs"
        | "numericmorphs"
        | "nullablenumericmorphs"
        | "ulidmorphs"
        | "nullableulidmorphs" => Some("MIXED"),
        "string" | "char" | "tinytext" => Some("VARCHAR"),
        "text" | "mediumtext" | "longtext" => Some("TEXT"),
        "geometry" | "geography" | "tsvector" | "vector" | "rawcolumn" => Some("MIXED"),
        _ => None,
    }
}

fn migration_column(name: &str, database_type: &str, nullable: bool) -> SchemaColumn {
    migration_column_with_chain(name, database_type, nullable, "")
}

fn migration_column_with_chain(
    name: &str,
    database_type: &str,
    nullable: bool,
    chain: &str,
) -> SchemaColumn {
    let (generated_expression, generated_mode) = generated_column_from_chain(chain);
    SchemaColumn {
        name: name.to_string(),
        database_type: database_type.to_string(),
        nullable,
        default: None,
        generated_expression,
        generated_mode,
        php_type: database_type_to_php_type(database_type, nullable),
    }
}

fn generated_column_from_chain(chain: &str) -> (Option<String>, Option<String>) {
    if let Some(expression) = chain_string_arg_after_method(chain, "virtualAs") {
        return (Some(expression), Some("virtual".to_string()));
    }
    if let Some(expression) = chain_string_arg_after_method(chain, "storedAs") {
        return (Some(expression), Some("stored".to_string()));
    }
    if let Some(expression) = chain_string_arg_after_method(chain, "computed") {
        return (Some(expression), None);
    }
    (None, None)
}

fn chain_string_arg_after_method(chain: &str, method: &str) -> Option<String> {
    let needle = format!("->{method}");
    let start = find_ascii_case_insensitive(chain.as_bytes(), needle.as_bytes(), 0)?;
    let open = skip_php_whitespace(chain, start + needle.len());
    if chain.as_bytes().get(open) != Some(&b'(') {
        return None;
    }
    let close = find_matching_paren(chain, open)?;
    first_string_arg(&chain[open + 1..close])
}

fn chain_contains_nullable(chain: &str) -> bool {
    let Some(nullable) = find_ascii_case_insensitive(chain.as_bytes(), b"->nullable", 0) else {
        return false;
    };
    let after = skip_php_whitespace(chain, nullable + "->nullable".len());
    if chain.as_bytes().get(after) != Some(&b'(') {
        return true;
    }
    let Some(close) = find_matching_paren(chain, after) else {
        return true;
    };
    !chain[after + 1..close].trim().eq_ignore_ascii_case("false")
}

fn first_string_arg(args: &str) -> Option<String> {
    string_args(args).into_iter().next()
}

fn string_args(args: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while offset < args.len() {
        let rest = &args[offset..];
        let Some(relative_quote) = rest.find(['\'', '"']) else {
            break;
        };
        let quote = offset + relative_quote;
        if let Some((value, consumed)) = parse_php_string(&args[quote..]) {
            values.push(value);
            offset = quote + consumed;
        } else {
            offset = quote + 1;
        }
    }
    values
}

fn parse_php_string(input: &str) -> Option<(String, usize)> {
    let quote = input.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let mut value = String::new();
    let mut escaped = false;
    for (index, ch) in input[quote.len_utf8()..].char_indices() {
        let consumed = quote.len_utf8() + index + ch.len_utf8();
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some((value, consumed));
        }
        value.push(ch);
    }
    None
}

fn skip_php_whitespace(input: &str, mut offset: usize) -> usize {
    while let Some(ch) = input[offset..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        offset += ch.len_utf8();
        if offset >= input.len() {
            break;
        }
    }
    offset
}
