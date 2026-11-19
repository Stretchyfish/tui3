# Plan

## Phase one: T3 server terminal interface

- [x] Build a terminal UI in Rust using Ratatui.
- [x] Add arrow-key and Vim-style pane navigation with a separate typing mode.
- [x] Allow Down/j to highlight the prompt pane without Tab; Enter/i starts typing.
- [x] Show the Enter/i typing hint in the message header when the prompt pane is highlighted.
- [x] Focus the Message composer directly with the `m` shortcut.
- [x] Connect to a T3 server (discovery, identity, session checks, and token authentication).
- [x] Pair with the local T3 server from the TUI and remember the session without manual tokens.
- [x] Show the list of projects and their chat threads in a left sidebar.
- [x] Show the connected T3 environment's machine label next to each project name.
- [x] Allow selecting a project and thread.
- [x] Accept Space as Enter for navigation actions and menu choices while preserving spaces in text fields.
- [x] Select visible project and thread rows with a left mouse click.
- [x] Open a compact project menu with Enter to create a T3 thread; anchor project and thread menus beside the highlighted sidebar row.
- [x] Use the project name as its menu title without repeating it inside the menu.
- [x] Use the thread name as its menu title and remove the duplicate interior heading.
- [x] Read and display the selected thread's chat history.
- [x] Send new chat messages through the T3 server.
- [x] Display incoming agent responses and chat updates in real time, with automatic reconnection.

## Chat improvements before code editing

- [x] Show unsent attachments at the bottom of chat with filename, size, and mouse removal; support Ctrl+Backspace/Ctrl+H from Chat or Message and cancel pending pastes on clear.

- [x] Attach desktop clipboard images with Ctrl+V in Message, show attachment count, preserve per-thread drafts, and send images using T3’s attachment upload protocol.

- [x] Open a settings panel with s in navigation mode and close it with Esc or s.
- [x] Show the desktop clipboard access check in Settings and allow rechecking with r.
- [x] Add a saved Regular/Vim/Helix typing-style preference in Settings (mode behavior remains future work).
- [x] Display all three typing styles as adjacent boxes with the selected option highlighted.
- [x] Add a focusable current-model box and choose another model through T3.
- [x] Choose the selected model's T3-reported reasoning level in the model picker.
- [x] Separate model and reasoning selection in the picker and show the active reasoning level in the Model box.
- [x] Keep the model picker open when Enter selects a model, then apply after choosing reasoning.
- [x] Place reasoning choices to the right of the model list.
- [x] Use Left/Right to switch between model and reasoning selection, and Up/Down to choose within each.
- [x] Place the model box in a controls row below the message box, with room for more controls.
- [x] Remember the last selected project/thread for each T3 environment and restore it on startup.
- [x] Compact the model selector and add live session/weekly usage-remaining circles beside it.
- [x] Make Session left and Week left focusable controls that show countdowns to T3's reported reset times.
- [x] Distinguish user and agent messages with labeled, colored message blocks and clearer spacing.
- [x] Preserve speaker-colored borders on every wrapped message line, including streaming updates and terminal resizing.
- [x] Show when the agent is thinking and how long the current run has been active.
- [x] Display the live working duration as gray text above the green AGENT header.
- [x] Keep working until T3 reports the run has ended, and show its duration once above the final AGENT message header.
- [x] Show a recent activity feed for run, command, file, and search progress.
- [x] Remove activity entries when they finish, showing only ongoing work.
- [x] Animate running activity with a pulsing amber color and spinner.
- [x] Show changed files beneath agent replies, grouped by run, with server-reported addition/deletion counts.
- [x] Use T3 checkpoint file summaries for chat file lists and counts, replacing intermediate tool edits completely.
- [x] Render changed files inside the green AGENT message block with matching borders and text styling.
- [x] Label file summaries as "Changing X files" while counts are unknown, and "X changed files" once counts are available.
- [x] Fetch completed runs' checkpoint diffs asynchronously when complete checkpoint summaries are unavailable.
- [x] Improve prompt editing with cursor movement and Delete/Home/End.
- [x] Select visible text with a mouse drag anywhere in the TUI and copy with Ctrl+C; select Message draft text with Shift+arrows.
- [x] Keep multiline Chat selection inside Chat, copy wrapped lines as continuous text, and offer Copy/Paste in a right-click menu.
- [x] Keep the desktop clipboard owner alive after copying so Linux clipboard managers can receive the selection.
- [x] Fix Ctrl+Enter steering dispatch and visibly mark T3-reported queued messages and runs.
- [x] Jump between word starts with Ctrl+Left/Right while typing in the Message box.
- [x] Keep draft wrapping, cursor placement, and scrolling consistent; delete Unicode graphemes and normalize pasted control characters.
- [x] Show send status only in the Message box, removing its duplicate in the chat.
- [x] Show a chat notice when reported usage is exhausted and allow `r` to resend the previous message after usage returns.
- [x] Send with Ctrl+Enter as a steering message; retain Enter for queueing and show both shortcuts in the footer.
- [x] Keep the thread title in the Chat header without repeating it in the Message header.
- [x] Toggle voice dictation with `Shift+D` in navigation mode (and stop with `Shift+D` or cancel with `Esc`), showing a live indicator on the Message panel and transcribing via T3's speech endpoint.

## Launch and first-time setup

- [x] Default the exit dialog to No, keeping the T3 server running.
- [x] Verify that declining server shutdown preserves the server after the tui3 runtime exits.

- [x] Check for a running T3 server when tui3 launches and reuse it when available.
- [x] Save a default-on "Start T3 when tui3 launches" setting; this does not install an operating-system boot service.
- [x] Match launch-setting navigation to its box order: Left/h highlights On; Right/l highlights Off.
- [x] Start a missing local server asynchronously through `t3 serve`, with visible progress and failures; never start a local replacement for remote URLs or occupied ports.
- [x] Offer an explicit official T3 CLI installation in a setup dialog when missing, then start the server and offer local pairing.
- [x] Discover newly installed CLIs outside PATH and retry connection discovery after setup creates a runtime file.
- [x] Support setup reopening with o, keyboard/mouse controls, error/retry states, and dismissal without installing.

## Right-side workspace panel

Add a workspace panel to the right of the main conversation, similar to T3 Code's
side panel. Keep the project sidebar and main chat available while switching
between workspace tools. Build each feature incrementally.

### Panel foundation

- [ ] Inspect T3 Code's supported workspace, diff, terminal, Git, and conversation operations before choosing integration details.
- [x] Add a focusable right-side panel with a connected top tab strip for Diffs, Editor, Terminal, Git, and Side chat.
- [x] Place tool selection at the top inside a shared Workspace frame; Up/k reaches the selector and Down/j returns to content.
- [x] Add case-sensitive navigation shortcuts: D Diffs, e Editor, t Terminal, g Git, c Side chat; show them in the footer and preserve text entry and modal controls.
- [x] Combine the Code and Files workspace tabs into one Editor tab opened with `e`.
- [x] Move Right/l from the message panel to the workspace panel; Down/j still focuses the model selector.
- [x] Keep all five workspace selector boxes in one horizontal row, progressively shortening names to fit the terminal width.
- [x] Style workspace tabs as a connected strip, with the active tab open into the workspace and keyboard focus marked in blue.
- [x] Remove the redundant placeholder panel title, tighten the tab-to-content spacing, and rename Side chat to Chat.
- [x] Join the active workspace tab to one shared content frame, removing the extra Workspace frame and duplicate tool frames.
- [x] Use one shared border column between adjacent workspace tabs.
- [x] Extend the blue workspace focus outline around the entire panel, including the tab strip.
- [x] Allow mouse clicks to focus panes and activate workspace/settings selector boxes.
- [x] Use click-only mouse tracking and drain unused mouse events to prevent input lag.
- [x] Scroll the chat with the mouse wheel while hovering over it, preserving typing focus and auto-follow behavior.
- [x] Distinguish highlighted choices from active choices in workspace and settings selectors; Enter/Space activates the highlighted choice.
- [x] Mark active selectors with a green outline and keyboard focus with a separate blue outer outline in Settings and the workspace panel.
- [x] Add keyboard navigation between the sidebar, main conversation, and workspace panel, plus tab switching within the panel.
- [x] Move left from the Diffs workspace tab to Chat instead of wrapping to the last tool.
- [x] Allow showing/hiding the panel with w, resizing with { / }, and automatically hide it in narrow terminals.
- [ ] Bind workspace tools to the selected project's/thread's actual workspace or worktree.
- [ ] Show loading, empty, unavailable, and error states for each tool.

### Diffs

- [x] Display T3 checkpoint diffs for completed runs with colored additions/deletions, keyboard/mouse scrolling, horizontal panning, and run selection.
- [x] Add a `w` toggle to wrap long diff lines, with vertical scrolling in wrapped mode.
- [x] Select individual changed files by keyboard or mouse and show only that file's colored diff.
- [x] Keep a full-height file browser to the right of the selected diff, with counts and readable path truncation; Left/Right or h/l moves between the browser and patch.
- [x] Cache parsed patches and wrapped chat content, reflow only changed chat lines, and eliminate repeated history scans during navigation.
- [x] Preserve checkpoint diff caches and pending results across thread switches; prioritize the visible run's diff.
- [x] Add a repeatable release performance test with a 17 ms p95 redraw budget for large histories, diffs, and streaming updates.
- [ ] Refresh diffs as workspace changes arrive and open a changed file in the code view.

### Files and code

- [ ] Browse the workspace's file tree and open files in the code view.
- [ ] Display code with syntax highlighting, line numbers, and scrolling.
- [ ] Add editing, cursor movement, selection, undo/redo, and saving with unsaved-change handling.
- [ ] Implement Regular, Vim, and Helix editing behavior using the saved typing-style preference.
- [ ] Handle external file changes without silently overwriting edits.

### Terminal

- [x] Add a floating terminal placeholder toggled with Ctrl+J or Shift+T and closed with Esc.
- [ ] Integrate an interactive terminal in the selected workspace using the supported T3 interface where available.
- [ ] Support terminal input, output, resizing, scrollback, and clean process/session lifecycle handling.
- [ ] Make switching between terminal input and pane navigation explicit.

### Git

- [x] Inspect T3's Git contracts and target the selected thread's worktree or project workspace.
- [x] Show repository status, current branch, ahead/behind counts, and changed files.
- [x] Choose local branches and commit all changes with a message or push, with confirmations and asynchronous progress/error handling.
- [x] Generate an editable AI commit subject/body from T3's actual working-tree diff using the installed Codex CLI, preserving drafts on failure.
- [ ] Revisit commit-message generation to make the flow smoother and support other providers, ideally using T3's configured source-control writer.
- [x] Extend local pairing for filesystem reads and Git writes, with p in Git to refresh older sessions.
- [ ] Distinguish staged and unstaged changes if supported by the chosen T3 interface.
- [ ] Open Git changes in the diff view.
- [ ] Support staging, unstaging, and committing selected changes with clear results and error states.

### Side chat

- [ ] Add a secondary T3 conversation view with thread selection and an independent composer.
- [ ] Support reading, sending, and streaming responses without replacing the main conversation.
- [ ] Keep drafts, model selection, activity, and working indicators associated with the correct conversation.
