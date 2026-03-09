#!/bin/sh

REMOVED=""

echo "Uninstalling tmux-agent-bus (cleaning up all versions)..."

# --- Remove binaries (current Rust binary) ---

for dir in /usr/local/bin "$HOME/.local/bin"; do
  if [ -x "$dir/tmux-agent-bus" ]; then
    if [ -w "$dir" ]; then
      rm "$dir/tmux-agent-bus"
      echo "Removed $dir/tmux-agent-bus"
      REMOVED="${REMOVED}binary, "
    elif [ -e /dev/tty ] && sudo -v < /dev/tty 2>/dev/null; then
      sudo rm "$dir/tmux-agent-bus" < /dev/tty
      echo "Removed $dir/tmux-agent-bus"
      REMOVED="${REMOVED}binary, "
    else
      echo "WARNING: cannot remove $dir/tmux-agent-bus (no write access)"
    fi
  fi
done

# --- Remove old Node.js version ---

OLD_DIRS="$HOME/src/agent-bus $HOME/src/tmux-agent-bus/node_modules"
for dir in $OLD_DIRS; do
  if [ -d "$dir" ] && [ -f "$dir/index.js" ]; then
    echo "Found old Node.js version at $dir"
    rm -rf "$dir"
    echo "Removed $dir"
    REMOVED="${REMOVED}old-node, "
  fi
done

# --- Remove PATH entries from shell rc (both old and new installer comments) ---

for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile"; do
  if [ -f "$rc" ]; then
    for marker in "# Added by tmux-agent-bus installer" "# Added by agent-bus installer"; do
      if grep -q "$marker" "$rc" 2>/dev/null; then
        # Installer adds 3 lines: blank line, comment, export PATH=...
        if command -v python3 >/dev/null 2>&1; then
          python3 -c "
import sys
p, m = sys.argv[1], sys.argv[2]
with open(p) as f:
    lines = f.readlines()
out, i = [], 0
while i < len(lines):
    if m in lines[i]:
        # Remove this line + next (export), and preceding blank line
        if out and out[-1].strip() == '':
            out.pop()
        i += 2
    else:
        out.append(lines[i])
        i += 1
with open(p, 'w') as f:
    f.writelines(out)
" "$rc" "$marker"
        else
          sed -i.bak "/$marker/{N;d;}" "$rc" 2>/dev/null || \
            sed -i '' "/$marker/{N;d;}" "$rc"
          rm -f "${rc}.bak"
        fi
        echo "Removed PATH entry ($marker) from $rc"
        REMOVED="${REMOVED}PATH, "
      fi
    done
  fi
done

# --- Remove MCP client configs (both "agent-bus" and "tmux-agent-bus" keys) ---

remove_mcp_json() {
  config_file="$1"
  client_name="$2"
  key="$3"

  if [ ! -f "$config_file" ]; then
    return
  fi

  if ! grep -q "\"$key\"" "$config_file" 2>/dev/null; then
    return
  fi

  # Use python3 for safe JSON manipulation
  if command -v python3 >/dev/null 2>&1; then
    python3 -c "
import json, sys
p, k = sys.argv[1], sys.argv[2]
with open(p) as f:
    data = json.load(f)
if 'mcpServers' in data:
    data['mcpServers'].pop(k, None)
with open(p, 'w') as f:
    json.dump(data, f, indent=2)
    f.write('\n')
" "$config_file" "$key" 2>/dev/null && {
      echo "  $client_name: removed \"$key\""
      REMOVED="${REMOVED}${client_name}, "
      return
    }
  fi

  # Fallback: sed
  sed -i.bak "s/\"$key\"[[:space:]]*:[[:space:]]*{[^}]*}[[:space:]]*,\\?//g" "$config_file" 2>/dev/null || \
    sed -i '' "s/\"$key\"[[:space:]]*:[[:space:]]*\\{[^}]*\\}[[:space:]]*,\\{0,1\\}//g" "$config_file"
  rm -f "${config_file}.bak"
  echo "  $client_name: removed \"$key\""
  REMOVED="${REMOVED}${client_name}, "
}

remove_mcp_config() {
  config_file="$1"
  client_name="$2"
  remove_mcp_json "$config_file" "$client_name" "tmux-agent-bus"
  remove_mcp_json "$config_file" "$client_name" "agent-bus"
}

echo ""
echo "Removing MCP client configs..."

# Claude Code (both old and new names)
if command -v claude >/dev/null 2>&1; then
  for name in tmux-agent-bus agent-bus; do
    if claude mcp get "$name" >/dev/null 2>&1; then
      claude mcp remove -s user "$name" 2>/dev/null && {
        echo "  Claude Code: removed \"$name\""
        REMOVED="${REMOVED}Claude Code, "
      } || echo "  Claude Code: failed to remove \"$name\""
    fi
  done
fi

OS="$(uname -s)"
case "$OS" in
  Darwin) PLATFORM="darwin" ;;
  Linux)  PLATFORM="linux" ;;
  *)      PLATFORM="unknown" ;;
esac

# Cline
if [ "$PLATFORM" = "darwin" ]; then
  remove_mcp_config "$HOME/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json" "Cline (VSCode)"
  remove_mcp_config "$HOME/Library/Application Support/Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json" "Cline (Cursor)"
elif [ "$PLATFORM" = "linux" ]; then
  XDG_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
  remove_mcp_config "$XDG_CONFIG/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json" "Cline (VSCode)"
  remove_mcp_config "$XDG_CONFIG/Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json" "Cline (Cursor)"
fi

if [ "$PLATFORM" = "darwin" ]; then
  APP_SUPPORT="$HOME/Library/Application Support"
  remove_mcp_config "$APP_SUPPORT/Claude/claude_desktop_config.json" "Claude Desktop"
  remove_mcp_config "$APP_SUPPORT/ChatGPT/mcp.json" "ChatGPT Desktop"
  remove_mcp_config "$HOME/.cursor/mcp.json" "Cursor"
  remove_mcp_config "$HOME/.codeium/windsurf/mcp_config.json" "Windsurf"
elif [ "$PLATFORM" = "linux" ]; then
  XDG_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
  remove_mcp_config "$XDG_CONFIG/Claude/claude_desktop_config.json" "Claude Desktop"
  remove_mcp_config "$XDG_CONFIG/chatgpt/mcp.json" "ChatGPT Desktop"
  remove_mcp_config "$HOME/.cursor/mcp.json" "Cursor"
  remove_mcp_config "$HOME/.codeium/windsurf/mcp_config.json" "Windsurf"
fi

# --- Remove from project-level MCP configs ---

echo ""
echo "Scanning for project-level MCP configs..."
PROJECT_CONFIGS=""

# Known project-level MCP config patterns:
#   .mcp.json              (Claude Code)
#   .cursor/mcp.json       (Cursor)
#   .windsurf/mcp.json     (Windsurf)
#   .vscode/cline_mcp_settings.json (Cline)
PROJECT_CONFIGS="$(find "$HOME" -maxdepth 6 \
  \( -name .mcp.json -o -path '*/.cursor/mcp.json' -o -path '*/.windsurf/mcp.json' -o -path '*/.vscode/cline_mcp_settings.json' \) \
  -not -path '*/node_modules/*' \
  -not -path '*/.git/*' \
  -not -path '*/Library/Application Support/*' \
  2>/dev/null | xargs grep -l '"tmux-agent-bus"\|"agent-bus"' 2>/dev/null || true)"

if [ -n "$PROJECT_CONFIGS" ]; then
  echo "$PROJECT_CONFIGS" | while read -r pconfig; do
    remove_mcp_config "$pconfig" "project ($pconfig)"
  done
else
  echo "  No project-level configs found."
fi

# Codex (both old and new names)
if command -v codex >/dev/null 2>&1; then
  for name in tmux-agent-bus agent-bus; do
    if codex mcp list 2>/dev/null | grep -q "$name"; then
      codex mcp remove "$name" 2>/dev/null && {
        echo "  Codex: removed \"$name\""
        REMOVED="${REMOVED}Codex, "
      } || echo "  Codex: failed to remove \"$name\""
    fi
  done
fi

# --- Remove channel data ---

if [ -d "$HOME/.agent-bus" ]; then
  rm -rf "$HOME/.agent-bus"
  echo "Removed ~/.agent-bus"
  REMOVED="${REMOVED}data, "
fi

echo ""
if [ -z "$REMOVED" ]; then
  echo "Nothing to uninstall — tmux-agent-bus was not found."
else
  echo "Done! tmux-agent-bus has been fully uninstalled."
fi
