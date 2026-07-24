use tower_lsp::lsp_types::Position;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IgnoreCodeSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IgnoreTagSpan {
    pub start: usize,
    pub end: usize,
}

pub(crate) fn phpstan_ignore_code_prefix(content: &str, position: Position) -> Option<String> {
    let offset = crate::text_position::position_to_offset(content, position) as usize;
    let offset = offset.min(content.len());
    let line_start = content[..offset]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let before_cursor = &content[line_start..offset];
    let tag_pos = before_cursor.rfind("@phpstan-ignore")?;
    let after_tag = &before_cursor[tag_pos + "@phpstan-ignore".len()..];
    if after_tag.starts_with("-line") || after_tag.starts_with("-next-line") {
        return None;
    }

    let mut depth = 0u32;
    let mut segment_start = 0usize;
    for (idx, ch) in after_tag.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => segment_start = idx + ch.len_utf8(),
            _ => {}
        }
    }
    if depth > 0 {
        return None;
    }

    let segment = after_tag[segment_start..].trim_start();
    if segment.contains('(') {
        return None;
    }

    let prefix = segment
        .trim_end()
        .chars()
        .rev()
        .take_while(|ch| is_identifier_char(*ch))
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Some(prefix)
}

pub(crate) fn phpstan_ignore_code_spans(content: &str) -> Vec<IgnoreCodeSpan> {
    let mut spans = Vec::new();
    let mut line_start = 0usize;

    for line in content.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        collect_line_spans(line_without_newline, line_start, &mut spans);
        line_start += line.len();
    }

    if !content.ends_with('\n') {
        return spans;
    }
    spans
}

pub(crate) fn phpstan_ignore_tag_spans(content: &str) -> Vec<IgnoreTagSpan> {
    let mut spans = Vec::new();
    let mut line_start = 0usize;

    for line in content.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        if let Some(tag_pos) = line_without_newline.find("@phpstan-ignore") {
            let after_tag = &line_without_newline[tag_pos + "@phpstan-ignore".len()..];
            if !after_tag.starts_with("-line") && !after_tag.starts_with("-next-line") {
                spans.push(IgnoreTagSpan {
                    start: line_start + tag_pos,
                    end: line_start + tag_pos + "@phpstan-ignore".len(),
                });
            }
        }
        line_start += line.len();
    }

    spans
}

fn collect_line_spans(line: &str, line_start: usize, spans: &mut Vec<IgnoreCodeSpan>) {
    let Some(tag_pos) = line.find("@phpstan-ignore") else {
        return;
    };
    let after_tag_start = tag_pos + "@phpstan-ignore".len();
    let after_tag = &line[after_tag_start..];
    if after_tag.starts_with("-line") || after_tag.starts_with("-next-line") {
        return;
    }

    let ids_end = after_tag.find("*/").unwrap_or(after_tag.len());
    let ids_region = &after_tag[..ids_end];
    let mut depth = 0u32;
    let mut idx = 0usize;

    while idx < ids_region.len() {
        let ch = ids_region[idx..].chars().next().unwrap();
        match ch {
            '(' => {
                depth += 1;
                idx += 1;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                idx += 1;
            }
            _ if depth == 0 && is_identifier_start(ch) => {
                let start = idx;
                idx += ch.len_utf8();
                while idx < ids_region.len() {
                    let next = ids_region[idx..].chars().next().unwrap();
                    if !is_identifier_char(next) {
                        break;
                    }
                    idx += next.len_utf8();
                }
                spans.push(IgnoreCodeSpan {
                    start: line_start + after_tag_start + start,
                    end: line_start + after_tag_start + idx,
                });
            }
            _ => idx += ch.len_utf8(),
        }
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}
