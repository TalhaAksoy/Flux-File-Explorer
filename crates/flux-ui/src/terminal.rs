//! Embedded terminal panel — a real PTY (portable-pty) driven by the
//! alacritty_terminal grid/parser, rendered as an egui bottom panel.
//!
//! Data flow: shell output → reader thread → `vte::ansi::Processor` →
//! `Term` grid (behind a Mutex) → repaint request → we draw cells.
//! Keyboard events are drained early in `ui()` while the panel is focused,
//! translated to VT bytes and written to the PTY master.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{self, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color, CursorShape, NamedColor, Processor, Rgb, StdSyncHandler,
};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

use crate::app::{Dialog, FluxApp};
use crate::theme::Palette;

/// Forwards terminal events off the parser thread to the app: clipboard
/// stores go straight to the OS clipboard, Wakeup nudges a repaint, the
/// rest land on a channel drained once per frame.
struct Proxy {
    tx: mpsc::Sender<Event>,
    ctx: egui::Context,
}

impl EventListener for Proxy {
    fn send_event(&self, e: Event) {
        match e {
            Event::Wakeup => self.ctx.request_repaint(),
            Event::ClipboardStore(_, text) => self.ctx.copy_text(text),
            _ => {
                let _ = self.tx.send(e);
            }
        }
    }
}

/// Which quoting dialect the spawned shell understands for the "↓ cwd"
/// command — detected once from the shell binary name at spawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShellKind {
    /// cmd.exe (Windows %COMSPEC% default).
    Cmd,
    /// powershell.exe / pwsh.
    PowerShell,
    /// POSIX-ish: sh, bash, zsh, fish, nushell — single-quote escaping.
    Posix,
}

/// Classify a shell from its binary path/name.
fn shell_kind(shell_path: &str) -> ShellKind {
    let exe = Path::new(shell_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if exe.contains("powershell") || exe.contains("pwsh") {
        ShellKind::PowerShell
    } else if (exe.contains("sh") && !exe.contains("cmd")) || exe.starts_with("nu") {
        // bash/zsh/fish/git-bash ("*sh*"), nushell ("nu", "nushell").
        // Checked before "cmd" since nothing sane contains both.
        ShellKind::Posix
    } else if cfg!(windows) {
        // cmd.exe and anything unrecognized under Windows.
        ShellKind::Cmd
    } else {
        ShellKind::Posix
    }
}

/// The `cd` line written to the PTY for `dir` — command text plus the
/// trailing CR that submits it, quoted for `kind`.
fn cd_command(kind: ShellKind, dir: &Path) -> String {
    match kind {
        // `cd /d` so a drive change applies too. Double quotes protect
        // spaces and cmd metachars (`&`, `^`); `"` is not a legal Windows
        // filename char, so no inner-quote escaping exists or is needed.
        // Residual edge: a directory literally named like `a%TEMP%b` still
        // undergoes cmd's %VAR% expansion — same limitation Explorer's own
        // "open command window here" historically had.
        ShellKind::Cmd => format!("cd /d \"{}\"\r", dir.display()),
        // LiteralPath avoids glob/bracket interpretation; `'` → `''`.
        ShellKind::PowerShell => format!(
            "Set-Location -LiteralPath '{}'\r",
            dir.display().to_string().replace('\'', "''")
        ),
        // POSIX single quotes, escaped as `'\''`.
        ShellKind::Posix => format!(
            "cd '{}'\r",
            dir.display().to_string().replace('\'', "'\\''")
        ),
    }
}

/// A live embedded terminal session.
pub struct EmbeddedTerm {
    term: Arc<Mutex<Term<Proxy>>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    rx: mpsc::Receiver<Event>,
    /// Directory the shell was spawned in — shown in the header and used
    /// by the "cd here" button.
    pub cwd: PathBuf,
    /// Quoting dialect of the spawned shell, for `cd_to`.
    shell_kind: ShellKind,
    /// Whether keystrokes go to the shell vs the app's shortcuts.
    pub focused: bool,
    exited: bool,
    /// Fractional scroll carry — trackpads emit sub-line deltas that would
    /// otherwise truncate to zero every frame.
    scroll_accum: f32,
}

impl EmbeddedTerm {
    pub fn spawn(cwd: PathBuf, ctx: &egui::Context) -> Option<Self> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .ok()?;

        #[cfg(windows)]
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
        #[cfg(not(windows))]
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());

        let mut cmd = CommandBuilder::new(&shell);
        #[cfg(not(windows))]
        cmd.arg("-l"); // login shell — picks up the user's PATH/profile
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("TERM_PROGRAM", "flux-files");
        let child = pair.slave.spawn_command(cmd).ok()?;

        let mut reader = pair.master.try_clone_reader().ok()?;
        let writer = pair.master.take_writer().ok()?;
        drop(pair.slave);

        let shell_kind = shell_kind(&shell);
        let (tx, rx) = mpsc::channel();
        let proxy = Proxy {
            tx,
            ctx: ctx.clone(),
        };
        let cfg = term::Config {
            scrolling_history: 10_000,
            ..Default::default()
        };
        let size = term::test::TermSize::new(80, 24);
        let term = Arc::new(Mutex::new(Term::new(cfg, &size, proxy)));

        // Reader thread: PTY bytes → VT parser → grid; repaint on change.
        let t2 = term.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let mut processor = Processor::<StdSyncHandler>::new();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        {
                            let mut t = t2.lock().unwrap_or_else(|e| e.into_inner());
                            processor.advance(&mut *t, &buf[..n]);
                        }
                        ctx2.request_repaint();
                    }
                    Err(_) => break,
                }
            }
        });

        Some(Self {
            term,
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            child: Some(child),
            rx,
            cwd,
            shell_kind,
            focused: true,
            exited: false,
            scroll_accum: 0.0,
        })
    }

    /// Term mutex guard that survives a panicked reader thread instead of
    /// crash-looping the UI every frame.
    fn term_lock(&self) -> MutexGuard<'_, Term<Proxy>> {
        self.term.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn input(&self, bytes: &[u8]) {
        let _ = self
            .writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write_all(bytes);
    }

    /// Events arriving on the proxy channel (queries to answer), plus the
    /// child exit check — alacritty's `EventLoop` (which would emit
    /// `ChildExit`) isn't used, so we reap via `try_wait` once a frame.
    fn drain_events(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::PtyWrite(s) => self.input(s.as_bytes()),
                // OSC52 clipboard reads — answer with an empty clipboard
                // so querying programs don't stall waiting on a reply.
                Event::ClipboardLoad(_, fmt) => self.input(fmt("").as_bytes()),
                _ => {}
            }
        }
        if !self.exited
            && let Some(c) = self.child.as_mut()
            && matches!(c.try_wait(), Ok(Some(_)))
        {
            self.exited = true;
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let (c, r) = (cols.max(4), rows.max(2));
        self.term_lock()
            .resize(term::test::TermSize::new(c as usize, r as usize));
        let _ = self.master.resize(PtySize {
            rows: r,
            cols: c,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// `cd` the running shell to `dir` — writes the command as if typed,
    /// quoted for the shell we actually spawned (cmd/PowerShell/POSIX).
    fn cd_to(&mut self, dir: &Path) {
        self.input(cd_command(self.shell_kind, dir).as_bytes());
    }
}

impl Drop for EmbeddedTerm {
    fn drop(&mut self) {
        // `Child::kill` waits up to ~250ms for a wedged shell — do it off
        // the UI thread. The master then closes → reader read() fails →
        // the reader thread exits on its own.
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.kill();
            });
        }
    }
}

/// Standard xterm palette: 0-15 ANSI, 16-231 the 6×6×6 color cube,
/// 232-255 grayscale. The Term's `colors` table stays empty unless a
/// program sets dynamic colors, so this supplies the defaults.
fn ansi(i: usize) -> egui::Color32 {
    const BASE: [[u8; 3]; 16] = [
        [0, 0, 0],       // black
        [205, 49, 49],   // red
        [13, 188, 121],  // green
        [229, 229, 16],  // yellow
        [36, 114, 200],  // blue
        [188, 63, 188],  // magenta
        [17, 168, 205],  // cyan
        [229, 229, 229], // white
        [102, 102, 102], // bright black
        [241, 76, 76],   // bright red
        [35, 209, 139],  // bright green
        [245, 245, 67],  // bright yellow
        [59, 142, 234],  // bright blue
        [214, 112, 214], // bright magenta
        [41, 184, 219],  // bright cyan
        [255, 255, 255], // bright white
    ];
    match i {
        0..=15 => {
            let c = BASE[i];
            egui::Color32::from_rgb(c[0], c[1], c[2])
        }
        16..=231 => {
            let i = i - 16;
            let v = |n: usize| if n == 0 { 0 } else { (55 + 40 * n) as u8 };
            egui::Color32::from_rgb(v(i / 36), v(i / 6 % 6), v(i % 6))
        }
        _ => {
            let g = (8 + (i.min(255) - 232) * 10) as u8;
            egui::Color32::from_rgb(g, g, g)
        }
    }
}

fn rgb(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.r, c.g, c.b)
}

/// Translate an egui key event to the bytes a PTY expects.
fn key_bytes(key: egui::Key, mods: egui::Modifiers) -> Option<Vec<u8>> {
    use egui::Key;
    // Fieldless-enum discriminant check — A..Z contiguous in egui::Key.
    let (k, a, z) = (key as usize, Key::A as usize, Key::Z as usize);
    let is_letter = (a..=z).contains(&k);

    // Ctrl+letter → control byte (SIGINT, EOF, …). Must test `mac_cmd`,
    // not `command` — on Linux/Windows egui aliases command==ctrl.
    // Unmapped ctrl-chords fall THROUGH so Ctrl+Arrow etc. reach the
    // CSI-parameter path below.
    if mods.ctrl && !mods.mac_cmd {
        if is_letter {
            return Some(vec![(k - a + 1) as u8]);
        }
        let ctrl_byte = match key {
            Key::Space | Key::Num2 => Some(0x00),
            Key::Num3 | Key::OpenBracket => Some(0x1b),
            Key::Num4 | Key::Backslash => Some(0x1c),
            Key::Num5 | Key::CloseBracket => Some(0x1d),
            Key::Num6 => Some(0x1e),
            Key::Num7 | Key::Slash | Key::Minus => Some(0x1f),
            Key::Num8 | Key::Backspace => Some(0x7f),
            _ => None,
        };
        if let Some(b) = ctrl_byte {
            return Some(vec![b]);
        }
    }

    // Alt = Meta → ESC prefix (readline M-b/M-f/M-d/M-BS).
    if mods.alt && !mods.ctrl && !mods.mac_cmd && is_letter {
        let lower = (k - a) as u8;
        return Some(vec![
            0x1b,
            if mods.shift {
                b'A' + lower
            } else {
                b'a' + lower
            },
        ]);
    }

    // Arrow/Home/End modifiers → CSI 1;<param><final>, param = 1+s+2a+4c.
    let mod_param = |m: egui::Modifiers| 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8;
    let arrow = |code: u8| -> Vec<u8> {
        if mods.any() {
            format!("\x1b[1;{}{}", mod_param(mods), code as char).into_bytes()
        } else {
            vec![0x1b, b'[', code]
        }
    };
    let bytes: &[u8] = match key {
        Key::Enter => b"\r",
        Key::Backspace => b"\x7f",
        Key::Tab if mods.shift => b"\x1b[Z",
        Key::Tab => b"\t",
        Key::Escape => b"\x1b",
        Key::ArrowUp => return Some(arrow(b'A')),
        Key::ArrowDown => return Some(arrow(b'B')),
        Key::ArrowRight => return Some(arrow(b'C')),
        Key::ArrowLeft => return Some(arrow(b'D')),
        Key::Home => b"\x1b[H",
        Key::End => b"\x1b[F",
        Key::PageUp => b"\x1b[5~",
        Key::PageDown => b"\x1b[6~",
        Key::Insert => b"\x1b[2~",
        Key::Delete => b"\x1b[3~",
        Key::F1 => b"\x1bOP",
        Key::F2 => b"\x1bOQ",
        Key::F3 => b"\x1bOR",
        Key::F4 => b"\x1bOS",
        Key::F5 => b"\x1b[15~",
        Key::F6 => b"\x1b[17~",
        Key::F7 => b"\x1b[18~",
        Key::F8 => b"\x1b[19~",
        Key::F9 => b"\x1b[20~",
        Key::F10 => b"\x1b[21~",
        Key::F11 => b"\x1b[23~",
        Key::F12 => b"\x1b[24~",
        _ => return None,
    };
    let mut bytes = bytes.to_vec();
    if mods.alt {
        bytes.insert(0, 0x1b); // Meta prefix for Backspace/Enter/F-keys…
    }
    Some(bytes)
}

impl FluxApp {
    /// While the terminal is focused, keyboard input belongs to the shell —
    /// drain egui key/text events into PTY bytes BEFORE `handle_keys` runs
    /// so typing can't trigger app shortcuts (and Esc reaches vim).
    pub(crate) fn terminal_input(&mut self, ctx: &egui::Context) {
        let Some(term) = &self.terminal else { return };
        if !term.focused {
            return;
        }
        // An open overlay (palette/dialog/search) needs the keyboard —
        // don't starve it while the terminal happens to be focused.
        if self.palette_open || !matches!(self.dialog, Dialog::None) || self.search_ui.active {
            return;
        }
        let bracketed = term.term_lock().mode().contains(TermMode::BRACKETED_PASTE);
        let mut keep = Vec::new();
        let mut out = Vec::new();
        let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
        for ev in events {
            let drop_ev = match &ev {
                egui::Event::Text(t) => {
                    out.extend_from_slice(t.as_bytes());
                    true
                }
                egui::Event::Paste(t) => {
                    if bracketed {
                        out.extend_from_slice(b"\x1b[200~");
                    }
                    out.extend_from_slice(t.as_bytes());
                    if bracketed {
                        out.extend_from_slice(b"\x1b[201~");
                    }
                    true
                }
                // egui-winit converts Ctrl+C/X into Copy/Cut instead of a
                // Key event — map them to SIGINT/SUSP so the shell stays
                // usable on Linux/Windows. On macOS Cmd+C stays a no-op.
                egui::Event::Copy => {
                    !cfg!(target_os = "macos") && {
                        out.push(0x03);
                        true
                    }
                }
                egui::Event::Cut => {
                    !cfg!(target_os = "macos") && {
                        out.push(0x18);
                        true
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key_bytes(*key, *modifiers) {
                    Some(b) => {
                        out.extend_from_slice(&b);
                        true
                    }
                    // Unmapped command chords (Cmd+P etc.) stay visible so
                    // app-level shortcuts keep working while focused.
                    None => !(modifiers.command || modifiers.mac_cmd),
                },
                _ => false,
            };
            if !drop_ev {
                keep.push(ev);
            }
        }
        if !out.is_empty()
            && let Some(t) = &self.terminal
        {
            t.input(&out);
        }
        ctx.input_mut(|i| i.events = keep);
    }

    /// The docked terminal panel above the status bar.
    pub(crate) fn terminal_panel(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        if self.terminal.is_none() {
            return;
        }
        // The terminal tracks the directory the user would land back on —
        // the active dir tab's path, or the most recent dir on workspaces.
        let current_dir = self.last_dir_path(); // before the &mut field borrow
        let term = self.terminal.as_mut().unwrap();
        term.drain_events();

        let mono = egui::FontId::monospace(self.settings.terminal_font_size);
        // `glyph_width`/`row_height` lazily load fonts → need &mut Fonts.
        let (cell_w, line_h) = ui
            .ctx()
            .fonts_mut(|f| (f.glyph_width(&mono, 'M').max(4.0), f.row_height(&mono)));

        // Header: painted prompt glyph + cwd, spacer, actions. The bundled
        // font lacks ❯/⧉/✕ characters — everything is a drawn Glyph.
        ui.spacing_mut().interact_size.y = 24.0;
        let mut close = false;
        let mut external = false;
        let mut cd = false;
        ui.horizontal(|ui| {
            let (grect, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
            crate::glyphs::paint_glyph(
                ui.painter(),
                grect,
                crate::glyphs::Glyph::Terminal,
                pal.accent,
            );
            ui.label(
                egui::RichText::new(term.cwd.display().to_string())
                    .color(pal.text_muted)
                    .size(11.5),
            );
            if term.exited {
                ui.label(
                    egui::RichText::new("· process exited")
                        .color(pal.text_muted)
                        .size(11.5),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = crate::glyphs::icon_button(
                    ui,
                    crate::glyphs::Glyph::Close,
                    "Close terminal",
                    true,
                )
                .clicked();
                external = crate::glyphs::icon_button(
                    ui,
                    crate::glyphs::Glyph::External,
                    "Open in external Terminal",
                    true,
                )
                .clicked();
                cd = ui
                    .button(egui::RichText::new("↓ cwd").size(11.5))
                    .on_hover_text("cd to the current folder")
                    .clicked();
            });
        });
        if close {
            self.terminal = None;
            return;
        }
        let term = self.terminal.as_mut().unwrap();
        if let Some(dir) = &current_dir {
            if external {
                let _ = flux_platform::open_in_terminal(dir);
            }
            if cd {
                term.cd_to(dir);
            }
        }

        // Grid viewport.
        let avail = ui.available_size();
        let cols = ((avail.x - 12.0) / cell_w) as u16;
        let rows = ((avail.y - 4.0) / line_h) as u16;
        {
            let t = term.term_lock();
            if t.columns() != cols as usize || t.screen_lines() != rows as usize {
                drop(t);
                term.resize(cols, rows);
            }
        }

        let resp = ui.interact(
            ui.available_rect_before_wrap(),
            ui.id().with("term_area"),
            egui::Sense::click(),
        );
        if resp.clicked() {
            term.focused = true;
        }
        // Lose focus only when the click lands outside the whole panel —
        // header buttons shouldn't steal keyboard focus from the shell.
        let panel_rect = ui.max_rect();
        let clicked_outside = ui.input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .latest_pos()
                    .is_some_and(|p| !panel_rect.contains(p))
        });
        if clicked_outside {
            term.focused = false;
        }

        // Mouse wheel scrolls scrollback — accumulate sub-line deltas so
        // trackpads/pixel wheels don't truncate to zero per frame.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.01 && resp.contains_pointer() {
            term.scroll_accum += scroll;
            let lines = (term.scroll_accum / line_h) as i32;
            term.scroll_accum -= lines as f32 * line_h;
            if lines != 0 {
                term.term_lock().scroll_display(Scroll::Delta(lines));
            }
        }

        let painter = ui.painter().clone();
        painter.rect_filled(resp.rect, egui::CornerRadius::same(6), pal.bg_window);
        let term = self.terminal.as_ref().unwrap();
        let t = term.term_lock();
        let content = t.renderable_content();
        let off = content.display_offset as i32;
        let origin = resp.rect.left_top() + egui::vec2(6.0, 3.0);

        /// Resolve a cell color to a paint color; `None` = "default"
        /// (window bg / primary fg handled by the caller).
        macro_rules! color_of {
            ($c:expr) => {
                match $c {
                    Color::Named(NamedColor::Foreground | NamedColor::BrightForeground) => {
                        Some(pal.text_primary)
                    }
                    Color::Named(NamedColor::DimForeground) => Some(pal.text_secondary),
                    Color::Named(NamedColor::Background) => None,
                    Color::Named(n) => {
                        let idx = n as usize;
                        content.colors[n].map(rgb).or_else(|| match idx {
                            0..=255 => Some(ansi(idx)),
                            259..=266 => Some(ansi(idx - 259)), // DimBlack..DimWhite
                            _ => None,
                        })
                    }
                    Color::Indexed(i) => Some(
                        content.colors[i as usize]
                            .map(rgb)
                            .unwrap_or_else(|| ansi(i as usize)),
                    ),
                    Color::Spec(r) => Some(rgb(r)),
                }
            };
        }

        // Collect once — the display iterator can only be walked once.
        let cells: Vec<_> = content.display_iter.collect();
        let mut rows_touched: std::collections::BTreeMap<i32, Vec<(usize, String, egui::Color32)>> =
            Default::default();

        for ic in &cells {
            let row = ic.point.line.0 + off;
            if row < 0 || ic.cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let col = ic.point.column.0;
            let flags = ic.cell.flags;

            // Bold on a normal named color renders bright (xterm rule).
            let mut fg = ic.cell.fg;
            if flags.contains(Flags::BOLD)
                && let Color::Named(n) = fg
                && (n as usize) < 8
            {
                fg = Color::Indexed(n as u8 + 8);
            }

            let inverse = flags.contains(Flags::INVERSE);
            let (mut fg_c, bg_c) = if inverse {
                (
                    color_of!(ic.cell.bg).unwrap_or(pal.bg_window),
                    color_of!(fg).unwrap_or(pal.text_primary),
                )
            } else {
                (
                    color_of!(fg).unwrap_or(pal.text_primary),
                    color_of!(ic.cell.bg).unwrap_or(egui::Color32::TRANSPARENT),
                )
            };
            if flags.contains(Flags::DIM) {
                fg_c = fg_c.gamma_multiply(0.6);
            }

            if bg_c != egui::Color32::TRANSPARENT {
                // Wide chars own the spacer cell next to them — paint both.
                let w = if flags.contains(Flags::WIDE_CHAR) {
                    2.0 * cell_w
                } else {
                    cell_w
                };
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        origin + egui::vec2(col as f32 * cell_w, row as f32 * line_h),
                        egui::vec2(w, line_h),
                    ),
                    0.0,
                    bg_c,
                );
            }
            if flags.contains(Flags::HIDDEN) || ic.cell.c == ' ' {
                continue;
            }
            let row_runs = rows_touched.entry(row).or_default();
            match row_runs.last_mut() {
                Some((sc, s, f)) if *f == fg_c && *sc + s.chars().count() == col => {
                    s.push(ic.cell.c)
                }
                _ => row_runs.push((col, ic.cell.c.to_string(), fg_c)),
            }
        }

        // Cursor under the text — translucent accent block.
        let cp = content.cursor.point;
        let crow = cp.line.0 + off;
        if !matches!(content.cursor.shape, CursorShape::Hidden) && crow >= 0 && crow < rows as i32 {
            let crect = egui::Rect::from_min_size(
                origin + egui::vec2(cp.column.0 as f32 * cell_w, crow as f32 * line_h),
                egui::vec2(cell_w, line_h),
            );
            match content.cursor.shape {
                CursorShape::Block => {
                    painter.rect_filled(crect, 2.0, pal.accent_fill);
                }
                CursorShape::Underline => {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(crect.left(), crect.bottom() - 2.0),
                            crect.right_bottom(),
                        ),
                        0.0,
                        pal.accent,
                    );
                }
                _ => {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            crect.left_top(),
                            egui::pos2(crect.left() + 1.5, crect.bottom()),
                        ),
                        0.0,
                        pal.accent,
                    );
                }
            }
        }

        drop(t); // release the grid before painting text
        for (row, rruns) in rows_touched {
            for (col, text, fg) in rruns {
                painter.text(
                    origin + egui::vec2(col as f32 * cell_w, row as f32 * line_h),
                    egui::Align2::LEFT_TOP,
                    text,
                    mono.clone(),
                    fg,
                );
            }
        }

        // Claim the remaining space — required for Panel::resizable to
        // keep a dragged height; without it the panel's content-min is
        // just the header and the drag snaps back on release.
        ui.take_available_space();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_kind_detects_common_shells() {
        // COMSPEC/cmd classification only exists under Windows — on other
        // OSes an unrecognized name is POSIX-ish by definition.
        #[cfg(windows)]
        {
            assert_eq!(shell_kind("cmd.exe"), ShellKind::Cmd);
            assert_eq!(shell_kind("C:\\Windows\\System32\\cmd.exe"), ShellKind::Cmd);
            assert_eq!(shell_kind("powershell.exe"), ShellKind::PowerShell);
            assert_eq!(
                shell_kind("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
                ShellKind::PowerShell
            );
        }
        // Portable: these names classify the same on every OS (pwsh and
        // nushell exist on Linux/macOS too).
        assert_eq!(shell_kind("/bin/bash"), ShellKind::Posix);
        assert_eq!(shell_kind("/bin/zsh"), ShellKind::Posix);
        assert_eq!(shell_kind("nu.exe"), ShellKind::Posix);
        assert_eq!(shell_kind("/usr/bin/nushell"), ShellKind::Posix);
        assert_eq!(shell_kind("/usr/bin/pwsh"), ShellKind::PowerShell);
    }

    #[test]
    fn cd_command_quotes_for_cmd() {
        let c = cd_command(ShellKind::Cmd, Path::new("C:\\My Stuff\\docs"));
        assert_eq!(c, "cd /d \"C:\\My Stuff\\docs\"\r");
    }

    #[test]
    fn cd_command_quotes_for_powershell() {
        let c = cd_command(ShellKind::PowerShell, Path::new("C:\\it's\\dir"));
        assert_eq!(c, "Set-Location -LiteralPath 'C:\\it''s\\dir'\r");
    }

    #[test]
    fn cd_command_quotes_for_posix() {
        let c = cd_command(ShellKind::Posix, Path::new("/home/u/my dir"));
        assert_eq!(c, "cd '/home/u/my dir'\r");
        let q = cd_command(ShellKind::Posix, Path::new("/it's/here"));
        assert_eq!(q, "cd '/it'\\''s/here'\r");
    }
}
