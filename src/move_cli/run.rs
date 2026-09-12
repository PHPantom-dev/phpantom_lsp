//! The `move` command driver: resolving what the user named, planning
//! the edits and file renames, and applying them to disk.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::{
    DocumentChangeOperation, DocumentChanges, OneOf, ResourceOp, TextEdit, Url, WorkspaceEdit,
};

use crate::analyse::OutputFormat;
use crate::{Backend, composer, config};

use super::{MoveOptions, MovePlan, MoveSummary, MoveWarning, output, residual};

enum MoveTarget {
    Class(String),
    Namespace(String),
}

/// What would leave the project unloadable once the plan is applied.
///
/// A declaration the move rewrites without taking its file along ends up
/// at a path PSR-4 no longer maps to its name, so the autoloader stops
/// finding it. Nothing else reports that, and a script driving this
/// command cannot see it from the exit code.
enum AutoloadRisk {
    /// The file declaring the moved class, which has to move with it.
    ClassFile(PathBuf),
    /// No PSR-4 mapping covers the destination namespace, so none of the
    /// files declaring it can be placed where the autoloader looks.
    UnmappedNamespace,
    /// Nothing to report.
    None,
}

/// Run a class or namespace move and return the process exit code.
pub async fn run(options: MoveOptions) -> i32 {
    match execute(&options).await {
        Ok(summary) => {
            match options.output_format {
                OutputFormat::Table => {
                    // Annotate in CI without giving up the readable
                    // summary, the way `analyze` and `fix` do.
                    if std::env::var("GITHUB_ACTIONS").is_ok() {
                        output::print_github_annotations(&summary);
                    }
                    output::print_table(&summary, options.use_colour);
                }
                OutputFormat::Github => output::print_github_annotations(&summary),
                OutputFormat::Json => output::print_json(&summary),
            }
            0
        }
        Err(message) => {
            eprintln!("Error: {message}");
            1
        }
    }
}

/// Plan a move, apply it unless the options ask for a dry run, and
/// report what it did.
///
/// [`run`] wraps this for the command line, where only the exit code
/// survives; callers that need the outcome itself use this directly.
pub async fn execute(options: &MoveOptions) -> Result<MoveSummary, String> {
    let root = options
        .workspace_root
        .canonicalize()
        .map_err(|e| format!("cannot resolve project root: {e}"))?;
    let cfg = config::load_config_from(&root, options.global_config.as_deref())
        .unwrap_or_else(|_| config::Config::default());

    let backend = Backend::new_headless_refactoring();
    crate::analyse::open_headless_project(&backend, &root, cfg).await;
    backend.supports_file_rename.store(true, Ordering::Release);
    backend.ensure_workspace_indexed();

    let from = resolve_source(&backend, &root, &options.from)?;
    let to = resolve_destination(&backend, &root, &options.to, &from)?;
    // The pre-move location, kept so the residual scan can recognize it
    // spelled as a path string rather than as a class name.
    let old_path;
    let (kind, from_name, to_name, edit, risk) = match (from, to) {
        (MoveTarget::Class(from), MoveTarget::Class(to)) => {
            // Read before planning: the edit itself does not say which file
            // declared the class, and that is what the check below needs.
            let definition = class_definition_path(&backend, &from);
            old_path = definition.clone();
            let risk = definition.map_or(AutoloadRisk::None, AutoloadRisk::ClassFile);
            let edit = backend
                .plan_class_move(&from, &to)?
                .ok_or_else(|| "the requested move would not change anything".to_string())?;
            ("class", from, to, edit, risk)
        }
        (MoveTarget::Namespace(from), MoveTarget::Namespace(to)) => {
            let mappings = backend.psr4_mappings().read();
            let risk = match composer::psr4_directory_for_namespace(&mappings, &root, &to) {
                Some(_) => AutoloadRisk::None,
                None => AutoloadRisk::UnmappedNamespace,
            };
            old_path = composer::psr4_directory_for_namespace(&mappings, &root, &from);
            drop(mappings);
            let edit = backend
                .plan_namespace_move(&from, &to)?
                .ok_or_else(|| "the requested move would not change anything".to_string())?;
            ("namespace", from, to, edit, risk)
        }
        _ => return Err("source and destination must both identify classes or namespaces".into()),
    };

    let plan = build_plan(&root, edit)?;
    let mut warnings = Vec::new();
    match risk {
        AutoloadRisk::ClassFile(definition)
            if !plan.moves.iter().any(|(from, _)| from == &definition) =>
        {
            warnings.push(MoveWarning {
                message: format!(
                    "This file now declares `{to_name}`, but no PSR-4 mapping covers that name, \
                     so it was left where it is and the autoloader will not find the class."
                ),
                file: Some(relative_display(&root, &definition)),
                line: None,
            });
        }
        AutoloadRisk::UnmappedNamespace => {
            warnings.push(MoveWarning {
                message: format!(
                    "No PSR-4 mapping covers `{to_name}`, so the files were left where they are \
                     and the autoloader will not find the classes they now declare."
                ),
                file: None,
                line: None,
            });
        }
        AutoloadRisk::ClassFile(_) | AutoloadRisk::None => {}
    }
    warnings.extend(residual::residual_warnings(
        &backend,
        &root,
        &from_name,
        old_path.as_deref(),
        &plan,
    ));

    let summary = MoveSummary {
        dry_run: options.dry_run,
        kind,
        from: from_name,
        to: to_name,
        files_changed: plan.writes.len(),
        paths_moved: plan.moves.len(),
        warnings,
    };
    if !options.dry_run {
        apply_plan(plan)?;
    }
    Ok(summary)
}

/// A path as the user typed it, relative to the project root.
pub(super) fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The file a class is declared in, as far as the index knows.
fn class_definition_path(backend: &Backend, fqn: &str) -> Option<PathBuf> {
    let uri = backend.symbols.fqn_uri_index.read().get(fqn).cloned()?;
    Url::parse(&uri).ok()?.to_file_path().ok()
}

fn resolve_source(backend: &Backend, root: &Path, value: &str) -> Result<MoveTarget, String> {
    if let Some(path) = existing_path(root, value) {
        if path.is_file() {
            return Ok(MoveTarget::Class(class_fqn_from_path(
                backend, root, &path,
            )?));
        }
        if path.is_dir() {
            return Ok(MoveTarget::Namespace(namespace_from_dir(
                backend, root, &path,
            )?));
        }
    }

    let normalized = value.trim_start_matches('\\');
    if backend
        .symbols
        .fqn_uri_index
        .read()
        .contains_key(normalized)
    {
        return Ok(MoveTarget::Class(normalized.to_string()));
    }
    let prefix = format!("{normalized}\\");
    if backend
        .symbols
        .fqn_uri_index
        .read()
        .keys()
        .any(|fqn| fqn.starts_with(&prefix))
    {
        return Ok(MoveTarget::Namespace(normalized.to_string()));
    }
    Err(format!(
        "`{value}` does not identify an indexed class, namespace, file, or directory"
    ))
}

fn resolve_destination(
    backend: &Backend,
    root: &Path,
    value: &str,
    source: &MoveTarget,
) -> Result<MoveTarget, String> {
    if looks_like_path(value) {
        let path = absolute_path(root, value);
        return match source {
            MoveTarget::Class(_) => Ok(MoveTarget::Class(class_fqn_from_path(
                backend, root, &path,
            )?)),
            MoveTarget::Namespace(_) => Ok(MoveTarget::Namespace(namespace_from_dir(
                backend, root, &path,
            )?)),
        };
    }
    let name = value.trim_start_matches('\\').to_string();
    match source {
        MoveTarget::Class(_) => Ok(MoveTarget::Class(name)),
        MoveTarget::Namespace(_) => Ok(MoveTarget::Namespace(name)),
    }
}

fn existing_path(root: &Path, value: &str) -> Option<PathBuf> {
    let path = absolute_path(root, value);
    path.exists().then_some(path)
}

fn absolute_path(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn looks_like_path(value: &str) -> bool {
    Path::new(value).is_absolute()
        || value.ends_with(".php")
        || value.contains('/')
        || value.starts_with('.')
}

fn class_fqn_from_path(backend: &Backend, root: &Path, path: &Path) -> Result<String, String> {
    let mappings = backend.psr4_mappings().read();
    let (namespace, class) = composer::resolve_namespace_from_path(&mappings, root, path)
        .ok_or_else(|| format!("{} is not a PHP file under a PSR-4 mapping", path.display()))?;
    Ok(namespace.map_or(class.clone(), |namespace| format!("{namespace}\\{class}")))
}

fn namespace_from_dir(backend: &Backend, root: &Path, path: &Path) -> Result<String, String> {
    let marker = path.join("__PHPantomNamespace.php");
    let mappings = backend.psr4_mappings().read();
    let (namespace, _) = composer::resolve_namespace_from_path(&mappings, root, &marker)
        .ok_or_else(|| {
            format!(
                "{} is not a directory under a PSR-4 mapping",
                path.display()
            )
        })?;
    namespace.ok_or_else(|| "the global namespace cannot be moved as a directory".to_string())
}

fn build_plan(root: &Path, edit: WorkspaceEdit) -> Result<MovePlan, String> {
    let mut edits: HashMap<PathBuf, Vec<TextEdit>> = HashMap::new();
    let mut moves = Vec::new();
    if let Some(changes) = edit.changes {
        for (uri, file_edits) in changes {
            edits
                .entry(uri_path(&uri, root)?)
                .or_default()
                .extend(file_edits);
        }
    }
    if let Some(document_changes) = edit.document_changes {
        match document_changes {
            DocumentChanges::Edits(document_edits) => {
                for document in document_edits {
                    let path = uri_path(&document.text_document.uri, root)?;
                    edits
                        .entry(path)
                        .or_default()
                        .extend(document.edits.into_iter().map(|edit| match edit {
                            OneOf::Left(edit) => edit,
                            OneOf::Right(edit) => edit.text_edit,
                        }));
                }
            }
            DocumentChanges::Operations(operations) => {
                for operation in operations {
                    match operation {
                        DocumentChangeOperation::Edit(document) => {
                            let path = uri_path(&document.text_document.uri, root)?;
                            edits
                                .entry(path)
                                .or_default()
                                .extend(document.edits.into_iter().map(|edit| match edit {
                                    OneOf::Left(edit) => edit,
                                    OneOf::Right(edit) => edit.text_edit,
                                }));
                        }
                        DocumentChangeOperation::Op(ResourceOp::Rename(rename)) => moves.push((
                            uri_path(&rename.old_uri, root)?,
                            uri_path(&rename.new_uri, root)?,
                        )),
                        DocumentChangeOperation::Op(_) => {
                            return Err(
                                "the move plan contains an unsupported file operation".into()
                            );
                        }
                    }
                }
            }
        }
    }

    let mut writes = Vec::with_capacity(edits.len());
    for (path, file_edits) in edits {
        let source = source_path_for_target(&path, &moves);
        let content = std::fs::read_to_string(&source)
            .map_err(|e| format!("failed to read {}: {e}", source.display()))?;
        writes.push((path, apply_text_edits(content, file_edits)?));
    }
    validate_moves(root, &moves)?;
    Ok(MovePlan { writes, moves })
}

fn uri_path(uri: &Url, root: &Path) -> Result<PathBuf, String> {
    let path = uri
        .to_file_path()
        .map_err(|_| format!("unsupported non-file URI: {uri}"))?;
    if !path.starts_with(root) {
        return Err(format!(
            "move would modify a path outside the project: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn source_path_for_target(target: &Path, moves: &[(PathBuf, PathBuf)]) -> PathBuf {
    moves
        .iter()
        .find_map(|(from, to)| {
            if target == to {
                return Some(from.clone());
            }
            target.strip_prefix(to).ok().and_then(|relative| {
                (!relative.as_os_str().is_empty()).then(|| from.join(relative))
            })
        })
        .unwrap_or_else(|| target.to_path_buf())
}

fn apply_text_edits(mut content: String, mut edits: Vec<TextEdit>) -> Result<String, String> {
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
    for edit in edits {
        let start = crate::text_position::position_to_byte_offset(&content, edit.range.start);
        let end = crate::text_position::position_to_byte_offset(&content, edit.range.end);
        if start > end || end > content.len() {
            return Err("a planned text edit no longer matches its source file".into());
        }
        content.replace_range(start..end, &edit.new_text);
    }
    Ok(content)
}

fn validate_moves(root: &Path, moves: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    let sources: HashSet<&Path> = moves.iter().map(|(from, _)| from.as_path()).collect();
    let mut destinations = HashSet::new();
    for (from, to) in moves {
        if !from.starts_with(root) || !to.starts_with(root) {
            return Err("move would leave the project root".into());
        }
        if !from.exists() {
            return Err(format!("move source does not exist: {}", from.display()));
        }
        if !destinations.insert(to) {
            return Err(format!("more than one path would move to {}", to.display()));
        }
        if to.exists() && !sources.contains(to.as_path()) {
            return Err(format!("move destination already exists: {}", to.display()));
        }
    }
    Ok(())
}

fn apply_plan(plan: MovePlan) -> Result<(), String> {
    for (from, to) in &plan.moves {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        std::fs::rename(from, to)
            .map_err(|e| format!("failed to move {} to {}: {e}", from.display(), to.display()))?;
    }
    for (path, content) in plan.writes {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    }
    Ok(())
}
