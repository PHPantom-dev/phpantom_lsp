//! Which package a file belongs to, and so where a symbol a completion
//! offers comes from.

use std::path::Path;

use crate::Backend;

/// Where a class, function or constant comes from, which decides how
/// completion ranks it: the project's own code first, then the standard
/// library, then the packages the project requires, then their
/// dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassCompletionOrigin {
    #[default]
    Project,
    CoreStub,
    VendorExplicit,
    VendorTransitive,
}

impl ClassCompletionOrigin {
    pub(crate) fn sort_tier(self) -> char {
        match self {
            Self::Project => '0',
            Self::CoreStub => '1',
            Self::VendorExplicit => '2',
            Self::VendorTransitive => '3',
        }
    }
}

impl Backend {
    pub(crate) fn completion_origin_for_uri(&self, uri: &str) -> ClassCompletionOrigin {
        self.package_info_for_uri(uri).0
    }

    /// Return the completion origin **and** the Composer package name
    /// (e.g. `"laravel/framework"`) for the given file path.
    ///
    /// Returns `(ClassCompletionOrigin::Project, None)` for project files,
    /// `(ClassCompletionOrigin::CoreStub, None)` for stubs, and
    /// `(origin, Some(package_name))` for vendor files.
    pub(crate) fn package_info_for_path(
        &self,
        path: &Path,
    ) -> (ClassCompletionOrigin, Option<String>) {
        let vendor_paths = self.workspace.vendor_dir_paths.lock();
        let roots = self.workspace.vendor_package_origin_roots.read();

        if vendor_paths.iter().any(|vp| path.starts_with(vp)) {
            return match crate::indexing::vendor_package_root_for_path(path, &vendor_paths, &roots)
            {
                Some((_, origin, pkg_name)) => (*origin, Some(pkg_name.clone())),
                None => (ClassCompletionOrigin::VendorTransitive, None),
            };
        }
        drop(vendor_paths);

        // Path is outside vendor/.  This is usually project code, but
        // it can also be a symlinked path-repository package whose
        // canonical path resolved outside vendor/.  Check whether any
        // vendor package root matches.  If so, treat it as project code
        // only when the root is inside the workspace (a local module);
        // otherwise show the package provenance.
        for (root, origin, pkg_name) in roots.iter() {
            if path.starts_with(root) {
                let ws = self.workspace.workspace_root.read();
                if let Some(ref workspace) = *ws {
                    let canonical_ws = workspace
                        .canonicalize()
                        .unwrap_or_else(|_| workspace.clone());
                    if root.starts_with(&canonical_ws) {
                        return (ClassCompletionOrigin::Project, None);
                    }
                }
                return (*origin, Some(pkg_name.clone()));
            }
        }

        (ClassCompletionOrigin::Project, None)
    }

    /// Return the completion origin **and** the Composer package name
    /// for the given file URI.
    pub(crate) fn package_info_for_uri(
        &self,
        uri: &str,
    ) -> (ClassCompletionOrigin, Option<String>) {
        if crate::stubs::is_embedded_stub_uri(uri) {
            return (ClassCompletionOrigin::CoreStub, None);
        }
        if let Some(url) = crate::util::parse_url(uri)
            && let Some(path) = crate::util::url_to_path(&url)
        {
            return self.package_info_for_path(&path);
        }
        (ClassCompletionOrigin::Project, None)
    }
}
