# tui3

A fast, Rust/Ratatui terminal interface for [T3 Code](https://t3.chat). Browse projects and threads, stream agent responses, steer or queue messages, inspect checkpoint diffs, manage Git branches/commits/pushes, and open an embedded local shell.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Stretchyfish/tui3/main/install.sh | sh
```

Or build from source:

```bash
cargo install --path .
# or run directly:
cargo run
```

## Keybindings

### Navigation
- **Arrows / `h j k l`**: Move between panes and items
- **`m` / `c`**: Focus Message composer / Chat
- **`Enter` / `i`**: Start typing in Message (`Esc` to exit)
- **`w` / `{` `}`**: Toggle or resize the right-side workspace panel
- **`e` / `d` / `t` / `g`**: Open Editor, Diffs, Terminal, or Git workspace tab
- **`Ctrl+J` / `Shift+T`**: Toggle floating local terminal overlay
- **`s`**: Open Settings (typing style, auto-start T3, diff wrap, layout presets, server update)
- **`Ctrl+Q`**: Quit

### Messaging
- **`Enter`**: Queue message after the active run
- **`Ctrl+Enter` / `Alt+Enter`**: Send as a steering message
- **`Shift+Enter`**: Insert a newline
- **`Shift+D`**: Toggle voice dictation (navigation mode)
- **`Ctrl+V` / `Ctrl+C`**: Paste text/images or copy mouse-selected text
