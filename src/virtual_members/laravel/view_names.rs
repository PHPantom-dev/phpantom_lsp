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
    let target_suffixes = [
        format!("/resources/views/{}.blade.php", rel),
        format!("/resources/views/{}.php", rel),
    ];

    let snapshot = backend.user_file_symbol_maps();

    for (file_uri, _) in snapshot {
        if target_suffixes.iter().any(|s| file_uri.ends_with(s))
            && let Ok(uri) = Url::parse(&file_uri)
        {
            results.push(crate::definition::point_location(uri, Position::new(0, 0)));
        }
    }
    results
}
