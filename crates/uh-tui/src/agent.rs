//! The agent: one `unharnessed --mode acp` process, and the JSON-RPC lines
//! exchanged with it over its input and output, as the VS Code extension
//! does (`vscode/src/agent.ts`). It runs in a process group of its own, so
//! what the terminal sends its foreground programs (an interrupt, a hangup
//! when the window closes) does not reach it: it ends when its input
//! closes, its kernel saved.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use serde_json::Value;

/// How to start the agent.
#[derive(Clone, Debug)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The folder it starts in.
    pub dir: PathBuf,
}

/// What comes from the agent.
#[derive(Debug)]
pub enum Told {
    /// A JSON-RPC frame: an answer, or a notification.
    Frame(Value),
    /// A line it wrote for whoever reads its errors.
    Said(String),
    /// Its output closed: it ended, or is about to.
    Closed,
}

pub struct Agent {
    child: Child,
    /// The lines to write to its input; let go of, the input closes.
    lines: Option<Sender<String>>,
}

impl Agent {
    /// Starts the agent; what comes from it goes to `tell`, from threads of
    /// their own.
    ///
    /// # Errors
    ///
    /// The program could not be started.
    pub fn start(
        launch: &Launch,
        tell: impl Fn(Told) + Send + Clone + 'static,
    ) -> std::io::Result<Agent> {
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .current_dir(&launch.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn()?;
        let (Some(mut input), Some(output), Some(errors)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(std::io::Error::other("the agent's pipes are missing"));
        };
        // Written from a thread: an agent that stops reading holds that
        // thread, not the screen.
        let (lines, written) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in written {
                if input
                    .write_all(line.as_bytes())
                    .and_then(|()| input.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let said = tell.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(errors).lines().map_while(Result::ok) {
                said(Told::Said(line));
            }
        });
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                // What is not a frame is not for the conversation.
                if let Ok(frame) = serde_json::from_str::<Value>(&line) {
                    tell(Told::Frame(frame));
                }
            }
            tell(Told::Closed);
        });
        Ok(Agent {
            child,
            lines: Some(lines),
        })
    }

    /// A frame for the agent, if it is still there to take it.
    pub fn send(&self, frame: &Value) {
        let mut line = frame.to_string();
        line.push('\n');
        if let Some(lines) = &self.lines {
            let _ = lines.send(line);
        }
    }

    /// Closes the agent's input: it stops its turn, saves its kernel and
    /// exits. Waits for that up to `patience`, then kills it. Answers how
    /// it ended, when it did by itself.
    #[must_use]
    pub fn stop(mut self, patience: Duration) -> Option<ExitStatus> {
        self.lines = None;
        let began = Instant::now();
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Some(status);
            }
            if began.elapsed() >= patience {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}
