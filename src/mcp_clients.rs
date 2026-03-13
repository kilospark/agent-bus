use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const SERVER_NAME: &str = "agent-bus";
const OLD_NAMES: &[&str] = &["tmux-agent-bus"];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Add agent-bus to all detected MCP clients.
pub fn configure_clients() {
    let binary_path = match env::current_exe() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => {
            println!("  Error: could not determine binary path: {e}");
            return;
        }
    };

    println!();
    println!("Configuring MCP clients...");

    let mut any = false;

    // -- Type C: CLI-based clients --

    if has_binary("claude") {
        any = true;
        // Migrate old name
        for old in OLD_NAMES {
            if run_silent(&["claude", "mcp", "get", old]) {
                run_silent(&["claude", "mcp", "remove", "-s", "user", old]);
                println!("  Claude Code: migrated from {old}");
            }
        }
        configure_cli_client(
            "Claude Code",
            &["claude", "mcp", "get", SERVER_NAME],
            &["claude", "mcp", "add", "-s", "user", SERVER_NAME, &binary_path],
        );
    }

    if has_binary("codex") {
        any = true;
        // Migrate old name
        for old in OLD_NAMES {
            if run_grep(&["codex", "mcp", "list"], old) {
                run_silent(&["codex", "mcp", "remove", old]);
                println!("  Codex: migrated from {old}");
            }
        }
        configure_cli_client_grep(
            "Codex",
            &["codex", "mcp", "list"],
            SERVER_NAME,
            &["codex", "mcp", "add", SERVER_NAME, "--", &binary_path],
        );
    }

    if has_binary("gemini") {
        any = true;
        configure_cli_client_grep(
            "Gemini CLI",
            &["gemini", "mcp", "list"],
            SERVER_NAME,
            &["gemini", "mcp", "add", "-s", "user", SERVER_NAME, &binary_path],
        );
    }

    // -- Type A: mcpServers config files --

    for client in file_clients_mcp_servers() {
        let create = match client.create_when {
            CreateWhen::Never => false,
            CreateWhen::BinaryDetected(bin) => has_binary(bin),
        };
        if let Some(status) = upsert_mcp_servers(&client.path, &binary_path, create) {
            any = true;
            println!("  {}: {status}", client.name);
        }
    }

    // -- Type B: Opencode --

    if has_binary("opencode") {
        any = true;
        let path = xdg_config_dir().join("opencode/config.json");
        match upsert_opencode(&path, &binary_path) {
            Some(status) => println!("  Opencode: {status}"),
            None => println!("  Opencode: configured"),
        }
    }

    println!();
    if any {
        println!("Done! Restart your MCP client to start using agent-bus.");
    } else {
        println!("  No MCP clients detected. Add manually to your client config:");
        println!();
        println!(
            "  {{ \"mcpServers\": {{ \"{SERVER_NAME}\": {{ \"command\": \"{binary_path}\", \"args\": [] }} }} }}"
        );
    }
}

/// Remove agent-bus from all detected MCP clients.
pub fn remove_clients() {
    println!();
    println!("Removing agent-bus from MCP clients...");

    let mut any = false;
    let all_names: Vec<&str> = std::iter::once(SERVER_NAME).chain(OLD_NAMES.iter().copied()).collect();

    // -- Type C: CLI-based clients --

    if has_binary("claude") {
        for name in &all_names {
            if run_silent(&["claude", "mcp", "get", name]) {
                any = true;
                if run_silent(&["claude", "mcp", "remove", "-s", "user", name]) {
                    println!("  Claude Code: removed \"{name}\"");
                } else {
                    println!("  Claude Code: failed to remove (try: claude mcp remove -s user {name})");
                }
            }
        }
    }

    if has_binary("codex") {
        for name in &all_names {
            if run_grep(&["codex", "mcp", "list"], name) {
                any = true;
                if run_silent(&["codex", "mcp", "remove", name]) {
                    println!("  Codex: removed \"{name}\"");
                } else {
                    println!("  Codex: failed to remove (try: codex mcp remove {name})");
                }
            }
        }
    }

    if has_binary("gemini") {
        for name in &all_names {
            if run_grep(&["gemini", "mcp", "list"], name) {
                any = true;
                if run_silent(&["gemini", "mcp", "remove", "-s", "user", name]) {
                    println!("  Gemini CLI: removed \"{name}\"");
                } else {
                    println!("  Gemini CLI: failed to remove (try: gemini mcp remove -s user {name})");
                }
            }
        }
    }

    // -- Type A: mcpServers config files --

    for client in file_clients_mcp_servers() {
        if let Some(status) = remove_mcp_servers(&client.path) {
            any = true;
            println!("  {}: {status}", client.name);
        }
    }

    // Copilot (always check for removal)
    if let Some(home) = dirs::home_dir() {
        let copilot = home.join(".copilot/mcp-config.json");
        if let Some(status) = remove_mcp_servers(&copilot) {
            any = true;
            println!("  Copilot CLI: {status}");
        }
    }

    // -- Type B: Opencode --

    let opencode_path = xdg_config_dir().join("opencode/config.json");
    if let Some(status) = remove_opencode(&opencode_path) {
        any = true;
        println!("  Opencode: {status}");
    }

    // -- Clean up data created by agent-bus --

    if let Some(home) = dirs::home_dir() {
        let data_dir = home.join(".agent-bus");
        if data_dir.is_dir() {
            if std::fs::remove_dir_all(&data_dir).is_ok() {
                any = true;
                println!("  Removed {}", data_dir.display());
            }
        }
    }

    println!();
    if any {
        println!("Done! agent-bus has been uninstalled.");
    } else {
        println!("  Nothing to uninstall — no agent-bus configs or data found.");
    }
}

// ---------------------------------------------------------------------------
// Client definitions
// ---------------------------------------------------------------------------

enum CreateWhen {
    Never,
    BinaryDetected(&'static str),
}

struct FileClient {
    name: &'static str,
    path: PathBuf,
    create_when: CreateWhen,
}

fn file_clients_mcp_servers() -> Vec<FileClient> {
    let mut clients = Vec::new();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return clients,
    };

    let is_macos = cfg!(target_os = "macos");

    if is_macos {
        clients.push(FileClient {
            name: "Claude Desktop",
            path: home.join("Library/Application Support/Claude/claude_desktop_config.json"),
            create_when: CreateWhen::Never,
        });
    } else {
        clients.push(FileClient {
            name: "Claude Desktop",
            path: xdg_config_dir().join("Claude/claude_desktop_config.json"),
            create_when: CreateWhen::Never,
        });
    }

    if is_macos {
        clients.push(FileClient {
            name: "ChatGPT Desktop",
            path: home.join("Library/Application Support/ChatGPT/mcp.json"),
            create_when: CreateWhen::Never,
        });
    } else {
        clients.push(FileClient {
            name: "ChatGPT Desktop",
            path: xdg_config_dir().join("chatgpt/mcp.json"),
            create_when: CreateWhen::Never,
        });
    }

    let cursor_name = if has_binary("agent") { "Cursor / Agent" } else { "Cursor" };
    clients.push(FileClient {
        name: cursor_name,
        path: home.join(".cursor/mcp.json"),
        create_when: CreateWhen::BinaryDetected("agent"),
    });

    clients.push(FileClient {
        name: "Windsurf",
        path: home.join(".codeium/windsurf/mcp_config.json"),
        create_when: CreateWhen::Never,
    });

    if is_macos {
        clients.push(FileClient {
            name: "Cline (VSCode)",
            path: home.join("Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
            create_when: CreateWhen::Never,
        });
        clients.push(FileClient {
            name: "Cline (Cursor)",
            path: home.join("Library/Application Support/Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
            create_when: CreateWhen::Never,
        });
    } else {
        clients.push(FileClient {
            name: "Cline (VSCode)",
            path: xdg_config_dir().join("Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
            create_when: CreateWhen::Never,
        });
        clients.push(FileClient {
            name: "Cline (Cursor)",
            path: xdg_config_dir().join("Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
            create_when: CreateWhen::Never,
        });
    }

    clients.push(FileClient {
        name: "Copilot CLI",
        path: home.join(".copilot/mcp-config.json"),
        create_when: CreateWhen::BinaryDetected("copilot"),
    });

    clients
}

// ---------------------------------------------------------------------------
// Type A: mcpServers config file operations
// ---------------------------------------------------------------------------

fn upsert_mcp_servers(path: &Path, binary_path: &str, create_if_missing: bool) -> Option<String> {
    if !path.exists() {
        if create_if_missing {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if fs::write(path, "{}\n").is_err() {
                return Some("failed to create config file".into());
            }
        } else {
            return None;
        }
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Some("failed to read config".into()),
    };

    let mut data = match parse_json_lenient(&raw) {
        Some(v) => v,
        None => return Some("failed to parse config JSON".into()),
    };

    let obj = match data.as_object_mut() {
        Some(o) => o,
        None => return Some("config is not a JSON object".into()),
    };

    let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
    let servers_map = match servers.as_object_mut() {
        Some(m) => m,
        None => return Some("mcpServers is not an object".into()),
    };

    // Migrate old names
    for old in OLD_NAMES {
        servers_map.remove(*old);
    }

    if let Some(existing) = servers_map.get(SERVER_NAME) {
        if let Some(cmd) = existing.get("command").and_then(Value::as_str) {
            if cmd == binary_path {
                return Some("already configured".into());
            }
        }
    }

    servers_map.insert(
        SERVER_NAME.into(),
        json!({"command": binary_path, "args": []}),
    );

    match write_json(path, &data) {
        Ok(()) => Some("configured".into()),
        Err(_) => Some("failed to write config".into()),
    }
}

fn remove_mcp_servers(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let all_names: Vec<&str> = std::iter::once(SERVER_NAME).chain(OLD_NAMES.iter().copied()).collect();
    if !all_names.iter().any(|n| raw.contains(&format!("\"{n}\""))) {
        return None;
    }

    let mut data = match parse_json_lenient(&raw) {
        Some(v) => v,
        None => return Some("failed to parse config JSON".into()),
    };

    let mut removed = false;
    if let Some(obj) = data.as_object_mut() {
        if let Some(servers) = obj.get_mut("mcpServers") {
            if let Some(m) = servers.as_object_mut() {
                for name in &all_names {
                    if m.remove(*name).is_some() {
                        removed = true;
                    }
                }
            }
        }
    }

    if !removed {
        return None;
    }

    match write_json(path, &data) {
        Ok(()) => Some("removed".into()),
        Err(_) => Some("failed to write config".into()),
    }
}

// ---------------------------------------------------------------------------
// Type B: Opencode config file operations
// ---------------------------------------------------------------------------

fn upsert_opencode(path: &Path, binary_path: &str) -> Option<String> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(path, "{}\n").is_err() {
            return Some("failed to create config file".into());
        }
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Some("failed to read config".into()),
    };

    let mut data = match parse_json_lenient(&raw) {
        Some(v) => v,
        None => return Some("failed to parse config JSON".into()),
    };

    let obj = match data.as_object_mut() {
        Some(o) => o,
        None => return Some("config is not a JSON object".into()),
    };

    let mcp = obj.entry("mcp").or_insert_with(|| json!({}));
    let mcp_map = match mcp.as_object_mut() {
        Some(m) => m,
        None => return Some("mcp key is not an object".into()),
    };

    // Migrate old names
    for old in OLD_NAMES {
        mcp_map.remove(*old);
    }

    if mcp_map.contains_key(SERVER_NAME) {
        return Some("already configured".into());
    }

    mcp_map.insert(
        SERVER_NAME.into(),
        json!({"type": "local", "command": [binary_path]}),
    );

    match write_json(path, &data) {
        Ok(()) => None, // caller prints "configured"
        Err(_) => Some("failed to write config".into()),
    }
}

fn remove_opencode(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let all_names: Vec<&str> = std::iter::once(SERVER_NAME).chain(OLD_NAMES.iter().copied()).collect();
    if !all_names.iter().any(|n| raw.contains(&format!("\"{n}\""))) {
        return None;
    }

    let mut data = match parse_json_lenient(&raw) {
        Some(v) => v,
        None => return Some("failed to parse config JSON".into()),
    };

    let mut removed = false;
    if let Some(obj) = data.as_object_mut() {
        for key in &["mcp", "mcpServers"] {
            if let Some(section) = obj.get_mut(*key) {
                if let Some(m) = section.as_object_mut() {
                    for name in &all_names {
                        if m.remove(*name).is_some() {
                            removed = true;
                        }
                    }
                }
            }
        }
    }

    if !removed {
        return None;
    }

    match write_json(path, &data) {
        Ok(()) => Some("removed".into()),
        Err(_) => Some("failed to write config".into()),
    }
}

// ---------------------------------------------------------------------------
// Type C: CLI-based client helpers
// ---------------------------------------------------------------------------

fn configure_cli_client(name: &str, check_args: &[&str], add_args: &[&str]) {
    if run_silent(check_args) {
        println!("  {name}: already configured");
        return;
    }
    if run_silent(add_args) {
        println!("  {name}: configured");
    } else {
        let cmd = add_args.join(" ");
        println!("  {name}: failed to configure (try: {cmd})");
    }
}

fn configure_cli_client_grep(name: &str, list_args: &[&str], grep_pattern: &str, add_args: &[&str]) {
    if run_grep(list_args, grep_pattern) {
        println!("  {name}: already configured");
        return;
    }
    if run_silent(add_args) {
        println!("  {name}: configured");
    } else {
        let cmd = add_args.join(" ");
        println!("  {name}: failed to configure (try: {cmd})");
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn has_binary(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_silent(args: &[&str]) -> bool {
    if args.is_empty() { return false; }
    Command::new(args[0])
        .args(&args[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_grep(args: &[&str], pattern: &str) -> bool {
    if args.is_empty() { return false; }
    Command::new(args[0])
        .args(&args[1..])
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(pattern))
        .unwrap_or(false)
}

fn xdg_config_dir() -> PathBuf {
    if let Ok(dir) = env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    match dirs::home_dir() {
        Some(h) => h.join(".config"),
        None => PathBuf::from("/tmp"),
    }
}

fn parse_json_lenient(raw: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return Some(v);
    }
    let cleaned = strip_trailing_commas(raw);
    serde_json::from_str::<Value>(&cleaned).ok()
}

fn strip_trailing_commas(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b',' {
            let mut j = i + 1;
            while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            if j < bytes.len() && matches!(bytes[j], b'}' | b']') {
                i += 1;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

fn write_json(path: &Path, data: &Value) -> Result<(), std::io::Error> {
    let serialized = serde_json::to_string_pretty(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, format!("{serialized}\n"))
}
