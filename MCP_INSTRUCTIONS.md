# Agent Bus — Inter-Agent Communication

You are one of multiple AI agents working on the same project in adjacent tmux panes. The agent-bus lets you hand off work and send messages to other agents without the user manually relaying.

## First Thing: Register

At the start of every session, call `register` with your agent name (e.g. "claude" or "codex"). This tells the bus which tmux pane you're in. You MUST do this before using signal_done or send_message.

## Tools

**`register`** — Call once at session start to register with the bus.
- `name`: your agent name (e.g. "claude" or "codex")

**`signal_done`** — Call when you finish a task and need another agent to take over.
- `next`: which agent should go next
- `summary`: what you just finished
- `request`: what you need the next agent to do

**`send_message`** — Send a message without handing off. Use for questions or FYIs.
- `to`: which agent to message
- `message`: the message to send

## When to Use

- When you start a session — call `register`.
- When you finish a task that another agent should review, audit, or continue — call `signal_done`.
- When you have a question for another agent or want to share information mid-task — call `send_message`.
- Do NOT ask the user to relay messages between agents. Use these tools instead.

## How It Works

Messages are injected into the target agent's tmux pane via `tmux send-keys`. The other agent sees your message as input and acts on it.

## When You Receive a Message

If your input starts with `[from claude]:` or `[from codex]:`, that is another agent handing off to you or sending you a message. Read the request and act on it. When you're done, use `signal_done` to hand back.

## Coordination File

Use `CLAUDE-CODEX-CHAT.md` in the project root as the shared written record for longer discussions, reconciliation data, and decisions. The bus handles turn-taking; the file handles documentation.
