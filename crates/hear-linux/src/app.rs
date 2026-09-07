use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

use crate::delivery;
use crate::recording::Recorder;
use crate::transcriber;

const ICON_SIZE: u16 = 22;
const SYSTEM_TRAY_REQUEST_DOCK: u32 = 0;

const COLOR_IDLE: u32 = 0x666666;
const COLOR_RECORDING: u32 = 0xCC3333;
const COLOR_TRANSCRIBING: u32 = 0xCC9933;

pub enum AppEvent {
    TranscriptionFinished(Result<String, String>),
}

enum State {
    Idle,
    Recording(Recorder),
    Transcribing,
}

pub struct App {
    conn: RustConnection,
    screen_num: usize,
    icon_window: Window,
    state: State,
    paste: bool,
    event_rx: mpsc::Receiver<AppEvent>,
    event_tx: mpsc::Sender<AppEvent>,
    hotkey: HotKey,
    _hotkey_manager: Option<GlobalHotKeyManager>,
}

impl App {
    pub fn run() -> Result<()> {
        let (conn, screen_num) = RustConnection::connect(None)
            .context("could not connect to X11 display")?;
        let screen = &conn.setup().roots[screen_num];

        let icon_window = conn.generate_id()?;
        conn.create_window(
            screen.root_depth,
            icon_window,
            screen.root,
            0, 0,
            ICON_SIZE, ICON_SIZE,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new()
                .background_pixel(COLOR_IDLE)
                .override_redirect(1)
                .event_mask(
                    EventMask::EXPOSURE
                        | EventMask::BUTTON_PRESS
                        | EventMask::STRUCTURE_NOTIFY,
                ),
        )?;
        conn.flush()?;

        request_dock(&conn, screen, icon_window)?;

        let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => match manager.register(hotkey) {
                Ok(()) => Some(manager),
                Err(error) => {
                    eprintln!("Could not register Alt-Space: {error}. Use the tray icon instead.");
                    None
                }
            },
            Err(error) => {
                eprintln!("Could not initialize global hotkeys: {error}. Use the tray icon instead.");
                None
            }
        };

        let (event_tx, event_rx) = mpsc::channel();
        let mut app = Self {
            conn,
            screen_num,
            icon_window,
            state: State::Idle,
            paste: true,
            event_rx,
            event_tx,
            hotkey,
            _hotkey_manager: manager,
        };

        app.run_loop()
    }

    fn run_loop(&mut self) -> Result<()> {
        loop {
            while let Some(event) = self.conn.poll_for_event()? {
                match event {
                    x11rb::protocol::Event::Expose(_) => self.draw_icon()?,
                    x11rb::protocol::Event::ButtonPress(event) => {
                        if event.detail == 1 {
                            self.toggle_recording();
                        } else if event.detail == 3 {
                            self.toggle_paste();
                        }
                    }
                    x11rb::protocol::Event::DestroyNotify(event) => {
                        if event.window == self.icon_window {
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }

            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.id == self.hotkey.id() && event.state == HotKeyState::Pressed {
                    self.toggle_recording();
                }
            }

            while let Ok(event) = self.event_rx.try_recv() {
                match event {
                    AppEvent::TranscriptionFinished(result) => {
                        self.transcription_finished(result);
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn toggle_recording(&mut self) {
        match std::mem::replace(&mut self.state, State::Transcribing) {
            State::Idle => match Recorder::start() {
                Ok(recorder) => {
                    self.state = State::Recording(recorder);
                    eprintln!("Recording…");
                    let _ = self.update_icon(COLOR_RECORDING);
                }
                Err(error) => {
                    self.state = State::Idle;
                    eprintln!("Could not record: {error:#}");
                    let _ = self.update_icon(COLOR_IDLE);
                }
            },
            State::Recording(recorder) => match recorder.finish() {
                Ok(recording) => {
                    eprintln!("Transcribing…");
                    let _ = self.update_icon(COLOR_TRANSCRIBING);
                    let tx = self.event_tx.clone();
                    transcriber::transcribe_async(recording, tx);
                }
                Err(error) => {
                    self.state = State::Idle;
                    eprintln!("Could not finish recording: {error:#}");
                    let _ = self.update_icon(COLOR_IDLE);
                }
            },
            State::Transcribing => {
                self.state = State::Transcribing;
            }
        }
    }

    fn transcription_finished(&mut self, result: Result<String, String>) {
        self.state = State::Idle;
        let _ = self.update_icon(COLOR_IDLE);
        match result {
            Ok(transcript) => match delivery::deliver(&transcript, self.paste) {
                Ok(true) => eprintln!("Pasted."),
                Ok(false) => eprintln!("Copied to clipboard."),
                Err(error) => eprintln!("Could not deliver transcript: {error:#}"),
            },
            Err(error) => eprintln!("Transcription failed: {error}"),
        }
    }

    fn toggle_paste(&mut self) {
        self.paste = !self.paste;
        eprintln!(
            "Paste automatically: {}",
            if self.paste { "on" } else { "off" }
        );
    }

    fn update_icon(&self, color: u32) -> Result<()> {
        let screen = &self.conn.setup().roots[self.screen_num];
        self.conn.change_window_attributes(
            self.icon_window,
            &ChangeWindowAttributesAux::new().background_pixel(color),
        )?;
        self.conn.clear_area(true, self.icon_window, 0, 0, screen.width_in_pixels, screen.height_in_pixels)?;
        self.conn.flush()?;
        Ok(())
    }

    fn draw_icon(&self) -> Result<()> {
        let color = match self.state {
            State::Idle => COLOR_IDLE,
            State::Recording(_) => COLOR_RECORDING,
            State::Transcribing => COLOR_TRANSCRIBING,
        };
        let gc = self.conn.generate_id()?;
        self.conn.create_gc(
            gc,
            self.icon_window,
            &CreateGCAux::new().foreground(color),
        )?;
        let pad = 3;
        let diameter = ICON_SIZE - 2 * pad;
        self.conn.poly_fill_arc(
            self.icon_window,
            gc,
            &[Arc {
                x: pad as i16,
                y: pad as i16,
                width: diameter,
                height: diameter,
                angle1: 0,
                angle2: 360 * 64,
            }],
        )?;
        self.conn.free_gc(gc)?;
        self.conn.flush()?;
        Ok(())
    }
}

fn request_dock(
    conn: &RustConnection,
    screen: &Screen,
    icon_window: Window,
) -> Result<()> {
    let tray_atom_name = format!("_NET_SYSTEM_TRAY_S{}", screen.root_visual);
    let tray_atom = conn
        .intern_atom(false, b"_NET_SYSTEM_TRAY_S0")?
        .reply()
        .context("could not intern _NET_SYSTEM_TRAY_S0")?
        .atom;
    let opcode_atom = conn
        .intern_atom(false, b"_NET_SYSTEM_TRAY_OPCODE")?
        .reply()
        .context("could not intern _NET_SYSTEM_TRAY_OPCODE")?
        .atom;

    let tray_owner = conn
        .get_selection_owner(tray_atom)?
        .reply()
        .context("could not find the system tray")?
        .owner;
    if tray_owner == x11rb::NONE {
        bail!(
            "no system tray is running (no owner for {tray_atom_name}). \
             Make sure your window manager has a systray enabled."
        );
    }

    conn.send_event(
        false,
        tray_owner,
        EventMask::NO_EVENT,
        ClientMessageEvent::new(
            32,
            tray_owner,
            opcode_atom,
            [x11rb::CURRENT_TIME, SYSTEM_TRAY_REQUEST_DOCK, icon_window, 0, 0],
        ),
    )?;
    conn.map_window(icon_window)?;
    conn.flush()?;
    Ok(())
}
