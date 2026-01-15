use anyhow::Result;
use portable_pty::{Child, CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const INIT_MARKER: &str = ">>INIT_DONE<<";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const OSC_CMD_FINISHED_PREFIX: &str = "\x1b]133;D;";
const OSC_PROMPT_START: &str = "\x1b]133;A\x07";

/// Mimics the Agent's view of a terminal session
pub struct TerminalSession {
    writer: Box<dyn Write + Send>,
    // The shared buffer contains output since last read
    output_buffer: Arc<Mutex<String>>,
    // Keep child process to kill it on drop
    child: Box<dyn Child + Send>,
    // Status of the background reader
    is_alive: Arc<AtomicBool>,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl TerminalSession {
    pub fn new(workdir: Option<PathBuf>) -> Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new("bash");

        // We set CWD here. We do NOT set PS1/PROMPT_COMMAND here because .bashrc
        // will likely override them. We set them via the writer below.

        if let Some(wd) = workdir {
            cmd.cwd(wd);
        }

        let child = pair.slave.spawn_command(cmd)?;

        let mut writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let output_buffer = Arc::new(Mutex::new(String::new()));
        let buffer_clone = output_buffer.clone();
        let is_alive = Arc::new(AtomicBool::new(true));
        let is_alive_clone = is_alive.clone();

        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        let s = String::from_utf8_lossy(&buf[0..n]);
                        let mut locked = buffer_clone.lock().unwrap();
                        locked.push_str(&s);
                    }
                    Ok(_) => {
                        // EOF
                        is_alive_clone.store(false, Ordering::Relaxed);
                        break;
                    }
                    Err(e) => {
                        eprintln!("Terminal background reader error: {}", e);
                        is_alive_clone.store(false, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });

        // Initialize shell
        // 1. Disable echo to avoid double output
        writeln!(writer, "stty -echo")?;
        // 2. Disable bracketed paste
        writeln!(writer, "bind 'set enable-bracketed-paste off'")?;

        // 3. Configure OSC 133 Semantic Prompts
        // We do this here ensures it overrides .bashrc
        // D;<code>: Command finished with exit code
        // A: Prompt start
        // Note: We need careful escaping for the printf string inside the export.
        // PROMPT_COMMAND='printf "\033]133;D;%s\007" $?'
        // PS1='\[\033]133;A\007\]'
        writeln!(
            writer,
            "export PROMPT_COMMAND='printf \"\\033]133;D;%s\\007\" $?'"
        )?;
        writeln!(writer, "export PS1='\\[\\033]133;A\\007\\]'")?;

        // 4. Handshake
        // We use a specific marker output that won't be confused with the command echo.
        // We need to wait for the prompt to appear properly configured.
        let handshake_cmd = format!("echo \"{}\"", INIT_MARKER);
        writeln!(writer, "{}", handshake_cmd)?;

        // Wait for handshake
        let start = Instant::now();
        loop {
            if start.elapsed() > HANDSHAKE_TIMEOUT {
                let locked = output_buffer.lock().unwrap();
                let content_sample = if locked.len() > 200 {
                    &locked[locked.len() - 200..]
                } else {
                    &locked
                };
                return Err(anyhow::anyhow!(
                    "Failed to initialize terminal: timeout waiting for handshake. Buffer (last 200 chars): {:?}",
                    content_sample
                ));
            }
            if !is_alive.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!(
                    "Failed to initialize terminal: background thread exited"
                ));
            }
            {
                let mut locked = output_buffer.lock().unwrap();
                // Check if we found the marker.
                if let Some(idx) = locked.find(INIT_MARKER) {
                    // Check if we have seen the semantic prompt marker "OSC 133;A" AFTER the Init marker
                    let after = &locked[idx + INIT_MARKER.len()..];
                    if after.contains(OSC_PROMPT_START) {
                        // Found it.
                        // Clear buffer to be clean for next command
                        *locked = String::new();
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }

        Ok(Self {
            writer,
            output_buffer,
            child,
            is_alive,
        })
    }

    pub fn execute(&mut self, cmd: &str, timeout_ms: u64) -> Result<(String, i32)> {
        // Check health
        if !self.is_alive.load(Ordering::Relaxed) {
            return Err(anyhow::anyhow!("Terminal session is dead"));
        }

        // Just write the command. bash will handle the rest via PROMPT_COMMAND.
        writeln!(self.writer, "{}", cmd)?;

        let start = Instant::now();
        let duration = Duration::from_millis(timeout_ms);

        loop {
            if start.elapsed() > duration {
                return Ok((self.drain_output(), -1));
            }
            if !self.is_alive.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!(
                    "Terminal background thread died during execution"
                ));
            }

            {
                let locked = self.output_buffer.lock().unwrap();
                // Look for OSC 133;D;<code>\x07
                if locked.contains(OSC_CMD_FINISHED_PREFIX) {
                    break;
                }
            }

            thread::sleep(Duration::from_millis(10));
        }

        let output = self.drain_output();

        // Parse exit code from OSC 133;D;<code>\x07
        // There might be multiple if user somehow chained commands, but we take the last one.
        // Note: The marker is printed *after* the command output.
        // We probably want to remove the marker and everything after it (the prompt) from the returned output.

        if let Some(pos) = output.rfind(OSC_CMD_FINISHED_PREFIX) {
            let after_marker = &output[pos + OSC_CMD_FINISHED_PREFIX.len()..];
            if let Some(end_pos) = after_marker.find('\x07') {
                let code_str = &after_marker[..end_pos];
                let exit_code = code_str.parse().unwrap_or(-1);

                // The output is everything BEFORE the marker
                let actual_output = &output[..pos];
                return Ok((actual_output.trim_end().to_string(), exit_code));
            }
        }

        Ok((output, -1))
    }

    fn drain_output(&mut self) -> String {
        let mut locked = self.output_buffer.lock().unwrap();
        let current_content = locked.clone();
        *locked = String::new();
        current_content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_simple_command() {
        let mut session = TerminalSession::new(None).unwrap();
        let (output, exit_code) = session.execute("echo hello", 1000).unwrap();
        assert_eq!(exit_code, 0);
        assert!(output.contains("hello"));
    }

    #[test]
    fn test_execute_state_persistence() {
        let mut session = TerminalSession::new(None).unwrap();
        session.execute("export MY_VAR=123", 1000).unwrap();

        let (output, exit_code) = session.execute("echo $MY_VAR", 1000).unwrap();
        assert_eq!(exit_code, 0);
        assert!(output.contains("123"));
    }

    #[test]
    fn test_execute_directory_persistence() {
        let mut session = TerminalSession::new(None).unwrap();
        session.execute("mkdir -p /tmp/test_dir", 1000).unwrap();
        session.execute("cd /tmp/test_dir", 1000).unwrap();

        let (output, exit_code) = session.execute("pwd", 1000).unwrap();
        assert_eq!(exit_code, 0);
        assert!(output.contains("/tmp/test_dir"));
    }

    #[test]
    fn test_execute_timeout() {
        let mut session = TerminalSession::new(None).unwrap();
        let (_output, exit_code) = session.execute("sleep 2", 500).unwrap();
        assert_eq!(exit_code, -1);
    }

    #[test]
    fn test_execute_exit_code() {
        let mut session = TerminalSession::new(None).unwrap();

        let (_output, exit_code) = session.execute("false", 1000).unwrap();
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn test_execute_large_output() {
        let mut session = TerminalSession::new(None).unwrap();
        // seq 1 10000 generates roughly 48KB of text
        let (output, exit_code) = session.execute("seq 1 10000", 5000).unwrap();
        assert_eq!(exit_code, 0);
        // PTY often converts newlines to CRLF
        assert!(output.starts_with("1\r\n") || output.starts_with("1\n"));
        assert!(output.contains("10000"));
        // Check approximate length to ensure we didn't drop huge chunks
        assert!(output.len() > 40000);
    }

    #[test]
    fn test_concurrent_sessions() {
        let mut handles = vec![];
        for i in 0..5 {
            handles.push(thread::spawn(move || {
                let mut session = TerminalSession::new(None).unwrap();
                let (output, exit_code) = session
                    .execute(&format!("echo thread {}", i), 1000)
                    .unwrap();
                assert_eq!(exit_code, 0);
                assert!(output.contains(&format!("thread {}", i)));
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_interrupt_exit_code() {
        let mut session = TerminalSession::new(None).unwrap();
        // sh -c 'kill -TERM $$' causes the subshell to die with signal 15 (TERM).
        // Bash reports this as 128 + 15 = 143.
        let (_output, exit_code) = session.execute("sh -c 'kill -TERM $$'", 1000).unwrap();
        assert_eq!(exit_code, 143);
    }
}
