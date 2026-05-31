use std::path::{Path, PathBuf};

use clickweave_mcp::McpClient;

/// Controls when `CLICKWEAVE_MCP_BINARY` is honoured.
///
/// - `Always` — CLI path: always honour the env var so developers can
///   point at a local build without recompiling.
/// - `DebugOnly` — Tauri path: honour the env var only in debug builds
///   so packaged release builds ignore it and always use the bundled sidecar,
///   preserving packaging integrity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvOverride {
    Always,
    DebugOnly,
}

/// Pure helper — decides whether `CLICKWEAVE_MCP_BINARY` should be
/// consulted given the override policy and the current build mode.
///
/// Separated from `resolve_mcp_binary` so it can be unit-tested without
/// toggling `#[cfg(debug_assertions)]` (which requires two compilations).
pub fn should_honor_env(env_override: EnvOverride, debug_assertions: bool) -> bool {
    match env_override {
        EnvOverride::Always => true,
        EnvOverride::DebugOnly => debug_assertions,
    }
}

/// Resolve the path to the native-devtools-mcp binary as a UTF-8 string.
///
/// When `env_override` is `Always`, or when it is `DebugOnly` and the binary
/// was compiled in debug mode, the `CLICKWEAVE_MCP_BINARY` environment variable
/// is checked first. Otherwise the sidecar placed beside the current executable
/// is used.
pub fn resolve_mcp_binary(env_override: EnvOverride) -> anyhow::Result<String> {
    if should_honor_env(env_override, cfg!(debug_assertions))
        && let Ok(path) = std::env::var("CLICKWEAVE_MCP_BINARY")
    {
        match validated_mcp_binary_path(Path::new(&path)) {
            Ok(p) => {
                tracing::info!("Using MCP binary from env: {}", p);
                return Ok(p);
            }
            Err(_) => {
                tracing::warn!(
                    "CLICKWEAVE_MCP_BINARY='{}' not found, falling back to sidecar",
                    path
                );
            }
        }
    }

    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine executable directory"))?
        .to_path_buf();

    let sidecar = sidecar_path(&exe_dir);
    let path_str = validated_mcp_binary_path(&sidecar)?;

    tracing::info!("Using sidecar MCP binary: {}", path_str);
    Ok(path_str)
}

/// Spawn an MCP subprocess and return the connected client.
pub async fn spawn_mcp(command: &str, args: &[&str]) -> anyhow::Result<McpClient> {
    McpClient::spawn(command, args).await
}

pub(crate) fn sidecar_path(exe_dir: &Path) -> PathBuf {
    let binary_name = if cfg!(target_os = "windows") {
        "native-devtools-mcp.exe"
    } else {
        "native-devtools-mcp"
    };
    exe_dir.join(binary_name)
}

pub(crate) fn validated_mcp_binary_path(path: &Path) -> anyhow::Result<String> {
    if !path.is_file() {
        anyhow::bail!("MCP binary not found at {}", path.display());
    }
    Ok(path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("MCP binary path is not valid UTF-8"))?
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Pure helper tests (full matrix, no recompilation needed) ---

    #[test]
    fn should_honor_env_always_true_regardless_of_debug() {
        assert!(should_honor_env(EnvOverride::Always, true));
        assert!(should_honor_env(EnvOverride::Always, false));
    }

    #[test]
    fn should_honor_env_debug_only_true_when_debug() {
        assert!(should_honor_env(EnvOverride::DebugOnly, true));
    }

    #[test]
    fn should_honor_env_debug_only_false_when_release() {
        assert!(!should_honor_env(EnvOverride::DebugOnly, false));
    }

    // --- Ported tests from the Tauri shell's mcp_resolve module ---

    #[test]
    fn sidecar_path_requires_existing_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let err = validated_mcp_binary_path(&sidecar_path(tmp.path())).unwrap_err();
        assert!(err.to_string().contains("MCP binary not found"));
    }

    #[test]
    fn sidecar_path_accepts_existing_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let path = sidecar_path(tmp.path());
        std::fs::write(&path, b"binary").unwrap();
        let result = validated_mcp_binary_path(&path).unwrap();
        assert_eq!(result, path.to_str().unwrap());
    }
}
