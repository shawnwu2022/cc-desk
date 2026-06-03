//! MCP 协议模块
//!
//! 通过 MCP 协议获取服务器详情（tools、prompts、resources）
//! 支持 HTTP/SSE 和 stdio 类型

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

// ==================== 数据结构 ====================

/// MCP Server 详细信息（通过协议获取）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDetail {
    pub name: String,
    pub server_info: Option<ServerInfo>,
    pub capabilities: Option<ServerCapabilities>,
    pub tools: Vec<McpToolInfo>,
    pub prompts: Vec<McpPromptInfo>,
    pub resources: Vec<McpResourceInfo>,
    pub cached_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub tools: bool,
    pub prompts: bool,
    pub resources: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptInfo {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

// ==================== JSON-RPC 结构 ====================

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct JsonRpcResponse {
    pub(crate) jsonrpc: String,
    pub(crate) id: u64,
    pub(crate) result: Option<serde_json::Value>,
    pub(crate) error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcError {
    code: i32,
    message: String,
}

// ==================== MCP HTTP 客户端 ====================

pub struct McpHttpClient {
    url: String,
    headers: Option<HashMap<String, String>>,
    request_id: u64,
}

impl McpHttpClient {
    pub fn new(url: String, headers: Option<HashMap<String, String>>) -> Self {
        Self {
            url,
            headers,
            request_id: 0,
        }
    }

    fn next_id(&mut self) -> u64 {
        self.request_id += 1;
        self.request_id
    }

    /// 获取服务器详情
    pub async fn get_server_detail(&mut self) -> Result<McpServerDetail, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;

        // 1. Initialize
        let init_response = self
            .send_request(
                &client,
                "initialize",
                Some(serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "Claude-Tauri-GUI",
                        "version": "1.0.0"
                    }
                })),
            )
            .await?;

        // 解析 initialize 结果
        let init_result = init_response.result.unwrap_or(serde_json::json!({}));
        let server_info = parse_server_info(&init_result);
        let capabilities = parse_capabilities(&init_result);

        // 2. 发送 initialized 通知（无响应）
        self.send_notification(&client, "notifications/initialized")
            .await?;

        // 3. 列出工具
        let tools = if capabilities.tools {
            let tools_response = self.send_request(&client, "tools/list", None).await?;
            parse_tools(&tools_response)
        } else {
            vec![]
        };

        // 4. 列出 prompts
        let prompts = if capabilities.prompts {
            let prompts_response = self.send_request(&client, "prompts/list", None).await?;
            parse_prompts(&prompts_response)
        } else {
            vec![]
        };

        // 5. 列出 resources
        let resources = if capabilities.resources {
            let resources_response = self.send_request(&client, "resources/list", None).await?;
            parse_resources(&resources_response)
        } else {
            vec![]
        };

        Ok(McpServerDetail {
            name: self.url.clone(),
            server_info,
            capabilities: Some(capabilities),
            tools,
            prompts,
            resources,
            cached_at: Some(chrono::Utc::now().timestamp()),
        })
    }

    async fn send_request(
        &mut self,
        client: &reqwest::Client,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id(),
            method: method.to_string(),
            params,
        };

        let mut req = client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json,text/event-stream")
            .header("MCP-Protocol-Version", "2024-11-05")
            .json(&request);

        // 添加自定义 headers
        if let Some(headers) = &self.headers {
            for (key, value) in headers {
                req = req.header(key, value);
            }
        }

        let response = req
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("HTTP error {}: {}", status, body));
        }

        // 解析 SSE 格式响应或 JSON 格式
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if content_type.contains("text/event-stream") {
            // SSE 格式：解析 data: 行
            let text = response.text().await.map_err(|e| e.to_string())?;
            parse_sse_response(&text)
        } else {
            // JSON 格式
            let json: JsonRpcResponse = response
                .json()
                .await
                .map_err(|e| format!("JSON parse error: {}", e))?;

            if let Some(error) = json.error {
                return Err(format!("RPC error {}: {}", error.code, error.message));
            }

            Ok(json)
        }
    }

    async fn send_notification(
        &self,
        client: &reqwest::Client,
        method: &str,
    ) -> Result<(), String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 0, // notifications 不需要 id
            method: method.to_string(),
            params: None,
        };

        let mut req = client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json,text/event-stream")
            .header("MCP-Protocol-Version", "2024-11-05")
            .json(&request);

        // 添加自定义 headers
        if let Some(headers) = &self.headers {
            for (key, value) in headers {
                req = req.header(key, value);
            }
        }

        req.send()
            .await
            .map_err(|e| format!("Notification failed: {}", e))?;

        Ok(())
    }
}

// ==================== 解析函数 ====================

/// 解析 SSE 格式响应
pub(crate) fn parse_sse_response(text: &str) -> Result<JsonRpcResponse, String> {
    // SSE 格式：
    // id:1
    // event:message
    // data:{"jsonrpc":"2.0","id":1,"result":{...}}
    for line in text.lines() {
        if line.starts_with("data:") {
            let json_str = line.trim_start_matches("data:").trim();
            let json: JsonRpcResponse = serde_json::from_str(json_str)
                .map_err(|e| format!("JSON parse error in SSE: {} (line: {})", e, json_str))?;

            if let Some(error) = json.error {
                return Err(format!("RPC error {}: {}", error.code, error.message));
            }

            return Ok(json);
        }
    }

    Err("No data line found in SSE response".to_string())
}

pub(crate) fn parse_server_info(result: &serde_json::Value) -> Option<ServerInfo> {
    result.get("serverInfo").and_then(|info| {
        Some(ServerInfo {
            name: info
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            version: info
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    })
}

pub(crate) fn parse_capabilities(result: &serde_json::Value) -> ServerCapabilities {
    let empty = serde_json::json!({});
    let caps = result.get("capabilities").unwrap_or(&empty);
    ServerCapabilities {
        tools: caps.get("tools").is_some(),
        prompts: caps.get("prompts").is_some(),
        resources: caps.get("resources").is_some(),
    }
}

pub(crate) fn parse_tools(response: &JsonRpcResponse) -> Vec<McpToolInfo> {
    response
        .result
        .as_ref()
        .and_then(|r| r.get("tools"))
        .and_then(|tools| tools.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    Some(McpToolInfo {
                        name: t
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: t
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        input_schema: t.get("inputSchema").cloned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_prompts(response: &JsonRpcResponse) -> Vec<McpPromptInfo> {
    response
        .result
        .as_ref()
        .and_then(|r| r.get("prompts"))
        .and_then(|prompts| prompts.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some(McpPromptInfo {
                        name: p
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: p
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        arguments: p
                            .get("arguments")
                            .and_then(|args| args.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|a| {
                                        Some(PromptArgument {
                                            name: a
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            description: a
                                                .get("description")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string()),
                                            required: a
                                                .get("required")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false),
                                        })
                                    })
                                    .collect()
                            }),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_resources(response: &JsonRpcResponse) -> Vec<McpResourceInfo> {
    response
        .result
        .as_ref()
        .and_then(|r| r.get("resources"))
        .and_then(|resources| resources.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    Some(McpResourceInfo {
                        uri: r
                            .get("uri")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        name: r
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: r
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        mime_type: r
                            .get("mimeType")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ==================== 缓存管理 ====================

type CacheKey = String; // server_name or URL
type CacheValue = McpServerDetail;

/// MCP Detail 缓存（永久缓存，除非强制刷新）
pub struct McpDetailCache {
    cache: Arc<Mutex<HashMap<CacheKey, CacheValue>>>,
}

impl McpDetailCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 获取缓存（不检查时间，永久有效）
    pub async fn get(&self, key: &str) -> Option<McpServerDetail> {
        let cache = self.cache.lock().await;
        cache.get(key).cloned()
    }

    /// 设置缓存
    pub async fn set(&self, key: String, detail: McpServerDetail) {
        let mut cache = self.cache.lock().await;
        cache.insert(key, detail);
    }
}

// 全局缓存实例
static MCP_CACHE: once_cell::sync::Lazy<McpDetailCache> =
    once_cell::sync::Lazy::new(McpDetailCache::new);

pub fn get_mcp_cache() -> &'static McpDetailCache {
    &MCP_CACHE
}

// ==================== MCP Stdio 客户端 ====================

pub struct McpStdioClient {
    command: String,
    args: Vec<String>,
    request_id: u64,
}

impl McpStdioClient {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            request_id: 0,
        }
    }

    fn next_id(&mut self) -> u64 {
        self.request_id += 1;
        self.request_id
    }

    /// 获取服务器详情（通过 stdin/stdout 通信）
    pub fn get_server_detail(&mut self) -> Result<McpServerDetail, String> {
        // 启动进程
        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            crate::platform::configure_command(&mut cmd);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {}", e))?;

        let stdin = child.stdin.take().ok_or("Failed to get stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut stdin = stdin;

        // 1. Initialize
        let init_response = self.send_request(
            &mut stdin,
            &mut reader,
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {
                    "name": "Claude-Tauri-GUI",
                    "version": "1.0.0"
                }
            })),
        )?;

        let init_result = init_response.result.unwrap_or(serde_json::json!({}));
        let server_info = parse_server_info(&init_result);
        let capabilities = parse_capabilities(&init_result);

        // 2. initialized 通知
        self.send_notification(&mut stdin)?;

        // 3. tools/list
        let tools = if capabilities.tools {
            let tools_response = self.send_request(&mut stdin, &mut reader, "tools/list", None)?;
            parse_tools(&tools_response)
        } else {
            vec![]
        };

        // 4. prompts/list
        let prompts = if capabilities.prompts {
            let prompts_response =
                self.send_request(&mut stdin, &mut reader, "prompts/list", None)?;
            parse_prompts(&prompts_response)
        } else {
            vec![]
        };

        // 5. resources/list
        let resources = if capabilities.resources {
            let resources_response =
                self.send_request(&mut stdin, &mut reader, "resources/list", None)?;
            parse_resources(&resources_response)
        } else {
            vec![]
        };

        // 关闭进程
        child.kill().ok();

        Ok(McpServerDetail {
            name: self.command.clone(),
            server_info,
            capabilities: Some(capabilities),
            tools,
            prompts,
            resources,
            cached_at: Some(chrono::Utc::now().timestamp()),
        })
    }

    fn send_request<R: BufRead>(
        &mut self,
        stdin: &mut std::process::ChildStdin,
        reader: &mut R,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id(),
            method: method.to_string(),
            params,
        };

        let json_str = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        stdin
            .write_all(json_str.as_bytes())
            .map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;

        // 读取响应
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .map_err(|e| e.to_string())?;

        let json: JsonRpcResponse = serde_json::from_str(&response_line.trim())
            .map_err(|e| format!("JSON parse error: {} (line: {})", e, response_line))?;

        if let Some(error) = json.error {
            return Err(format!("RPC error {}: {}", error.code, error.message));
        }

        Ok(json)
    }

    fn send_notification(&self, stdin: &mut std::process::ChildStdin) -> Result<(), String> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        let json_str = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        stdin
            .write_all(json_str.as_bytes())
            .map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;

        Ok(())
    }
}

// ==================== 获取 MCP Server 详情 ====================

/// 通过 URL 获取 HTTP MCP Server 详情
pub async fn fetch_http_mcp_detail(
    url: &str,
    headers: Option<&HashMap<String, String>>,
) -> Result<McpServerDetail, String> {
    let mut client = McpHttpClient::new(url.to_string(), headers.map(|h| h.clone()));
    client.get_server_detail().await
}

/// 通过命令获取 stdio MCP Server 详情
pub fn fetch_stdio_mcp_detail(command: &str) -> Result<McpServerDetail, String> {
    // 解析命令和参数
    let parts: Vec<String> = command.split_whitespace().map(|s| s.to_string()).collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }

    let cmd = parts[0].clone();
    let args = parts[1..].to_vec();

    let mut client = McpStdioClient::new(cmd, args);
    client.get_server_detail()
}

/// 获取 MCP Server 详情（带缓存）
pub async fn get_mcp_server_detail_cached(
    server_name: &str,
    url: Option<&str>,
    command: Option<&str>,
    headers: Option<&HashMap<String, String>>,
    force_refresh: bool,
) -> Result<Option<McpServerDetail>, String> {
    let cache = get_mcp_cache();

    // 检查缓存
    if !force_refresh {
        if let Some(detail) = cache.get(server_name).await {
            return Ok(Some(detail));
        }
    }

    // HTTP/SSE server
    if let Some(url) = url {
        if url.starts_with("http://") || url.starts_with("https://") {
            let detail = fetch_http_mcp_detail(url, headers).await?;
            cache.set(server_name.to_string(), detail.clone()).await;
            return Ok(Some(detail));
        }
    }

    // stdio server
    if let Some(command) = command {
        let command_str = command.to_string();
        // 在 tokio runtime 中执行同步操作
        let detail = tokio::task::spawn_blocking(move || fetch_stdio_mcp_detail(&command_str))
            .await
            .map_err(|e| e.to_string())??;

        cache.set(server_name.to_string(), detail.clone()).await;
        return Ok(Some(detail));
    }

    Ok(None)
}

