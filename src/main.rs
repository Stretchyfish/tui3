mod app;
mod catalog;
mod chat;
mod composer;
mod diff;
mod history;
mod input;
mod message;
mod models;
mod pairing;
mod preferences;
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
    let runtime = tokio::runtime::Runtime::new()?;
    let backend = t3::start(&runtime);
    let mut resume = preferences::ResumeStore::load();
    let mut terminal = ratatui::init();
    let result = execute!(
        terminal.backend_mut(),
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .and_then(|_| {
        // Keep click reporting, but disable Crossterm's pointer-motion reporting.
        #[cfg(not(windows))]
        execute!(
            terminal.backend_mut(),
            crossterm::style::Print("\x1b[?1003l\x1b[?1002l\x1b[?1000h")
        )?;
        run(&mut terminal, backend, &mut resume)
    });
    let _ = execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
    ratatui::restore();
    resume.flush();
    if let Some(warning) = resume.warning {
        eprintln!("Could not save UI preferences: {warning}");
    }
    runtime.shutdown_timeout(Duration::from_millis(100));
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    backend: t3::Backend,
    resume: &mut preferences::ResumeStore,
) -> io::Result<()> {
    let mut app = app::App {
        typing_style: resume.typing_style(),
        ..app::App::default()
    };
    while !app.quit {
        while let Ok(update) = backend.updates.try_recv() {
            let update = match update {
                t3::Update::RunDiff {
                    thread_id,
                    run_id,
                    result,
                } => {
                    if app.history_thread.as_deref() == Some(&thread_id) {
                        match result {
                            Ok(patch) => {
                                app.chat_revision = app.chat_revision.wrapping_add(1);
                                app.run_diffs
                                    .entry(run_id.clone())
                                    .or_insert_with(|| crate::history::diff_counts(&patch));
                                app.diff_view.patches.insert(run_id, patch.into());
                            }
                            Err(error) => {
                                app.diff_view.errors.insert(run_id, error);
                            }
                        }
                    }
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
                | t3::Update::Catalog(_)
                | t3::Update::History { .. }
                | t3::Update::Sent { .. }
                | t3::Update::Live { .. }
                | t3::Update::Usage(_)
                | t3::Update::Models(_)
                | t3::Update::RunDiff { .. }
                | t3::Update::ModelChanged { .. } => {
                    unreachable!()
                }
            };
        }
        app.sync_history_selection();
        if app.authenticated
            && let Some(thread) = app.history_thread.as_ref()
        {
            let viewed = if app.workspace_tab == 0 {
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
            for run in app.runs.values() {
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
        resume.remember(&app);
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            // Discard unused motion/release events in a batch instead of rendering
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
                Event::Paste(text) if app.editing => app.insert_draft(&text),
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
    Ok(())
}

fn useful_input(event: &Event) -> bool {
    use crossterm::event::{KeyEventKind, MouseButton, MouseEventKind};
    match event {
        Event::Mouse(mouse) => matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
        ),
        Event::Key(key) => key.kind != KeyEventKind::Release,
        Event::FocusGained | Event::FocusLost => false,
        _ => true,
    }
}
