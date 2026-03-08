#!/usr/bin/env node

import { readFileSync, writeFileSync, mkdirSync, existsSync, appendFileSync } from "fs";
import { execSync } from "child_process";
import { homedir } from "os";
import { join, dirname } from "path";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const INSTRUCTIONS = readFileSync(join(__dirname, "MCP_INSTRUCTIONS.md"), "utf-8");

const BUS_DIR = join(homedir(), ".agent-bus");
const CONFIG_PATH = join(BUS_DIR, "config.json");
const LOG_PATH = join(BUS_DIR, "history.jsonl");

function loadConfig() {
  mkdirSync(BUS_DIR, { recursive: true });
  if (!existsSync(CONFIG_PATH)) {
    writeFileSync(CONFIG_PATH, JSON.stringify({ agents: {} }, null, 2) + "\n");
  }
  return JSON.parse(readFileSync(CONFIG_PATH, "utf-8"));
}

function saveConfig(config) {
  writeFileSync(CONFIG_PATH, JSON.stringify(config, null, 2) + "\n");
}

function detectPane() {
  try {
    const paneList = execSync(
      "tmux list-panes -a -F '#{pane_pid} #{session_name}:#{window_index}.#{pane_index}'",
      { timeout: 3000 }
    ).toString().trim().split("\n");

    const paneMap = {};
    for (const line of paneList) {
      const [pid, paneId] = line.split(" ");
      paneMap[pid] = paneId;
    }

    let pid = process.pid;
    while (pid && pid !== 1) {
      if (paneMap[String(pid)]) {
        return paneMap[String(pid)];
      }
      try {
        pid = parseInt(
          execSync(`ps -o ppid= -p ${pid}`, { timeout: 1000 }).toString().trim()
        );
      } catch {
        break;
      }
    }
  } catch {
    // tmux not running or ps failed
  }
  return null;
}

function sendToPane(pane, message) {
  try {
    const sanitized = message.replace(/\n+/g, " ").trim();
    execSync(`tmux send-keys -t ${JSON.stringify(pane)} -l ${JSON.stringify(sanitized)}`, { timeout: 5000 });
    execSync(`tmux send-keys -t ${JSON.stringify(pane)} Enter`, { timeout: 5000 });
    return { success: true };
  } catch (err) {
    return { success: false, error: err.message };
  }
}

function logHandoff(record) {
  const entry = JSON.stringify({ ts: new Date().toISOString(), ...record });
  appendFileSync(LOG_PATH, entry + "\n");
}

function getAgents() {
  return loadConfig().agents || {};
}

const server = new McpServer({
  name: "agent-bus",
  version: "0.3.0",
}, {
  instructions: INSTRUCTIONS,
});

server.tool(
  "who",
  "List all agents currently registered on the bus. Call this before registering to see what names are taken.",
  {},
  async () => {
    const agents = getAgents();
    const names = Object.keys(agents);
    if (names.length === 0) {
      return { content: [{ type: "text", text: "No agents registered yet. Be the first — call register." }] };
    }
    const lines = names.map((n) => `- ${n} (pane ${agents[n].pane})`).join("\n");
    return { content: [{ type: "text", text: `Registered agents:\n${lines}` }] };
  }
);

server.tool(
  "register",
  "Register this agent with the bus. Call this once at the start of a session. Pick a unique name — call 'who' first to see what's taken. The bus auto-detects your tmux pane.",
  {
    name: z.string().describe("Your unique agent name, e.g. 'claude-1', 'codex-alpha'. Must be unique on the bus."),
  },
  async ({ name }) => {
    const pane = detectPane();
    if (!pane) {
      return { content: [{ type: "text", text: "Failed to detect tmux pane. Are you running inside tmux?" }], isError: true };
    }
    const config = loadConfig();
    if (config.agents[name] && config.agents[name].pane !== pane) {
      return { content: [{ type: "text", text: `Name "${name}" is already taken by pane ${config.agents[name].pane}. Pick a different name. Call 'who' to see registered agents.` }], isError: true };
    }
    config.agents[name] = { pane };
    saveConfig(config);
    const others = Object.keys(config.agents).filter((k) => k !== name);
    return {
      content: [{ type: "text", text: `Registered as "${name}" on tmux pane ${pane}. Other agents on bus: ${others.length ? others.join(", ") : "none yet"}.` }],
    };
  }
);

server.tool(
  "signal_done",
  "Signal that you are done with your task and hand off to another agent. This injects a message into the other agent's tmux pane with your summary and request.",
  {
    from: z.string().describe("Your registered agent name"),
    next: z.string().describe("Which agent should go next"),
    summary: z.string().describe("What you just finished"),
    request: z.string().describe("What you need the next agent to do"),
  },
  async ({ from, next, summary, request }) => {
    const agents = getAgents();
    const pane = agents[next]?.pane;
    if (!pane) {
      const available = Object.keys(agents);
      return { content: [{ type: "text", text: `Unknown agent: "${next}". Registered agents: ${available.length ? available.join(", ") : "none — agents must call register first"}.` }], isError: true };
    }
    const message = `[from ${from}]: ${summary} Request: ${request}`;
    const result = sendToPane(pane, message);
    logHandoff({ type: "signal_done", from, to: next, summary, request });
    if (!result.success) {
      return { content: [{ type: "text", text: `Failed to reach ${next}: ${result.error}` }], isError: true };
    }
    return { content: [{ type: "text", text: `Handed off to ${next}. Message delivered to tmux pane ${pane}.` }] };
  }
);

server.tool(
  "send_message",
  "Send a message to another agent without handing off. Use for mid-task questions or FYIs.",
  {
    from: z.string().describe("Your registered agent name"),
    to: z.string().describe("Which agent to message"),
    message: z.string().describe("The message to send"),
  },
  async ({ from, to, message }) => {
    const agents = getAgents();
    const pane = agents[to]?.pane;
    if (!pane) {
      const available = Object.keys(agents);
      return { content: [{ type: "text", text: `Unknown agent: "${to}". Registered agents: ${available.length ? available.join(", ") : "none — agents must call register first"}.` }], isError: true };
    }
    const fullMessage = `[message from ${from}]: ${message}`;
    const result = sendToPane(pane, fullMessage);
    logHandoff({ type: "send_message", from, to, message });
    if (!result.success) {
      return { content: [{ type: "text", text: `Failed to reach ${to}: ${result.error}` }], isError: true };
    }
    return { content: [{ type: "text", text: `Message sent to ${to} in tmux pane ${pane}.` }] };
  }
);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("agent-bus MCP server running on stdio");
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
