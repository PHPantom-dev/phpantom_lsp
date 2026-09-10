//! Headless class and namespace moves for the command-line interface.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::{
    DocumentChangeOperation, DocumentChanges, OneOf, ResourceOp, TextEdit, Url, WorkspaceEdit,
};

use crate::analyse::OutputFormat;
use crate::{Backend, composer, config};

mod output;
mod residual;

/// Options for the move command.
#[derive(Debug)]
pub struct MoveOptions {
    /// Source class, namespace, file, or directory.
    pub from: String,
    /// Destination class, namespace, file, or directory.
    pub to: String,
    /// Workspace root.
    pub workspace_root: PathBuf,
    /// Preview the move without writing files.
    pub dry_run: bool,
    /// Whether to output with ANSI colours.
    pub use_colour: bool,
    /// Output format, shared with `analyze` and `fix`.
    pub output_format: OutputFormat,
    /// Global configuration path.
    pub global_config: Option<PathBuf>,
}

#[derive(Debug)]
struct MoveSummary {
    dry_run: bool,
    kind: &'static str,
    from: String,
    to: String,
    files_changed: usize,
    paths_moved: usize,
    /// Conditions the caller needs to act on even though the move applied.
    warnings: Vec<MoveWarning>,
}

/// Something the move left for the caller to deal with: a destination
/// the autoloader cannot reach, or a mention of the old name or path
/// the rewriter had no way to see.
#[derive(Debug)]
struct MoveWarning {
    /// What went wrong, without the location, which is carried
    /// separately so the JSON and GitHub formats can place it.
    message: String,
    /// The project-relative file the warning is about, after the move.
    file: Option<String>,
    /// The 1-based line within that file.
    line: Option<usize>,
}

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

struct MovePlan {
    writes: Vec<(PathBuf, String)>,
    moves: Vec<(PathBuf, PathBuf)>,
}

/// Run a class or namespace move and return the process exit code.
pub async fn run(options: MoveOptions) -> i32 {
    match run_inner(&options).await {
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

async fn run_inner(options: &MoveOptions) -> Result<MoveSummary, String> {
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
fn relative_display(root: &Path, path: &Path) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).expect("src");
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("composer");
        for (relative, content) in files {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, content).expect("file");
        }
        dir
    }

    #[tokio::test]
    async fn moves_class_by_fqn_and_updates_references() {
        let dir = project(&[
            (
                "src/Old/Widget.php",
                "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\n\nuse App\\Old\\Widget;\n\nnew Widget();\n",
            ),
        ]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\Domain\\Gadget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("move");
        assert_eq!(summary.kind, "class");
        assert!(!dir.path().join("src/Old/Widget.php").exists());
        let declaration =
            std::fs::read_to_string(dir.path().join("src/Domain/Gadget.php")).expect("declaration");
        assert!(declaration.contains("namespace App\\Domain;"));
        assert!(declaration.contains("class Gadget"));
        let consumer =
            std::fs::read_to_string(dir.path().join("src/Consumer.php")).expect("consumer");
        assert!(consumer.contains("use App\\Domain\\Gadget;"));
        assert!(consumer.contains("new Gadget()"));
    }

    #[tokio::test]
    async fn moves_class_by_path() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "src/Old/Widget.php".into(),
            to: "src/New/Widget.php".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");
        let content =
            std::fs::read_to_string(dir.path().join("src/New/Widget.php")).expect("moved");
        assert!(content.contains("namespace App\\New;"));
    }

    #[tokio::test]
    async fn moves_namespace_by_directory() {
        let dir = project(&[
            (
                "src/Old/Widget.php",
                "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
            ),
            (
                "src/Old/Nested/Thing.php",
                "<?php\nnamespace App\\Old\\Nested;\n\nclass Thing {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\n\nuse App\\Old\\Widget;\n",
            ),
        ]);
        let options = MoveOptions {
            from: "src/Old".into(),
            to: "src/Domain".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");
        assert!(!dir.path().join("src/Old").exists());
        let widget =
            std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("widget");
        let nested = std::fs::read_to_string(dir.path().join("src/Domain/Nested/Thing.php"))
            .expect("nested");
        assert!(widget.contains("namespace App\\Domain;"));
        assert!(nested.contains("namespace App\\Domain\\Nested;"));
        let consumer =
            std::fs::read_to_string(dir.path().join("src/Consumer.php")).expect("consumer");
        assert!(consumer.contains("use App\\Domain\\Widget;"));
    }

    #[tokio::test]
    async fn dry_run_changes_nothing() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\New\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: true,
            use_colour: false,
            output_format: OutputFormat::Json,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("plan");
        assert!(summary.dry_run);
        assert!(dir.path().join("src/Old/Widget.php").exists());
        assert!(!dir.path().join("src/New/Widget.php").exists());
    }

    #[tokio::test]
    async fn moves_namespace_by_fqn() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old".into(),
            to: "App\\New".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");
        let content =
            std::fs::read_to_string(dir.path().join("src/New/Widget.php")).expect("moved");
        assert!(content.contains("namespace App\\New;"));
    }

    #[tokio::test]
    async fn warns_when_psr4_cannot_place_the_moved_class() {
        // `Other\` is outside the autoload map, so the declaration is
        // rewritten but the file cannot follow it.  Reporting that as a
        // plain success would hand back a class the autoloader misses.
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "Other\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("move");
        assert_eq!(summary.paths_moved, 0);
        assert!(
            summary.warnings.iter().any(|warning| {
                warning.file.as_deref() == Some("src/Old/Widget.php")
                    && warning.message.contains("PSR-4")
            }),
            "expected a PSR-4 warning, got {:?}",
            summary.warnings
        );
        assert!(
            std::fs::read_to_string(dir.path().join("src/Old/Widget.php"))
                .expect("declaration")
                .contains("namespace Other;")
        );
    }

    #[tokio::test]
    async fn a_placed_class_move_warns_about_nothing() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\Domain\\Gadget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("move");
        assert_eq!(summary.paths_moved, 1);
        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    }

    #[tokio::test]
    async fn imports_the_siblings_the_moved_class_reached_by_namespace() {
        let dir = project(&[
            (
                "src/Old/Widget.php",
                "<?php\nnamespace App\\Old;\n\nclass Widget\n{\n    public function make(Cog $cog): Gear\n    {\n        return new Gear($cog);\n    }\n}\n",
            ),
            (
                "src/Old/Cog.php",
                "<?php\nnamespace App\\Old;\n\nclass Cog {}\n",
            ),
            (
                "src/Old/Gear.php",
                "<?php\nnamespace App\\Old;\n\nclass Gear\n{\n    public function __construct(Cog $cog) {}\n}\n",
            ),
        ]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\Domain\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");
        let moved =
            std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("moved");
        assert!(
            moved.contains("use App\\Old\\Cog;") && moved.contains("use App\\Old\\Gear;"),
            "{moved}"
        );
        // The references keep their short spelling; the imports are what
        // makes them resolve again.
        assert!(moved.contains("make(Cog $cog): Gear"), "{moved}");
    }

    #[tokio::test]
    async fn imports_the_sibling_functions_and_constants_too() {
        let dir = project(&[
            (
                "src/Old/Widget.php",
                "<?php\nnamespace App\\Old;\n\nclass Widget\n{\n    public function make(): string\n    {\n        return spin(LIMIT);\n    }\n}\n",
            ),
            (
                "src/Old/helpers.php",
                "<?php\nnamespace App\\Old;\n\nconst LIMIT = 3;\n\nfunction spin(int $n): string\n{\n    return (string) $n;\n}\n",
            ),
        ]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\Domain\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");
        let moved =
            std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("moved");
        assert!(moved.contains("use const App\\Old\\LIMIT;"), "{moved}");
        assert!(moved.contains("use function App\\Old\\spin;"), "{moved}");
    }

    #[tokio::test]
    async fn a_name_the_moved_file_declares_itself_is_not_imported() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget\n{\n    public function make(): Helper\n    {\n        return new Helper();\n    }\n}\n\nclass Helper {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\Domain\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");
        let moved =
            std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("moved");
        assert!(!moved.contains("use App\\Old\\Helper;"), "{moved}");
        assert!(!moved.contains("use App\\Old\\Widget;"), "{moved}");
    }

    #[tokio::test]
    async fn warns_when_psr4_cannot_place_the_moved_namespace() {
        // `Other\` is outside the autoload map, so there is nowhere to put
        // the directory.  The declarations are still rewritten, which is
        // what makes the files unreachable and worth reporting.
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old".into(),
            to: "Other\\Domain".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("move");
        assert_eq!(summary.paths_moved, 0);
        assert!(
            summary.warnings.iter().any(|warning| {
                warning.message.contains("Other\\Domain") && warning.message.contains("PSR-4")
            }),
            "expected a PSR-4 warning, got {:?}",
            summary.warnings
        );
        // The file stays put rather than landing in a directory built out
        // of a prefix the destination never had.
        assert!(
            std::fs::read_to_string(dir.path().join("src/Old/Widget.php"))
                .expect("declaration")
                .contains("namespace Other\\Domain;")
        );
    }

    #[tokio::test]
    async fn a_namespace_destination_shorter_than_the_mapping_is_refused_not_a_panic() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old".into(),
            to: "Xy".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: true,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("plan");
        assert_eq!(summary.paths_moved, 0);
        assert!(!summary.warnings.is_empty());
    }

    #[tokio::test]
    async fn moves_a_namespace_between_psr4_mappings() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/","Lib\\":"lib/"}}}"#,
        )
        .expect("composer");
        let options = MoveOptions {
            from: "App\\Old".into(),
            to: "Lib\\Domain".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("move");
        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
        assert!(!dir.path().join("src/Old").exists());
        assert!(
            std::fs::read_to_string(dir.path().join("lib/Domain/Widget.php"))
                .expect("moved")
                .contains("namespace Lib\\Domain;")
        );
    }

    /// A project whose `Tests\\` prefix is served by two directories,
    /// which is what Composer's array form allows.
    fn two_root_project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload-dev":{"psr-4":{"Tests\\":["tests/","shared/tests/"]}}}"#,
        )
        .expect("composer");
        for (relative, content) in files {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, content).expect("file");
        }
        dir
    }

    #[tokio::test]
    async fn refuses_a_namespace_that_lives_in_two_psr4_roots() {
        // Naming one root resolves to the namespace both of them serve,
        // and from there the other root is indistinguishable.  Planning
        // the move anyway carries the second root's files along, onto the
        // same destination, so it is refused with both roots named.
        let dir = two_root_project(&[
            (
                "tests/Unit/TokenTransferTest.php",
                "<?php\nnamespace Tests\\Unit;\n\nclass TokenTransferTest {}\n",
            ),
            (
                "shared/tests/Support/Helper.php",
                "<?php\nnamespace Tests\\Support;\n\nclass Helper {}\n",
            ),
        ]);
        let options = MoveOptions {
            from: "shared/tests".into(),
            to: "tests/Shared".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: true,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let error = run_inner(&options).await.expect_err("refusal");
        assert!(
            error.contains("tests/") && error.contains("shared/tests/"),
            "expected both roots named, got {error}"
        );
        assert!(
            !error.contains("No such file"),
            "expected a refusal, not a derived path failing to open: {error}"
        );
    }

    #[tokio::test]
    async fn a_second_root_that_holds_nothing_does_not_block_the_move() {
        // `shared/tests/` is mapped but was never created, so the moved
        // namespace still sits in exactly one directory.
        let dir = two_root_project(&[(
            "tests/Unit/TokenTransferTest.php",
            "<?php\nnamespace Tests\\Unit;\n\nclass TokenTransferTest {}\n",
        )]);
        let options = MoveOptions {
            from: "Tests\\Unit".into(),
            to: "Tests\\Feature".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("move");
        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
        assert!(
            std::fs::read_to_string(dir.path().join("tests/Feature/TokenTransferTest.php"))
                .expect("moved")
                .contains("namespace Tests\\Feature;")
        );
    }

    #[tokio::test]
    async fn two_prefixes_naming_one_directory_are_one_root() {
        // `Tests\\` at `tests/` and `Tests\\Unit\\` at `tests/Unit/` both
        // place `Tests\\Unit` in the same directory, which is one place to
        // move from, not two.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload-dev":{"psr-4":{"Tests\\":"tests/","Tests\\Unit\\":"tests/Unit/"}}}"#,
        )
        .expect("composer");
        let path = dir.path().join("tests/Unit/TokenTransferTest.php");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(
            &path,
            "<?php\nnamespace Tests\\Unit;\n\nclass TokenTransferTest {}\n",
        )
        .expect("file");
        let options = MoveOptions {
            from: "Tests\\Unit".into(),
            to: "Tests\\Feature".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("move");
        assert_eq!(summary.paths_moved, 1);
        assert!(
            std::fs::read_to_string(dir.path().join("tests/Feature/TokenTransferTest.php"))
                .expect("moved")
                .contains("namespace Tests\\Feature;")
        );
    }

    #[tokio::test]
    async fn refuses_occupied_class_destination_without_changes() {
        let old = "<?php\nnamespace App\\Old;\n\nclass Widget {}\n";
        let existing = "<?php\nnamespace App\\New;\n\nclass Widget {}\n";
        let dir = project(&[
            ("src/Old/Widget.php", old),
            ("src/New/Widget.php", existing),
        ]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\New\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let error = run_inner(&options).await.expect_err("conflict");
        assert!(error.contains("already"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/Old/Widget.php")).expect("old"),
            old
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/New/Widget.php")).expect("existing"),
            existing
        );
    }

    #[tokio::test]
    async fn a_moved_class_keeps_its_imports_indentation_in_a_template() {
        // A `use` inside an `@php` block is indented to the block, and
        // the import is rewritten as a whole statement rather than name
        // by name (an alias can appear or disappear).  Taking the whole
        // line along with it flattened the import against the margin.
        let template = concat!(
            "<div>\n",
            "@php\n",
            "    use App\\Old\\Widget;\n",
            "    $widget = new Widget();\n",
            "@endphp\n",
            "</div>\n",
        );
        let dir = project(&[
            (
                "src/Old/Widget.php",
                "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
            ),
            ("resources/views/panel.blade.php", template),
        ]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\Domain\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");

        let result = std::fs::read_to_string(dir.path().join("resources/views/panel.blade.php"))
            .expect("template");
        assert!(
            result.contains("    use App\\Domain\\Widget;"),
            "the import has to follow the move, indentation and all:\n{result}"
        );
    }

    #[tokio::test]
    async fn reports_the_old_name_left_behind_in_a_template() {
        // A Blade template names the class as a string the rewriter has
        // no way to resolve, so the move cannot take it along.  Silently
        // omitting it from `files_changed` would read as a complete
        // rewrite.
        let dir = project(&[
            (
                "src/Old/Widget.php",
                "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
            ),
            (
                "resources/views/panel.blade.php",
                "@php\n$class = 'App\\Old\\Widget';\n@endphp\n",
            ),
        ]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "App\\Domain\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: true,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("plan");
        let residual = summary
            .warnings
            .iter()
            .find(|warning| warning.file.as_deref() == Some("resources/views/panel.blade.php"))
            .unwrap_or_else(|| panic!("expected a template warning, got {:?}", summary.warnings));
        assert_eq!(residual.line, Some(2));
        assert!(residual.message.contains("App\\Old\\Widget"));
    }

    #[tokio::test]
    async fn reports_the_old_directory_left_behind_in_a_path_string() {
        // `app_path()`-relative and project-relative spellings of the
        // moved directory both have to be reported: neither is a symbol
        // reference, and both break the moment the directory moves.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        )
        .expect("composer");
        for (relative, content) in [
            (
                "app/Elastic/Config/Mapping.php",
                "<?php\nnamespace App\\Elastic\\Config;\n\nclass Mapping {}\n",
            ),
            (
                "config/audit.php",
                "<?php\nreturn [\n    'ilm' => app_path('Elastic/Config/ILM/'),\n    \
                 'map' => base_path('app/Elastic/Config/Mappings.json'),\n];\n",
            ),
        ] {
            let path = dir.path().join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, content).expect("file");
        }
        let options = MoveOptions {
            from: "App\\Elastic\\Config".into(),
            to: "App\\Search\\Config".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: true,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("plan");
        let lines: Vec<Option<usize>> = summary
            .warnings
            .iter()
            .filter(|warning| warning.file.as_deref() == Some("config/audit.php"))
            .map(|warning| warning.line)
            .collect();
        assert_eq!(
            lines,
            vec![Some(3), Some(4)],
            "expected both path spellings, got {:?}",
            summary.warnings
        );
    }

    #[tokio::test]
    async fn a_longer_name_that_merely_starts_the_same_is_not_reported() {
        let dir = project(&[
            (
                "src/Old/Widget.php",
                "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
            ),
            (
                "src/Older/Gadget.php",
                "<?php\nnamespace App\\Older;\n\nclass Gadget {}\n",
            ),
            (
                "notes.md",
                "The `App\\Older` namespace and the `src/Older` directory stay put.\n",
            ),
        ]);
        let options = MoveOptions {
            from: "App\\Old".into(),
            to: "App\\Domain".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: true,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("plan");
        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    }

    #[tokio::test]
    async fn a_class_leaving_the_global_namespace_is_not_reported_as_left_behind() {
        // The old FQN of a global class is a bare short name, and the
        // move keeps the declaration spelled exactly that way.
        let dir = project(&[
            ("legacy/Widget.php", "<?php\n\nclass Widget {}\n"),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\n\nnew \\Widget();\n",
            ),
        ]);
        let options = MoveOptions {
            from: "Widget".into(),
            to: "App\\Casts\\Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: true,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        let summary = run_inner(&options).await.expect("plan");
        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    }

    #[tokio::test]
    async fn a_class_moving_into_the_global_namespace_drops_its_namespace_statement() {
        let dir = project(&[(
            "src/Old/Widget.php",
            "<?php\n\nnamespace App\\Old;\n\nclass Widget {}\n",
        )]);
        let options = MoveOptions {
            from: "App\\Old\\Widget".into(),
            to: "Widget".into(),
            workspace_root: dir.path().to_path_buf(),
            dry_run: false,
            use_colour: false,
            output_format: OutputFormat::Table,
            global_config: None,
        };

        run_inner(&options).await.expect("move");
        let declaration =
            std::fs::read_to_string(dir.path().join("src/Old/Widget.php")).expect("declaration");
        assert_eq!(declaration, "<?php\n\nclass Widget {}\n");
    }
}
