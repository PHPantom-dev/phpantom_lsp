//! The imports a moved file needs for the siblings it reached through the
//! namespace it is leaving.

use std::collections::{BTreeMap, HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::use_edit::UseBlockInfo;
use crate::symbol_map::{ClassRefContext, SymbolKind};

impl Backend {
    /// The imports the moved file needs for the names it used to reach
    /// through its own namespace.
    ///
    /// An unqualified class, function, or constant name resolves against
    /// the namespace the file declares, so a file leaving a populated
    /// namespace silently loses every sibling it named that way: the
    /// reference still reads the same, it just points at a name the
    /// destination namespace has never heard of.  Each one becomes an
    /// explicit import of the name it used to reach.
    ///
    /// Returns the imports sorted into the order a `use` block puts them
    /// in: classes, then constants, then functions, alphabetical within
    /// each group.
    pub(super) fn sibling_imports_for_move(
        &self,
        symbol_map: &crate::symbol_map::SymbolMap,
        content: &str,
        use_map: &HashMap<String, String>,
        old_ns: Option<&str>,
    ) -> Vec<SiblingImport> {
        // Only the first `namespace` declaration is rewritten, so in a
        // file that declares several there is no single old namespace to
        // resolve the unqualified names against.
        if symbol_map
            .spans
            .iter()
            .filter(|span| matches!(span.kind, SymbolKind::NamespaceDeclaration { .. }))
            .count()
            > 1
        {
            return Vec::new();
        }

        // Whatever the file declares itself travels with it.  Classes,
        // functions, and constants are three separate PHP namespaces, so
        // a file declaring `function helper()` says nothing about the
        // class `Helper` it names.
        let mut declared_classes: HashSet<String> = HashSet::new();
        let mut declared_functions: HashSet<String> = HashSet::new();
        let mut declared_constants: HashSet<&str> = HashSet::new();
        for span in &symbol_map.spans {
            match &span.kind {
                SymbolKind::ClassDeclaration { name } => {
                    declared_classes.insert(name.to_lowercase());
                }
                SymbolKind::FunctionCall {
                    name,
                    is_definition: true,
                    ..
                } => {
                    declared_functions.insert(name.to_lowercase());
                }
                SymbolKind::ConstantReference {
                    name,
                    is_definition: true,
                } => {
                    declared_constants.insert(name.as_str());
                }
                _ => {}
            }
        }

        let mut imports: Vec<SiblingImport> = Vec::new();
        let mut seen: HashSet<(SiblingKind, String)> = HashSet::new();

        for span in &symbol_map.spans {
            let (kind, name) = match &span.kind {
                // A prose-tolerant `@see` target is not worth an import:
                // it may name anything, and an unresolvable one is not an
                // error.
                SymbolKind::ClassReference {
                    name,
                    is_fqn: false,
                    context,
                } if !matches!(
                    context,
                    ClassRefContext::UseImport | ClassRefContext::DocblockSee
                ) =>
                {
                    (SiblingKind::Class, name.as_str())
                }
                SymbolKind::FunctionCall {
                    name,
                    is_definition: false,
                    is_docblock_reference: false,
                } => (SiblingKind::Function, name.as_str()),
                SymbolKind::ConstantReference {
                    name,
                    is_definition: false,
                } => (SiblingKind::Constant, name.as_str()),
                _ => continue,
            };

            if name.is_empty() || name.contains('\\') {
                continue;
            }
            // A leading `\` names the global namespace outright, and the
            // function and constant spans drop it from the stored name,
            // so the source text is what tells the two apart.
            if content
                .get(span.start as usize..span.end as usize)
                .is_some_and(|text| text.starts_with('\\'))
            {
                continue;
            }
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "self" | "static" | "parent"
            ) {
                continue;
            }
            let declared = match kind {
                SiblingKind::Class => declared_classes.contains(&name.to_lowercase()),
                SiblingKind::Function => declared_functions.contains(&name.to_lowercase()),
                SiblingKind::Constant => declared_constants.contains(name),
            };
            if declared {
                continue;
            }
            // An imported name never resolved through the namespace in
            // the first place.  Constant imports are case-sensitive;
            // class and function imports are not.
            let imported = match kind {
                SiblingKind::Constant => use_map.contains_key(name),
                _ => use_map.keys().any(|alias| alias.eq_ignore_ascii_case(name)),
            };
            if imported {
                continue;
            }

            let fqn = match old_ns {
                Some(ns) => format!("{ns}\\{name}"),
                None => name.to_string(),
            };
            // An unqualified function or constant falls back to the
            // global namespace, so one that was global stays reachable.
            if !fqn.contains('\\') && kind != SiblingKind::Class {
                continue;
            }
            if !self.sibling_symbol_exists(kind, &fqn) {
                continue;
            }
            let dedupe_key = match kind {
                SiblingKind::Constant => fqn.clone(),
                _ => fqn.to_lowercase(),
            };
            if !seen.insert((kind, dedupe_key)) {
                continue;
            }
            imports.push(SiblingImport {
                sort_key: kind.sort_key(&fqn),
                statement: kind.statement(&fqn),
            });
        }

        imports.sort_by(|a, b| {
            UseBlockInfo::key_group(&a.sort_key)
                .cmp(&UseBlockInfo::key_group(&b.sort_key))
                .then_with(|| a.sort_key.cmp(&b.sort_key))
        });
        imports
    }

    /// Whether the old namespace really holds the sibling an unqualified
    /// reference named.
    fn sibling_symbol_exists(&self, kind: SiblingKind, fqn: &str) -> bool {
        match kind {
            SiblingKind::Class => self.symbols.fqn_uri_index.read().contains_key(fqn),
            SiblingKind::Function => {
                self.symbols.global_functions.read().contains_key(fqn)
                    || self
                        .symbols
                        .autoload_function_index
                        .read()
                        .contains_key(fqn)
            }
            SiblingKind::Constant => {
                self.symbols.global_defines.read().contains_key(fqn)
                    || self
                        .symbols
                        .autoload_constant_index
                        .read()
                        .contains_key(fqn)
            }
        }
    }
}

/// Which flavour of `use` statement a sibling reference needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SiblingKind {
    Class,
    Constant,
    Function,
}

impl SiblingKind {
    /// The key [`UseBlockInfo`] sorts this import by.
    fn sort_key(self, fqn: &str) -> String {
        match self {
            Self::Class => fqn.to_lowercase(),
            Self::Constant => format!("const {}", fqn.to_lowercase()),
            Self::Function => format!("function {}", fqn.to_lowercase()),
        }
    }

    fn statement(self, fqn: &str) -> String {
        match self {
            Self::Class => format!("use {};", fqn),
            Self::Constant => format!("use const {};", fqn),
            Self::Function => format!("use function {};", fqn),
        }
    }
}

/// One `use` statement the moved file needs.
pub(super) struct SiblingImport {
    pub(super) sort_key: String,
    pub(super) statement: String,
}

/// Place `imports` in the file's existing `use` block.
///
/// The positions are all computed against the unmodified block, so
/// imports that would land on the same line are merged into a single
/// edit: two zero-width edits sharing an offset land in whichever order
/// the client applies them.
pub(super) fn build_sibling_import_edits(
    content: &str,
    imports: &[SiblingImport],
) -> Vec<TextEdit> {
    if imports.is_empty() {
        return Vec::new();
    }

    let use_block = crate::completion::use_edit::analyze_use_block(content);

    let mut by_line: BTreeMap<u32, Vec<&SiblingImport>> = BTreeMap::new();
    for import in imports {
        let line = use_block.insert_position_for_key(&import.sort_key).line;
        by_line.entry(line).or_default().push(import);
    }

    // Which of the three import groups the file already writes, so the
    // blank line that separates a group is only added when this move
    // opens the group.
    let mut group_present = [false; 3];
    for (_, key) in &use_block.existing {
        group_present[UseBlockInfo::key_group(key) as usize] = true;
    }

    let mut first = true;
    let mut edits = Vec::with_capacity(by_line.len());
    for (line, group) in by_line {
        let mut new_text = String::new();
        for import in group {
            let idx = UseBlockInfo::key_group(&import.sort_key) as usize;
            let opens_use_block = first && use_block.existing.is_empty() && use_block.has_namespace;
            let opens_group =
                !group_present[idx] && group_present[..idx].iter().any(|present| *present);
            if opens_use_block || opens_group {
                new_text.push('\n');
            }
            new_text.push_str(&import.statement);
            new_text.push('\n');
            group_present[idx] = true;
            first = false;
        }
        let position = Position { line, character: 0 };
        edits.push(TextEdit {
            range: Range {
                start: position,
                end: position,
            },
            new_text,
        });
    }

    edits
}
