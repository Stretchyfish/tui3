# tui3

A Rust/Ratatui terminal interface for T3 Code. Currently supports pane navigation,
local server discovery, pairing, project/thread browsing, live chat, sending messages,
and model selection.

Run with `cargo run`. Use arrows or `h/j/k/l` to switch panes, Enter or `i` in the
composer to type, Esc to return to navigation, and Ctrl+Q to quit.

Press `s` in navigation mode to open Settings. Use Left/Right or `h/l` to choose
Regular, Vim, or Helix typing style; all three appear in adjacent boxes with the
highlighted option outlined in blue outside the box and the active option outlined in green. Enter or
Space activates the highlighted style. The preference
is saved automatically in `ui-state.json`. Vim/Helix editing behavior is future work;
this setting currently records your preferred style. The panel also shows the connection
and model. Press Esc or `s` to close it and
return to the same pane. While typing a prompt, `s` inserts text normally.
Clicking a typing-style box activates and saves it immediately.

## Workspace panel

The right-side workspace panel includes a live Diffs viewer. Code (C), Files (F),
Terminal (T), Git (G), and Side chat (S) currently show letter placeholders.
From chat or the message panel in navigation mode, press Right or `l` to focus it.
Down or `j` in the workspace panel focuses the boxed tool selector
underneath; all six boxes stay on one row, progressively shortening their names as
space shrinks and showing full names when they fit.
Left/Right or `h/l` moves the blue outer outline; Enter or Space activates the tool,
shown with a green inner outline. Up or `k` returns to the
panel, then Left or `h` returns to chat. Tab/Shift+Tab also cycle through these panes.
You can also click panes to focus them, or click a tool box to activate it directly.
Clicking the message box highlights it; Enter or `i` still starts typing.

The Diffs tab shows the latest completed run's checkpoint patch from T3, with green
additions, red deletions, and yellow hunk headers. While focused on the panel, use
`[`/`]` to browse completed runs. Each run opens a full-height file list with
addition/deletion counts. Use Up/Down, `j/k`, or the wheel to select a file;
Enter or a click opens only that file's diff. Press `f` or Esc to return to the list.
Long paths shorten from the left to keep the filename visible.
Use Page Up/Down or Shift+Up/Down to scroll,
Home/End to jump, and Shift+Left/Right to pan long lines. The mouse wheel scrolls
when hovering over the viewer. Down/`j` still focuses the tool selector.
Diff loading runs in the background. Empty diffs and unavailable checkpoints have
explicit states; `r` retries a failed fetch. These are per-run diffs, not current
uncommitted Git changes. Editing remains planned.

For the smoothest normal use, run `cargo run --release`. The repeatable rendering
stress test uses 1,000 messages, 200 runs, and a 100-file patch, measuring both
navigation redraws and streaming updates at 160×48 terminal cells:

```sh
cargo test --release performance_large_thread_navigation -- --ignored --nocapture
```

It enforces a 17 ms p95 frame budget in release builds. Measurements cover app
rendering on the test machine; terminal rendering and server latency are separate.

## Connecting to T3

Start T3 Code, then launch tui3. By default, tui3 reads the server origin from
`~/.t3/userdata/server-runtime.json`. Set `T3_STATE_DIR` for another state directory,
or set `T3_SERVER_URL` to an explicit HTTP(S) origin:

```sh
T3_SERVER_URL=http://127.0.0.1:3773 cargo run
```

The client checks `/.well-known/t3/environment` and `/api/auth/session` at startup
and every five seconds. Requests run in a background Tokio task with five-second
timeouts. The UI reports connection errors, server identity, and authentication status.
The supported orchestration protocol is version 2 (verified against T3 Code 0.1.8).

## Pairing with the local server

With T3 Code running and the `t3` CLI on PATH, press **p** in navigation mode.
The app runs T3's supported local pairing command in the background, exchanges
its one-time credential, and saves your session automatically. There are no tokens
to copy and no browser is needed for this local flow. Press Esc first if typing.

The pairing is labeled `tui3` and requests only `orchestration:read` and
`orchestration:operate` for the planned project and chat workflow. The local runtime
must match the configured server origin; this flow never pairs with a different
server or reuses the desktop GUI's private session. Pairing times out after 20 seconds
and can be retried with **p**. T3's connection policy may prevent pairing.

Sessions are stored outside the repository at `$XDG_CONFIG_HOME/tui3/session.json`
(or `~/.config/tui3/session.json`). On Unix, the directory is private (0700) and the
file is readable only by your user (0600). The saved session is bound to both the
server origin and environment ID, and is reused on startup. If the session expires
or is revoked, press **p** again. Delete the session file to forget it locally; the
server's Connections settings can revoke access.

Remote browser sign-in and DPoP credentials are not implemented yet. For advanced
configuration, `T3_ACCESS_TOKEN_FILE`, `T3_ACCESS_TOKEN`, or `T3_BOOTSTRAP_TOKEN`
remain available and take precedence over the saved session. Explicitly pressing
**p** replaces those credentials in memory with a newly paired local session.
Use HTTPS for credentials sent to remote servers. Redirects are disabled, and
credentials and CLI pairing output are never displayed or logged.

## Projects and threads

After sign-in, the sidebar loads the real projects and threads from
`/api/orchestration/shell` using orchestration protocol version 2. Threads appear
under their projects; archived entries are labeled. Deleted entries are excluded.

- Use Up/Down or `j/k` in the sidebar to select a project or thread.
- Press Right, `l`, or Enter to focus the details pane.
- Press Left or `h` to return to the sidebar, or Tab to cycle panes.
- The details pane shows the selected project's workspace or thread's status.

The list refreshes automatically every five seconds and keeps your selected item
when it is still available. Longer lists scroll to follow selection. Failed refreshes
show an error and retain the last loaded list; losing authentication clears it.
This currently lists projects from the connected server only.

## Remembering your last location

TUI3 remembers the selected project or thread separately for each T3 environment.
When you launch it again, the matching item is restored after the project list loads;
a restored thread opens its live conversation automatically. Typing mode stays off.
If a saved thread was deleted, its project is selected instead. If that project also
no longer exists, the first available item is selected.

Selections are saved as they change in `$XDG_CONFIG_HOME/tui3/ui-state.json`
(or `~/.config/tui3/ui-state.json`), separately from authentication credentials.
Loading failures, empty lists, and signing out do not overwrite the last saved
location. Missing or invalid preferences fall back to the normal startup selection.

## Chat history

Selecting a thread opens T3's authenticated `orchestration.subscribeThread`
WebSocket stream. A full conversation snapshot is followed by live updates. User and assistant messages appear in transcript order, including inherited
conversation items. Raw tool input/output and private reasoning are omitted; a compact
activity feed shows ongoing run, command, file, and search progress from T3's live
events. Entries disappear when they finish. Attachments show a placeholder rather than a preview. Messages have distinct bordered sections: cyan
**YOU** headers for your prompts, green **AGENT** headers for replies, and yellow
system headers. Labels and borders distinguish speakers even without color.
Message bodies remain plain text, preserving whitespace and code indentation.
Changed-file summaries appear inside each run's latest green AGENT message block, listing
paths with green additions and red deletions. File lists and counts come from T3's
checkpoint summaries, matching the source used by its chat cards. Individual tool
edits are not summed. If no complete checkpoint summary is available, the app
requests the run's checkpoint diff in the background and uses its full file list.
Until that data is available, file-event paths can appear with unknown counts.
An empty checkpoint summary removes files that were edited but ultimately reverted.
Opening diffs is planned for the workspace panel.

Threads open at the latest message. In the chat pane, use Shift+J/Shift+K or
the mouse wheel over the chat to scroll (three lines per wheel step).
Wheel scrolling keeps your current input focus. You can also use
Shift+Up/Down to scroll one line, Page Up/Down to move ten lines, Home to jump to the
start, and End to return to the latest messages. Use Down or `j` to highlight the composer, then Enter or `i` to type.
Use Up or `k` in the composer to return to chat, or `h` to return to the sidebar.

Replies update as the agent generates them. While a run is active, the chat shows an
gray **Working for Ns** line above the green AGENT header with a continuously updated elapsed
timer. When the run finishes, **Worked for Ns** appears once above its final green
AGENT message header, using T3's run timestamps, including after reopening a thread.
Finishing an individual progress message does not end the working indicator;
T3's run status determines completion. Reconnecting snapshots preserve the timer. The chat also
shows current agent activity, and its header shows **live** or **reconnecting**. Scrolling up
preserves your reading position; End resumes following the latest messages.
Switching threads cancels the previous subscription and ignores late updates.

If the stream disconnects, the last loaded conversation stays readable. The app
attempts an HTTP transcript resync and reconnects with increasing delays, requesting
a fresh snapshot to recover missed updates. Sequence checks discard duplicate events,
and updates replace messages by item ID. A temporary server outage preserves the
conversation and reading position; an explicit sign-out clears it. Loading, empty,
and error states are shown. Project listings and session checks still refresh every
five seconds; healthy chat streams do not poll the transcript endpoint.

## Sending messages

Select a thread, use Tab to focus its composer, then press Enter or `i` to type.
While typing, **Enter sends** and **Alt+Enter** adds a newline (Shift+Enter also works
in terminals that report it). Arrow keys move the cursor; Up/Down keep its column
when moving between lines. Home/End move to the current line's start/end, and Delete
removes the character after the cursor. Backspace removes the character before it.
Ctrl+Left/Right jumps to the previous/next word start, skipping punctuation and
whitespace across lines; at either end it stops at the draft boundary.
Esc returns to navigation. Bracketed paste inserts at the cursor and preserves
multiline text without submitting it.
The composer uses character wrapping with a cursor-following viewport. Backspace
and Delete remove whole Unicode characters including combined accents and emoji;
pasted line endings are normalized and tabs expand to four spaces.

The composer shows its target thread. Each thread keeps its own draft in memory
while you switch between conversations. Sending requires a selected thread and an
authenticated connection. Acknowledgements clear only the submitted text; edits
made while sending and drafts in other threads are preserved.

Messages use T3's authenticated `/ws?orchestrationProtocol=2` connection and
`orchestration.dispatchCommand` with `message.dispatch`, matching the GUI's command
API. The existing thread's provider and model are used. A message starts a turn when
the thread is idle, or queues after the active run. This version sends text only.
Sent messages and replies arrive through the live thread stream.

Only one message is submitted at a time. Failed sends keep the draft. If a connection
is lost before acknowledgement, delivery may have succeeded: check the conversation,
then press Enter again to retry if needed. An unchanged retry in the same app session
reuses its command and message IDs so T3 can replay its receipt without duplicating
the message. An explicit rejection or editing the text creates a new attempt.
Drafts and retry IDs are not persisted across application restarts.

## Model selection

The model box below the prompt shows the selected thread's model and provider.
From the prompt pane, use Down or `j` to highlight it; Tab or `m` in navigation
mode also focuses it. Press Enter to open the model picker.

- Up/Down or `j/k` moves through models; Page Up/Down and Home/End work too.
- Enter confirms the highlighted model. Esc cancels without changing anything.
- The current model is marked with `*`; `r` refreshes the available choices.
- Up or `k` returns from the model box to the prompt. Enter or `i` still starts typing.

Choices come from T3's configured, enabled, installed providers. Providers known to
be unavailable or signed out are excluded. Confirming uses T3's
`thread.model-selection.set` command for that thread; it does not submit a prompt.
The new setting applies to future messages and does not interrupt a running response.
Choosing a different model uses its default options; selecting the current model
leaves its existing options unchanged. Model changes made in T3's GUI update the box
through the live stream.

The previous model remains displayed until T3 acknowledges the change. Failures
show an error and preserve the prompt. Sending waits for an in-flight model change,
so a new message cannot accidentally use the previous model.

## Usage remaining

The compact controls row below the message contains the model selector, **Session
left**, and **Week left**. Usage boxes show circular fill indicators and percentages
remaining, with green, yellow, or red indicating the remaining allowance. The circle
uses quarter-step fills; the percentage provides the more precise value.

Values come from T3's live provider-status/configuration stream and follow the selected
thread's provider account. Session refers to the provider's session quota window
(often a rolling five-hour window), not the number of tokens in this chat. Weekly
values use the provider's weekly window; monthly-only allowances are not relabeled.
Where applicable, model-specific limits are combined with the general allowance.

Missing or unsupported windows show **N/A**. Disconnected or failed-probe data is
marked **stale**; known account data is retained while reconnecting. Signing out or
switching accounts clears unrelated usage data. No usage is estimated from messages.

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

To check WebSocket authentication against your paired local server without sending
messages or starting an agent run:

```sh
cargo test live_server_websocket_authentication -- --ignored
cargo test live_server_thread_subscription -- --ignored
cargo test live_server_model_catalog -- --ignored
cargo test live_server_usage_subscription -- --ignored
```
