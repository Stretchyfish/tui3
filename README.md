# tui3

A terminal interface for [T3 Code](https://t3.codes), built in Rust with Ratatui.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/Stretchyfish/tui3/main/install.sh | sh
```

Then run:

```bash
tui3
```

`tui3` connects to your local T3 server automatically (or walks you through installing and starting it). When a newer release is available, it lets you know in the top bar—re-run the install command above to update.

## Basics

- **Arrows / `h j k l`**: Navigate panes, projects, and threads
- **`Enter` / `i`**: Start typing (`Esc` to return to navigation)
- **`w`**: Toggle the workspace panel (Editor, Diffs, Terminal, Git)
- **`s`**: Open Settings
- **`q` / `Ctrl+Q`**: Quit

## Build from source

```bash
cargo install --path .
```
