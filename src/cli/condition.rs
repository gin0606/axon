use super::display;
use axon::lifecycle::Entity;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

const CONDITION_OUTPUT_LIMIT: usize = 64 * 1024;
const CONDITION_OUTPUT_EDGE: usize = CONDITION_OUTPUT_LIMIT / 2;
const TERMINATION_GRACE: Duration = Duration::from_secs(1);
const WAIT_INTERVAL: Duration = Duration::from_millis(10);

static INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn record_interrupt(_: libc::c_int) {
    INTERRUPTS.fetch_add(1, Ordering::Relaxed);
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct EvaluationError(String);

pub type Result<T> = std::result::Result<T, EvaluationError>;

pub struct Evaluation {
    root: PathBuf,
    timeout: Duration,
    trace: Option<RefCell<Box<dyn Write>>>,
}

impl Evaluation {
    pub fn with_timeout(root: PathBuf, timeout: Duration) -> Self {
        Self {
            root,
            timeout,
            trace: None,
        }
    }

    pub fn tracing(root: PathBuf, timeout: Duration) -> Self {
        Self::with_trace_writer(root, timeout, Box::new(std::io::stderr()))
    }

    fn with_trace_writer(root: PathBuf, timeout: Duration, writer: Box<dyn Write>) -> Self {
        Self {
            root,
            timeout,
            trace: Some(RefCell::new(writer)),
        }
    }

    pub fn run_command(&self, entity: &Entity, script: &str) -> Result<bool> {
        let interrupt_guard = InterruptGuard::install().map_err(|error| {
            EvaluationError(format!(
                "{}: Command({}) could not install Ctrl-C handling: {error}",
                entity.id,
                display::human_text(script)
            ))
        })?;
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", script])
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let child = command.spawn().map_err(|error| {
            EvaluationError(format!(
                "{}: Command({}) could not start in {}: {error}",
                entity.id,
                display::human_text(script),
                display::human_text(self.root.display())
            ))
        })?;
        let outcome = supervise(child, self.timeout, interrupt_guard).map_err(|failure| {
            let detail = match failure.kind {
                SupervisionFailureKind::Timeout => format!(
                    "timed out after {}; process group termination: {}",
                    display_duration(self.timeout),
                    failure.termination
                ),
                SupervisionFailureKind::Interrupted => format!(
                    "interrupted by Ctrl-C; process group termination: {}",
                    failure.termination
                ),
                SupervisionFailureKind::Internal(error) => format!(
                    "evaluation supervision failed: {error}; process group termination: {}",
                    failure.termination
                ),
            };
            EvaluationError(format!(
                "{}: Command({}) evaluation failed in {}: {detail}\nstdout:\n{}\nstderr:\n{}",
                entity.id,
                display::human_text(script),
                display::human_text(self.root.display()),
                failure.stdout.render(),
                failure.stderr.render()
            ))
        })?;
        match outcome.status.code() {
            Some(code @ (0 | 1)) => {
                self.write_trace(entity, code, &outcome.stdout, &outcome.stderr)?;
                Ok(code == 0)
            }
            _ => Err(EvaluationError(format!(
                "{}: Command({}) failed in {}: {}\nstdout:\n{}\nstderr:\n{}",
                entity.id,
                display::human_text(script),
                display::human_text(self.root.display()),
                outcome.status,
                outcome.stdout.render(),
                outcome.stderr.render()
            ))),
        }
    }

    fn write_trace(
        &self,
        entity: &Entity,
        code: i32,
        stdout: &CapturedStream,
        stderr: &CapturedStream,
    ) -> Result<()> {
        let Some(writer) = &self.trace else {
            return Ok(());
        };
        let result = if code == 0 {
            "satisfied"
        } else {
            "not satisfied"
        };
        let mut block = format!(
            "Condition trace: {}\ncwd: {}\nresult: {result} (exit {code})\n",
            entity.id,
            display::human_text(self.root.display())
        );
        append_trace_stream(&mut block, "stdout", stdout);
        append_trace_stream(&mut block, "stderr", stderr);
        block.push_str(&format!("End condition trace: {}\n", entity.id));

        let mut writer = writer.borrow_mut();
        writer
            .write_all(block.as_bytes())
            .and_then(|()| writer.flush())
            .map_err(|error| {
                EvaluationError(format!(
                    "{}: Command condition trace could not be written: {error}",
                    entity.id
                ))
            })
    }
}

fn append_trace_stream(block: &mut String, label: &str, stream: &CapturedStream) {
    block.push_str(label);
    block.push_str(":\n");
    let rendered = stream.render();
    block.push_str(&rendered);
    if !rendered.ends_with('\n') {
        block.push('\n');
    }
}

#[derive(Default)]
struct CapturedStream {
    head: Vec<u8>,
    tail: Vec<u8>,
    total: usize,
}

impl CapturedStream {
    fn push(&mut self, mut bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len());
        if self.head.len() < CONDITION_OUTPUT_EDGE {
            let count = bytes.len().min(CONDITION_OUTPUT_EDGE - self.head.len());
            self.head.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
        }
        if bytes.len() >= CONDITION_OUTPUT_EDGE {
            self.tail.clear();
            self.tail
                .extend_from_slice(&bytes[bytes.len() - CONDITION_OUTPUT_EDGE..]);
        } else if !bytes.is_empty() {
            let overflow = self
                .tail
                .len()
                .saturating_add(bytes.len())
                .saturating_sub(CONDITION_OUTPUT_EDGE);
            if overflow > 0 {
                self.tail.drain(..overflow);
            }
            self.tail.extend_from_slice(bytes);
        }
    }

    fn omitted(&self) -> usize {
        self.total.saturating_sub(self.head.len() + self.tail.len())
    }

    fn render(&self) -> String {
        if self.total == 0 {
            return "(empty)".to_string();
        }
        let omitted = self.omitted();
        if omitted == 0 {
            let bytes = [self.head.as_slice(), self.tail.as_slice()].concat();
            return display::human_text(String::from_utf8_lossy(&bytes));
        }
        let mut rendered = display::human_text(String::from_utf8_lossy(&self.head));
        if omitted > 0 {
            rendered.push_str(&format!("\n... {omitted} bytes omitted ...\n"));
        }
        rendered.push_str(&display::human_text(String::from_utf8_lossy(&self.tail)));
        rendered
    }
}

enum StreamMessage {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    StdoutDone(std::io::Result<()>),
    StderrDone(std::io::Result<()>),
}

struct CommandOutcome {
    status: ExitStatus,
    stdout: CapturedStream,
    stderr: CapturedStream,
}

enum SupervisionFailureKind {
    Timeout,
    Interrupted,
    Internal(String),
}

struct SupervisionFailure {
    kind: SupervisionFailureKind,
    termination: String,
    stdout: CapturedStream,
    stderr: CapturedStream,
}

struct InterruptGuard {
    previous: libc::sigaction,
    active: bool,
}

enum Handoff {
    Complete,
    Interrupted,
    TimedOut,
}

impl InterruptGuard {
    fn install() -> std::io::Result<Self> {
        INTERRUPTS.store(0, Ordering::Relaxed);
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = record_interrupt as *const () as usize;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            previous,
            active: true,
        })
    }

    fn handoff(&mut self, started: Instant, timeout: Duration) -> std::io::Result<Handoff> {
        let previous_mask = block_sigint()?;
        let mut pending: libc::sigset_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigpending(&mut pending) } == -1 {
            let error = std::io::Error::last_os_error();
            let _ = restore_signal_mask(&previous_mask);
            return Err(error);
        }
        let interrupted = INTERRUPTS.load(Ordering::Relaxed) > 0
            || unsafe { libc::sigismember(&pending, libc::SIGINT) } == 1;
        let result = if interrupted {
            Handoff::Interrupted
        } else if started.elapsed() >= timeout {
            Handoff::TimedOut
        } else {
            if unsafe { libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut()) } == -1
            {
                let error = std::io::Error::last_os_error();
                let _ = restore_signal_mask(&previous_mask);
                return Err(error);
            }
            self.active = false;
            Handoff::Complete
        };
        restore_signal_mask(&previous_mask)?;
        Ok(result)
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
            }
        }
    }
}

fn block_sigint() -> std::io::Result<libc::sigset_t> {
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
    }
    let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut previous) };
    if result == 0 {
        Ok(previous)
    } else {
        Err(std::io::Error::from_raw_os_error(result))
    }
}

fn restore_signal_mask(previous: &libc::sigset_t) -> std::io::Result<()> {
    let result =
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, previous, std::ptr::null_mut()) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(result))
    }
}

fn stream_reader(mut reader: impl Read, sender: SyncSender<StreamMessage>, stdout: bool) {
    let mut buffer = vec![0; 8192];
    let result = loop {
        match reader.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(count) => {
                let bytes = buffer[..count].to_vec();
                let message = if stdout {
                    StreamMessage::Stdout(bytes)
                } else {
                    StreamMessage::Stderr(bytes)
                };
                if sender.send(message).is_err() {
                    return;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => break Err(error),
        }
    };
    let _ = sender.send(if stdout {
        StreamMessage::StdoutDone(result)
    } else {
        StreamMessage::StderrDone(result)
    });
}

fn collect_message(
    message: StreamMessage,
    stdout: &mut CapturedStream,
    stderr: &mut CapturedStream,
    streams_open: &mut usize,
) -> std::io::Result<()> {
    match message {
        StreamMessage::Stdout(bytes) => stdout.push(&bytes),
        StreamMessage::Stderr(bytes) => stderr.push(&bytes),
        StreamMessage::StdoutDone(result) | StreamMessage::StderrDone(result) => {
            *streams_open = streams_open.saturating_sub(1);
            result?;
        }
    }
    Ok(())
}

enum SignalDelivery {
    Sent,
    AlreadyGone,
}

enum GroupExit {
    Gone,
    Present,
    Unknown(std::io::Error),
}

fn signal_group(pid: u32, signal: libc::c_int) -> std::io::Result<SignalDelivery> {
    let result = unsafe { libc::kill(-(pid as libc::pid_t), signal) };
    if result == 0 {
        Ok(SignalDelivery::Sent)
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(SignalDelivery::AlreadyGone)
        } else {
            Err(error)
        }
    }
}

fn process_group_exists(pid: u32) -> std::io::Result<bool> {
    let result = unsafe { libc::kill(-(pid as libc::pid_t), 0) };
    if result == 0 {
        Ok(true)
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else if error.raw_os_error() == Some(libc::EPERM) {
            Ok(true)
        } else {
            Err(error)
        }
    }
}

fn terminate_group(
    child: &mut Child,
    receiver: &Receiver<StreamMessage>,
    stdout: &mut CapturedStream,
    stderr: &mut CapturedStream,
    streams_open: &mut usize,
) -> String {
    let pid = child.id();
    let term_result = signal_group(pid, libc::SIGTERM);
    if let Err(term_error) = term_result {
        let kill_result = signal_group(pid, libc::SIGKILL);
        let group_exit = wait_for_group_exit(pid, child);
        drain_after_termination(receiver, stdout, stderr, streams_open);
        return match kill_result {
            Ok(SignalDelivery::Sent) => format!(
                "TERM failed ({term_error}); KILL sent; {}",
                group_exit_label(group_exit)
            ),
            Ok(SignalDelivery::AlreadyGone) => {
                format!("TERM failed ({term_error}); process group exited before KILL")
            }
            Err(kill_error) => {
                format!("TERM failed ({term_error}); KILL failed ({kill_error})")
            }
        };
    }
    if matches!(term_result, Ok(SignalDelivery::AlreadyGone)) {
        reap_child(child);
        drain_after_termination(receiver, stdout, stderr, streams_open);
        return "process group exited before TERM".to_string();
    }
    let deadline = Instant::now() + TERMINATION_GRACE;
    while Instant::now() < deadline {
        match receiver.recv_timeout(WAIT_INTERVAL) {
            Ok(message) => {
                let _ = collect_message(message, stdout, stderr, streams_open);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => *streams_open = 0,
        }
        let _ = child.try_wait();
        if matches!(process_group_exists(pid), Ok(false)) {
            reap_child(child);
            drain_after_termination(receiver, stdout, stderr, streams_open);
            return "TERM completed".to_string();
        }
    }
    let kill_result = signal_group(pid, libc::SIGKILL);
    let group_exit = wait_for_group_exit(pid, child);
    drain_after_termination(receiver, stdout, stderr, streams_open);
    match kill_result {
        Err(error) => format!("TERM sent; KILL failed after 1s grace ({error})"),
        Ok(SignalDelivery::AlreadyGone) => {
            "TERM completed during 1s grace; process group exited before KILL".to_string()
        }
        Ok(SignalDelivery::Sent) => format!(
            "TERM followed by KILL after 1s grace; {}",
            group_exit_label(group_exit)
        ),
    }
}

fn wait_for_group_exit(pid: u32, child: &mut Child) -> GroupExit {
    let deadline = Instant::now() + TERMINATION_GRACE;
    while Instant::now() < deadline {
        if let Err(error) = child.try_wait() {
            return GroupExit::Unknown(error);
        }
        match process_group_exists(pid) {
            Ok(false) => {
                reap_child(child);
                return GroupExit::Gone;
            }
            Ok(true) => std::thread::sleep(WAIT_INTERVAL),
            Err(error) => return GroupExit::Unknown(error),
        }
    }
    match process_group_exists(pid) {
        Ok(false) => {
            reap_child(child);
            GroupExit::Gone
        }
        Ok(true) => GroupExit::Present,
        Err(error) => GroupExit::Unknown(error),
    }
}

fn reap_child(child: &mut Child) {
    let deadline = Instant::now() + TERMINATION_GRACE;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => std::thread::sleep(WAIT_INTERVAL),
        }
    }
}

fn group_exit_label(exit: GroupExit) -> String {
    match exit {
        GroupExit::Gone => "process group exited".to_string(),
        GroupExit::Present => "process group still observable".to_string(),
        GroupExit::Unknown(error) => format!("process group state unknown ({error})"),
    }
}

fn emergency_kill(child: &mut Child) -> String {
    let pid = child.id();
    match signal_group(pid, libc::SIGKILL) {
        Ok(SignalDelivery::AlreadyGone) => "process group already exited".to_string(),
        Ok(SignalDelivery::Sent) => format!(
            "KILL sent; {}",
            group_exit_label(wait_for_group_exit(pid, child))
        ),
        Err(error) => format!("KILL failed ({error})"),
    }
}

fn drain_after_termination(
    receiver: &Receiver<StreamMessage>,
    stdout: &mut CapturedStream,
    stderr: &mut CapturedStream,
    streams_open: &mut usize,
) {
    let deadline = Instant::now() + TERMINATION_GRACE;
    while *streams_open > 0 && Instant::now() < deadline {
        match receiver.recv_timeout(WAIT_INTERVAL) {
            Ok(message) => {
                let _ = collect_message(message, stdout, stderr, streams_open);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => *streams_open = 0,
        }
    }
}

fn supervise(
    mut child: Child,
    timeout: Duration,
    mut guard: InterruptGuard,
) -> std::result::Result<CommandOutcome, Box<SupervisionFailure>> {
    let stdout_reader = child.stdout.take().expect("piped stdout");
    let stderr_reader = child.stderr.take().expect("piped stderr");
    let reader_mask = block_sigint().map_err(|error| {
        let termination = emergency_kill(&mut child);
        Box::new(SupervisionFailure {
            kind: SupervisionFailureKind::Internal(format!(
                "could not block Ctrl-C while starting output readers: {error}"
            )),
            termination,
            stdout: CapturedStream::default(),
            stderr: CapturedStream::default(),
        })
    })?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(16);
    let stdout_sender = sender.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("axon-condition-stdout".to_string())
        .spawn(move || stream_reader(stdout_reader, stdout_sender, true))
    {
        let termination = emergency_kill(&mut child);
        let _ = restore_signal_mask(&reader_mask);
        return Err(Box::new(SupervisionFailure {
            kind: SupervisionFailureKind::Internal(format!(
                "could not start stdout reader: {error}"
            )),
            termination,
            stdout: CapturedStream::default(),
            stderr: CapturedStream::default(),
        }));
    }
    if let Err(error) = std::thread::Builder::new()
        .name("axon-condition-stderr".to_string())
        .spawn(move || stream_reader(stderr_reader, sender, false))
    {
        let termination = emergency_kill(&mut child);
        let _ = restore_signal_mask(&reader_mask);
        return Err(Box::new(SupervisionFailure {
            kind: SupervisionFailureKind::Internal(format!(
                "could not start stderr reader: {error}"
            )),
            termination,
            stdout: CapturedStream::default(),
            stderr: CapturedStream::default(),
        }));
    }
    if let Err(error) = restore_signal_mask(&reader_mask) {
        let termination = emergency_kill(&mut child);
        return Err(Box::new(SupervisionFailure {
            kind: SupervisionFailureKind::Internal(format!(
                "could not restore Ctrl-C mask after starting output readers: {error}"
            )),
            termination,
            stdout: CapturedStream::default(),
            stderr: CapturedStream::default(),
        }));
    }

    let started = Instant::now();
    let mut stdout = CapturedStream::default();
    let mut stderr = CapturedStream::default();
    let mut streams_open = 2;
    let mut status = None;
    loop {
        if INTERRUPTS.load(Ordering::Relaxed) > 0 {
            let termination = terminate_group(
                &mut child,
                &receiver,
                &mut stdout,
                &mut stderr,
                &mut streams_open,
            );
            drop(guard);
            return Err(Box::new(SupervisionFailure {
                kind: SupervisionFailureKind::Interrupted,
                termination,
                stdout,
                stderr,
            }));
        }
        if started.elapsed() >= timeout {
            let termination = terminate_group(
                &mut child,
                &receiver,
                &mut stdout,
                &mut stderr,
                &mut streams_open,
            );
            return Err(Box::new(SupervisionFailure {
                kind: SupervisionFailureKind::Timeout,
                termination,
                stdout,
                stderr,
            }));
        }
        match receiver.recv_timeout(WAIT_INTERVAL.min(timeout.saturating_sub(started.elapsed()))) {
            Ok(message) => {
                if let Err(error) =
                    collect_message(message, &mut stdout, &mut stderr, &mut streams_open)
                {
                    let termination = terminate_group(
                        &mut child,
                        &receiver,
                        &mut stdout,
                        &mut stderr,
                        &mut streams_open,
                    );
                    return Err(Box::new(SupervisionFailure {
                        kind: SupervisionFailureKind::Internal(format!(
                            "could not collect condition output: {error}"
                        )),
                        termination,
                        stdout,
                        stderr,
                    }));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                streams_open = 0;
                std::thread::sleep(WAIT_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
            }
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(observed) => status = observed,
                Err(error) => {
                    let termination = terminate_group(
                        &mut child,
                        &receiver,
                        &mut stdout,
                        &mut stderr,
                        &mut streams_open,
                    );
                    return Err(Box::new(SupervisionFailure {
                        kind: SupervisionFailureKind::Internal(format!(
                            "could not observe condition process: {error}"
                        )),
                        termination,
                        stdout,
                        stderr,
                    }));
                }
            }
        }
        if let Some(status) = status
            && streams_open == 0
        {
            let kind = match guard.handoff(started, timeout) {
                Ok(Handoff::Complete) => {
                    return Ok(CommandOutcome {
                        status,
                        stdout,
                        stderr,
                    });
                }
                Ok(Handoff::Interrupted) => SupervisionFailureKind::Interrupted,
                Ok(Handoff::TimedOut) => SupervisionFailureKind::Timeout,
                Err(error) => SupervisionFailureKind::Internal(format!(
                    "could not finish Ctrl-C handling: {error}"
                )),
            };
            let termination = terminate_group(
                &mut child,
                &receiver,
                &mut stdout,
                &mut stderr,
                &mut streams_open,
            );
            return Err(Box::new(SupervisionFailure {
                kind,
                termination,
                stdout,
                stderr,
            }));
        }
    }
}

fn display_duration(duration: Duration) -> String {
    if duration.subsec_nanos() == 0 && duration.as_secs().is_multiple_of(3600) {
        format!("{}h", duration.as_secs() / 3600)
    } else if duration.subsec_nanos() == 0 && duration.as_secs().is_multiple_of(60) {
        format!("{}m", duration.as_secs() / 60)
    } else if duration.subsec_nanos() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon::lifecycle::*;
    use std::collections::BTreeSet;

    struct FlushFailure;
    impl Write for FlushFailure {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("flush failed"))
        }
    }

    #[test]
    fn real_process_spawn_and_trace_flush_failures_are_errors() {
        let mut snapshot = Snapshot::new(StoreId::generate());
        let id = "condition".to_string().try_into().unwrap();
        snapshot
            .create(
                id,
                Kind::Issue,
                Current {
                    title: "condition".into(),
                    description: String::new(),
                    lifecycle: Lifecycle::NotStarted,
                    condition: None,
                    parent: None,
                    dependencies: BTreeSet::new(),
                },
                Context {
                    at: chrono::Utc::now(),
                    recorder: None,
                },
            )
            .unwrap();
        let entity = snapshot.entities().next().unwrap();
        let missing =
            std::env::temp_dir().join(format!("axon-missing-{:032x}", rand::random::<u128>()));
        let error = Evaluation::with_timeout(missing.clone(), Duration::from_secs(1))
            .run_command(entity, "exit 0")
            .unwrap_err()
            .to_string();
        assert!(error.contains("could not start"));
        assert!(error.contains(missing.to_str().unwrap()));
        let error = Evaluation::with_trace_writer(
            std::env::current_dir().unwrap(),
            Duration::from_secs(1),
            Box::new(FlushFailure),
        )
        .run_command(entity, "exit 0")
        .unwrap_err()
        .to_string();
        assert!(error.contains("trace could not be written"));
        assert!(error.contains("flush failed"));

        fn cpu_time() -> f64 {
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
            usage.ru_utime.tv_sec as f64
                + usage.ru_stime.tv_sec as f64
                + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.0
        }
        let before = cpu_time();
        Evaluation::with_timeout(std::env::current_dir().unwrap(), Duration::from_secs(2))
            .run_command(entity, "exec >/dev/null 2>&1; sleep 0.5")
            .unwrap();
        assert!(
            cpu_time() - before < 0.25,
            "closed streams must not busy-poll"
        );
    }
}
