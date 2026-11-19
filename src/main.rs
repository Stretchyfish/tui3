mod app;
mod catalog;
mod chat;
mod clipboard;
mod commit_message;
mod composer;
mod dictation;
mod diff;
mod git;
mod history;
mod input;
mod message;
mod models;
mod pairing;
mod preferences;
mod startup;
mod t3;
mod ui;
mod usage;

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event,
    },
    execute,
};
use std::{io, time::Duration};

fn main() -> io::Result<()> {
    let _instance_guard = startup::InstanceGuard::register();
    let runtime = tokio::runtime::Runtime::new()?;
    let mut resume = preferences::ResumeStore::load();
    let startup = startup::spawn(&runtime, resume.auto_start_t3());
    let backend = t3::start(&runtime);
    let mut terminal = ratatui::init();
    let result = execute!(
        terminal.backend_mut(),
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .and_then(|_| {
        // Track drags for text selection without reporting idle pointer motion.
        #[cfg(not(windows))]
        execute!(
            terminal.backend_mut(),
            crossterm::style::Print("\x1b[?1003l\x1b[?1002h\x1b[?1000h")
        )?;
        run(&mut terminal, backend, &mut resume, startup)
    });
    let _ = execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
    ratatui::restore();
    if let Ok(Some(pid)) = &result {
        startup::shutdown_local_server(*pid);
    }
    resume.flush();
    if let Some(warning) = resume.warning {
        eprintln!("Could not save UI preferences: {warning}");
    }
    runtime.shutdown_timeout(Duration::from_millis(100));
    result.map(|_| ())
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    backend: t3::Backend,
    resume: &mut preferences::ResumeStore,
    startup: startup::Backend,
) -> io::Result<Option<u32>> {
    let mut app = app::App {
        typing_style: resume.typing_style(),
        auto_start_t3: resume.auto_start_t3(),
        check_other_instances: Some(startup::other_tui3_instances_running),
        ..app::App::default()
    };
    let (paste_sender, paste_results) = std::sync::mpsc::channel();
    let clipboard_writer = clipboard::Writer::start();
    let (context_paste_sender, context_paste_results) =
        std::sync::mpsc::channel::<(String, u64, Result<clipboard::Content, String>)>();
    let (clipboard_sender, clipboard_results) = std::sync::mpsc::channel();
    while !app.quit {
        while let Ok(result) = clipboard_writer.results.try_recv() {
            match result {
                Ok(()) => {
                    app.clipboard_status = app::ClipboardStatus::Available;
                    app.send_notice = Some("Selected text copied.".into());
                }
                Err(error) => {
                    app.clipboard_status = app::ClipboardStatus::Unavailable(error.clone());
                    app.send_notice = Some(error);
                }
            }
        }
        if let Some(text) = app.copy_requested.take()
            && let Err(error) = clipboard_writer.copy(text)
        {
            app.send_notice = Some(error);
        }
        while let Ok(result) = clipboard_results.try_recv() {
            app.clipboard_check_pending = false;
            app.clipboard_status = match result {
                Ok(()) => app::ClipboardStatus::Available,
                Err(error) => app::ClipboardStatus::Unavailable(error),
            };
        }
        if app.clipboard_check_requested && !app.clipboard_check_pending {
            app.clipboard_check_requested = false;
            app.clipboard_check_pending = true;
            app.clipboard_status = app::ClipboardStatus::Checking;
            let sender = clipboard_sender.clone();
            std::thread::spawn(move || {
                let _ = sender.send(clipboard::status());
            });
        }
        while let Ok((thread, generation, result)) = paste_results.try_recv() {
            if generation != app.paste_generation {
                continue;
            }
            app.paste_pending = false;
            match result {
                Ok(image) => app.attach_clipboard_image(thread, image),
                Err(error) => app.send_notice = Some(error),
            }
        }
        while let Ok((thread, generation, result)) = context_paste_results.try_recv() {
            if generation != app.paste_generation {
                continue;
            }
            app.paste_pending = false;
            match result {
                Ok(clipboard::Content::Text(text)) => {
                    if app.insert_into_thread_draft(&thread, &text) {
                        app.focus = app::Focus::Composer;
                        app.editing = true;
                        app.send_notice = Some("Clipboard text pasted into Message.".into());
                    } else {
                        app.send_notice =
                            Some("Clipboard text added to the original thread's draft.".into());
                    }
                }
                Ok(clipboard::Content::Image(image)) => app.attach_clipboard_image(thread, image),
                Err(error) => app.send_notice = Some(error),
            }
        }
        if app.context_paste_requested {
            app.context_paste_requested = false;
            if app.paste_pending {
                app.send_notice = Some("Wait for the current clipboard paste to finish.".into());
            } else if let Some(thread) = app.draft_thread.clone() {
                app.paste_pending = true;
                app.send_notice = Some("Reading desktop clipboard…".into());
                let sender = context_paste_sender.clone();
                let generation = app.paste_generation;
                std::thread::spawn(move || {
                    let _ = sender.send((thread, generation, clipboard::content()));
                });
            } else {
                app.send_notice = Some("Select a thread before pasting into Message.".into());
            }
        }
        if app.paste_requested {
            app.paste_requested = false;
            if let Some(thread) = app.draft_thread.clone() {
                app.paste_pending = true;
                app.send_notice = Some("Reading clipboard image…".into());
                let sender = paste_sender.clone();
                let generation = app.paste_generation;
                std::thread::spawn(move || {
                    let _ = sender.send((thread, generation, clipboard::image()));
                });
            }
        }
        while let Ok(event) = startup.updates.try_recv() {
            if !event.busy {
                app.local_server_pid = event.local_server_pid;
            }
            app.startup.apply(event);
        }
        startup.auto_start.send_if_modified(|value| {
            if *value == app.auto_start_t3 {
                false
            } else {
                *value = app.auto_start_t3;
                true
            }
        });
        if let Some(command) = app.startup.request.take() {
            if startup.commands.try_send(command).is_ok() {
                app.startup.event.busy = true;
                app.startup.event.message = "Checking T3 setup…".into();
            } else {
                app.startup.event.detail = "Setup is already busy. Please wait.".into();
            }
        }
        while let Ok(update) = backend.updates.try_recv() {
            let update = match update {
                t3::Update::Git { request, result } => {
                    app.git.finish(&request, result);
                    continue;
                }
                t3::Update::GitProgress { id, cwd, message } => {
                    if app.git.pending.as_deref() == Some(&id)
                        && app.git.cwd.as_deref() == Some(&cwd)
                    {
                        app.git.notice = message;
                    }
                    continue;
                }
                t3::Update::RunDiff {
                    thread_id,
                    run_id,
                    result,
                } => {
                    app.finish_diff(thread_id, run_id, result);
                    continue;
                }
                t3::Update::Usage(result) => {
                    match result {
                        Ok(providers) if app.authenticated => app.usage.apply(providers),
                        Ok(_) => (),
                        Err(error) => app.usage.error = Some(error),
                    }
                    continue;
                }
                t3::Update::Models(result) => {
                    app.models.loading = false;
                    match result {
                        Ok(choices) if app.authenticated => {
                            app.models.choices = choices;
                            app.models.error = None;
                            app.select_current_model();
                        }
                        Ok(_) => (),
                        Err(error) => app.models.error = Some(error),
                    }
                    continue;
                }
                t3::Update::ModelChanged { request, result } => {
                    app.finish_model_change(request, result);
                    continue;
                }
                t3::Update::ThreadAction { action, result } => {
                    app.finish_thread_action(action, result);
                    continue;
                }
                t3::Update::Live { thread_id, result } => {
                    app.sync_history_selection();
                    app.apply_live(&thread_id, result);
                    continue;
                }
                t3::Update::Sent { request, result } => {
                    app.sync_history_selection();
                    app.finish_send(request, result);
                    continue;
                }
                t3::Update::History { thread_id, result } => {
                    // Synchronize first: a queued result can arrive after keyboard selection changed.
                    app.sync_history_selection();
                    app.apply_history(&thread_id, result);
                    continue;
                }
                t3::Update::Catalog(result) => {
                    match result {
                        Ok(catalog) => app.update_catalog(catalog),
                        Err(error) => app.catalog_error = Some(error),
                    }
                    continue;
                }
                t3::Update::Dictation(event) => {
                    app.apply_dictation(event);
                    continue;
                }
                t3::Update::Pairing(message) => {
                    app.pairing_message = message;
                    app.pairing_in_progress = false;
                    continue;
                }
                update => update,
            };
            app.connection = match update {
                t3::Update::Ready(connection) => {
                    resume.activate(&connection.environment.environment_id, &mut app);
                    app.environment_label = Some(connection.environment.label.clone());
                    if app.local_server_pid.is_none() {
                        app.local_server_pid = startup::local_server_pid();
                    }
                    app.authenticated = connection.authenticated;
                    if app.authenticated {
                        app.pairing_in_progress = false;
                        app.pairing_message = "Signed in to T3.".into();
                    }
                    if !app.authenticated {
                        app.clear_catalog();
                        app.pairing_message = "Press p to pair with your local T3 server.".into();
                    }
                    let auth = if connection.authenticated {
                        "Authenticated"
                    } else {
                        "Authentication required"
                    };
                    format!(
                        "{} | T3 {} | {auth}",
                        connection.environment.label, connection.environment.server_version
                    )
                }
                t3::Update::Failed(error) => {
                    // Preserve the conversation on transient connection failures. Explicit
                    // session rejection is handled by Ready(authenticated = false).
                    app.catalog_error = Some(error.clone());
                    format!("Disconnected | {error}")
                }
                t3::Update::Pairing(_)
                | t3::Update::Dictation(_)
                | t3::Update::Catalog(_)
                | t3::Update::History { .. }
                | t3::Update::Sent { .. }
                | t3::Update::Live { .. }
                | t3::Update::Usage(_)
                | t3::Update::Models(_)
                | t3::Update::Git { .. }
                | t3::Update::GitProgress { .. }
                | t3::Update::RunDiff { .. }
                | t3::Update::ModelChanged { .. }
                | t3::Update::ThreadAction { .. } => {
                    unreachable!()
                }
            };
        }
        app.sync_history_selection();
        git::sync(&mut app);
        if let Some(request) = app.git.request.take()
            && backend.git.try_send(request.clone()).is_err()
        {
            app.git.finish(
                &request,
                Err("Git worker unavailable. Press r to retry.".into()),
            );
        }
        if app.authenticated
            && let Some(thread) = app.history_thread.as_ref()
        {
            let viewed = if app.workspace_visible && app.workspace_tab == 0 {
                app.diff_view.run_id(&app.runs).map(str::to_owned)
            } else {
                None
            };
            let missing: std::collections::HashSet<_> = app
                .transcript
                .as_ref()
                .into_iter()
                .flat_map(|history| history.items.iter())
                .filter(|entry| entry.item.kind == "file_change")
                .filter_map(|entry| entry.item.run_id.as_ref())
                .filter(|id| !app.run_diffs.contains_key(*id))
                .collect();
            let mut runs: Vec<_> = app.runs.values().collect();
            runs.sort_by_key(|run| {
                (
                    viewed.as_ref() != Some(&run.id),
                    std::cmp::Reverse(run.ordinal),
                )
            });
            for run in runs {
                if run.status != "completed" || app.diff_requested.contains(&run.id) {
                    continue;
                }
                let Some(ordinal) = run.ordinal else {
                    continue;
                };
                if (viewed.as_ref() == Some(&run.id) || missing.contains(&run.id))
                    && backend
                        .diffs
                        .try_send((thread.clone(), run.id.clone(), ordinal))
                        .is_ok()
                {
                    app.diff_requested.insert(run.id.clone());
                }
            }
        }
        backend.selected_thread.send_if_modified(|selected| {
            if *selected == app.history_thread {
                false
            } else {
                selected.clone_from(&app.history_thread);
                true
            }
        });
        app.check_dictation_limit();
        if let Some(action) = app.thread_action_requested.take()
            && backend.thread_actions.try_send(action.clone()).is_err()
        {
            app.finish_thread_action(action, Err("T3 thread action queue is busy.".into()));
        }
        if let Some(command) = app.take_dictation_command()
            && backend.dictation.try_send(command).is_err()
        {
            app.apply_dictation(dictation::Event::Finished(Err(
                "Dictation is busy. Please wait and try again.".into(),
            )));
        }
        resume.remember(&app);
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            // Discard unused motion events in a batch instead of rendering
            // the full transcript for each queued pointer event.
            let mut next = event::read()?;
            let mut ignored = 0;
            while !useful_input(&next) && ignored < 256 && event::poll(Duration::ZERO)? {
                next = event::read()?;
                ignored += 1;
            }
            match next {
                Event::Key(key) => input::handle_key(&mut app, key),
                Event::Mouse(mouse) => input::handle_mouse(&mut app, mouse),
                Event::Paste(text)
                    if !app.terminal_overlay_open
                        && app.git.editing
                        && app.focus == app::Focus::Workspace
                        && app.workspace_tab == 3 =>
                {
                    app.git.message.push_str(&text.replace(['\r', '\n'], " "))
                }
                Event::Paste(text) if app.editing && !app.terminal_overlay_open => {
                    app.insert_draft(&text)
                }
                _ => (),
            }
            if app.models.load_requested {
                app.models.load_requested = false;
                if backend.models.try_send(models::ModelCommand::Load).is_err() {
                    app.models.loading = false;
                    app.models.error =
                        Some("Model catalog is unavailable. Press r to retry.".into());
                }
            }
            if app.models.change_requested {
                app.models.change_requested = false;
                if let Some(request) = app.prepare_model_change()
                    && backend
                        .models
                        .try_send(models::ModelCommand::Set(request.clone()))
                        .is_err()
                {
                    app.finish_model_change(
                        request,
                        Err("Model changes are unavailable. Please retry.".into()),
                    );
                }
            }
            if app.send_requested {
                app.send_requested = false;
                if let Some(request) = app.prepare_send()
                    && backend.messages.try_send(request.clone()).is_err()
                {
                    app.finish_send(
                        request,
                        Err("Message sender is unavailable. Your draft is kept.".into()),
                    );
                }
            }
            if app.pairing_requested {
                app.pairing_requested = false;
                match backend.pairing.try_send(()) {
                    Ok(()) => {
                        app.pairing_in_progress = true;
                        app.pairing_message = "Pairing with local T3…".into();
                    }
                    Err(_) => {
                        app.pairing_message =
                            "Pairing is unavailable. Check the server configuration and restart."
                                .into()
                    }
                }
            }
        }
    }
    resume.remember(&app);
    Ok(app.server_to_shutdown())
}

fn useful_input(event: &Event) -> bool {
    use crossterm::event::{KeyEventKind, MouseButton, MouseEventKind};
    match event {
        Event::Mouse(mouse) => matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Down(MouseButton::Right)
                | MouseEventKind::Drag(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left)
                | MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
        ),
        Event::Key(key) => key.kind != KeyEventKind::Release,
        Event::FocusGained | Event::FocusLost => false,
        _ => true,
    }
}
