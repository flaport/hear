use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::config::Config;
use crate::delivery;
use crate::recording::Recorder;
use crate::transcriber;

const ICON_SIZE: u16 = 22;
const SYSTEM_TRAY_REQUEST_DOCK: u32 = 0;

const COLOR_IDLE: u32 = 0xFFF2F2F2;
const COLOR_RECORDING: u32 = 0xFFCC3333;
const COLOR_TRANSCRIBING: u32 = 0xFFCC9933;

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
    icon_window: Window,
    state: State,
    paste: bool,
    config: Config,
    event_rx: mpsc::Receiver<AppEvent>,
    event_tx: mpsc::Sender<AppEvent>,
    hotkey: HotKey,
    _hotkey_manager: Option<GlobalHotKeyManager>,
}

impl App {
    pub fn run(config: Config) -> Result<()> {
        let (conn, screen_num) =
            RustConnection::connect(None).context("could not connect to X11 display")?;
        let screen = &conn.setup().roots[screen_num];

        let tray_visual = find_tray_visual(&conn, screen);
        let (depth, visual, colormap) = match tray_visual {
            Some((d, v)) => {
                let cmap = conn.generate_id()?;
                conn.create_colormap(ColormapAlloc::NONE, cmap, screen.root, v)?;
                (d, v, Some(cmap))
            }
            None => (screen.root_depth, screen.root_visual, None),
        };

        let icon_window = conn.generate_id()?;
        let mut aux = CreateWindowAux::new()
            .background_pixel(COLOR_IDLE)
            .override_redirect(1)
            .event_mask(
                EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::STRUCTURE_NOTIFY,
            );
        if let Some(cmap) = colormap {
            aux = aux.colormap(cmap).border_pixel(0);
        }
        conn.create_window(
            depth,
            icon_window,
            screen.root,
            0,
            0,
            ICON_SIZE,
            ICON_SIZE,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &aux,
        )?;

        let xembed_info_atom = conn
            .intern_atom(false, b"_XEMBED_INFO")?
            .reply()
            .context("could not intern _XEMBED_INFO")?
            .atom;
        conn.change_property32(
            PropMode::REPLACE,
            icon_window,
            xembed_info_atom,
            xembed_info_atom,
            &[1, 1],
        )?;
        conn.change_property8(
            PropMode::REPLACE,
            icon_window,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            b"hear-app\0Hear-app",
        )?;
        conn.change_property8(
            PropMode::REPLACE,
            icon_window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"hear-app",
        )?;
        conn.flush()?;

        request_dock(&conn, icon_window)?;

        let hotkey = config.hotkey;
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => match manager.register(hotkey) {
                Ok(()) => Some(manager),
                Err(error) => {
                    eprintln!("Could not register {hotkey}: {error}. Use the tray icon instead.");
                    None
                }
            },
            Err(error) => {
                eprintln!(
                    "Could not initialize global hotkeys: {error}. Use the tray icon instead."
                );
                None
            }
        };

        let (event_tx, event_rx) = mpsc::channel();
        let mut app = Self {
            conn,
            icon_window,
            state: State::Idle,
            paste: config.paste_automatically,
            config,
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
                    x11rb::protocol::Event::DestroyNotify(event)
                        if event.window == self.icon_window =>
                    {
                        return Ok(());
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
                    let _ = self.update_icon();
                }
                Err(error) => {
                    self.state = State::Idle;
                    eprintln!("Could not record: {error:#}");
                    let _ = self.update_icon();
                }
            },
            State::Recording(recorder) => match recorder.finish() {
                Ok(recording) => {
                    eprintln!("Transcribing…");
                    let _ = self.update_icon();
                    let tx = self.event_tx.clone();
                    transcriber::transcribe_async(recording, tx, self.config.hear_options.clone());
                }
                Err(error) => {
                    self.state = State::Idle;
                    eprintln!("Could not finish recording: {error:#}");
                    let _ = self.update_icon();
                }
            },
            State::Transcribing => {
                self.state = State::Transcribing;
            }
        }
    }

    fn transcription_finished(&mut self, result: Result<String, String>) {
        self.state = State::Idle;
        let _ = self.update_icon();
        match result {
            Ok(transcript) => match delivery::deliver(&transcript, self.paste, &self.config) {
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

    fn update_icon(&self) -> Result<()> {
        self.conn
            .clear_area(false, self.icon_window, 0, 0, ICON_SIZE, ICON_SIZE)?;
        self.draw_icon()
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
            &CreateGCAux::new().foreground(color).line_width(2),
        )?;
        // Microphone capsule.
        self.conn.poly_fill_rectangle(
            self.icon_window,
            gc,
            &[Rectangle {
                x: 8,
                y: 5,
                width: 6,
                height: 7,
            }],
        )?;
        self.conn.poly_fill_arc(
            self.icon_window,
            gc,
            &[
                Arc {
                    x: 8,
                    y: 2,
                    width: 6,
                    height: 6,
                    angle1: 0,
                    angle2: 360 * 64,
                },
                Arc {
                    x: 8,
                    y: 9,
                    width: 6,
                    height: 6,
                    angle1: 0,
                    angle2: 360 * 64,
                },
            ],
        )?;
        // Pickup cradle and stand.
        self.conn.poly_line(
            CoordMode::ORIGIN,
            self.icon_window,
            gc,
            &[
                Point { x: 5, y: 8 },
                Point { x: 5, y: 10 },
                Point { x: 7, y: 13 },
                Point { x: 9, y: 15 },
                Point { x: 13, y: 15 },
                Point { x: 15, y: 13 },
                Point { x: 17, y: 10 },
                Point { x: 17, y: 8 },
            ],
        )?;
        self.conn.poly_line(
            CoordMode::ORIGIN,
            self.icon_window,
            gc,
            &[
                Point { x: 11, y: 15 },
                Point { x: 11, y: 19 },
                Point { x: 8, y: 19 },
                Point { x: 14, y: 19 },
            ],
        )?;
        self.conn.free_gc(gc)?;
        self.conn.flush()?;
        Ok(())
    }
}

fn find_tray_visual(conn: &RustConnection, screen: &Screen) -> Option<(u8, Visualid)> {
    let tray_atom = conn
        .intern_atom(false, b"_NET_SYSTEM_TRAY_S0")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let visual_atom = conn
        .intern_atom(false, b"_NET_SYSTEM_TRAY_VISUAL")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let tray_owner = conn
        .get_selection_owner(tray_atom)
        .ok()?
        .reply()
        .ok()?
        .owner;
    if tray_owner == x11rb::NONE {
        return None;
    }
    let prop = conn
        .get_property(false, tray_owner, visual_atom, AtomEnum::VISUALID, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let visual_id = prop.value32()?.next()?;
    for depth_info in &screen.allowed_depths {
        for vis in &depth_info.visuals {
            if vis.visual_id == visual_id {
                return Some((depth_info.depth, visual_id));
            }
        }
    }
    None
}

fn request_dock(conn: &RustConnection, icon_window: Window) -> Result<()> {
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
            "no system tray is running (no owner for _NET_SYSTEM_TRAY_S0). \
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
            [
                x11rb::CURRENT_TIME,
                SYSTEM_TRAY_REQUEST_DOCK,
                icon_window,
                0,
                0,
            ],
        ),
    )?;
    // The tray manager maps the window after embedding it. Mapping it here can
    // race with reparenting and make some managers discard the icon on the
    // resulting UnmapNotify event.
    conn.flush()?;
    Ok(())
}
