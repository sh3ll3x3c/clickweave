use crate::protocol::*;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, info};

pub struct McpClient {
    process: Child,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    request_id: AtomicU64,
    tools: Vec<Tool>,
}

impl McpClient {
    /// Spawn native-devtools-mcp and initialize the connection
    pub fn spawn(command: &str, args: &[&str]) -> Result<Self> {
        info!("Spawning MCP server: {} {:?}", command, args);

        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn MCP server")?;

        let stdin = process.stdin.take().ok_or_else(|| anyhow!("No stdin"))?;
        let stdout = process.stdout.take().ok_or_else(|| anyhow!("No stdout"))?;

        let mut client = Self {
            process,
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            request_id: AtomicU64::new(1),
            tools: Vec::new(),
        };

        client.initialize()?;
        client.fetch_tools()?;

        Ok(client)
    }

    /// Spawn using npx (cross-platform default)
    pub fn spawn_npx() -> Result<Self> {
        Self::spawn("npx", &["-y", "native-devtools-mcp"])
    }

    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    fn send_request(&self, method: &str, params: Option<Value>) -> Result<JsonRpcResponse> {
        let id = self.next_id();
        let request = JsonRpcRequest::new(id, method, params);
        let request_json = serde_json::to_string(&request)?;

        debug!("MCP request: {}", request_json);

        {
            let mut stdin = self.stdin.lock().unwrap();
            writeln!(stdin, "{}", request_json)?;
            stdin.flush()?;
        }

        let response_line = {
            let mut stdout = self.stdout.lock().unwrap();
            let mut line = String::new();
            stdout.read_line(&mut line)?;
            line
        };

        debug!("MCP response: {}", response_line.trim());

        let response: JsonRpcResponse =
            serde_json::from_str(&response_line).context("Failed to parse MCP response")?;

        if let Some(err) = &response.error {
            error!("MCP error: {} (code {})", err.message, err.code);
        }

        Ok(response)
    }

    fn initialize(&mut self) -> Result<()> {
        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: ClientInfo {
                name: "clickweave".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let response = self.send_request("initialize", Some(serde_json::to_value(params)?))?;

        if let Some(result) = response.result {
            let init_result: InitializeResult = serde_json::from_value(result)?;
            info!(
                "MCP initialized: protocol={}, server={:?}",
                init_result.protocol_version,
                init_result.server_info.as_ref().map(|s| &s.name)
            );
        }

        // Send initialized notification
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        {
            let mut stdin = self.stdin.lock().unwrap();
            writeln!(stdin, "{}", serde_json::to_string(&notification)?)?;
            stdin.flush()?;
        }

        Ok(())
    }

    fn fetch_tools(&mut self) -> Result<()> {
        let response = self.send_request("tools/list", None)?;

        if let Some(result) = response.result {
            let tools_result: ToolsListResult = serde_json::from_value(result)?;
            info!("Loaded {} MCP tools", tools_result.tools.len());
            for tool in &tools_result.tools {
                debug!("  - {}: {:?}", tool.name, tool.description);
            }
            self.tools = tools_result.tools;
        }

        Ok(())
    }

    /// Get available tools
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// Call a tool by name with arguments
    pub fn call_tool(&self, name: &str, arguments: Option<Value>) -> Result<ToolCallResult> {
        let params = ToolCallParams {
            name: name.to_string(),
            arguments,
        };

        let response = self.send_request("tools/call", Some(serde_json::to_value(params)?))?;

        if let Some(err) = response.error {
            return Err(anyhow!("Tool call failed: {}", err.message));
        }

        let result = response
            .result
            .ok_or_else(|| anyhow!("No result from tool call"))?;

        let tool_result: ToolCallResult = serde_json::from_value(result)?;
        Ok(tool_result)
    }

    /// Convert MCP tools to OpenAI-compatible tool format
    pub fn tools_as_openai(&self) -> Vec<HashMap<String, Value>> {
        self.tools
            .iter()
            .map(|tool| {
                let mut map = HashMap::new();
                map.insert("type".to_string(), Value::String("function".to_string()));
                map.insert(
                    "function".to_string(),
                    serde_json::json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema
                    }),
                );
                map
            })
            .collect()
    }

    /// Check if process is still running
    pub fn is_running(&mut self) -> bool {
        matches!(self.process.try_wait(), Ok(None))
    }

    /// Kill the MCP server process
    pub fn kill(&mut self) -> Result<()> {
        self.process.kill().context("Failed to kill MCP server")?;
        Ok(())
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}
