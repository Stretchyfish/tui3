# Additional Features

- [x] ctrl + backspace in message should delete entire words
- [x] ctrl + a should select all the text
- [x] if the user clicks enter while dictation is active it finishes dictation and sends the message immediately.
- [x] Prompt the user to choose whether to shut down the T3 server on close if all instances of tui3 are down (defaulting to yes).
- [x] Show which thread is currently running in the project menu (e.g., a green indicator showing it is currently doing work).
- [x] if the user clicks enter while in the project panel on a thread it open another sub panel for more options, for now lets go with rename & delete.
- [x] Add an access level panel next to the model selector, and show approval requests in chat with an interactive pop-up prompt for the user to respond to.
- [x] Show queued messages at the bottom of the chat in a distinct color to highlight that they are not active yet.
- [x] Swap the Editor and Diffs workspace panels so Editor is the leftmost tab.
- [x] Investigate and refine the steering functionality (user is not convinced it is actually working).
- [x] Add layout presets to Settings (starting with two layouts: the current default layout, and an editor-focused layout with a narrower chat window and a wider workspace/editor panel, similar to resizing with `{`/`}`).
- [x] Remove the Tab hotkey for switching between panels, along with the Tab hint text in the bottom footer.
- [x] Overhaul `README.md` to be concise (short overview + easy `curl` install instructions; discuss details when we get there).
- [x] Overhaul the Git panel so instead of inline commit message / commit-all fields, it shows the three actions (with Push as the third), and selecting one opens a modal/overlay panel on top with the additional fields to fill out (e.g., generate commit message).
