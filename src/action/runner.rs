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
    pub spec: ActionSpec,
    pub parser: vt100::Parser,
    pub writer: Box<dyn Write + Send>,
    pub state: TaskState,
    pub has_unseen_output: bool,
    _master: Box<dyn MasterPty + Send>,
}

/// Spawn `spec.command` in a PTY of size rows×cols. Output bytes are sent as
/// `AppEvent::PtyOutput`; on exit, `AppEvent::ActionFinished` is sent. Reading
/// and waiting happen on dedicated OS threads (blocking IO).
pub fn start_action(
    spec: ActionSpec,
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
                    if tx_read.send(AppEvent::PtyOutput(buf[..n].to_vec())).is_err() {
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
        let _ = tx.send(AppEvent::ActionFinished { success, code });
    });

    Ok(ActiveTask {
        spec,
        parser: vt100::Parser::new(rows, cols, 2000),
        writer,
        state: TaskState::Running,
        has_unseen_output: false,
        _master: pair.master,
    })
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
        let mut task = start_action(spec, 24, 80, tx).expect("spawn pty");

        let mut got_output = false;
        let mut finished = false;
        for _ in 0..1000 {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(AppEvent::PtyOutput(bytes))) => {
                    task.parser.process(&bytes);
                    got_output = true;
                }
                Ok(Some(AppEvent::ActionFinished { success, code })) => {
                    task.state = TaskState::Done { success, code };
                    finished = true;
                    break;
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
