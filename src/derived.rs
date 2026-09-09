use crate::display;
use crate::domain::*;
use chrono::Utc;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

pub const DEFAULT_CONDITION_TIMEOUT: Duration = Duration::from_secs(30);
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
    commands_enabled: bool,
    results: RefCell<HashMap<EntityId, Result<bool>>>,
    trace: Option<RefCell<Box<dyn Write>>>,
}

impl Evaluation {
    pub fn new(root: PathBuf) -> Self {
        Self::with_timeout(root, DEFAULT_CONDITION_TIMEOUT)
    }

    pub fn with_timeout(root: PathBuf, timeout: Duration) -> Self {
        Self {
            root,
            timeout,
            commands_enabled: true,
            results: RefCell::new(HashMap::new()),
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
            commands_enabled: true,
            results: RefCell::new(HashMap::new()),
            trace: Some(RefCell::new(writer)),
        }
    }

    fn command(&self, entity: &Entity, script: &str) -> Result<bool> {
        if !self.commands_enabled {
            return Err(EvaluationError("Command evaluation skipped".to_string()));
        }
        if let Some(result) = self.results.borrow().get(&entity.id) {
            return result.clone();
        }
        let result = self.run_command(entity, script);
        self.results
            .borrow_mut()
            .insert(entity.id.clone(), result.clone());
        result
    }

    fn run_command(&self, entity: &Entity, script: &str) -> Result<bool> {
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
                self.root.display()
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
                "{}: Command({}) evaluation failed: {detail}\nstdout:\n{}\nstderr:\n{}",
                entity.id,
                display::human_text(script),
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
                "{}: Command({}) failed: {}\nstdout:\n{}\nstderr:\n{}",
                entity.id,
                display::human_text(script),
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
        let mut rendered = display::human_text(String::from_utf8_lossy(&self.head));
        let omitted = self.omitted();
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
            Err(RecvTimeoutError::Disconnected) => streams_open = 0,
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

#[derive(Clone)]
pub struct View {
    by_id: HashMap<EntityId, Entity>,
    order: Vec<EntityId>,
    deps: Vec<(EntityId, EntityId)>,
    evaluation: Rc<Evaluation>,
    at: Option<chrono::DateTime<Utc>>,
}

impl View {
    #[cfg(test)]
    pub fn new(entities: Vec<Entity>, deps: Vec<(EntityId, EntityId)>) -> Self {
        Self::with_evaluation(
            entities,
            deps,
            Rc::new(Evaluation::new(std::env::current_dir().unwrap())),
        )
    }

    pub fn with_evaluation(
        entities: Vec<Entity>,
        deps: Vec<(EntityId, EntityId)>,
        evaluation: Rc<Evaluation>,
    ) -> Self {
        let order = entities.iter().map(|entity| entity.id.clone()).collect();
        let by_id = entities
            .into_iter()
            .map(|entity| (entity.id.clone(), entity))
            .collect();
        Self {
            by_id,
            order,
            deps,
            evaluation,
            at: None,
        }
    }

    pub fn without_command_evaluation(mut self) -> Self {
        let mut evaluation = Evaluation::new(self.evaluation.root.clone());
        evaluation.commands_enabled = false;
        self.evaluation = Rc::new(evaluation);
        self
    }

    pub fn observed_surfaced(&self, entity: &Entity) -> Option<bool> {
        match &entity.resurface_condition {
            ResurfaceCondition::Command(_) => None,
            ResurfaceCondition::Always => Some(true),
            ResurfaceCondition::Manual => Some(false),
            ResurfaceCondition::AtDate(at) => {
                Some(at.instant() <= self.at.unwrap_or_else(Utc::now))
            }
            ResurfaceCondition::AfterEntity(target) => {
                Some(self.get(target).is_none_or(Entity::is_terminal))
            }
        }
    }

    pub fn observed_gate(&self, entity: &Entity) -> Option<bool> {
        observed_all([
            Some(
                entity.kind == EntityKind::Group
                    && matches!(entity.progress, Progress::InProgress(_))
                    && entity.disposition == Disposition::Accepted
                    && !self.is_blocked(&entity.id)
                    && !self.is_orphaned(&entity.id),
            ),
            self.observed_surfaced(entity),
        ])
    }

    pub fn observed_active_scope(&self, id: &EntityId) -> Option<bool> {
        observed_all(
            self.ancestors(id)
                .into_iter()
                .map(|group| self.observed_gate(group)),
        )
    }

    pub fn observed_ready(&self, entity: &Entity) -> Option<bool> {
        observed_all([
            Some(
                matches!(entity.progress, Progress::NotStarted)
                    && entity.disposition == Disposition::Accepted
                    && !self.is_blocked(&entity.id)
                    && !self.is_orphaned(&entity.id),
            ),
            self.observed_surfaced(entity),
            self.observed_active_scope(&entity.id),
        ])
    }

    pub fn observed_blocking_causes(&self, id: &EntityId) -> Option<Vec<&Entity>> {
        fn walk<'a>(
            view: &'a View,
            id: &EntityId,
            seen: &mut HashSet<EntityId>,
            result: &mut Vec<&'a Entity>,
        ) -> Option<()> {
            for target in view.dependency_targets(id) {
                if target.is_terminal() || !seen.insert(target.id.clone()) {
                    continue;
                }
                let not_root = observed_all([
                    Some(!view.is_orphaned(&target.id)),
                    view.observed_surfaced(target),
                    view.observed_active_scope(&target.id),
                    Some(
                        !view
                            .dependency_targets(&target.id)
                            .into_iter()
                            .all(Entity::is_terminal),
                    ),
                ])?;
                if !not_root {
                    result.push(target);
                } else {
                    walk(view, &target.id, seen, result)?;
                }
            }
            Some(())
        }
        let mut result = Vec::new();
        walk(self, id, &mut HashSet::new(), &mut result)?;
        Some(result)
    }

    pub fn at(mut self, at: chrono::DateTime<Utc>) -> Self {
        self.at = Some(at);
        self
    }

    pub fn get(&self, id: &EntityId) -> Option<&Entity> {
        self.by_id.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.order.iter().filter_map(|id| self.by_id.get(id))
    }

    pub fn dependencies(&self) -> &[(EntityId, EntityId)] {
        &self.deps
    }

    pub fn direct_dependencies(&self, id: &EntityId) -> Vec<&Entity> {
        self.deps
            .iter()
            .filter(|(source, _)| source == id)
            .filter_map(|(_, target)| self.get(target))
            .collect()
    }

    pub fn direct_dependents(&self, id: &EntityId) -> Vec<&Entity> {
        self.deps
            .iter()
            .filter(|(_, target)| target == id)
            .filter_map(|(source, _)| self.get(source))
            .collect()
    }

    pub fn direct_after_entity_waiters(&self, id: &EntityId) -> Vec<&Entity> {
        let mut waiters = self
            .iter()
            .filter(|entity| {
                matches!(
                    &entity.resurface_condition,
                    ResurfaceCondition::AfterEntity(target) if target == id
                )
            })
            .collect::<Vec<_>>();
        waiters.sort_by(|a, b| a.id.cmp(&b.id));
        waiters
    }

    pub fn ancestors(&self, id: &EntityId) -> Vec<&Entity> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut current = self.get(id).and_then(|entity| entity.parent.as_ref());
        while let Some(parent_id) = current {
            if !seen.insert(parent_id.clone()) {
                break;
            }
            let Some(parent) = self.get(parent_id) else {
                break;
            };
            result.push(parent);
            current = parent.parent.as_ref();
        }
        result
    }

    pub fn descendants(&self, id: &EntityId) -> Vec<&Entity> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![id.clone()];
        while let Some(parent) = stack.pop() {
            for child in self
                .iter()
                .filter(|entity| entity.parent.as_ref() == Some(&parent))
            {
                if seen.insert(child.id.clone()) {
                    result.push(child);
                    stack.push(child.id.clone());
                }
            }
        }
        result
    }

    pub fn direct_children(&self, id: &EntityId) -> Vec<&Entity> {
        self.iter()
            .filter(|entity| entity.parent.as_ref() == Some(id))
            .collect()
    }

    fn waiting_sources(&self, id: &EntityId) -> Vec<&Entity> {
        self.get(id).into_iter().chain(self.ancestors(id)).collect()
    }

    pub fn dependency_targets(&self, id: &EntityId) -> Vec<&Entity> {
        let mut seen = HashSet::new();
        self.waiting_sources(id)
            .into_iter()
            .flat_map(|source| self.direct_dependencies(&source.id))
            .filter(|target| seen.insert(target.id.clone()))
            .collect()
    }

    pub fn is_surfaced(&self, entity: &Entity) -> Result<bool> {
        match &entity.resurface_condition {
            ResurfaceCondition::Command(script) => self.evaluation.command(entity, script),
            _ => Ok(self
                .observed_surfaced(entity)
                .expect("non-Command condition is known")),
        }
    }

    pub fn is_blocked(&self, id: &EntityId) -> bool {
        self.dependency_targets(id)
            .into_iter()
            .any(|target| !target.is_terminal())
    }

    pub fn is_orphaned(&self, id: &EntityId) -> bool {
        self.dependency_targets(id)
            .into_iter()
            .any(|target| target.disposition == Disposition::Rejected)
    }

    pub fn opens_descendants(&self, group: &Entity) -> Result<bool> {
        Ok(group.kind == EntityKind::Group
            && matches!(group.progress, Progress::InProgress(_))
            && group.disposition == Disposition::Accepted
            && self.is_surfaced(group)?
            && !self.is_blocked(&group.id)
            && !self.is_orphaned(&group.id))
    }

    pub fn within_active_scope(&self, id: &EntityId) -> Result<bool> {
        for group in self.ancestors(id) {
            if !self.opens_descendants(group)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn is_ready(&self, entity: &Entity) -> Result<bool> {
        Ok(matches!(entity.progress, Progress::NotStarted)
            && entity.disposition == Disposition::Accepted
            && self.is_surfaced(entity)?
            && self.within_active_scope(&entity.id)?
            && !self.is_blocked(&entity.id)
            && !self.is_orphaned(&entity.id))
    }

    pub fn ready(&self, include: impl Fn(&Entity) -> bool) -> Result<Vec<&Entity>> {
        let mut result = Vec::new();
        for entity in self.iter().filter(|entity| include(entity)) {
            if self.is_ready(entity)? {
                result.push(entity);
            }
        }
        Ok(result)
    }

    pub fn triage(
        &self,
        include: impl Fn(&Entity) -> bool,
    ) -> Result<Vec<(&Entity, TriageReason)>> {
        let mut result = Vec::new();
        for entity in self
            .iter()
            .filter(|entity| include(entity) && !entity.is_terminal())
        {
            let reason = if entity.disposition == Disposition::Undecided {
                TriageReason::Undecided
            } else if self.is_orphaned(&entity.id) {
                TriageReason::Orphaned
            } else {
                continue;
            };
            if self.is_surfaced(entity)? && self.within_active_scope(&entity.id)? {
                result.push((entity, reason));
            }
        }
        Ok(result)
    }

    pub fn claims(&self) -> Vec<(&Entity, &Claim)> {
        self.iter()
            .filter_map(|entity| entity.progress.claim().map(|claim| (entity, claim)))
            .collect()
    }

    pub fn group_completion_satisfied(&self, id: &EntityId) -> bool {
        self.descendants(id).into_iter().all(Entity::is_terminal)
    }

    pub fn can_complete_group(&self, id: &EntityId) -> bool {
        self.get(id).is_some_and(|entity| {
            entity.kind == EntityKind::Group
                && matches!(entity.progress, Progress::InProgress(_))
                && self.group_completion_satisfied(id)
        })
    }

    pub fn in_progress_descendants(&self, id: &EntityId) -> Vec<&Entity> {
        self.descendants(id)
            .into_iter()
            .filter(|entity| matches!(entity.progress, Progress::InProgress(_)))
            .collect()
    }

    pub fn group_summary(&self, id: &EntityId) -> GroupSummary {
        let direct = self.direct_children(id);
        let descendants = self.descendants(id);
        GroupSummary {
            direct: EntityCounts::from_entities(&direct),
            descendants: EntityCounts::from_entities(&descendants),
        }
    }

    fn is_blocking_root_cause(&self, entity: &Entity) -> Result<bool> {
        Ok(self.is_orphaned(&entity.id)
            || !self.is_surfaced(entity)?
            || !self.within_active_scope(&entity.id)?
            || self
                .dependency_targets(&entity.id)
                .into_iter()
                .all(Entity::is_terminal))
    }

    pub fn blocking_causes(&self, id: &EntityId) -> Result<Vec<&Entity>> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        self.walk_causes(id, &mut seen, &mut result)?;
        Ok(result)
    }

    fn walk_causes<'a>(
        &'a self,
        id: &EntityId,
        seen: &mut HashSet<EntityId>,
        result: &mut Vec<&'a Entity>,
    ) -> Result<()> {
        for target in self.dependency_targets(id) {
            if target.is_terminal() || !seen.insert(target.id.clone()) {
                continue;
            }
            if self.is_blocking_root_cause(target)? {
                result.push(target);
            } else {
                self.walk_causes(&target.id, seen, result)?;
            }
        }
        Ok(())
    }
}

fn observed_all(values: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut result = Some(true);
    for value in values {
        if value == Some(false) {
            return Some(false);
        }
        if value.is_none() {
            result = None;
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageReason {
    Undecided,
    Orphaned,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EntityCounts {
    pub total: usize,
    pub issues: usize,
    pub groups: usize,
    pub not_started: usize,
    pub in_progress: usize,
    pub ended: usize,
    pub undecided: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub terminal: usize,
}

impl EntityCounts {
    fn from_entities(entities: &[&Entity]) -> Self {
        let mut counts = Self::default();
        for entity in entities {
            counts.total += 1;
            match entity.kind {
                EntityKind::Issue => counts.issues += 1,
                EntityKind::Group => counts.groups += 1,
            }
            match entity.progress {
                Progress::NotStarted => counts.not_started += 1,
                Progress::InProgress(_) => counts.in_progress += 1,
                Progress::Ended => counts.ended += 1,
            }
            match entity.disposition {
                Disposition::Undecided => counts.undecided += 1,
                Disposition::Accepted => counts.accepted += 1,
                Disposition::Rejected => counts.rejected += 1,
            }
            counts.terminal += usize::from(entity.is_terminal());
        }
        counts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    pub direct: EntityCounts,
    pub descendants: EntityCounts,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn entity(
        id: &str,
        kind: EntityKind,
        progress: Progress,
        disposition: Disposition,
        parent: Option<&str>,
    ) -> Entity {
        let now = Utc::now();
        Entity {
            id: EntityId::from_stored(id),
            kind,
            title: id.to_string(),
            description: None,
            progress,
            disposition,
            current_revision: (disposition != Disposition::Undecided)
                .then(|| RecordId::new(RecordKind::Revision)),
            resurface_condition: ResurfaceCondition::Always,
            parent: parent.map(EntityId::from_stored),
            created_at: now,
            updated_at: now,
        }
    }

    fn claim() -> Claim {
        Claim {
            actor: "tester".to_string(),
            worktree: "/worktree".to_string(),
            at: Utc::now(),
        }
    }

    fn id(value: &str) -> EntityId {
        EntityId::from_stored(value)
    }

    #[test]
    fn at_date_compares_the_exact_instant_across_offsets() {
        let mut item = entity(
            "timed",
            EntityKind::Issue,
            Progress::NotStarted,
            Disposition::Accepted,
            None,
        );
        item.resurface_condition =
            ResurfaceCondition::AtDate("2026-09-08T12:00:00.123456789+09:00".parse().unwrap());
        let before = "2026-09-08T03:00:00.123456788Z".parse().unwrap();
        let exact = "2026-09-08T03:00:00.123456789Z".parse().unwrap();
        assert_eq!(
            View::new(vec![item.clone()], vec![])
                .at(before)
                .observed_surfaced(&item),
            Some(false)
        );
        assert_eq!(
            View::new(vec![item.clone()], vec![])
                .at(exact)
                .observed_surfaced(&item),
            Some(true)
        );
    }

    #[test]
    fn command_spawn_failure_is_an_error_and_is_shared() {
        let entity = entity(
            "command",
            EntityKind::Issue,
            Progress::NotStarted,
            Disposition::Accepted,
            None,
        );
        let evaluation = Evaluation::new(
            std::env::temp_dir().join(format!("axon-missing-command-root-{}", std::process::id())),
        );
        let first = evaluation
            .command(&entity, "exit 0")
            .unwrap_err()
            .to_string();
        assert!(first.contains("command"));
        assert!(first.contains("could not start"));
        assert!(first.contains("exit 0"));
        assert_eq!(
            evaluation
                .command(&entity, "exit 0")
                .unwrap_err()
                .to_string(),
            first
        );
        assert_eq!(evaluation.results.borrow().len(), 1);
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("trace sink failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn command_trace_write_failure_is_an_evaluation_error_and_is_shared() {
        let entity = entity(
            "command",
            EntityKind::Issue,
            Progress::NotStarted,
            Disposition::Accepted,
            None,
        );
        let evaluation = Evaluation::with_trace_writer(
            std::env::current_dir().unwrap(),
            DEFAULT_CONDITION_TIMEOUT,
            Box::new(FailingWriter),
        );
        let first = evaluation
            .command(&entity, "exit 0")
            .unwrap_err()
            .to_string();
        assert!(first.contains("command"));
        assert!(first.contains("trace could not be written"));
        assert!(first.contains("trace sink failed"));
        assert_eq!(
            evaluation
                .command(&entity, "exit 0")
                .unwrap_err()
                .to_string(),
            first
        );
        assert_eq!(evaluation.results.borrow().len(), 1);
    }

    #[test]
    fn group_gate_exposes_only_the_current_frontier() {
        let closed = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("g"),
                ),
            ],
            vec![],
        );
        assert_eq!(
            closed
                .ready(|_| true)
                .unwrap()
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["g"]
        );

        let open = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("g"),
                ),
            ],
            vec![],
        );
        assert_eq!(
            open.ready(|_| true)
                .unwrap()
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["i"]
        );
    }

    #[test]
    fn group_dependencies_apply_to_all_descendants() {
        let view = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("g"),
                ),
                entity(
                    "x",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    None,
                ),
            ],
            vec![(id("g"), id("x"))],
        );
        assert!(view.is_blocked(&id("g")));
        assert!(view.is_blocked(&id("i")));
        assert!(!view.is_ready(view.get(&id("i")).unwrap()).unwrap());
    }

    #[test]
    fn rejected_group_does_not_make_descendants_terminal() {
        let view = View::new(
            vec![
                entity(
                    "parent",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "child",
                    EntityKind::Group,
                    Progress::NotStarted,
                    Disposition::Rejected,
                    Some("parent"),
                ),
                entity(
                    "issue",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Accepted,
                    Some("child"),
                ),
            ],
            vec![],
        );
        assert!(!view.group_completion_satisfied(&id("parent")));
    }

    #[test]
    fn triage_stops_at_an_inactive_parent() {
        let view = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::NotStarted,
                    Disposition::Undecided,
                    None,
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::NotStarted,
                    Disposition::Undecided,
                    Some("g"),
                ),
            ],
            vec![],
        );
        assert_eq!(
            view.triage(|_| true)
                .unwrap()
                .iter()
                .map(|(e, _)| e.id.as_str())
                .collect::<Vec<_>>(),
            ["g"]
        );
    }

    #[test]
    fn completion_and_summary_include_all_descendants() {
        let view = View::new(
            vec![
                entity(
                    "g",
                    EntityKind::Group,
                    Progress::InProgress(claim()),
                    Disposition::Accepted,
                    None,
                ),
                entity(
                    "child",
                    EntityKind::Group,
                    Progress::Ended,
                    Disposition::Accepted,
                    Some("g"),
                ),
                entity(
                    "i",
                    EntityKind::Issue,
                    Progress::Ended,
                    Disposition::Accepted,
                    Some("child"),
                ),
            ],
            vec![],
        );
        assert!(view.can_complete_group(&id("g")));
        let summary = view.group_summary(&id("g"));
        assert_eq!(summary.direct.total, 1);
        assert_eq!(summary.descendants.total, 2);
        assert_eq!(summary.descendants.terminal, 2);
    }
}
