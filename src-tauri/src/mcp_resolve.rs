/// Resolve the path to the native-devtools-mcp binary as a UTF-8 string.
///
/// In debug builds, checks `CLICKWEAVE_MCP_BINARY` env var first.
/// Otherwise resolves relative to the current executable (where Tauri
/// places sidecar binaries: `Contents/MacOS/` on macOS, install dir on Windows).
pub fn resolve_mcp_binary() -> anyhow::Result<String> {
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("CLICKWEAVE_MCP_BINARY") {
        if std::path::Path::new(&path).exists() {
            tracing::info!("Using MCP binary from env: {}", path);
            return Ok(path);
        }
        tracing::warn!(
            "CLICKWEAVE_MCP_BINARY='{}' not found, falling back to sidecar",
            path
        );
    }

    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine executable directory"))?
        .to_path_buf();

    let binary_name = if cfg!(target_os = "windows") {
        "native-devtools-mcp.exe"
    } else {
        "native-devtools-mcp"
    };

    let sidecar_path = exe_dir.join(binary_name);
    let path_str = sidecar_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("MCP binary path is not valid UTF-8"))?
        .to_string();

    tracing::info!("Using sidecar MCP binary: {}", path_str);
    Ok(path_str)
}
