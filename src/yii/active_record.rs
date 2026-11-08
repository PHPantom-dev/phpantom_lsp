//! Yii Active Record (`yiisoft/active-record`): the attributes and
//! relations a record and its queries name as strings.
//!
//! ```php
//! $post = Post::query()->with('author')->one();
//! $post->get('title');
//! ```
//!
//! A record's `get()`, `set()`, and the rest of its attribute API take an
//! attribute, which is a property of the record, or, for a record that
//! declares none, a column of its table, as the project's migrations
//! create it (see [`crate::yii::migrations`]).  Its `relation()` and the rest of
//! its relation API take a relation, and so do a query's `with()` and
//! `joinWith()`, as a path through relations (`'author.profile'`).
//!
//! A relation is declared by an arm of the `match` a record's
//! `relationQuery()` returns (`'author' => $this->hasOne(User::class, …)`),
//! or, in a record that uses `MagicRelationsTrait`, by a
//! `get{Name}Query()` method.  The arms are read where the method is
//! parsed and kept on it as [`RecordRelation`]s, together with the record
//! class each one builds its query for, which is what the next segment of
//! a path is a relation of.  A getter says the same with its return type,
//! which [`relation_getter_return_type`] fills in from the body.
//!
//! An attribute name is a [`DocblockMemberRef::YiiAttribute`] access on the
//! receiver, so it navigates, renames, and is checked like the property it
//! names, or, when it names a column, navigates to the migration that
//! creates it.  A relation name is not a member of the record, so it is kept
//! beside the spans instead, as a [`YiiRelationSite`].
//!
//! [`DocblockMemberRef::YiiAttribute`]: crate::symbol_map::DocblockMemberRef::YiiAttribute
//! [`YiiRelationSite`]: crate::symbol_map::YiiRelationSite

use std::sync::Arc;

use mago_syntax::cst::class_like::method::{Method, MethodBody};
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{Atom, atom, bytes_to_str};
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::parser::DocblockCtx;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, RecordRelation};
use crate::util::short_name;
use crate::yii::migrations::{Column, Schema, Table, bare_table_name};

/// The interface every record implements.
pub(crate) const RECORD_INTERFACE: &str = "Yiisoft\\ActiveRecord\\ActiveRecordInterface";

/// The interface every query for records implements.
const QUERY_INTERFACE: &str = "Yiisoft\\ActiveRecord\\ActiveQueryInterface";

/// The trait that makes a record's `get{Name}Query()` methods relations.
const MAGIC_RELATIONS_TRAIT: &str = "Yiisoft\\ActiveRecord\\Trait\\MagicRelationsTrait";

/// The package's own namespace: its classes declare no attributes or
/// relations of a record.
const PACKAGE_NAMESPACE: &str = "Yiisoft\\ActiveRecord\\";

/// What a string a record or query method takes names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NamedArgument {
    /// An attribute of the record the method is called on.
    Attribute,
    /// A relation of the record the method is called on.
    RecordRelation,
    /// A path of relations from the record the query is for, in a string
    /// or in the keys and values of an array.
    QueryRelations,
}

/// What the argument at `index` of a call to `method` names, when the
/// method is one of a record's or a query's that take a name there.  The
/// receiver still has to be a record or a query for it to name anything.
pub(crate) fn named_argument(method: &str, index: usize) -> Option<NamedArgument> {
    const ATTRIBUTE_METHODS: &[&str] = &[
        "get",
        "set",
        "oldValue",
        "isPropertyChanged",
        "isPropertyChangedNonStrict",
    ];
    const RELATION_METHODS: &[&str] = &[
        "relation",
        "relationQuery",
        "isRelationPopulated",
        "resetRelation",
        "populateRelation",
        "link",
        "unlink",
        "unlinkAll",
    ];
    let is = |names: &[&str]| names.iter().any(|name| name.eq_ignore_ascii_case(method));
    if method.eq_ignore_ascii_case("with") {
        return Some(NamedArgument::QueryRelations);
    }
    if index != 0 {
        return None;
    }
    if is(ATTRIBUTE_METHODS) {
        Some(NamedArgument::Attribute)
    } else if is(RELATION_METHODS) {
        Some(NamedArgument::RecordRelation)
    } else if is(&["joinWith", "innerJoinWith"]) {
        Some(NamedArgument::QueryRelations)
    } else {
        None
    }
}

/// Whether `class` is a record.
pub(crate) fn is_record(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    crate::class_lookup::is_subtype_of(class, RECORD_INTERFACE, class_loader)
}

/// Whether `class` is a query for records.
pub(crate) fn is_query(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    crate::class_lookup::is_subtype_of(class, QUERY_INTERFACE, class_loader)
}

/// The record class a query is for: the argument a resolved
/// `ActiveQueryInterface<Post>` carries.
pub(crate) fn query_model(query: &PhpType) -> Option<Atom> {
    query.union_members().into_iter().find_map(|member| {
        let TypeKind::Generic(generic) = member.kind() else {
            return None;
        };
        match generic.args.first()?.kind() {
            TypeKind::Named(name) | TypeKind::StaticType(name) | TypeKind::ThisType(name) => {
                Some(*name)
            }
            _ => None,
        }
    })
}

// ─── Parse time ────────────────────────────────────────────────────────────

/// The relations `method` declares, when it is a `relationQuery()` that
/// returns a `match` on its parameter: one per string condition of the
/// match's arms.  `None` for any other method, and for a
/// `relationQuery()` whose names are not known: one that returns
/// something else, or whose `default` arm builds a query of its own
/// rather than handing the name to `parent::relationQuery()`.
pub(crate) fn declared_relations(
    method: &Method<'_>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Option<Box<[RecordRelation]>> {
    if !method.name.value.eq_ignore_ascii_case(b"relationQuery") {
        return None;
    }
    let param = method.parameter_list.parameters.first()?.variable.name;
    let MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    let mut returns = block
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Return(ret) => ret.value,
            _ => None,
        });
    let Some(Expression::Match(matched)) = returns.next() else {
        return None;
    };
    if !matches!(
        matched.expression,
        Expression::Variable(Variable::Direct(subject)) if subject.name == param
    ) {
        return None;
    }
    let mut relations = Vec::new();
    for arm in matched.arms.iter() {
        match arm {
            MatchArm::Expression(arm) => {
                let related = related_class(arm.expression, doc_ctx);
                for condition in arm.conditions.iter() {
                    let Expression::Literal(Literal::String(string)) = condition else {
                        continue;
                    };
                    let Some(name) = string.value.and_then(crate::atom::literal_bytes_to_str)
                    else {
                        continue;
                    };
                    relations.push(RecordRelation {
                        name: atom(name),
                        offset: string.span.start.offset + 1,
                        related,
                    });
                }
            }
            MatchArm::Default(arm) if !is_parent_relation_query(arm.expression) => return None,
            MatchArm::Default(_) => {}
        }
    }
    Some(relations.into_boxed_slice())
}

/// Whether `expr` is `parent::relationQuery(…)`.
fn is_parent_relation_query(expr: &Expression<'_>) -> bool {
    let Expression::Call(Call::StaticMethod(call)) = expr else {
        return false;
    };
    matches!(call.class, Expression::Parent(_))
        && matches!(
            &call.method,
            ClassLikeMemberSelector::Identifier(method)
                if method.value.eq_ignore_ascii_case(b"relationQuery")
        )
}

/// The record class a relation query is built for: the class given to the
/// `$this->hasOne()` or `$this->hasMany()` that `expr` starts its chain
/// with (`$this->hasMany(Tag::class, …)->viaTable(…)`), fully qualified.
fn related_class(mut expr: &Expression<'_>, doc_ctx: Option<&DocblockCtx<'_>>) -> Option<Atom> {
    loop {
        let Expression::Call(Call::Method(call)) = expr else {
            return None;
        };
        let ClassLikeMemberSelector::Identifier(method) = &call.method else {
            return None;
        };
        let builds = method.value.eq_ignore_ascii_case(b"hasOne")
            || method.value.eq_ignore_ascii_case(b"hasMany");
        if builds
            && matches!(call.object, Expression::Variable(Variable::Direct(v)) if v.name == b"$this")
        {
            let Some(Argument::Positional(first)) = call.argument_list.arguments.first() else {
                return None;
            };
            let identifier = crate::parser::class_fetch_identifier(first.value)?;
            let ctx = doc_ctx?;
            return Some(atom(&crate::names::resolve_to_fqn(
                bytes_to_str(identifier.value()),
                &ctx.use_map,
                &ctx.namespace,
            )));
        }
        expr = call.object;
    }
}

/// A relation getter's return type, filled in from its body when it is
/// declared as a bare query: `getAuthorQuery(): ActiveQueryInterface`
/// returning `$this->hasOne(User::class, …)` returns a query for `User`.
pub(crate) fn relation_getter_return_type(
    declared: Option<PhpType>,
    method: &Method<'_>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Option<PhpType> {
    let declared = declared?;
    let TypeKind::Named(name) = declared.kind() else {
        return Some(declared);
    };
    if !matches!(short_name(name), "ActiveQueryInterface" | "ActiveQuery")
        || relation_getter_name(bytes_to_str(method.name.value)).is_none()
    {
        return Some(declared);
    }
    let MethodBody::Concrete(block) = &method.body else {
        return Some(declared);
    };
    let related = block
        .statements
        .iter()
        .find_map(|statement| match statement {
            Statement::Return(ret) => related_class(ret.value?, doc_ctx),
            _ => None,
        });
    match related {
        Some(related) => Some(PhpType::generic_atom(
            *name,
            vec![PhpType::named(atom(&format!("\\{related}")))],
        )),
        None => Some(declared),
    }
}

/// The table `method` names, when it is a `tableName()` that returns a
/// string literal and nothing else.
pub(crate) fn declared_table(method: &Method<'_>) -> Option<Atom> {
    if !method.name.value.eq_ignore_ascii_case(b"tableName") {
        return None;
    }
    let MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    let mut statements = block.statements.iter();
    let (Some(Statement::Return(ret)), None) = (statements.next(), statements.next()) else {
        return None;
    };
    let Some(Expression::Literal(Literal::String(table))) = ret.value else {
        return None;
    };
    table
        .value
        .and_then(crate::atom::literal_bytes_to_str)
        .map(atom)
}

/// The relation a `get{Name}Query()` method declares under
/// `MagicRelationsTrait`: `getAuthorQuery` declares `author`.
pub(crate) fn relation_getter_name(method: &str) -> Option<String> {
    let name = method.strip_prefix("get")?.strip_suffix("Query")?;
    let mut chars = name.chars();
    let first = chars.next()?;
    Some(first.to_lowercase().chain(chars).collect())
}

// ─── Lookup ────────────────────────────────────────────────────────────────

/// Where a relation is declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RelationSite {
    /// An arm of `relationQuery()`'s `match`, by the offset of its name
    /// inside the quotes.
    Arm(u32),
    /// A `get{Name}Query()` method, by its name.
    Getter(Atom),
}

/// A relation of a record.
#[derive(Debug, Clone)]
pub(crate) struct Relation {
    pub(crate) name: Atom,
    /// The class that declares it, fully qualified.
    pub(crate) declaring_class: Atom,
    pub(crate) site: RelationSite,
    /// The record class its query is for, fully qualified, when known.
    pub(crate) related: Option<Atom>,
}

/// The relations `record` declares, its ancestors' included, or `None`
/// when they are not all known (see [`declared_relations`]).
pub(crate) fn record_relations(
    record: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Vec<Relation>> {
    let mut relations: Vec<Relation> = Vec::new();
    let mut magic = false;
    let mut getters: Vec<(Atom, Arc<crate::types::MethodInfo>)> = Vec::new();
    for class in own_declarations(record, class_loader) {
        let fqn = class.fqn();
        magic |= class
            .used_traits
            .iter()
            .any(|used| used.trim_start_matches('\\') == MAGIC_RELATIONS_TRAIT);
        for method in class.methods.iter() {
            if method.name.eq_ignore_ascii_case("relationQuery") {
                let arms = method.extras.as_ref()?.record_relations.as_ref()?;
                for arm in arms.iter() {
                    if !relations.iter().any(|known| known.name == arm.name) {
                        relations.push(Relation {
                            name: arm.name,
                            declaring_class: fqn,
                            site: RelationSite::Arm(arm.offset),
                            related: arm.related,
                        });
                    }
                }
            } else if !method.is_static && relation_getter_name(&method.name).is_some() {
                getters.push((fqn, Arc::clone(method)));
            }
        }
    }
    if magic {
        for (fqn, method) in getters {
            let Some(name) = relation_getter_name(&method.name) else {
                continue;
            };
            let name = atom(&name);
            if relations.iter().any(|known| known.name == name) {
                continue;
            }
            relations.push(Relation {
                name,
                declaring_class: fqn,
                site: RelationSite::Getter(method.name),
                related: method.return_type.as_ref().and_then(query_model),
            });
        }
    }
    Some(relations)
}

/// The record a relation path leads to from `record`: `author.profile`
/// from a `Post` is the `Profile` its `author` relation's record relates
/// to.  The empty path is `record` itself.
pub(crate) fn relation_path_owner(
    record: Arc<ClassInfo>,
    path: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Arc<ClassInfo>> {
    let mut owner = record;
    for segment in path.split('.').filter(|segment| !segment.is_empty()) {
        let relations = record_relations(&owner, class_loader)?;
        let related = relations
            .iter()
            .find(|relation| relation.name == segment)?
            .related?;
        owner = class_loader(Fqn::new(&related))?;
    }
    Some(owner)
}

/// The attributes `record` declares as properties, real or `@property`
/// ones, its ancestors' and traits' included, each with its type when it
/// has one.  Empty for a record that declares none, which reads its
/// attributes from its table's columns (see [`Backend::yii_record_columns`]).
pub(crate) fn attribute_names(
    record: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<(Atom, Option<PhpType>)> {
    let mut names: Vec<(Atom, Option<PhpType>)> = Vec::new();
    let mut push = |name: Atom, ty: Option<&PhpType>| {
        if !names.iter().any(|(known, _)| *known == name) {
            names.push((name, ty.cloned()));
        }
    };
    for owner in own_declarations(record, class_loader) {
        for property in owner.properties.iter().filter(|p| !p.is_static) {
            push(property.name, property.type_hint.as_ref());
        }
        if let Some(members) = &owner.doc_members {
            for (name, ty) in &members.properties {
                push(*name, ty.as_ref());
            }
        }
    }
    names
}

/// The table `record` reads and writes: the one its `tableName()` names,
/// or, when it declares none, the one `AbstractActiveRecord::tableName()`
/// derives from its short name (`OrderItem` is `order_item`).  `None` when
/// its `tableName()` says it some other way.
pub(crate) fn record_table(
    record: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<String> {
    for class in own_declarations(record, class_loader) {
        if let Some(method) = class
            .methods
            .iter()
            .find(|method| !method.is_static && method.name.eq_ignore_ascii_case("tableName"))
        {
            let table = method.extras.as_ref()?.record_table?;
            return Some(bare_table_name(&table).to_string());
        }
    }
    Some(derived_table_name(short_name(&record.fqn())))
}

/// The table `AbstractActiveRecord::tableName()` derives from a record's
/// short name: an underscore before each word, a run of capitals counting
/// as one, then all of it in lower case.
fn derived_table_name(short_name: &str) -> String {
    let chars: Vec<char> = short_name.chars().collect();
    let mut name = String::with_capacity(short_name.len() + 4);
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_uppercase() {
            name.push('_');
            name.push(chars[i]);
            i += 1;
            while i < chars.len()
                && chars[i].is_ascii_uppercase()
                && !chars.get(i + 1).is_some_and(char::is_ascii_lowercase)
            {
                name.push(chars[i]);
                i += 1;
            }
        } else {
            name.push(chars[i]);
            i += 1;
        }
    }
    name.trim_start_matches('_').to_ascii_lowercase()
}

/// The columns of the table a record that declares no properties reads
/// its attributes from.
pub(crate) struct RecordColumns {
    schema: Arc<Schema>,
    table: String,
}

impl RecordColumns {
    fn table(&self) -> Option<&Table> {
        self.schema.table(&self.table)
    }

    /// The table's name, without the prefix marker.
    pub(crate) fn table_name(&self) -> &str {
        &self.table
    }

    /// The columns, in the order the migrations add them.
    pub(crate) fn columns(&self) -> &[Column] {
        self.table().map(Table::columns).unwrap_or_default()
    }

    /// The column `name`.
    pub(crate) fn column(&self, name: &str) -> Option<&Column> {
        self.table()?.column(name)
    }

    /// Whether the columns are all known, so a name that is not one of them
    /// can be told to name nothing.
    pub(crate) fn all_known(&self) -> bool {
        self.table()
            .is_some_and(|table| self.schema.columns_known(table))
    }
}

impl Backend {
    /// The columns the project's migrations give the table of `record`,
    /// which is where a record whose [`attribute_names`] are empty reads
    /// its attributes from.  `None` when its table is not known, or no
    /// migration creates or changes it.
    pub(crate) fn yii_record_columns(
        &self,
        record: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<RecordColumns> {
        let table = record_table(record, class_loader)?;
        let schema = self.yii_schema();
        schema.table(&table)?;
        Some(RecordColumns { schema, table })
    }

    /// Where the migration that creates the column `name` names it, when
    /// `record` declares no properties and reads its attributes from the
    /// columns of its table.
    pub(crate) fn yii_column_location(
        &self,
        record: &ClassInfo,
        name: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<tower_lsp_server::gen_lsp_types::Location> {
        if !attribute_names(record, class_loader).is_empty() {
            return None;
        }
        self.yii_record_columns(record, class_loader)?
            .column(name)?
            .location()
    }

    /// The hover of the column `name`, when `record` declares no properties
    /// and reads its attributes from the columns of its table: its type, and
    /// the migration that names it.
    pub(crate) fn yii_column_detail(
        &self,
        record: &ClassInfo,
        name: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<String> {
        if !attribute_names(record, class_loader).is_empty() {
            return None;
        }
        let columns = self.yii_record_columns(record, class_loader)?;
        let column = columns.column(name)?;
        let root = self.workspace.workspace_root.read().clone();
        let file = root
            .as_ref()
            .and_then(|root| column.file.strip_prefix(root).ok())
            .unwrap_or(&column.file)
            .to_string_lossy()
            .replace('\\', "/");
        let mut detail = format!("Column `{name}` of table `{}`", columns.table_name());
        if let Some(kind) = &column.kind {
            detail.push_str(&format!(": `{kind}`"));
        }
        detail.push_str(&format!("\n\nDeclared in `{file}`"));
        Some(detail)
    }
}

/// What declares the members of `record` outside the package: the record
/// as parsed, its ancestors up to the package's own classes, and the traits
/// each of them uses, each class followed by its traits.
fn own_declarations(
    record: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    let mut declarations: Vec<Arc<ClassInfo>> = Vec::new();
    let mut next = Some(record.fqn());
    while let Some(fqn) = next.take() {
        if fqn.starts_with(PACKAGE_NAMESPACE) || declarations.iter().any(|class| class.fqn() == fqn)
        {
            break;
        }
        let Some(class) = class_loader(Fqn::new(&fqn)) else {
            break;
        };
        next = class
            .parent_class
            .map(|parent| atom(parent.trim_start_matches('\\')));
        let mut pending: Vec<Atom> = class.used_traits.clone();
        declarations.push(class);
        while let Some(used) = pending.pop() {
            let used = used.trim_start_matches('\\');
            if used.starts_with(PACKAGE_NAMESPACE)
                || declarations.iter().any(|known| known.fqn() == used)
            {
                continue;
            }
            if let Some(used) = class_loader(Fqn::new(used)) {
                pending.extend(used.used_traits.iter().copied());
                declarations.push(used);
            }
        }
    }
    declarations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn getter_names_the_relation_with_a_lowercase_first_letter() {
        assert_eq!(
            relation_getter_name("getAuthorQuery").as_deref(),
            Some("author")
        );
        assert_eq!(
            relation_getter_name("getOrderItemsQuery").as_deref(),
            Some("orderItems")
        );
        assert_eq!(relation_getter_name("getQuery"), None);
        assert_eq!(relation_getter_name("authorQuery"), None);
    }

    #[test]
    fn a_table_name_is_derived_from_the_short_name() {
        assert_eq!(derived_table_name("Post"), "post");
        assert_eq!(derived_table_name("OrderItem"), "order_item");
        assert_eq!(derived_table_name("HTMLParser"), "html_parser");
        assert_eq!(derived_table_name("UserID"), "user_id");
    }

    #[test]
    fn with_names_relations_in_every_argument() {
        assert_eq!(
            named_argument("with", 2),
            Some(NamedArgument::QueryRelations)
        );
        assert_eq!(named_argument("joinWith", 1), None);
        assert_eq!(named_argument("GET", 0), Some(NamedArgument::Attribute));
        assert_eq!(
            named_argument("relation", 0),
            Some(NamedArgument::RecordRelation)
        );
    }
}
