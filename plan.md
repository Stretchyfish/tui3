# Plan

## Phase one: T3 server terminal interface

- [x] Build a terminal UI in Rust using Ratatui.
- [x] Add arrow-key and Vim-style pane navigation with a separate typing mode.
- [x] Allow Down/j to highlight the prompt pane without Tab; Enter/i starts typing.
- [x] Show the Enter/i typing hint in the message header when the prompt pane is highlighted.
- [x] Connect to a T3 server (discovery, identity, session checks, and token authentication).
- [x] Pair with the local T3 server from the TUI and remember the session without manual tokens.
- [x] Show the list of projects and their chat threads in a left sidebar.
- [x] Allow selecting a project and thread.
- [x] Read and display the selected thread's chat history.
- [x] Send new chat messages through the T3 server.
- [x] Display incoming agent responses and chat updates in real time, with automatic reconnection.

## Chat improvements before code editing

- [x] Open a settings panel with s in navigation mode and close it with Esc or s.
- [x] Add a saved Regular/Vim/Helix typing-style preference in Settings (mode behavior remains future work).
- [x] Display all three typing styles as adjacent boxes with the selected option highlighted.
- [x] Add a focusable current-model box and choose another model through T3.
- [x] Place the model box in a controls row below the message box, with room for more controls.
- [x] Remember the last selected project/thread for each T3 environment and restore it on startup.
- [x] Compact the model selector and add live session/weekly usage-remaining circles beside it.
- [x] Distinguish user and agent messages with labeled, colored message blocks and clearer spacing.
- [x] Show when the agent is thinking and how long the current run has been active.
- [x] Display the live working duration as gray text above the green AGENT header.
- [x] Keep working until T3 reports the run has ended, and show its duration once above the final AGENT message header.
- [x] Show a recent activity feed for run, command, file, and search progress.
- [x] Remove activity entries when they finish, showing only ongoing work.
- [x] Show changed files beneath agent replies, grouped by run, with server-reported addition/deletion counts.
- [x] Use T3 checkpoint file summaries for chat file lists and counts, replacing intermediate tool edits completely.
- [x] Render changed files inside the green AGENT message block with matching borders and text styling.
- [x] Fetch completed runs' checkpoint diffs asynchronously when complete checkpoint summaries are unavailable.
- [x] Improve prompt editing with cursor movement and Delete/Home/End.

## Right-side workspace panel

Add a workspace panel to the right of the main conversation, similar to T3 Code's
side panel. Keep the project sidebar and main chat available while switching
between workspace tools. Build each feature incrementally.

### Panel foundation

- [ ] Inspect T3 Code's supported workspace, diff, terminal, Git, and conversation operations before choosing integration details.
- [x] Add a focusable right-side panel with a boxed selector for Diffs, Code, Files, Terminal, Git, and Side chat, initially displaying letter placeholders.
- [x] Support moving down from the panel to its selector and switching tools with Left/Right or h/l.
- [x] Move Right/l from the message panel to the workspace panel; Down/j still focuses the model selector.
- [x] Keep all six workspace selector boxes in one horizontal row, progressively shortening names to fit the terminal width.
- [x] Compact the workspace selector to three rows, retaining separate active and focus outlines.
- [x] Allow mouse clicks to focus panes and activate workspace/settings selector boxes.
- [x] Use click-only mouse tracking and drain unused mouse events to prevent input lag.
- [x] Scroll the chat with the mouse wheel while hovering over it, preserving typing focus and auto-follow behavior.
- [x] Distinguish highlighted choices from active choices in workspace and settings selectors; Enter/Space activates the highlighted choice.
- [x] Mark active selectors with a green outline and keyboard focus with a separate blue outer outline in Settings and the workspace panel.
- [ ] Add keyboard navigation between the sidebar, main conversation, and workspace panel, plus tab switching within the panel.
- [ ] Allow showing, hiding, and resizing the panel; handle narrow terminals.
- [ ] Bind workspace tools to the selected project's/thread's actual workspace or worktree.
- [ ] Show loading, empty, unavailable, and error states for each tool.

### Diffs

- [ ] List changed files and show readable diffs with additions and deletions highlighted.
- [ ] Refresh diffs as workspace changes arrive and open a changed file in the code view.

### Files and code

- [ ] Browse the workspace's file tree and open files in the code view.
- [ ] Display code with syntax highlighting, line numbers, and scrolling.
- [ ] Add editing, cursor movement, selection, undo/redo, and saving with unsaved-change handling.
- [ ] Implement Regular, Vim, and Helix editing behavior using the saved typing-style preference.
- [ ] Handle external file changes without silently overwriting edits.

### Terminal

- [ ] Integrate an interactive terminal in the selected workspace using the supported T3 interface where available.
- [ ] Support terminal input, output, resizing, scrollback, and clean process/session lifecycle handling.
- [ ] Make switching between terminal input and pane navigation explicit.

### Git

- [ ] Show repository status, branch, and staged/unstaged changes.
- [ ] Open Git changes in the diff view.
- [ ] Support staging, unstaging, and committing selected changes with clear results and error states.

### Side chat

- [ ] Add a secondary T3 conversation view with thread selection and an independent composer.
- [ ] Support reading, sending, and streaming responses without replacing the main conversation.
- [ ] Keep drafts, model selection, activity, and working indicators associated with the correct conversation.
