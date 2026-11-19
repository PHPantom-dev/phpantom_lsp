//! Completion of the tables and columns a `yiisoft/db` query names:
//! `$query->from('|')` offers the tables the project's migrations create,
//! and `->where(['|' => …])` and `->orderBy('p.|')` the columns of the
//! tables the query reads from.  What each argument takes comes from
//! [`crate::yii::queries`], the same rules the symbol map's names follow.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::symbol_map::{SubjectText, YiiQuerySite};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::FileContext;
use crate::yii::queries::{QueryArgument, is_name, query_argument, takes_names};

use super::eloquent_string::{extract_identifier_backwards, extract_receiver_subject};

/// A name to offer: its label, its detail, and its kind.
type Candidate = (String, Option<String>, CompletionItemKind);

/// What the string being typed names.
enum Wanted {
    Table,
    /// A column, alone in the string or, when `list`, an item of a list
    /// (`'id, name DESC'`).
    Column {
        list: bool,
    },
}

impl Backend {
    /// Complete the table or column a query method's string argument is
    /// being typed as.
    pub(crate) fn try_yii_query_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        use StringArgumentPosition::*;

        let (quote, _) = code.open_string?;
        let cursor_offset = position_to_offset(content, position) as usize;
        let call = code.open_brackets.iter().rev().find(|bracket| {
            bracket.byte == b'('
                && !extract_identifier_backwards(&content[..bracket.code_before])
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("array"))
        })?;
        let operator = call.callee_operator.as_ref()?;
        if operator.is_static {
            return None;
        }
        let (method, _) = extract_identifier_backwards(&content[..call.code_before])?;
        if !takes_names(method) {
            return None;
        }
        let argument = string_argument(content, cursor_offset, code, call)?;
        if argument.name.is_some() {
            return None;
        }
        let position_in = argument.position;
        let wanted = match query_argument(method, argument.index)? {
            QueryArgument::From | QueryArgument::JoinTable => {
                matches!(position_in, Direct | ArrayValue | ArrayElement).then_some(Wanted::Table)
            }
            QueryArgument::JoinOn => matches!(position_in, ArrayKey | ArrayValue)
                .then_some(Wanted::Column { list: false }),
            QueryArgument::Condition => matches!(position_in, ArrayKey | ArrayElement)
                .then_some(Wanted::Column { list: false }),
            QueryArgument::Order => match position_in {
                ArrayKey => Some(Wanted::Column { list: false }),
                Direct | ArrayElement => Some(Wanted::Column { list: true }),
                ArrayValue => None,
            },
            QueryArgument::Columns => matches!(position_in, Direct | ArrayValue | ArrayElement)
                .then_some(Wanted::Column { list: true }),
            QueryArgument::Column => {
                (position_in == Direct).then_some(Wanted::Column { list: false })
            }
            QueryArgument::Alias => None,
        }?;

        let typed = content.get(quote + 1..cursor_offset)?;
        let typed = match wanted {
            Wanted::Column { list: true } => typed
                .rsplit_once(',')
                .map_or(typed, |(_, item)| item)
                .trim_start(),
            _ => typed,
        };
        if typed.contains(char::is_whitespace) {
            return None;
        }

        // The query the cursor is in: the innermost chain the symbol map
        // read around it, or, while the map has none, the receiver alone.
        let map = self.symbol_map_for(uri);
        let cursor = cursor_offset as u32;
        let found = map.as_ref().and_then(|map| {
            let site = map
                .yii_queries
                .iter()
                .filter(|site| site.start <= cursor && cursor <= site.end)
                .min_by_key(|site| site.end - site.start)?;
            Some((site, site.subject.as_str(map.source(content)?).to_string()))
        });
        let fallback;
        let (site, subject) = match found {
            Some(found) => found,
            None => {
                fallback = YiiQuerySite {
                    start: 0,
                    end: 0,
                    subject: SubjectText::Range { start: 0, end: 0 },
                    tables: Vec::new(),
                    sets_from: false,
                    alias: None,
                    open: false,
                };
                (&fallback, extract_receiver_subject(content, operator)?)
            }
        };
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let rctx = loaders.resolution_ctx_at(content, cursor);
        let tables = self.yii_query_tables(site, &subject, &rctx, class_loader)?;
        let schema = self.yii_schema();

        let (prefix, names): (&str, Vec<Candidate>) = match wanted {
            Wanted::Table => {
                let prefix = typed.trim_start_matches("{{").trim_start_matches('%');
                let mut names: Vec<_> = schema
                    .tables()
                    .map(|(name, _)| (name.to_string(), None, CompletionItemKind::Struct))
                    .collect();
                names.sort_by(|a, b| a.0.cmp(&b.0));
                (prefix, names)
            }
            Wanted::Column { .. } => {
                let (qualifier, prefix) = match typed.rsplit_once('.') {
                    Some((qualifier, prefix)) if is_name(qualifier) => (Some(qualifier), prefix),
                    Some(_) => return None,
                    None => (None, typed),
                };
                let mut names: Vec<Candidate> = Vec::new();
                for (table, column) in tables.columns(&schema, qualifier) {
                    if names.iter().any(|(known, _, _)| *known == column.name) {
                        continue;
                    }
                    let detail = match &column.kind {
                        Some(kind) => format!("{table}: {kind}"),
                        None => table.to_string(),
                    };
                    names.push((column.name.clone(), Some(detail), CompletionItemKind::Field));
                }
                (prefix, names)
            }
        };
        if names.is_empty() {
            return None;
        }

        let start = cursor_offset - prefix.len();
        let edit_range = Range {
            start: offset_to_position(content, start),
            end: position,
        };
        let prefix_lower = prefix.to_lowercase();
        let items: Vec<CompletionItem> = names
            .into_iter()
            .filter(|(name, _, _)| {
                name.to_lowercase().starts_with(&prefix_lower)
                    && !(position_in == ArrayKey
                        && argument.used_keys.iter().any(|used| used == name))
            })
            .enumerate()
            .map(|(i, (name, detail, kind))| CompletionItem {
                label: name.clone(),
                kind: Some(kind),
                detail,
                sort_text: Some(format!("{i:05}")),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: name,
                })),
                ..Default::default()
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }
}
