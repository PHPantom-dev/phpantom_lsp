use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;

/// Resolve `view('name')` or `View::make('name')` to the corresponding blade templates.
///
/// Converts dot-notation to a file path under `resources/views/`:
/// `'components.button'` → `resources/views/components/button.blade.php`
pub(crate) fn resolve_view_definitions(backend: &Backend, name: &str) -> Vec<Location> {
    let mut results = Vec::new();

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
    for root in backend.laravel_view_roots() {
        for suffix in &[".blade.php", ".php"] {
            let candidate = root.join(format!("{rel}{suffix}"));
            if candidate.is_file()
                && let Ok(uri) = Url::from_file_path(&candidate)
            {
                results.push(crate::definition::point_location(uri, Position::new(0, 0)));
            }
        }
    }
    results
}
