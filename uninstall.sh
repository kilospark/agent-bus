#!/bin/sh
set -e

BINARY="tmux-agent-bus"
REMOVED=""

echo "Uninstalling ${BINARY}..."

# --- Remove binary ---

for dir in /usr/local/bin "$HOME/.local/bin"; do
  if [ -x "$dir/${BINARY}" ]; then
    if [ -w "$dir" ]; then
      rm "$dir/${BINARY}"
      echo "Removed $dir/${BINARY}"
      REMOVED="${REMOVED}binary, "
    elif [ -e /dev/tty ] && sudo -v < /dev/tty 2>/dev/null; then
      sudo rm "$dir/${BINARY}" < /dev/tty
      echo "Removed $dir/${BINARY}"
      REMOVED="${REMOVED}binary, "
    else
      echo "WARNING: cannot remove $dir/${BINARY} (no write access)"
    fi
  fi
done

# --- Remove PATH entry from shell rc ---

for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile"; do
  if [ -f "$rc" ] && grep -q "# Added by tmux-agent-bus installer" "$rc" 2>/dev/null; then
    # Remove the comment and the PATH line that follows it
    sed -i.bak '/# Added by tmux-agent-bus installer/,+1d' "$rc" 2>/dev/null || \
      sed -i '' '/# Added by tmux-agent-bus installer/{N;d;}' "$rc"
    rm -f "${rc}.bak"
    echo "Removed PATH entry from $rc"
    REMOVED="${REMOVED}PATH, "
  fi
done

# --- Remove MCP client configs ---

# Remove from JSON config files (delete the "agent-bus": { ... } entry)
remove_mcp_config() {
  config_file="$1"
  client_name="$2"

  if [ ! -f "$config_file" ]; then
    return
  fi

  if ! grep -q '"agent-bus"' "$config_file" 2>/dev/null; then
    return
  fi

  # Remove "agent-bus": { "command": "..." }, or "agent-bus": { "command": "..." }
  # Handle both with and without trailing comma
  sed -i.bak 's/"agent-bus"[[:space:]]*:[[:space:]]*{[^}]*}[[:space:]]*,\?//g' "$config_file" 2>/dev/null || \
    sed -i '' 's/"agent-bus"[[:space:]]*:[[:space:]]*\{[^}]*\}[[:space:]]*,\{0,1\}//g' "$config_file"
  rm -f "${config_file}.bak"
  echo "  $client_name: removed"
  REMOVED="${REMOVED}${client_name}, "
}

# Claude Code
if command -v claude >/dev/null 2>&1; then
  if claude mcp get agent-bus >/dev/null 2>&1; then
    claude mcp remove agent-bus 2>/dev/null && {
      echo "  Claude Code: removed"
      REMOVED="${REMOVED}Claude Code, "
    } || echo "  Claude Code: failed to remove (try: claude mcp remove agent-bus)"
  fi
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

# Codex
if command -v codex >/dev/null 2>&1; then
  if codex mcp list 2>/dev/null | grep -q 'agent-bus'; then
    codex mcp remove agent-bus 2>/dev/null && {
      echo "  Codex: removed"
      REMOVED="${REMOVED}Codex, "
    } || echo "  Codex: failed to remove (try: codex mcp remove agent-bus)"
  fi
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
  echo "Done! tmux-agent-bus has been uninstalled."
fi
