//! Yii migrations (`yiisoft/db-migration`): the tables the project's
//! migrations create and the columns they give them, which a record that
//! declares no properties reads its attributes from.
//!
//! ```php
//! public function up(MigrationBuilder $b): void
//! {
//!     $column = $b->columnBuilder();
//!     $b->createTable('review', [
//!         'id' => $column::primaryKey(),
//!         'rating' => $column::integer()->notNull(),
//!     ]);
//! }
//! ```
//!
//! The migrations are the files of the directories the
//! `yiisoft/db-migration` params name, found the way the migrator finds
//! them: `newMigrationPath` and `sourcePaths` are directories, through the
//! path aliases, and `newMigrationNamespace` and `sourceNamespaces` are the
//! directories the project's PSR-4 mappings put those namespaces in.  The
//! calls in their `up()` methods are applied in the order of the files'
//! names, which start with the time each was created, so a column a later
//! migration renames or drops is renamed or dropped.
//!
//! A table is named by a string literal, or by a class constant that holds
//! one (`$b->createTable(Table::POST, …)`), which is read from the class
//! wherever it is declared.  A table a call names any other way could be
//! any table, so after one the columns of every table may be incomplete, and
//! a table whose columns a call gives that way may have more.  Neither is
//! read as wrong: their columns still complete, and no name is reported.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::text_position::LineIndex;

/// The params key the migrator's configuration is under.
const PARAMS_KEY: &str = "yiisoft/db-migration";

/// A column a migration creates.
#[derive(Debug, Clone)]
pub(crate) struct Column {
    pub(crate) name: String,
    /// The migration that gives the column its name.
    pub(crate) file: Arc<Path>,
    /// The name where that migration writes it, inside the quotes.
    pub(crate) range: Range,
    /// The type the migration gives it: the column builder method
    /// (`integer`), or the first word of a type written as a string.
    pub(crate) kind: Option<String>,
}

impl Column {
    /// Where the migration gives the column its name.
    pub(crate) fn location(&self) -> Option<Location> {
        Some(Location {
            uri: crate::util::path_to_url(&self.file)?,
            range: self.range,
        })
    }

    /// The hover of the column, a column of `table`: its type, and the
    /// migration that names it, relative to `root`.
    pub(crate) fn detail(&self, table: &str, root: Option<&Path>) -> String {
        let mut detail = format!("Column `{}` of table `{table}`", self.name);
        if let Some(kind) = &self.kind {
            detail.push_str(&format!(": `{kind}`"));
        }
        detail.push_str(&format!(
            "\n\nDeclared in `{}`",
            display_path(&self.file, root)
        ));
        detail
    }
}

/// `file` relative to `root`, with `/` between its parts.
pub(crate) fn display_path(file: &Path, root: Option<&Path>) -> String {
    root.and_then(|root| file.strip_prefix(root).ok())
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// A table the migrations create or change.
#[derive(Debug, Default)]
pub(crate) struct Table {
    columns: Vec<Column>,
    /// The migration that gives the table its name, and where it writes the
    /// name: inside the quotes of a string, or the class constant that
    /// holds it.
    named_at: Option<(Arc<Path>, Range)>,
    /// Whether a migration creates it.  A table the migrations only change
    /// was created some other way, with columns they do not say.
    created: bool,
    /// Whether a migration gives it a column whose name is not a string
    /// literal.
    open: bool,
}

impl Table {
    /// The columns, in the order the migrations add them.
    pub(crate) fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The column `name`.
    pub(crate) fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// Where the migration that creates the table names it.
    pub(crate) fn location(&self) -> Option<Location> {
        let (file, range) = self.named_at.as_ref()?;
        Some(Location {
            uri: crate::util::path_to_url(file)?,
            range: *range,
        })
    }

    /// The migration that creates the table.
    pub(crate) fn file(&self) -> Option<&Path> {
        self.named_at.as_ref().map(|(file, _)| &**file)
    }

    fn remove(&mut self, name: &str) -> Option<Column> {
        let at = self.columns.iter().position(|column| column.name == name)?;
        Some(self.columns.remove(at))
    }
}

/// The tables the project's migrations create.
#[derive(Debug, Default)]
pub(crate) struct Schema {
    /// The directories the migrations were read from.
    dirs: Vec<PathBuf>,
    /// By name, without the `{{%…}}` that marks the table prefix.
    tables: HashMap<String, Table>,
    /// Whether a migration names a table with anything but a string
    /// literal or a class constant that holds one, so that any table may
    /// have columns not listed here.
    open: bool,
    /// The files of the classes whose constants name tables.
    constant_files: Vec<PathBuf>,
}

/// The string a class constant holds, and the file of the class that
/// declares it, for a migration that names a table with one.
pub(crate) struct ConstantValue {
    pub(crate) value: String,
    pub(crate) file: Option<PathBuf>,
}

/// Look up the string the constant `name` of the class `class` (fully
/// qualified) holds.
pub(crate) type ConstantLookup<'a> = dyn Fn(&str, &str) -> Option<ConstantValue> + 'a;

impl Schema {
    /// The table `name` (with or without the `{{%…}}` around it).
    pub(crate) fn table(&self, name: &str) -> Option<&Table> {
        self.tables.get(bare_table_name(name))
    }

    /// The table `name` (with or without the `{{%…}}` around it), with its
    /// name as the schema holds it.
    pub(crate) fn table_entry(&self, name: &str) -> Option<(&str, &Table)> {
        self.tables
            .get_key_value(bare_table_name(name))
            .map(|(name, table)| (name.as_str(), table))
    }

    /// The tables, by name.
    pub(crate) fn tables(&self) -> impl Iterator<Item = (&str, &Table)> {
        self.tables
            .iter()
            .map(|(name, table)| (name.as_str(), table))
    }

    /// Whether the columns of `table` are all known, so a name that is not
    /// one of them can be told to name nothing.
    pub(crate) fn columns_known(&self, table: &Table) -> bool {
        !self.open && !table.open && table.created
    }

    /// Whether the tables are all known, so a name that is not one of them
    /// can be told to name nothing: there are migrations, and each names
    /// its tables in a way that can be read.
    pub(crate) fn tables_known(&self) -> bool {
        !self.open && !self.tables.is_empty()
    }

    /// Whether the file at `path` is one of the migrations, or declares a
    /// class whose constants name their tables.
    fn reads(&self, path: &Path) -> bool {
        path.parent()
            .is_some_and(|dir| self.dirs.iter().any(|known| known == dir))
            || self.constant_files.iter().any(|file| file == path)
    }

    /// Read the migrations in `dirs`, with `constants` for the class
    /// constants they name tables with.
    pub(crate) fn load(dirs: Vec<PathBuf>, constants: &ConstantLookup<'_>) -> Self {
        let mut files: Vec<PathBuf> = Vec::new();
        for dir in &dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "php")
                    && path.is_file()
                    && !files.contains(&path)
                {
                    files.push(path);
                }
            }
        }
        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()).then_with(|| a.cmp(b)));
        let mut schema = Self {
            dirs,
            ..Self::default()
        };
        for file in files {
            if let Ok(content) = std::fs::read_to_string(&file) {
                schema.apply_file(Arc::from(file.as_path()), &content, constants);
            }
        }
        schema
    }

    /// Apply the `up()` methods of the migration `content` holds.
    pub(crate) fn apply_file(
        &mut self,
        file: Arc<Path>,
        content: &str,
        constants: &ConstantLookup<'_>,
    ) {
        crate::parser::with_parsed_program(content, "yii_migration", |program, content| {
            let mut uses = HashMap::new();
            Backend::extract_use_statements_from_statements(
                program.statements.iter(),
                &mut uses,
                None,
            );
            let mut migration = Migration {
                schema: self,
                file,
                lines: LineIndex::new(content),
                namespace: Backend::extract_namespace_from_statements(program.statements.iter()),
                uses,
                class: None,
                constants,
            };
            for statement in program.statements.iter() {
                migration.statement_at_top(statement);
            }
        });
    }
}

/// A migration being applied.
struct Migration<'a, 'c> {
    schema: &'a mut Schema,
    file: Arc<Path>,
    lines: LineIndex<'c>,
    /// The file's namespace and imports, which the class of a constant
    /// naming a table is written against.
    namespace: Option<String>,
    uses: HashMap<String, String>,
    /// The migration class, fully qualified, which `self::` names.
    class: Option<String>,
    constants: &'a ConstantLookup<'a>,
}

impl Migration<'_, '_> {
    fn statement_at_top(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::Namespace(namespace) => {
                for statement in namespace.statements().iter() {
                    self.statement_at_top(statement);
                }
            }
            Statement::Class(class) => {
                self.class = Some(crate::names::resolve_to_fqn(
                    bytes_to_str(class.name.value),
                    &HashMap::new(),
                    &self.namespace,
                ));
                for member in class.members.iter() {
                    let ClassLikeMember::Method(method) = member else {
                        continue;
                    };
                    if !method.name.value.eq_ignore_ascii_case(b"up") {
                        continue;
                    }
                    if let MethodBody::Concrete(body) = &method.body {
                        self.statements(body.statements.iter());
                    }
                }
            }
            _ => {}
        }
    }

    fn statements<'s>(&mut self, statements: impl Iterator<Item = &'s Statement<'s>>) {
        for statement in statements {
            match statement {
                Statement::Expression(expression) => self.expression(expression.expression),
                Statement::Block(block) => self.statements(block.statements.iter()),
                Statement::If(if_statement) => {
                    crate::parser::for_each_if_branch(if_statement, |branch| {
                        self.statements(branch.iter());
                    });
                }
                _ => {}
            }
        }
    }

    /// Apply the builder call `expression` makes, if it is one that
    /// changes a table.
    fn expression(&mut self, expression: &Expression<'_>) {
        let Expression::Call(Call::Method(call)) = expression else {
            return;
        };
        let ClassLikeMemberSelector::Identifier(method) = &call.method else {
            return;
        };
        let method = bytes_to_str(method.value);
        let args = &call.argument_list.arguments;
        let is = |name: &str| method.eq_ignore_ascii_case(name);
        const CHANGES: &[&str] = &[
            "createTable",
            "dropTable",
            "renameTable",
            "addColumn",
            "dropColumn",
            "renameColumn",
            "alterColumn",
        ];
        if !CHANGES.iter().any(|name| is(name)) {
            return;
        }
        let Some((table, named_at)) = argument(args, 0, "table").and_then(|t| self.table_name(t))
        else {
            self.schema.open = true;
            return;
        };
        if is("createTable") {
            let mut created = Table {
                created: true,
                named_at: Some((Arc::clone(&self.file), named_at)),
                ..Table::default()
            };
            self.create_columns(&mut created, argument(args, 1, "columns"));
            self.schema.tables.insert(table, created);
        } else if is("dropTable") {
            self.schema.tables.remove(&table);
        } else if is("renameTable") {
            let Some((new_name, named_at)) =
                argument(args, 1, "newName").and_then(|t| self.table_name(t))
            else {
                self.schema.open = true;
                return;
            };
            if let Some(mut renamed) = self.schema.tables.remove(&table) {
                renamed.named_at = Some((Arc::clone(&self.file), named_at));
                self.schema.tables.insert(new_name, renamed);
            }
        } else {
            let lines = &self.lines;
            let file = &self.file;
            let target = self.schema.tables.entry(table).or_insert_with(|| Table {
                open: true,
                ..Table::default()
            });
            let name = argument(args, 1, "column").or_else(|| argument(args, 1, "name"));
            let Some((name, offset)) = name.and_then(string_literal_at) else {
                target.open = true;
                return;
            };
            if is("addColumn") {
                target.remove(name);
                target.columns.push(Column {
                    name: name.to_string(),
                    file: Arc::clone(file),
                    range: range(lines, offset, name),
                    kind: argument(args, 2, "type").and_then(column_kind),
                });
            } else if is("dropColumn") {
                target.remove(name);
            } else if is("renameColumn") {
                let Some((new_name, offset)) =
                    argument(args, 2, "newName").and_then(string_literal_at)
                else {
                    target.open = true;
                    return;
                };
                if let Some(mut column) = target.remove(name) {
                    column.name = new_name.to_string();
                    column.file = Arc::clone(file);
                    column.range = range(lines, offset, new_name);
                    target.columns.push(column);
                }
            } else if let Some(column) = target.columns.iter_mut().find(|c| c.name == name) {
                // alterColumn
                column.kind = argument(args, 2, "type").and_then(column_kind);
            }
        }
    }

    /// The table `expression` names, without the prefix marker, and where
    /// it is written: a string literal, or a class constant that holds one.
    fn table_name(&mut self, expression: &Expression<'_>) -> Option<(String, Range)> {
        if let Some((name, offset)) = string_literal_at(expression) {
            return Some((
                bare_table_name(name).to_string(),
                range(&self.lines, offset, name),
            ));
        }
        let Expression::Access(Access::ClassConstant(access)) = expression else {
            return None;
        };
        let class = match access.class {
            Expression::Identifier(class) => crate::names::resolve_to_fqn(
                bytes_to_str(class.value()),
                &self.uses,
                &self.namespace,
            ),
            Expression::Self_(_) | Expression::Static(_) => self.class.clone()?,
            _ => return None,
        };
        let ClassLikeConstantSelector::Identifier(constant) = &access.constant else {
            return None;
        };
        let found = (self.constants)(&class, bytes_to_str(constant.value))?;
        if let Some(file) = found.file
            && !self.schema.constant_files.contains(&file)
        {
            self.schema.constant_files.push(file);
        }
        let span = expression.span();
        let range = Range::new(
            self.lines.position(span.start.offset as usize),
            self.lines.position(span.end.offset as usize),
        );
        Some((bare_table_name(&found.value).to_string(), range))
    }

    /// Give `table` the columns `columns` lists: one per string key, whose
    /// value is the column's type.  A value without a key is a constraint.
    fn create_columns(&self, table: &mut Table, columns: Option<&Expression<'_>>) {
        let elements = match columns {
            Some(Expression::Array(array)) => &array.elements,
            Some(Expression::LegacyArray(array)) => &array.elements,
            _ => {
                table.open = true;
                return;
            }
        };
        for element in elements.iter() {
            match element {
                ArrayElement::KeyValue(entry) => {
                    let Some((name, offset)) = string_literal_at(entry.key) else {
                        table.open = true;
                        continue;
                    };
                    table.remove(name);
                    table.columns.push(Column {
                        name: name.to_string(),
                        file: Arc::clone(&self.file),
                        range: range(&self.lines, offset, name),
                        kind: column_kind(entry.value),
                    });
                }
                ArrayElement::Value(_) => {}
                _ => table.open = true,
            }
        }
    }
}

/// The argument at `index`, or the one named `name`.
fn argument<'a>(
    args: &'a TokenSeparatedSequence<'a, Argument<'a>>,
    index: usize,
    name: &str,
) -> Option<&'a Expression<'a>> {
    args.iter()
        .find(|arg| matches!(arg, Argument::Named(named) if named.name.value == name.as_bytes()))
        .or_else(|| match args.iter().nth(index) {
            Some(arg @ Argument::Positional(_)) => Some(arg),
            _ => None,
        })
        .map(|arg| arg.value())
}

/// A string literal's text and the offset it starts at, inside the quotes.
fn string_literal_at<'a>(expression: &'a Expression<'a>) -> Option<(&'a str, u32)> {
    let Expression::Literal(Literal::String(literal)) = expression else {
        return None;
    };
    let text = literal.value.and_then(literal_bytes_to_str)?;
    Some((text, literal.span.start.offset + 1))
}

fn range(lines: &LineIndex<'_>, offset: u32, name: &str) -> Range {
    let start = offset as usize;
    Range::new(lines.position(start), lines.position(start + name.len()))
}

/// The type a column definition gives a column: the first column builder
/// method of the chain (`$column::integer()->notNull()` is `integer`), or
/// the first word of a type written as a string (`'integer NOT NULL'`).
fn column_kind(definition: &Expression<'_>) -> Option<String> {
    let mut definition = definition;
    loop {
        match definition {
            Expression::Literal(Literal::String(literal)) => {
                let text = literal.value.and_then(literal_bytes_to_str)?;
                return text.split_whitespace().next().map(str::to_string);
            }
            Expression::Call(Call::StaticMethod(call)) => {
                let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                    return None;
                };
                return Some(bytes_to_str(method.value).to_string());
            }
            Expression::Call(Call::Method(call)) => {
                if matches!(call.object, Expression::Call(_)) {
                    definition = call.object;
                    continue;
                }
                let ClassLikeMemberSelector::Identifier(method) = &call.method else {
                    return None;
                };
                return Some(bytes_to_str(method.value).to_string());
            }
            _ => return None,
        }
    }
}

/// A table's name without the `{{%…}}` (or `{{…}}`) that marks where the
/// connection's table prefix goes.
pub(crate) fn bare_table_name(name: &str) -> &str {
    let name = name
        .strip_prefix("{{")
        .and_then(|inner| inner.strip_suffix("}}"))
        .unwrap_or(name);
    name.strip_prefix('%').unwrap_or(name)
}

impl Backend {
    /// The tables the project's migrations create, read on first use.
    pub(crate) fn yii_schema(&self) -> Arc<Schema> {
        let mut known = self.yii.schema.lock();
        if let Some(schema) = known.as_ref() {
            return Arc::clone(schema);
        }
        let constants = |class: &str, name: &str| self.yii_constant_string(class, name);
        let schema = Arc::new(Schema::load(self.yii_migration_dirs(), &constants));
        *known = Some(Arc::clone(&schema));
        schema
    }

    /// The string the constant `name` of the class `class` (fully
    /// qualified) holds, when it is declared as a plain string literal,
    /// with the file of the class.
    pub(crate) fn yii_constant_string(&self, class: &str, name: &str) -> Option<ConstantValue> {
        let declaring = self.find_or_load_class(class)?;
        let constant = declaring.constants.iter().find(|c| c.name == name)?;
        let value = crate::text_scan::unquote_php_string(constant.value.as_deref()?)?;
        if value.contains(['\\', '$']) {
            return None;
        }
        let file = self
            .find_class_file_uri(&declaring.fqn(), "")
            .and_then(|uri| crate::util::uri_to_path(&uri));
        Some(ConstantValue {
            value: value.to_string(),
            file,
        })
    }

    /// The directories the migrator reads migrations from.
    fn yii_migration_dirs(&self) -> Vec<PathBuf> {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return Vec::new();
        };
        let params = self.yii_params();
        let Some(config) = params.get(PARAMS_KEY) else {
            return Vec::new();
        };
        let named = |key: &str| {
            config
                .child(key)
                .map(|node| node.named_strings().to_vec())
                .unwrap_or_default()
        };
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut push = |dir: PathBuf| {
            let dir = crate::document_links::normalize_path(&dir);
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        };
        let aliases = self.yii_aliases();
        for path in named("newMigrationPath")
            .into_iter()
            .chain(named("sourcePaths"))
        {
            let path = if path.starts_with('@') {
                match aliases.resolve(&path) {
                    Some(path) => path,
                    None => continue,
                }
            } else {
                path
            };
            if !path.is_empty() {
                push(root.join(path));
            }
        }
        let mappings = self.workspace.psr4_mappings.read();
        for namespace in named("newMigrationNamespace")
            .into_iter()
            .chain(named("sourceNamespaces"))
        {
            let namespace = namespace.trim_matches('\\');
            if namespace.is_empty() {
                continue;
            }
            for (_, dir) in
                crate::composer::psr4_directories_for_namespace(&mappings, &root, namespace)
            {
                if dir.is_dir() {
                    push(dir);
                }
            }
        }
        dirs
    }

    /// Forget the tables, once the configuration, the dependencies, or a
    /// migration changed.
    pub(crate) fn forget_yii_schema(&self) {
        *self.yii.schema.lock() = None;
    }

    /// Forget the tables when the file at `path` is one of the migrations,
    /// or declares a class whose constants name their tables.
    pub(crate) fn note_yii_migration_change(&self, path: &Path) {
        let mut known = self.yii.schema.lock();
        if known.as_ref().is_some_and(|schema| schema.reads(path)) {
            *known = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(migrations: &[&str]) -> Schema {
        let mut schema = Schema::default();
        let constants = |class: &str, name: &str| {
            (class == "App\\Db\\Table" && name == "REVIEW").then(|| ConstantValue {
                value: "{{%review}}".to_string(),
                file: None,
            })
        };
        for content in migrations {
            schema.apply_file(Arc::from(Path::new("/m.php")), content, &constants);
        }
        schema
    }

    fn names(schema: &Schema, table: &str) -> Vec<(String, Option<String>)> {
        schema
            .table(table)
            .map(|table| {
                table
                    .columns()
                    .iter()
                    .map(|c| (c.name.clone(), c.kind.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    const CREATE: &str = r#"<?php
namespace App\Migration;
final class M1 implements RevertibleMigrationInterface
{
    public function up(MigrationBuilder $b): void
    {
        $column = $b->columnBuilder();
        $b->createTable('{{%review}}', [
            'id' => $column::primaryKey(),
            'rating' => $column::integer()->notNull(),
            'body' => 'text NOT NULL',
            'PRIMARY KEY (id)',
        ]);
    }
    public function down(MigrationBuilder $b): void
    {
        $b->dropTable('review');
    }
}
"#;

    #[test]
    fn create_table_lists_its_columns() {
        let schema = schema(&[CREATE]);
        assert_eq!(
            names(&schema, "review"),
            vec![
                ("id".to_string(), Some("primaryKey".to_string())),
                ("rating".to_string(), Some("integer".to_string())),
                ("body".to_string(), Some("text".to_string())),
            ]
        );
        let table = schema.table("{{%review}}").expect("table");
        assert!(schema.columns_known(table));
        assert_eq!(table.column("rating").map(|c| c.range.start.line), Some(9));
    }

    #[test]
    fn later_migrations_change_the_columns() {
        let alter = r#"<?php
final class M2
{
    public function up(MigrationBuilder $b): void
    {
        $b->addColumn('review', 'author', $b->columnBuilder()::string());
        $b->dropColumn('review', 'body');
        $b->renameColumn('{{%review}}', 'rating', 'stars');
    }
}
"#;
        let schema = schema(&[CREATE, alter]);
        assert_eq!(
            names(&schema, "review"),
            vec![
                ("id".to_string(), Some("primaryKey".to_string())),
                ("author".to_string(), Some("string".to_string())),
                ("stars".to_string(), Some("integer".to_string())),
            ]
        );
        assert_eq!(
            schema
                .table("review")
                .and_then(|t| t.column("stars"))
                .map(|c| c.range.start.line),
            Some(7)
        );
    }

    #[test]
    fn a_table_named_another_way_leaves_the_columns_open() {
        let dynamic = r#"<?php
final class M2
{
    public function up(MigrationBuilder $b): void
    {
        $b->addColumn(Table::REVIEW, 'author', 'string');
    }
}
"#;
        let schema = schema(&[CREATE, dynamic]);
        let table = schema.table("review").expect("table");
        assert!(!schema.columns_known(table));
    }

    #[test]
    fn a_table_named_by_a_class_constant_is_read() {
        let by_constant = r#"<?php
namespace App\Migration;
use App\Db\Table;
final class M1
{
    public function up(MigrationBuilder $b): void
    {
        $b->createTable(Table::REVIEW, ['id' => 'pk']);
        $b->addColumn(Table::REVIEW, 'rating', 'integer');
    }
}
"#;
        let schema = schema(&[by_constant]);
        let table = schema.table("review").expect("table");
        assert!(schema.columns_known(table));
        assert!(schema.tables_known());
        assert_eq!(names(&schema, "review").len(), 2);
        assert_eq!(table.named_at.as_ref().map(|(_, r)| r.start.line), Some(7));
    }

    #[test]
    fn a_table_only_altered_is_not_known_whole() {
        let alter = r#"<?php
final class M2
{
    public function up(MigrationBuilder $b): void
    {
        $b->addColumn('legacy', 'note', 'text');
    }
}
"#;
        let schema = schema(&[alter]);
        let table = schema.table("legacy").expect("table");
        assert_eq!(names(&schema, "legacy").len(), 1);
        assert!(!schema.columns_known(table));
    }

    #[test]
    fn dropped_and_renamed_tables_follow() {
        let rename = r#"<?php
final class M2
{
    public function up(MigrationBuilder $b): void
    {
        $b->renameTable('review', 'book_review');
    }
}
"#;
        let schema = schema(&[CREATE, rename]);
        assert!(schema.table("review").is_none());
        assert_eq!(names(&schema, "book_review").len(), 3);
    }
}
