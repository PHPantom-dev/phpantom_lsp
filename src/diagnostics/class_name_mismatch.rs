use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::types::{ClassInfo, ClassLikeKind};
use crate::util::offset_to_position;

impl Backend {
    pub fn collect_class_name_mismatch_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(diag) = class_name_mismatch_diagnostic(self, uri, content) else {
            return;
        };
        out.push(diag);
    }
}

pub(crate) fn class_name_mismatch_diagnostic(
    backend: &Backend,
    uri: &str,
    content: &str,
) -> Option<Diagnostic> {
    let file_path = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok())?;
    let file_stem = file_path.file_stem().and_then(|s| s.to_str())?;

    let classes = backend.parse_php(content);
    if classes.len() != 1 {
        return None;
    }
    let class = &classes[0];
    if class.name == file_stem {
        return None;
    }

    let range = class_name_range(content, class)?;

    Some(Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String("class_name_mismatch".to_string())),
        source: Some("phpantom".to_string()),
        message: format!(
            "Class name `{}` does not match filename `{}`",
            class.name, file_stem,
        ),
        ..Default::default()
    })
}

pub(crate) fn class_name_range(content: &str, class: &ClassInfo) -> Option<Range> {
    let keyword = match class.kind {
        ClassLikeKind::Class => "class",
        ClassLikeKind::Interface => "interface",
        ClassLikeKind::Trait => "trait",
        ClassLikeKind::Enum => "enum",
    };
    let kw_off = class.keyword_offset as usize;
    let slice = content.get(kw_off..)?;
    let after_kw = slice.strip_prefix(keyword)?;
    let ws = after_kw.len() - after_kw.trim_start().len();
    let name_start = kw_off + keyword.len() + ws;
    let name_end = name_start + class.name.len();
    Some(Range {
        start: offset_to_position(content, name_start),
        end: offset_to_position(content, name_end),
    })
}
