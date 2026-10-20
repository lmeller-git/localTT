//! The terminal task: input thread, effect `JoinSet`, event stream and draw loop
//! (`docs/rfcs/server.md` 3).

use std::{
    io::{self, Stdout, stdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crossterm::{
    cursor::Show,
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use localtt_core::types::PlayerId;
use ratatui::{Terminal, backend::CrosstermBackend};
use thiserror::Error;
use tokio::{sync::mpsc, task::JoinSet};

use crate::{
    app::AppHandle,
    error::AppError,
    tui::{
        render,
        state::{Effect, TuiMsg, TuiState},
    },
};

/// A TUI setup or runtime failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TuiError {
    /// The actor or a store read failed.
    #[error(transparent)]
    App(#[from] AppError),
    /// Terminal setup, drawing or restoration failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Runs the TUI until the user quits.
///
/// Performs the startup reads, subscribes to the projected stream, then owns
/// the terminal and input thread for the lifetime of the screen.
///
/// # Errors
///
/// [`TuiError::App`] for startup actor failures and [`TuiError::Io`] for
/// terminal failures.
#[inline]
pub async fn run(app: AppHandle, actor: PlayerId) -> Result<(), TuiError> {
    let result = run_screen(&app, actor).await;
    let shutdown = app.shutdown().await;
    result?;
    shutdown?;
    Ok(())
}

async fn run_screen(app: &AppHandle, actor: PlayerId) -> Result<(), TuiError> {
    let mut stream = app.subscribe(actor).await?;
    let campaign = app.campaign(actor).await?;
    let entities = app.list_entities(actor).await?;
    let feed = app.feed_after(actor, None, 200).await?;
    let mut state = TuiState::new(actor, campaign, entities, feed);
    let mut session = TerminalSession::enter()?;

    let (input_sender, mut input_receiver) = mpsc::channel(64);
    let stop = Arc::new(AtomicBool::new(false));
    let input_stop = Arc::clone(&stop);
    let input_thread = std::thread::Builder::new()
        .name("localtt-input".to_owned())
        .spawn(move || {
            while !input_stop.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => match event::read() {
                        Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                            if input_sender.blocking_send(TuiMsg::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(Event::Resize(_, _)) => {
                            if input_sender.blocking_send(TuiMsg::Resize).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    },
                    Ok(false) => {}
                    Err(_) => break,
                }
            }
        })?;

    let result = event_loop(
        app,
        actor,
        &mut session,
        &mut state,
        &mut stream,
        &mut input_receiver,
    )
    .await;

    stop.store(true, Ordering::Relaxed);
    let _ = input_thread.join();
    drop(session);
    result
}

async fn event_loop(
    app: &AppHandle,
    actor: PlayerId,
    session: &mut TerminalSession,
    state: &mut TuiState,
    stream: &mut crate::app::EventStream,
    input: &mut mpsc::Receiver<TuiMsg>,
) -> Result<(), TuiError> {
    let mut tasks = JoinSet::new();
    for effect in state.initial_effects() {
        spawn_effect(&mut tasks, app, actor, effect);
    }
    loop {
        session.terminal.draw(|frame| render::draw(frame, state))?;
        tokio::select! {
            Some(message) = input.recv() => {
                for effect in state.update(message) {
                    spawn_effect(&mut tasks, app, actor, effect);
                }
            }
            Some(event) = stream.recv() => {
                for effect in state.update(TuiMsg::Stream(event)) {
                    spawn_effect(&mut tasks, app, actor, effect);
                }
            }
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                match result {
                    Ok(message) => {
                        for effect in state.update(message) {
                            spawn_effect(&mut tasks, app, actor, effect);
                        }
                    }
                    Err(error) => {
                        state.status = format!("task failed: {error}");
                    }
                }
            }
            else => {
                state.status = "input and events closed".to_owned();
                state.should_quit = true;
            }
        }
        if state.should_quit {
            return Ok(());
        }
    }
}

fn spawn_effect(tasks: &mut JoinSet<TuiMsg>, app: &AppHandle, actor: PlayerId, effect: Effect) {
    let app = app.clone();
    tasks.spawn(async move {
        match effect {
            Effect::LoadCampaign => TuiMsg::Campaign(app.campaign(actor).await),
            Effect::LoadEntities => TuiMsg::Entities(app.list_entities(actor).await),
            Effect::LoadFeed { cursor } => TuiMsg::Feed(app.feed_after(actor, cursor, 200).await),
            Effect::Execute(command) => TuiMsg::Executed(app.execute(actor, command).await),
            Effect::LoadSheet { id } => TuiMsg::Sheet {
                id,
                result: app.sheet(actor, id).await,
            },
            Effect::Save => TuiMsg::Saved(app.save().await),
        }
    });
}

/// Installs a panic hook that restores the terminal before the default hook
/// prints the panic.
///
/// Call once before entering the TUI. The previous hook is preserved and still
/// runs.
#[inline]
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, Show);
        previous(info);
    }));
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = execute!(stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
        let _ = self.terminal.show_cursor();
    }
}
