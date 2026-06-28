use crate::event::AppEvent;
use crate::model::ActionSpec;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use tokio::sync::mpsc::UnboundedSender;

pub enum TaskState {
    Running,
    Done { success: bool, code: u32 },
}

pub struct ActiveTask {
    pub id: u64,
    pub spec: ActionSpec,
    pub parser: vt100::Parser,
    pub writer: Box<dyn Write + Send>,
    pub state: TaskState,
    pub has_unseen_output: bool,
    csi_fix: CsiLineFix,
    _master: Box<dyn MasterPty + Send>,
}

impl ActiveTask {
    /// Feed PTY bytes into the emulator, first rewriting the CSI sequences vt100
    /// 0.15 does not implement (see `CsiLineFix`).
    pub fn feed(&mut self, bytes: &[u8]) {
        let fixed = self.csi_fix.rewrite(bytes);
        self.parser.process(&fixed);
    }

    /// Resize both the emulated screen and the underlying PTY so output reflows
    /// to the visible area (and the newest lines stay at the bottom).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let _ = self._master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.set_size(rows, cols);
    }
}

/// Spawn `spec.command` in a PTY of size rows×cols. Output bytes are sent as
/// `AppEvent::PtyOutput`; on exit, `AppEvent::ActionFinished` is sent. Both
/// carry `id` so the UI can ignore events from a task it has since replaced.
/// Reading and waiting happen on dedicated OS threads (blocking IO).
pub fn start_action(
    spec: ActionSpec,
    id: u64,
    rows: u16,
    cols: u16,
    tx: UnboundedSender<AppEvent>,
) -> anyhow::Result<ActiveTask> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(spec.command.program.as_str());
    for arg in &spec.command.args {
        cmd.arg(arg.as_str());
    }
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave); // we don't need the slave handle anymore

    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;

    // Reader thread → PtyOutput events.
    let tx_read = tx.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let ev = AppEvent::PtyOutput { id, bytes: buf[..n].to_vec() };
                    if tx_read.send(ev).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Wait thread → ActionFinished event.
    std::thread::spawn(move || {
        let status = child.wait();
        let (success, code) = match status {
            Ok(s) => (s.success(), s.exit_code()),
            Err(_) => (false, 1),
        };
        let _ = tx.send(AppEvent::ActionFinished { id, success, code });
    });

    Ok(ActiveTask {
        id,
        spec,
        parser: vt100::Parser::new(rows, cols, 2000),
        writer,
        state: TaskState::Running,
        has_unseen_output: false,
        csi_fix: CsiLineFix::default(),
        _master: pair.master,
    })
}

/// vt100 0.15 implements neither CSI E (Cursor Next Line) nor CSI F (Cursor
/// Preceding Line); it silently drops both. pacman's parallel-download progress
/// relies on `ESC[<n>F` to move up onto its "Total" line and redraw each package
/// in place. Without it, every move is a no-op so packages and repeated "Total"
/// lines pile up and live progress lands on the wrong rows (the flicker bug).
///
/// Rewrite them into equivalents vt100 does handle: CPL `ESC[<n>F` becomes
/// `ESC[<n>A` then CR (up n lines, column 0); CNL `ESC[<n>E` becomes `ESC[<n>B`
/// then CR. The scanner is stateful because a sequence can split across PTY reads.
#[derive(Default)]
struct CsiLineFix {
    /// Bytes of an in-progress escape sequence held when a read ends mid-CSI.
    pending: Vec<u8>,
}

impl CsiLineFix {
    fn rewrite(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() + self.pending.len());
        for &b in input {
            if self.pending.is_empty() {
                if b == 0x1b {
                    self.pending.push(b);
                } else {
                    out.push(b);
                }
                continue;
            }
            self.pending.push(b);
            if self.pending.len() == 2 {
                // pending is [ESC, b]; a CSI must start with '['.
                if b != b'[' {
                    out.append(&mut self.pending);
                }
                continue;
            }
            // Inside a CSI: 0x20..=0x3f are parameter/intermediate bytes, keep
            // collecting; anything else terminates the sequence.
            if (0x20..=0x3f).contains(&b) {
                continue;
            }
            if b == b'F' || b == b'E' {
                let n = parse_csi_param(&self.pending[2..self.pending.len() - 1]);
                let dir = if b == b'F' { b'A' } else { b'B' };
                out.extend_from_slice(b"\x1b[");
                out.extend_from_slice(n.to_string().as_bytes());
                out.push(dir);
                out.push(b'\r');
            } else {
                out.append(&mut self.pending);
            }
            self.pending.clear();
        }
        out
    }
}

/// Parse the numeric parameter of a CSI sequence; defaults to 1 (and clamps 0 to
/// 1) since CPL/CNL move at least one line.
fn parse_csi_param(bytes: &[u8]) -> u16 {
    let n: u16 = std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    n.max(1)
}

/// Translate a key event into bytes to forward to the child PTY.
pub fn key_to_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                let upper = c.to_ascii_uppercase();
                if upper.is_ascii_alphabetic() {
                    return Some(vec![(upper as u8) - 0x40]);
                }
            }
            let mut b = [0u8; 4];
            Some(c.encode_utf8(&mut b).as_bytes().to_vec())
        }
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, ActionSpec, CommandLine, SourceId};
    use tokio::sync::mpsc;

    /// One pacman parallel-download "setup" step: print a package on its own
    /// line, print the Total line, then `ESC[1F` back up onto the Total line so
    /// the next package overwrites it. Mirrors the real captured byte stream.
    fn pacman_setup(pkgs: &[&str]) -> Vec<u8> {
        // pacman never clears lines; it writes fixed-width lines that fully
        // overwrite. Pad to a fixed width so the fixture behaves the same.
        let line = |s: &str| format!("{s:<40}");
        let mut out = String::new();
        for p in pkgs {
            out.push_str(&line(&format!(" {p}")));
            out.push_str("\r\n");
            out.push_str(&line(&format!(" Total ( 0/{})", pkgs.len())));
            out.push_str("\r\r\n\x1b[1F");
        }
        out.into_bytes()
    }

    fn nonblank_lines(screen: &str) -> Vec<String> {
        screen.lines().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty()).collect()
    }

    #[test]
    fn vt100_alone_drops_cpl_and_stacks_total_lines() {
        // Documents the bug (issue #4): without the rewrite, vt100 0.15 ignores
        // ESC[1F, so a "Total" line piles up after every package.
        let mut parser = vt100::Parser::new(24, 80, 2000);
        parser.process(&pacman_setup(&["a", "b", "c"]));
        let lines = nonblank_lines(&parser.screen().contents());
        let totals = lines.iter().filter(|l| l.contains("Total")).count();
        assert_eq!(totals, 3, "raw vt100 stacks a Total per package: {lines:?}");
    }

    #[test]
    fn csi_fix_collapses_to_single_total_at_bottom() {
        let mut fix = CsiLineFix::default();
        let fixed = fix.rewrite(&pacman_setup(&["a", "b", "c"]));
        let mut parser = vt100::Parser::new(24, 80, 2000);
        parser.process(&fixed);
        let lines = nonblank_lines(&parser.screen().contents());
        let totals = lines.iter().filter(|l| l.contains("Total")).count();
        assert_eq!(totals, 1, "fix leaves exactly one Total line: {lines:?}");
        // Packages stack above the single Total line, in order.
        assert_eq!(lines[0].trim(), "a");
        assert_eq!(lines[1].trim(), "b");
        assert_eq!(lines[2].trim(), "c");
        assert!(lines[3].contains("Total"));
    }

    #[test]
    fn csi_fix_handles_sequence_split_across_reads() {
        // ESC[1F split mid-sequence across two feeds must still rewrite.
        let mut fix = CsiLineFix::default();
        let a = fix.rewrite(b" a\r\n Total\r\r\n\x1b[");
        let b = fix.rewrite(b"1F b\r\n Total\r\r\n");
        let mut parser = vt100::Parser::new(24, 80, 2000);
        parser.process(&a);
        parser.process(&b);
        let lines = nonblank_lines(&parser.screen().contents());
        assert_eq!(lines.iter().filter(|l| l.contains("Total")).count(), 1, "{lines:?}");
    }

    #[test]
    fn csi_fix_passes_other_sequences_through() {
        let mut fix = CsiLineFix::default();
        // A color SGR then plain text must be untouched.
        assert_eq!(fix.rewrite(b"\x1b[31mred\x1b[0m"), b"\x1b[31mred\x1b[0m");
    }

    #[tokio::test]
    async fn runs_command_in_pty_and_streams_output() {
        let spec = ActionSpec {
            targets: vec!["x".into()],
            source_id: SourceId::Pacman,
            action: Action::Install,
            command: CommandLine {
                program: "printf".into(),
                args: vec!["hello-plaza".into()],
            },
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();
        let mut task = start_action(spec, 1, 24, 80, tx).expect("spawn pty");

        let mut got_output = false;
        let mut finished = false;
        for _ in 0..1000 {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(AppEvent::PtyOutput { bytes, .. })) => {
                    task.parser.process(&bytes);
                    got_output = true;
                }
                Ok(Some(AppEvent::ActionFinished { success, code, .. })) => {
                    task.state = TaskState::Done { success, code };
                    finished = true;
                    // Don't break: PtyOutput and ActionFinished come from two
                    // separate threads, so the finish event can arrive before
                    // buffered output. Keep draining until the channel closes
                    // (both sender threads drop tx) so we never miss output.
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert!(got_output, "expected PtyOutput events");
        assert!(finished, "expected ActionFinished");
        let screen = task.parser.screen().contents();
        assert!(screen.contains("hello-plaza"), "screen was: {screen:?}");
    }
}
