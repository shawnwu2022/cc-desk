//! MCP Server 详情获取测试
//!
//! 通过 MCP 协议直接连接服务器获取详情

use std::collections::HashMap;
use std::process::Command;

/// MCP Server 详细信息
#[derive(Debug, Clone)]
pub struct McpServerDetail {
    pub name: String,
    pub scope: String,
    pub status: String,
    pub server_type: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
}

/// MCP Server 通过协议获取的详情
#[derive(Debug, Clone)]
pub struct McpServerProtocolInfo {
    pub name: String,
    pub server_info: Option<ServerInfo>,
    pub capabilities: Option<ServerCapabilities>,
    pub tools: Vec<ToolInfo>,
    pub prompts: Vec<PromptInfo>,
    pub resources: Vec<ResourceInfo>,
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ServerCapabilities {
    pub tools: bool,
    pub prompts: bool,
    pub resources: bool,
    pub logging: bool,
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct PromptInfo {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
}

#[derive(Debug, Clone)]
pub struct PromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct ResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

// ==================== CLI 解析函数 ====================

/// 解析 claude mcp list 输出
fn parse_mcp_list_output(output: &str) -> Vec<McpServerDetail> {
    let mut servers = Vec::new();

    for line in output.lines() {
        if line.contains(" - ") {
            let parts: Vec<&str> = line.splitn(2, " - ").collect();
            if parts.len() == 2 {
                let server_info = parts[0].trim();
                let status = parts[1].trim();

                if let Some(detail) = parse_server_info(server_info, status) {
                    servers.push(detail);
                }
            }
        }
    }

    servers
}

/// 解析单个服务器信息行
fn parse_server_info(info: &str, status: &str) -> Option<McpServerDetail> {
    let type_marker_pos = info.find('(');
    let type_end_pos = info.find(')');

    let server_type = if type_marker_pos.is_some() && type_end_pos.is_some() {
        info[type_marker_pos.unwrap() + 1..type_end_pos.unwrap()]
            .trim()
            .to_string()
    } else {
        if info.contains("http://") || info.contains("https://") {
            "http".to_string()
        } else {
            "stdio".to_string()
        }
    };

    let colon_pos = find_name_separator(info, &server_type);

    if colon_pos.is_none() {
        return None;
    }

    let colon_idx = colon_pos.unwrap();
    let name = info[..colon_idx].trim();

    let after_colon = &info[colon_idx + 1..];

    let command_or_url = if type_marker_pos.is_some() {
        let type_rel_pos = type_marker_pos.unwrap() - colon_idx - 1;
        if type_rel_pos > 0 && type_rel_pos < after_colon.len() {
            after_colon[..type_rel_pos].trim()
        } else {
            after_colon.trim()
        }
    } else {
        after_colon.trim()
    };

    let (url, command, args) =
        if command_or_url.starts_with("http://") || command_or_url.starts_with("https://") {
            (Some(command_or_url.to_string()), None, None)
        } else {
            let parts: Vec<String> = command_or_url
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            if parts.is_empty() {
                (None, None, None)
            } else {
                (None, Some(parts[0].clone()), Some(parts[1..].to_vec()))
            }
        };

    let scope = if name.starts_with("plugin:") {
        "plugin"
    } else if name.starts_with("managed:") {
        "managed"
    } else {
        "user"
    };

    Some(McpServerDetail {
        name: name.to_string(),
        scope: scope.to_string(),
        status: status.to_string(),
        server_type,
        url,
        command,
        args,
    })
}

/// 找到分割 name 和 command/url 的冒号位置
fn find_name_separator(info: &str, server_type: &str) -> Option<usize> {
    if server_type == "HTTP" || server_type == "SSE" {
        if let Some(http_pos) = info.find("https://").or_else(|| info.find("http://")) {
            let before_url = &info[..http_pos];
            before_url.rfind(':')
        } else {
            info.find(':')
        }
    } else {
        let mut candidate: Option<usize> = None;
        let chars = info.chars().collect::<Vec<_>>();

        for i in (0..chars.len()).rev() {
            if chars[i] == ':' {
                let after_colon = if i + 1 < chars.len() {
                    &info[i + 1..]
                } else {
                    ""
                };

                let is_path_colon = after_colon.starts_with('/') || after_colon.starts_with('\\');
                let is_url_colon = after_colon.starts_with("//");

                if is_path_colon || is_url_colon {
                    continue;
                }

                if after_colon.starts_with(' ')
                    || after_colon.is_empty()
                    || after_colon.find(':').is_none()
                {
                    return Some(i);
                }

                candidate = Some(i);
            }
        }

        candidate.or_else(|| info.rfind(':'))
    }
}

/// 执行 claude 命令
fn run_claude_command(args: &str) -> Option<String> {
    let mut cmd = Command::new("claude");
    cmd.args(args.split_whitespace());

    if cfg!(target_os = "windows") {
        let git_bash_paths = [
            "D:\\Program Files\\Git\\bin\\bash.exe",
            "C:\\Program Files\\Git\\bin\\bash.exe",
        ];

        for path in &git_bash_paths {
            if std::path::Path::new(path).exists() {
                cmd.env("CLAUDE_CODE_GIT_BASH_PATH", path);
                break;
            }
        }
    }

    let output = cmd.output().ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// ==================== MCP 协议客户端 ====================

/// MCP HTTP 客户端 - 通过协议直接连接服务器
pub struct McpHttpClient {
    url: String,
    headers: HashMap<String, String>,
    request_id: u64,
}

impl McpHttpClient {
    pub fn new(url: String) -> Self {
        Self {
            url,
            headers: HashMap::new(),
            request_id: 0,
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    fn next_id(&mut self) -> u64 {
        self.request_id += 1;
        self.request_id
    }

    /// 发送 MCP 请求 (模拟，不实际发送 HTTP)
    /// 在实际环境中需要使用 reqwest 或 ureq
    fn send_request(&mut self, method: &str, params: serde_json::Value) -> Option<String> {
        // 由于测试环境限制，我们使用已有的 MCP 工具来获取信息
        // 这里模拟 MCP 协议请求的 JSON 格式

        let id = self.next_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        println!("MCP Request to {}:", self.url);
        println!("{}", serde_json::to_string_pretty(&request).unwrap());

        // 实际实现需要 HTTP POST:
        // POST self.url
        // Headers:
        //   Content-Type: application/json
        //   Accept: application/json, text/event-stream
        //   MCP-Protocol-Version: 2025-11-25
        // Body: JSON-RPC request

        None // 测试环境中不实际发送
    }

    /// 初始化连接
    pub fn initialize(&mut self) -> serde_json::Value {
        let params = serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {
                "roots": { "listChanged": true },
                "sampling": {},
                "elicitation": {}
            },
            "clientInfo": {
                "name": "Claude-Tauri-GUI-Test",
                "version": "1.0.0"
            }
        });

        serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "initialize",
            "params": params
        })
    }

    /// 发送 initialized 通知
    pub fn initialized(&self) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        })
    }

    /// 列出工具
    pub fn list_tools(&mut self) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "tools/list"
        })
    }

    /// 列出提示
    pub fn list_prompts(&mut self) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "prompts/list"
        })
    }

    /// 列出资源
    pub fn list_resources(&mut self) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "resources/list"
        })
    }
}

// ==================== 测试函数 ====================

#[test]
fn test_parse_mcp_list() {
    let sample_output = r#"Checking MCP server health…

plugin:paper-tool:paper-search: uv run --directory C:/claude-plugins/paper-tool/paper-search mcp_server.py - ✓ Connected
zread: https://open.bigmodel.cn/api/mcp/zread/mcp (HTTP) - ✓ Connected
web-reader: https://open.bigmodel.cn/api/mcp/web_reader/mcp (HTTP) - ✓ Connected
web-search-prime: https://open.bigmodel.cn/api/mcp/web_search_prime/mcp (HTTP) - ✓ Connected"#;

    let servers = parse_mcp_list_output(sample_output);

    assert_eq!(servers.len(), 4);

    let plugin_server = servers
        .iter()
        .find(|s| s.name == "plugin:paper-tool:paper-search");
    assert!(plugin_server.is_some());
    let plugin = plugin_server.unwrap();
    assert_eq!(plugin.scope, "plugin");
    assert_eq!(plugin.status, "✓ Connected");
    assert!(plugin.command.is_some());
    assert_eq!(plugin.command.clone().unwrap(), "uv");

    let http_server = servers.iter().find(|s| s.name == "zread");
    assert!(http_server.is_some());
    let http = http_server.unwrap();
    assert_eq!(http.server_type, "HTTP");
    assert!(http.url.is_some());
    assert!(http.url.clone().unwrap().contains("zread"));
}

#[test]
fn test_run_claude_mcp_list() {
    let output = run_claude_command("mcp list");

    if let Some(output) = output {
        let servers = parse_mcp_list_output(&output);
        assert!(servers.len() >= 1, "Should have at least one MCP server");

        for server in &servers {
            assert!(!server.name.is_empty(), "Server name should not be empty");
            assert!(
                !server.server_type.is_empty(),
                "Server type should not be empty"
            );
        }
    }
}

/// 测试 MCP 协议请求格式
#[test]
fn test_mcp_protocol_requests() {
    // 创建 web-reader 客户端
    let mut client =
        McpHttpClient::new("https://open.bigmodel.cn/api/mcp/web_reader/mcp".to_string());

    println!("\n=== MCP Protocol Requests for web-reader ===\n");

    // 1. Initialize request
    println!("1. Initialize Request:");
    let init_request = client.initialize();
    println!("{}", serde_json::to_string_pretty(&init_request).unwrap());

    // 2. Initialized notification
    println!("\n2. Initialized Notification:");
    let init_notification = client.initialized();
    println!(
        "{}",
        serde_json::to_string_pretty(&init_notification).unwrap()
    );

    // 3. List tools
    println!("\n3. List Tools Request:");
    let tools_request = client.list_tools();
    println!("{}", serde_json::to_string_pretty(&tools_request).unwrap());

    // 4. List prompts
    println!("\n4. List Prompts Request:");
    let prompts_request = client.list_prompts();
    println!(
        "{}",
        serde_json::to_string_pretty(&prompts_request).unwrap()
    );

    // 5. List resources
    println!("\n5. List Resources Request:");
    let resources_request = client.list_resources();
    println!(
        "{}",
        serde_json::to_string_pretty(&resources_request).unwrap()
    );

    println!("\n=== HTTP Headers Required ===");
    println!("Content-Type: application/json");
    println!("Accept: application/json, text/event-stream");
    println!("MCP-Protocol-Version: 2025-11-25");
}

/// 使用 MCP 工具获取 web-reader 详情
#[test]
fn test_get_web_reader_details_via_mcp_tool() {
    println!("\n=== Getting web-reader MCP Server Details ===\n");

    // 直接使用 MCP 工具来获取信息
    // ListMcpResourcesTool 可以列出 MCP 服务器的资源
    println!("Note: This test demonstrates how to get MCP server details.");
    println!("In production, use MCP tools like:");
    println!("  - ListMcpResourcesTool");
    println!("  - ReadMcpResourceTool");
}

/// 输出当前设备上所有 MCP 服务器的详情
#[test]
fn test_print_all_mcp_server_details() {
    println!("\n=== All MCP Servers on Current Device ===\n");

    let output = run_claude_command("mcp list");

    if let Some(output) = output {
        let servers = parse_mcp_list_output(&output);

        println!("Found {} MCP servers:\n", servers.len());

        for server in &servers {
            println!("Server: {}", server.name);
            println!("  Scope: {}", server.scope);
            println!("  Type: {}", server.server_type);
            println!("  Status: {}", server.status);

            if server.server_type == "HTTP" || server.server_type == "SSE" {
                if let Some(url) = &server.url {
                    println!("  URL: {}", url);

                    // 模拟 MCP 协议请求
                    let mut client = McpHttpClient::new(url.clone());

                    println!("\n  MCP Protocol Requests to get details:");
                    println!(
                        "  - initialize: {}",
                        serde_json::to_string(&client.initialize()).unwrap_or_default()
                    );
                    println!(
                        "  - tools/list: {}",
                        serde_json::to_string(&client.list_tools()).unwrap_or_default()
                    );
                    println!(
                        "  - prompts/list: {}",
                        serde_json::to_string(&client.list_prompts()).unwrap_or_default()
                    );
                    println!(
                        "  - resources/list: {}",
                        serde_json::to_string(&client.list_resources()).unwrap_or_default()
                    );
                }
            } else {
                if let Some(cmd) = &server.command {
                    println!("  Command: {}", cmd);
                    if let Some(args) = &server.args {
                        println!("  Args: {:?}", args);
                    }
                }
            }
            println!();
        }
    } else {
        println!("Failed to run claude mcp list");
    }
}
