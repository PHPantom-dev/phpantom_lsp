//! The tables and columns a `yiisoft/db` query chain names (see
//! [`crate::yii::queries`]).
//!
//! `$query->from('post p')->where(['p.status' => 1])` is read whole, from
//! its outermost call: the tables `from()` and the joins name, with their
//! aliases, become a [`YiiQuerySite`], and each table and column a string
//! names a [`YiiQueryName`] on it.
//!
//! The extractor cannot see types, and `where()` and `select()` are common
//! names, so it reads every such chain in a Yii project; the consumers
//! resolve the receiver and keep only a query.

use crate::atom::{Atom, atom};
use crate::symbol_map::{
    YiiQueryName, YiiQueryNameKind, YiiQuerySite, YiiQueryTable, YiiTableName,
};
use crate::yii::queries::{
    QueryArgument, column_list, column_text, joins_unnamed_tables, qualified_columns,
    query_argument, table_text, takes_names,
};

use super::yii_active_record::literal_range;
use super::*;

/// Read the query chain whose outermost call is `chain`.
pub(in crate::symbol_map::extraction) fn record_yii_query_chain<'a>(
    chain: &'a Expression<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    if !ctx.yii {
        return;
    }
    let mut links: Vec<(&str, &ArgumentList<'_>, &Expression<'_>)> = Vec::new();
    let mut object = chain;
    loop {
        let call = match object {
            Expression::Call(Call::Method(call)) => call,
            Expression::Call(Call::NullSafeMethod(call)) => {
                if let ClassLikeMemberSelector::Identifier(name) = &call.method {
                    links.push((bytes_to_str(name.value), &call.argument_list, call.object));
                }
                object = call.object;
                continue;
            }
            _ => break,
        };
        if let ClassLikeMemberSelector::Identifier(name) = &call.method {
            links.push((bytes_to_str(name.value), &call.argument_list, call.object));
        }
        object = call.object;
    }
    // Gathered outermost first.
    links.reverse();
    let Some(first) = links.iter().position(|(method, _, _)| takes_names(method)) else {
        return;
    };
    let span = chain.span();
    let query = ctx.yii_queries.len() as u32;
    let mut reader = QueryReader {
        site: YiiQuerySite {
            start: span.start.offset,
            end: span.end.offset,
            subject: ctx.spine_texts.subject_text_of(links[first].2, ctx.content),
            tables: Vec::new(),
            sets_from: false,
            alias: None,
            open: false,
        },
        query,
        names: Vec::new(),
    };
    reader.site.open = links
        .iter()
        .any(|(method, _, _)| joins_unnamed_tables(method));
    for (method, arguments, _) in &links[first..] {
        for (index, argument) in arguments.arguments.iter().enumerate() {
            let Argument::Positional(argument) = argument else {
                break;
            };
            if argument.ellipsis.is_some() {
                break;
            }
            if let Some(kind) = query_argument(method, index) {
                reader.argument(kind, argument.value, ctx);
            }
        }
    }
    ctx.yii_queries.push(reader.site);
    ctx.yii_query_names.extend(reader.names);
}

/// A query chain being read.
struct QueryReader {
    site: YiiQuerySite,
    query: u32,
    names: Vec<YiiQueryName>,
}

impl QueryReader {
    fn argument(&mut self, kind: QueryArgument, value: &Expression<'_>, ctx: &ExtractionCtx<'_>) {
        match kind {
            QueryArgument::From => {
                self.site.sets_from = true;
                self.tables(value, ctx);
            }
            QueryArgument::JoinTable => self.tables(value, ctx),
            QueryArgument::JoinOn => self.condition(value, true, ctx),
            QueryArgument::Condition => self.condition(value, false, ctx),
            QueryArgument::Order => match crate::parser::array_literal_elements(value) {
                Some(elements) => {
                    for element in elements.iter() {
                        match element {
                            ArrayElement::KeyValue(entry) => self.column(entry.key, ctx),
                            ArrayElement::Value(entry) => self.list(entry.value, ctx),
                            _ => {}
                        }
                    }
                }
                None => self.list(value, ctx),
            },
            QueryArgument::Columns => match crate::parser::array_literal_elements(value) {
                Some(elements) => {
                    for element in elements.iter() {
                        match element {
                            ArrayElement::KeyValue(entry) => self.list(entry.value, ctx),
                            ArrayElement::Value(entry) => self.list(entry.value, ctx),
                            _ => {}
                        }
                    }
                }
                None => self.list(value, ctx),
            },
            QueryArgument::Column => self.column(value, ctx),
            QueryArgument::Alias => {
                if let Some((start, end)) = literal_range(value, ctx.content) {
                    let alias = &ctx.content[start as usize..end as usize];
                    if crate::yii::queries::is_name(alias) {
                        self.site.alias = Some(atom(alias));
                    }
                }
            }
        }
    }

    /// The tables `value` names: a string, an array of them keyed by alias,
    /// or a class constant with the alias appended.
    fn tables(&mut self, value: &Expression<'_>, ctx: &ExtractionCtx<'_>) {
        if let Some(elements) = crate::parser::array_literal_elements(value) {
            for element in elements.iter() {
                match element {
                    ArrayElement::KeyValue(entry) => {
                        let alias = literal_range(entry.key, ctx.content)
                            .map(|(start, end)| &ctx.content[start as usize..end as usize])
                            .filter(|alias| crate::yii::queries::is_name(alias));
                        match alias {
                            Some(alias) => self.table(entry.value, Some(atom(alias)), ctx),
                            None => self.site.open = true,
                        }
                    }
                    ArrayElement::Value(entry) => self.table(entry.value, None, ctx),
                    _ => self.site.open = true,
                }
            }
        } else {
            self.table(value, None, ctx);
        }
    }

    /// The table `value` names, aliased `alias` by the key it is under.
    fn table(&mut self, value: &Expression<'_>, alias: Option<Atom>, ctx: &ExtractionCtx<'_>) {
        if let Some((start, end)) = literal_range(value, ctx.content) {
            let text = &ctx.content[start as usize..end as usize];
            let Some((at, name, written_alias)) = table_text(text) else {
                self.site.open = true;
                return;
            };
            let name_start = start + at as u32;
            self.names.push(YiiQueryName {
                start: name_start,
                end: name_start + name.len() as u32,
                query: self.query,
                kind: YiiQueryNameKind::Table,
            });
            self.site.tables.push(YiiQueryTable {
                name: YiiTableName::Literal(atom(name)),
                alias: alias.or_else(|| written_alias.map(atom)),
            });
            return;
        }
        let (constant, written_alias) = match value {
            Expression::Binary(binary)
                if matches!(binary.operator, BinaryOperator::StringConcat(_)) =>
            {
                let Some((start, end)) = literal_range(binary.rhs, ctx.content) else {
                    self.site.open = true;
                    return;
                };
                let suffix = &ctx.content[start as usize..end as usize];
                // The suffix is the alias: `' p'`, or `' AS p'`.
                let mut words = suffix.split_whitespace();
                let written = match (words.next(), words.next(), words.next()) {
                    (Some(alias), None, _) => Some(alias),
                    (Some(r#as), Some(alias), None) if r#as.eq_ignore_ascii_case("as") => {
                        Some(alias)
                    }
                    _ => None,
                };
                match written {
                    Some(alias)
                        if suffix.starts_with(char::is_whitespace)
                            && crate::yii::queries::is_name(alias) =>
                    {
                        (binary.lhs, Some(atom(alias)))
                    }
                    _ => {
                        self.site.open = true;
                        return;
                    }
                }
            }
            _ => (value, None),
        };
        let Some(name) = constant_table(constant, ctx) else {
            self.site.open = true;
            return;
        };
        self.site.tables.push(YiiQueryTable {
            name,
            alias: alias.or(written_alias),
        });
    }

    /// The columns a condition names: the keys of a hash, or the column an
    /// operator compares, through `and`, `or`, and `not`.  In a join's `ON`
    /// (`on`), the values of a hash are columns too.  A condition written
    /// as SQL names its qualified columns.
    fn condition(&mut self, value: &Expression<'_>, on: bool, ctx: &ExtractionCtx<'_>) {
        let Some(elements) = crate::parser::array_literal_elements(value) else {
            self.sql(value, ctx);
            return;
        };
        let mut elements = elements.iter();
        let operator = match elements.clone().next() {
            Some(ArrayElement::Value(first)) => literal_range(first.value, ctx.content)
                .map(|(start, end)| ctx.content[start as usize..end as usize].trim()),
            _ => None,
        };
        let Some(operator) = operator else {
            for element in elements {
                if let ArrayElement::KeyValue(entry) = element {
                    self.column(entry.key, ctx);
                    if on {
                        self.column(entry.value, ctx);
                    }
                }
            }
            return;
        };
        elements.next();
        let mut operands = elements.filter_map(|element| match element {
            ArrayElement::Value(entry) => Some(entry.value),
            _ => None,
        });
        let lower = operator.to_ascii_lowercase();
        match lower.as_str() {
            "and" | "or" | "not" => {
                for operand in operands {
                    self.condition(operand, on, ctx);
                }
            }
            "exists" | "not exists" => {}
            _ => {
                let Some(column) = operands.next() else {
                    return;
                };
                match crate::parser::array_literal_elements(column) {
                    // A composite `IN`: `['in', ['id', 'name'], …]`.
                    Some(columns) => {
                        for element in columns.iter() {
                            if let ArrayElement::Value(entry) = element {
                                self.column(entry.value, ctx);
                            }
                        }
                    }
                    None => self.column(column, ctx),
                }
            }
        }
    }

    /// The qualified columns a string of SQL names.
    fn sql(&mut self, value: &Expression<'_>, ctx: &ExtractionCtx<'_>) {
        let Some((start, end)) = literal_range(value, ctx.content) else {
            return;
        };
        let text = &ctx.content[start as usize..end as usize];
        for (at, word) in qualified_columns(text) {
            self.push_column(start + at as u32, word);
        }
    }

    /// The columns a list in a string names (`'id, name DESC'`).
    fn list(&mut self, value: &Expression<'_>, ctx: &ExtractionCtx<'_>) {
        let Some((start, end)) = literal_range(value, ctx.content) else {
            return;
        };
        let text = &ctx.content[start as usize..end as usize];
        for (at, word) in column_list(text) {
            self.push_column(start + at as u32, word);
        }
    }

    /// The column a string holding a single name names.
    fn column(&mut self, value: &Expression<'_>, ctx: &ExtractionCtx<'_>) {
        if let Some((start, end)) = literal_range(value, ctx.content) {
            self.push_column(start, &ctx.content[start as usize..end as usize]);
        }
    }

    /// The column `word`, written at `start`, when it is one.
    fn push_column(&mut self, start: u32, word: &str) {
        let Some((qualifier, at, name)) = column_text(word) else {
            return;
        };
        let name_start = start + at as u32;
        self.names.push(YiiQueryName {
            start: name_start,
            end: name_start + name.len() as u32,
            query: self.query,
            kind: YiiQueryNameKind::Column {
                qualifier: qualifier.map(atom),
            },
        });
    }
}

/// The class constant `value` reads (`Table::POST`), as a table name.
fn constant_table(value: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<YiiTableName> {
    let Expression::Access(Access::ClassConstant(access)) = value else {
        return None;
    };
    let Expression::Identifier(class) = access.class else {
        return None;
    };
    let ClassLikeConstantSelector::Identifier(constant) = &access.constant else {
        return None;
    };
    let class = ctx
        .resolved_name_at(class.span().start.offset)
        .unwrap_or(bytes_to_str(class.value()))
        .trim_start_matches('\\');
    Some(YiiTableName::Constant {
        class: atom(class),
        constant: atom(bytes_to_str(constant.value)),
    })
}
