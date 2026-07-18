use std::fs;
use zed_extension_api::{self as zed, LanguageServerId, Result};

struct PHPantomExtension {
    cached_binary_path: Option<String>,
}

impl PHPantomExtension {
    fn language_server_binary_path(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        // First, check if the user has phpantom_lsp on their PATH
        if let Some(path) = worktree.which("phpantom_lsp") {
            return Ok(path);
        }

        // Check cached path
        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|stat| stat.is_file()) {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            "PHPantom-dev/phpantom_lsp",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (platform, arch) = zed::current_platform();

        let (os_str, ext) = match platform {
            zed::Os::Mac => ("apple-darwin", "tar.gz"),
            zed::Os::Linux => ("unknown-linux-gnu", "tar.gz"),
            zed::Os::Windows => ("pc-windows-msvc", "zip"),
        };

        let arch_str = match arch {
            zed::Architecture::Aarch64 => "aarch64",
            zed::Architecture::X8664 => "x86_64",
            _ => return Err(format!("unsupported architecture: {arch:?}")),
        };

        let asset_name = format!("phpantom_lsp-{arch_str}-{os_str}.{ext}");

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!("no release asset found matching {asset_name:?} — you may need to build phpantom_lsp from source for your platform")
            })?;

        let version_dir = format!("phpantom_lsp-{}", release.version);
        fs::create_dir_all(&version_dir).map_err(|e| format!("failed to create directory: {e}"))?;

        let binary_name = match platform {
            zed::Os::Windows => "phpantom_lsp.exe",
            _ => "phpantom_lsp",
        };
        let binary_path = format!("{version_dir}/{binary_name}");

        if !fs::metadata(&binary_path).is_ok_and(|stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            let file_type = match ext {
                "tar.gz" => zed::DownloadedFileType::GzipTar,
                "zip" => zed::DownloadedFileType::Zip,
                _ => unreachable!(),
            };

            zed::download_file(&asset.download_url, &version_dir, file_type)
                .map_err(|e| format!("failed to download file: {e}"))?;

            zed::make_file_executable(&binary_path)?;

            // Clean up old versions
            let entries =
                fs::read_dir(".").map_err(|e| format!("failed to list working directory: {e}"))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("failed to load directory entry: {e}"))?;
                if entry.file_name().to_str() != Some(&version_dir) {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }

        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

impl zed::Extension for PHPantomExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary_path = self.language_server_binary_path(language_server_id, worktree)?;

        Ok(zed::Command {
            command: binary_path,
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(PHPantomExtension);
