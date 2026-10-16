//! sf: the search functions in one terminal window.

mod actions;
mod app;
mod commands;
mod config;
mod doctor;
mod engines;
mod filter;
mod items;
mod model;
mod platform;
mod preview;
mod runner;
#[cfg(test)]
mod testutil;
mod theme;
mod ui;

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io::stdout;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use anyhow::Context;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};

use crate::actions::{Env, Outcome, perform};
use crate::app::{App, Effect};
use crate::commands::{Cmd, ListPlan};
use crate::model::{Options, Tab};
use crate::preview::Previewer;
use crate::runner::Runner;

const USAGE: &str = "usage: sf [files|content|folders|git]
       sf open|reveal|copy-path PATH
       sf copy-text TEXT
       sf doctor

The search functions in one window: Files, Content (search inside files), Folders and Git. Tab / Shift-Tab or
a click switch the tab; ⚙ Config (or Ctrl-S) holds the tab's options; Esc quits. The keys for the
selected item are listed at the bottom. Options are kept in $XDG_CONFIG_HOME/sf/config.toml
(~/.config/sf/config.toml).

The other forms do one action as the window does, on Linux and on WSL alike: open PATH with its default
application, reveal it in the file manager, copy PATH (on WSL its Windows form) or TEXT to the
clipboard. doctor shows what sf finds here and how each action works.
";

/// How long the loop waits for a key before it looks at the engines' output again.
const FRAME: Duration = Duration::from_millis(30);
/// Engine messages handled per frame, so a flood of rows never holds up the keys.
const MESSAGES_PER_FRAME: usize = 2000;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let words: Vec<Option<&str>> = args.iter().map(|a| a.to_str()).collect();
    let tab = match words.as_slice() {
        [] => Tab::Files,
        [Some("-h" | "--help")] => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        [Some("doctor")] => return doctor(),
        [Some(name), _] if platform::COMMANDS.contains(name) => return command(name, &args[1]),
        [Some(name), ..] if platform::COMMANDS.contains(name) => {
            let what = if *name == "copy-text" { "TEXT" } else { "PATH" };
            eprint!("sf {name}: one {what} expected\n{USAGE}");
            return ExitCode::from(2);
        }
        [Some(a)] => match Tab::from_name(a) {
            Some(tab) => tab,
            None => {
                eprint!("sf: unknown tab: {a}\n{USAGE}");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(tab) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sf: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `sf open|reveal|copy-path PATH`, `sf copy-text TEXT`: a note or the error on stderr; exit 1 on error.
fn command(name: &str, value: &OsStr) -> ExitCode {
    let result = std::env::current_dir()
        .map_err(|e| format!("reading the current folder: {e}"))
        .and_then(|cwd| platform::command(name, value, &Env::detect().platform, &cwd));
    match result {
        Ok(note) => {
            if let Some(note) = note {
                eprintln!("sf {name}: {note}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sf {name}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn doctor() -> ExitCode {
    let env = Env::detect();
    let engines = engines::discover(&env.platform.path);
    let run = |program: &Path, args: &[&str]| {
        doctor::run_program(&env.platform.path, program, args, Duration::from_secs(3))
    };
    print!(
        "{}",
        doctor::report(&env, &engines, config::default_path().as_deref(), &run)
    );
    ExitCode::SUCCESS
}

fn run(tab: Tab) -> anyhow::Result<()> {
    let root = std::env::current_dir().context("reading the current folder")?;
    let engines = engines::discover(&std::env::var_os("PATH").unwrap_or_default());
    let env = Env::detect();
    let config_path = config::default_path();
    let (options, config_error) = match config_path.as_deref().map(config::load) {
        Some(Ok(options)) => (options, None),
        Some(Err(e)) => (Options::default(), Some(e)),
        None => (
            Options::default(),
            Some("No config folder: neither XDG_CONFIG_HOME nor HOME is set".into()),
        ),
    };
    let app = App::new(root, tab, options, engines, config_error, Instant::now());
    let window = TmuxWindow::rename(&env);

    let mut terminal = ratatui::try_init().context("starting the terminal")?;
    // ratatui's hook leaves raw mode and the alternate screen; this one also ends the mouse capture
    // and gives the tmux window its name back.
    let previous = std::panic::take_hook();
    let restore_window = window.clone();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        if let Some(w) = &restore_window {
            w.restore();
        }
        previous(info);
    }));
    let result = execute!(stdout(), EnableMouseCapture)
        .context("enabling the mouse")
        .and_then(|()| event_loop(&mut terminal, app, &env, config_path.as_deref()));
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    if let Some(w) = &window {
        w.restore();
    }
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    mut app: App,
    env: &Env,
    config_path: Option<&Path>,
) -> anyhow::Result<()> {
    let (list_tx, list_rx) = channel();
    let (preview_tx, preview_rx) = channel();
    let mut runner = Runner::new(list_tx);
    let previewer = Previewer::new(preview_tx);
    let mut pending: VecDeque<Effect> = app.start().into();
    let mut preview_width = 0;
    loop {
        while let Some(effect) = pending.pop_front() {
            let now = Instant::now();
            match effect {
                Effect::RunList => match app.list_plan() {
                    ListPlan::Run { cmd, kind } => runner.start(app.list_gen, cmd, kind, &app.root),
                    ListPlan::Hint(hint) => {
                        runner.cancel();
                        app.show_hint(&hint);
                    }
                },
                Effect::Preview => {
                    preview_width = app.layout.preview.width;
                    previewer.request(app.preview_gen, app.preview_plan());
                }
                Effect::SaveConfig => {
                    if let Some(path) = config_path {
                        if let Err(e) = config::save(path, &app.options) {
                            app.set_status(&format!("Options not saved: {e:#}"), true, now);
                        }
                    }
                }
                Effect::Act(kind) => {
                    let Some(item) = app.current().cloned() else {
                        continue;
                    };
                    match perform(
                        kind,
                        app.tab,
                        &item,
                        &app.query,
                        &app.root,
                        &app.engines,
                        env,
                    ) {
                        Outcome::Done(Some(message)) => app.set_status(&message, false, now),
                        Outcome::Done(None) => {}
                        Outcome::Failed(e) => app.set_status(&e, true, now),
                        Outcome::Foreground(cmds) => {
                            if let Err(e) = foreground(terminal, &cmds, &app.root) {
                                app.set_status(&format!("{e:#}"), true, Instant::now());
                            }
                        }
                    }
                }
                Effect::ChangeRoot(root) => pending.extend(app.set_root(root)),
                Effect::Quit => return Ok(()),
            }
        }

        terminal
            .draw(|f| ui::draw(f, &mut app))
            .context("drawing")?;
        if preview_width != 0
            && app.layout.preview.width != preview_width
            && app.layout.preview.width != 0
        {
            preview_width = app.layout.preview.width; // resized: bat's line band needs the new width
            pending.extend(app.refresh_preview());
        }

        if event::poll(FRAME).context("reading the keyboard")? {
            loop {
                let now = Instant::now();
                match event::read().context("reading the keyboard")? {
                    Event::Key(key) => pending.extend(app.handle_key(key, now)),
                    Event::Mouse(mouse) => pending.extend(app.handle_mouse(mouse, now)),
                    _ => {}
                }
                if pending.contains(&Effect::Quit)
                    || !event::poll(Duration::ZERO).context("reading the keyboard")?
                {
                    break;
                }
            }
        }
        for msg in list_rx.try_iter().take(MESSAGES_PER_FRAME) {
            pending.extend(app.on_list(msg));
        }
        for msg in preview_rx.try_iter() {
            app.on_preview(msg);
        }
        pending.extend(app.tick(Instant::now()));
    }
}

/// Hands the terminal to a program (a pipeline) and takes it back when the program ends.
fn foreground(terminal: &mut DefaultTerminal, cmds: &[Cmd], root: &Path) -> anyhow::Result<()> {
    execute!(stdout(), DisableMouseCapture).context("ending the mouse capture")?;
    ratatui::try_restore().context("giving the terminal to the program")?;
    let result = run_pipeline(cmds, root);
    enable_raw_mode().context("taking the terminal back")?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)
        .context("taking the terminal back")?;
    terminal.clear().context("redrawing")?;
    result
}

fn run_pipeline(cmds: &[Cmd], root: &Path) -> anyhow::Result<()> {
    // Ctrl-C belongs to the program now: sf ignores it meanwhile, the program gets the default action.
    // SAFETY: signal(2) with SIG_IGN / SIG_DFL; the previous handlers are put back below.
    let (old_int, old_quit) = unsafe {
        (
            libc::signal(libc::SIGINT, libc::SIG_IGN),
            libc::signal(libc::SIGQUIT, libc::SIG_IGN),
        )
    };
    let mut children = Vec::new();
    let mut stdin: Option<std::process::ChildStdout> = None;
    let mut result = Ok(());
    for (i, cmd) in cmds.iter().enumerate() {
        let last = i + 1 == cmds.len();
        let mut command = Command::new(&cmd.program);
        command
            .args(&cmd.args)
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .current_dir(root)
            .stdin(stdin.take().map_or_else(Stdio::inherit, Stdio::from))
            .stdout(if last {
                Stdio::inherit()
            } else {
                Stdio::piped()
            });
        // SAFETY: only async-signal-safe calls (signal) between fork and exec.
        unsafe {
            command.pre_exec(|| {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::signal(libc::SIGQUIT, libc::SIG_DFL);
                Ok(())
            });
        }
        match command.spawn() {
            Ok(mut child) => {
                stdin = child.stdout.take();
                children.push(child);
            }
            Err(e) => {
                result = Err(anyhow::anyhow!("{}: {e}", cmd.program.display()));
                break;
            }
        }
    }
    drop(stdin);
    for mut child in children {
        let _ = child.wait();
    }
    // SAFETY: as above.
    unsafe {
        libc::signal(libc::SIGINT, old_int);
        libc::signal(libc::SIGQUIT, old_quit);
    }
    result
}

/// The tmux window sf runs in: named "search" while sf runs, its own name again afterwards.
#[derive(Clone)]
struct TmuxWindow {
    /// sf's pane ($TMUX_PANE), so the right window is renamed even when another one is current.
    pane: Option<OsString>,
    name: String,
}

impl TmuxWindow {
    fn tmux(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("tmux");
        cmd.arg(args[0]);
        if let Some(pane) = &self.pane {
            cmd.arg("-t").arg(pane);
        }
        cmd.args(&args[1..])
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }

    fn rename(env: &Env) -> Option<TmuxWindow> {
        if !env.platform.tmux {
            return None;
        }
        let mut w = TmuxWindow {
            pane: std::env::var_os("TMUX_PANE").filter(|p| !p.is_empty()),
            name: String::new(),
        };
        let out = w
            .tmux(&["display-message", "-p", "#W"])
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        w.name = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string();
        let _ = w
            .tmux(&["rename-window", "search"])
            .stdout(Stdio::null())
            .status();
        Some(w)
    }

    fn restore(&self) {
        let _ = self
            .tmux(&["rename-window", "--", &self.name])
            .stdout(Stdio::null())
            .status();
    }
}
