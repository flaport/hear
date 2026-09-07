use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::delivery;
use crate::recording::Recorder;
use crate::transcriber;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum AppEvent {
    TranscriptionFinished(Result<String, String>),
}

enum State {
    Idle,
    Recording(Recorder),
    Transcribing,
}

struct Ui {
    _tray: TrayIcon,
    status: MenuItem,
    toggle: MenuItem,
    paste: CheckMenuItem,
    quit: MenuItem,
}

pub struct App {
    proxy: EventLoopProxy<AppEvent>,
    state: State,
    ui: Option<Ui>,
    hotkey_manager: Option<GlobalHotKeyManager>,
    hotkey: HotKey,
}

impl App {
    pub fn run() -> Result<()> {
        let event_loop = EventLoop::<AppEvent>::with_user_event()
            .build()
            .context("could not create the macOS event loop")?;
        let proxy = event_loop.create_proxy();
        let mut app = Self {
            proxy,
            state: State::Idle,
            ui: None,
            hotkey_manager: None,
            hotkey: HotKey::new(Some(Modifiers::ALT), Code::Space),
        };
        event_loop
            .run_app(&mut app)
            .context("the macOS event loop failed")
    }

    fn initialize(&mut self) -> Result<()> {
        let manager = GlobalHotKeyManager::new().context("could not initialize global hotkeys")?;
        manager
            .register(self.hotkey)
            .context("could not register Option-Space")?;

        let status = MenuItem::new("Idle — Option-Space to record", false, None);
        let toggle = MenuItem::new("Start Recording", true, None);
        let paste = CheckMenuItem::new("Paste Automatically", true, true, None);
        let quit = MenuItem::new("Quit Hear", true, None);
        let separator = PredefinedMenuItem::separator();
        let menu = Menu::with_items(&[&status, &toggle, &paste, &separator, &quit])?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Hear — Idle")
            .with_icon(icon(false)?)
            .with_icon_as_template(true)
            .build()?;
        self.hotkey_manager = Some(manager);
        self.ui = Some(Ui {
            _tray: tray,
            status,
            toggle,
            paste,
            quit,
        });
        Ok(())
    }

    fn toggle_recording(&mut self) {
        match std::mem::replace(&mut self.state, State::Transcribing) {
            State::Idle => match Recorder::start() {
                Ok(recorder) => {
                    self.state = State::Recording(recorder);
                    self.set_status("Recording…", "Stop Recording", "Hear — Recording");
                }
                Err(error) => {
                    self.state = State::Idle;
                    self.show_error(&format!("Could not record: {error:#}"));
                }
            },
            State::Recording(recorder) => match recorder.finish() {
                Ok(recording) => {
                    self.set_status("Transcribing…", "Transcribing…", "Hear — Transcribing");
                    if let Some(ui) = &self.ui {
                        ui.toggle.set_enabled(false);
                    }
                    transcriber::transcribe(recording, self.proxy.clone());
                }
                Err(error) => {
                    self.state = State::Idle;
                    self.show_error(&format!("Could not finish recording: {error:#}"));
                }
            },
            State::Transcribing => {
                self.state = State::Transcribing;
            }
        }
    }

    fn transcription_finished(&mut self, result: Result<String, String>) {
        self.state = State::Idle;
        if let Some(ui) = &self.ui {
            ui.toggle.set_enabled(true);
        }
        match result {
            Ok(transcript) => {
                let paste = self.ui.as_ref().is_some_and(|ui| ui.paste.is_checked());
                match delivery::deliver(&transcript, paste) {
                    Ok(true) => self.set_status(
                        "Pasted — Option-Space to record",
                        "Start Recording",
                        "Hear — Pasted",
                    ),
                    Ok(false) => self.set_status(
                        "Copied — Option-Space to record",
                        "Start Recording",
                        "Hear — Copied",
                    ),
                    Err(error) => {
                        self.show_error(&format!("Could not deliver transcript: {error:#}"))
                    }
                }
            }
            Err(error) => self.show_error(&format!("Transcription failed: {error}")),
        }
    }

    fn set_status(&self, status: &str, toggle: &str, tooltip: &str) {
        if let Some(ui) = &self.ui {
            ui.status.set_text(status);
            ui.toggle.set_text(toggle);
            let _ = ui._tray.set_tooltip(Some(tooltip));
            let _ = ui._tray.set_icon_with_as_template(
                icon(matches!(self.state, State::Recording(_))).ok(),
                true,
            );
        }
    }

    fn show_error(&self, message: &str) {
        eprintln!("{message}");
        self.set_status(message, "Start Recording", "Hear — Error");
    }

    fn handle_menu(&mut self, event: MenuEvent, event_loop: &ActiveEventLoop) {
        let Some(ui) = &self.ui else {
            return;
        };
        if event.id == *ui.toggle.id() {
            self.toggle_recording();
        } else if event.id == *ui.quit.id() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.ui.is_none()
            && let Err(error) = self.initialize()
        {
            eprintln!("hear-macos failed to initialize: {error:#}");
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TranscriptionFinished(result) => self.transcription_finished(result),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id == self.hotkey.id() && event.state == HotKeyState::Pressed {
                self.toggle_recording();
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            self.handle_menu(event, event_loop);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + POLL_INTERVAL));
    }
}

fn icon(recording: bool) -> Result<Icon> {
    let mut rgba = vec![0_u8; 16 * 16 * 4];
    let radius = if recording { 6.5 } else { 5.0 };
    for y in 0..16 {
        for x in 0..16 {
            let distance = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
            if distance <= radius {
                let offset = (y * 16 + x) * 4;
                rgba[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, 16, 16).context("could not create the menu-bar icon")
}
