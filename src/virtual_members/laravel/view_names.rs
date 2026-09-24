use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;

/// The canonical spelling of a view name.
///
/// Laravel's `FileViewFinder` turns `.` into a directory separator and
/// leaves `/` alone, so `partials/card`, `partials.card` and
/// `/partials/card` all name one template.  Canonicalising to the dotted
/// form means every consumer (diagnostics, references, go-to-definition,
/// Blade call-site inference) compares the same spelling.
pub(crate) fn canonical_view_name(name: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    let (namespace, view) = match name.split_once("::") {
        Some((namespace, view)) => (Some(namespace), view),
        None => (None, name),
    };
    if !view.contains('/') && !view.starts_with('.') {
        return Cow::Borrowed(name);
    }
    let normalised = view.trim_start_matches(['/', '.']).replace('/', ".");
    Cow::Owned(match namespace {
        Some(namespace) => format!("{namespace}::{normalised}"),
        None => normalised,
    })
}

/// Resolve `view('name')` or `View::make('name')` to the corresponding blade templates.
///
/// Converts dot-notation to a file path under `resources/views/`:
/// `'components.button'` → `resources/views/components/button.blade.php`
pub(crate) fn resolve_view_definitions(backend: &Backend, name: &str) -> Vec<Location> {
    let mut results = Vec::new();
    let name = &*canonical_view_name(name);

    // The discovery index already holds the file each name renders, so the
    // common case answers without touching the filesystem.  A miss still
    // probes below: a template created outside the editor is on disk before
    // any edit invalidates the index.
    if let Some(path) = backend.blade_view_path(name)
        && path.is_file()
        && let Ok(uri) = Url::from_file_path(&path)
    {
        return vec![crate::definition::point_location(uri, Position::new(0, 0))];
    }

    if let Some((namespace, view_name)) = name.split_once("::") {
        let rel = view_name.replace('.', "/");
        for res in &backend.laravel_provider_resources.read().view_dirs {
            if res.namespace != namespace {
                continue;
            }
            for suffix in &[".blade.php", ".php"] {
                let candidate = res.path.join(format!("{rel}{suffix}"));
                if candidate.is_file()
                    && let Ok(uri) = Url::from_file_path(&candidate)
                {
                    results.push(crate::definition::point_location(uri, Position::new(0, 0)));
                }
            }
        }
        return results;
    }

    let rel = name.replace('.', "/");

    // Check each configured view root (from `config/view.php`, falling
    // back to `resources/views`). Laravel resolves against these paths
    // in order, so the first existing candidate is the file that would
    // actually be rendered.
    for root in backend.laravel_view_roots().iter() {
        for suffix in &[".blade.php", ".php"] {
            let candidate = root.path.join(format!("{rel}{suffix}"));
            if candidate.is_file()
                && let Ok(uri) = Url::from_file_path(&candidate)
            {
                results.push(crate::definition::point_location(uri, Position::new(0, 0)));
            }
        }
    }
    results
}
