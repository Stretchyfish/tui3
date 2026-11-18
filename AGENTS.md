# Project direction

Build a Rust terminal editor and agent interface using Ratatui. The application
should integrate closely with T3 Code, especially the T3 server, so users can work
with their projects and agent threads from the terminal.

## Progress tracking

- When finishing a task, end the user-facing response by naming the next task you
  intend to work on. Use `plan.md` to choose the next unfinished item unless the
  user has given a different direction. This does not automatically authorize
  starting the next task.
- Keep `plan.md` up to date as work progresses.
- Mark completed and verified tasks with `[x]` in the same change that completes
  them. Leave incomplete or partially implemented tasks as `[ ]`.
- Add or split checklist items when needed so progress remains clear.

## First milestone

Start with the T3 project and thread interface. Full code editing is future work.

- Connect to a T3 server when the application opens.
- Show the server's projects and their threads in a sidebar on the left.
- Allow users to navigate and select projects and threads.
- Provide a main conversation area and input composer for the selected thread.
- Allow users to submit messages to a project's thread through the T3 server,
  with the same underlying behavior as sending a message through the T3 Code GUI.
- Display the resulting thread activity and agent responses, including updates
  while a response is being generated.
- Clearly display connection, loading, empty, and error states.

Use the T3 server as the source of truth for projects, threads, and conversations.
Inspect its actual protocol and supported operations before implementing the
integration. Do not invent endpoints or use local mock data as the finished
integration. Determine how server connection settings and authentication work
from the available T3 server interface.

## Future editing modes

The long-term application includes code editing with selectable interaction modes:

- A conventional, non-modal editing mode.
- A Helix mode with Helix-style motions and editing behavior.
- A Vim mode with Vim-style motions and editing behavior.
- Room for additional modes later.

Keep input handling and key bindings separate from application state and T3 server
communication so these modes can be added without rewriting the integration.
Preserve this direction while keeping the first milestone focused on projects,
threads, and agent conversations. Do not implement the future editor modes yet.

## Implementation principles

- Use Rust and Ratatui for the terminal application.
- Treat terminal-based agent assistance as a core workflow.
- Keep server communication asynchronous so it does not block terminal input or
  rendering.
- Separate the T3 client, application state, terminal rendering, and input handling.
- Prefer a small, working end-to-end implementation over speculative abstractions.
- Restore the terminal cleanly on normal exit and errors.

## Terminology to confirm

“Kata TUI” is interpreted as Ratatui. The exact conventional editor reference is
unclear; for now it means familiar non-modal editing. Confirm these interpretations
before implementing behavior that depends on a specific alternative.
