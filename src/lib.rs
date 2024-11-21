use std::fs;
use std::path::PathBuf;
use zed_extension_api::{self as zed, settings::LspSettings, Result};

struct HarperExtension {
    binary_cache: Option<PathBuf>,
}

#[derive(Clone)]
struct HarperBinary {
    path: PathBuf,
    env: Option<Vec<(String, String)>>,
}

impl HarperExtension {
    fn new() -> Self {
        Self { binary_cache: None }
    }

    fn get_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<HarperBinary> {
        if let Some(path) = worktree.which("harper-ls") {
            return Ok(HarperBinary {
                path: PathBuf::from(path),
                env: Some(worktree.shell_env()),
            });
        }

        if let Some(path) = &self.binary_cache {
            if path.exists() {
                return Ok(HarperBinary {
                    path: path.clone(),
                    env: None,
                });
            }
        }

        self.install_binary(language_server_id)
    }

    fn install_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
    ) -> Result<HarperBinary> {
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            "elijah-potter/harper",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )
        .map_err(|e| format!("Failed to fetch latest release: {}", e))?;

        let (platform, arch) = zed::current_platform();
        let arch_name = match arch {
            zed::Architecture::Aarch64 => "aarch64",
            zed::Architecture::X8664 => "x86_64",
            zed::Architecture::X86 => return Err("x86 architecture is not supported".into()),
        };

        let (os_str, file_ext) = match platform {
            zed::Os::Mac => ("apple-darwin", "tar.gz"),
            zed::Os::Linux => ("unknown-linux-gnu", "tar.gz"),
            zed::Os::Windows => ("pc-windows-msvc", "zip"),
        };

        let asset_name = format!("harper-ls-{arch_name}-{os_str}.{file_ext}");
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| format!("No compatible Harper binary found for {arch_name}-{os_str}"))?;

        let version_dir = format!("harper-ls-{}", release.version);
        let mut binary_path = PathBuf::from(&version_dir).join("harper-ls");

        if platform == zed::Os::Windows {
            binary_path.set_extension("exe");
        }

        if !binary_path.exists() {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            let download_result = (|| -> Result<()> {
                zed::download_file(
                    &asset.download_url,
                    &version_dir,
                    if platform == zed::Os::Windows {
                        zed::DownloadedFileType::Zip
                    } else {
                        zed::DownloadedFileType::GzipTar
                    },
                )
                .map_err(|e| format!("Failed to download Harper binary: {}", e))?;

                zed::make_file_executable(binary_path.to_str().ok_or("Invalid binary path")?)
                    .map_err(|e| format!("Failed to make binary executable: {}", e))?;

                Ok(())
            })();

            if let Err(e) = download_result {
                fs::remove_dir_all(&version_dir).ok();
                return Err(e);
            }

            if let Ok(entries) = fs::read_dir(".") {
                for entry in entries.flatten() {
                    if let Ok(name) = entry.file_name().into_string() {
                        if name != version_dir {
                            fs::remove_dir_all(entry.path()).ok();
                        }
                    }
                }
            }
        }

        self.binary_cache = Some(binary_path.clone());
        Ok(HarperBinary {
            path: binary_path,
            env: None,
        })
    }
}

impl zed::Extension for HarperExtension {
    fn new() -> Self {
        Self::new()
    }

    fn language_server_initialization_options(
        &mut self,
        server_id: &zed_extension_api::LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.initialization_options.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = self.get_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            command: binary
                .path
                .to_str()
                .ok_or("Failed to convert binary path to string")?
                .to_string(),
            args: vec!["--stdio".to_string()],
            env: binary.env.unwrap_or_default(),
        })
    }

    fn language_server_workspace_configuration(
        &mut self,
        server_id: &zed_extension_api::LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.settings.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }
}

zed::register_extension!(HarperExtension);
