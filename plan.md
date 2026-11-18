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

- [x] Add a focusable current-model box and choose another model through T3.
- [x] Place the model box in a controls row below the message box, with room for more controls.
- [x] Remember the last selected project/thread for each T3 environment and restore it on startup.
- [x] Compact the model selector and add live session/weekly usage-remaining circles beside it.
- [x] Distinguish user and agent messages with labeled, colored message blocks and clearer spacing.
- [x] Show when the agent is thinking and how long the current run has been active.
- [x] Keep working until T3 reports the run has ended, and show its duration once above the final AGENT message header.
- [x] Show a recent activity feed for run, command, file, and search progress.
- [x] Remove activity entries when they finish, showing only ongoing work.
- [x] Improve prompt editing with cursor movement and Delete/Home/End.

## Later phases

- [ ] Add code editing with conventional, Vim, and Helix modes.
