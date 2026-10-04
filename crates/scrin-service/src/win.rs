//! Windows side: service registration and control, launching the agent into
//! the console session, the local pipe, and the secure attention sequence.
//!
//! Every `unsafe` block calls one documented Win32 API with arguments that
//! are valid for the duration of the call; handles are closed by [`Owned`].

#![allow(unsafe_code)] // Win32 FFI; each block has a SAFETY comment

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DuplicateTokenEx, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SecurityImpersonation,
    SetTokenInformation, TOKEN_ALL_ACCESS, TokenPrimary, TokenSessionId,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use windows::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess,
    OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};
use winreg::RegKey;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};

use crate::ipc::{self, Decision, Reply, Request};
use crate::supervisor::{Action, Supervisor};

pub const SERVICE_NAME: &str = "scrin";
const DISPLAY_NAME: &str = "scrin remote desktop";
const DESCRIPTION: &str = "Keeps an accepted scrin session working on the lock screen and \
     elevation prompts, and sends Ctrl+Alt+Del when the remote technician asks.";
const POLICY_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
const POLICY_VALUE: &str = "SoftwareSASGeneration";
/// Where `install` remembers the value it replaced, so `uninstall` restores it.
const SAVED_KEY: &str = r"SOFTWARE\scrin\service";
const SAVED_VALUE: &str = "PreviousSoftwareSASGeneration";
/// 1 = services may generate the SAS (documented for `SendSAS`).
const SAS_BY_SERVICES: u32 = 1;
/// Pipe ACL: full access for `LocalSystem` only; nothing for anyone else.
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)";
const TICK: Duration = Duration::from_millis(500);

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A Win32 handle closed on drop.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: we own this handle and close it exactly once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

// SAFETY: a process handle may be used and closed from any thread.
unsafe impl Send for Owned {}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

// ---- install / uninstall ----------------------------------------------------

/// Registers the service (auto start, `LocalSystem`, restart on failure) and
/// enables `SoftwareSASGeneration` for services, remembering the old value.
pub fn install(agent: &Path) -> Result<()> {
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )?;
    let exe = std::env::current_exe()?;
    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec![OsString::from("run"), agent.as_os_str().to_owned()],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    let service =
        manager.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
    service.set_description(DESCRIPTION)?;
    let restart = || ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(5),
    };
    service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_hours(24)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart(), restart(), restart()]),
    })?;
    set_sas_policy()?;
    service.start::<&OsStr>(&[])?;
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    if service.query_status()?.current_state != ServiceState::Stopped {
        let _ = service.stop();
    }
    service.delete()?;
    restore_sas_policy()?;
    Ok(())
}

pub fn status() -> Result<String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(s) => Ok(format!("{:?}", s.query_status()?.current_state)),
        Err(_) => Ok("not installed".into()),
    }
}

fn set_sas_policy() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (policy, _) = hklm.create_subkey(POLICY_KEY)?;
    let previous: Option<u32> = policy.get_value(POLICY_VALUE).ok();
    let (saved, _) = hklm.create_subkey(SAVED_KEY)?;
    // Keep the very first value we replaced, even across reinstalls.
    if saved.get_value::<u32, _>(SAVED_VALUE).is_err() {
        // u32::MAX marks "the value did not exist".
        saved.set_value(SAVED_VALUE, &previous.unwrap_or(u32::MAX))?;
    }
    if previous.unwrap_or(0) & SAS_BY_SERVICES == 0 {
        policy.set_value(POLICY_VALUE, &(previous.unwrap_or(0) | SAS_BY_SERVICES))?;
    }
    Ok(())
}

fn restore_sas_policy() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(saved) = hklm.open_subkey_with_flags(SAVED_KEY, KEY_READ | KEY_WRITE) else {
        return Ok(());
    };
    let Ok(previous) = saved.get_value::<u32, _>(SAVED_VALUE) else {
        return Ok(());
    };
    let policy = hklm.open_subkey_with_flags(POLICY_KEY, KEY_WRITE)?;
    if previous == u32::MAX {
        let _ = policy.delete_value(POLICY_VALUE);
    } else {
        policy.set_value(POLICY_VALUE, &previous)?;
    }
    let _ = hklm.delete_subkey_all(r"SOFTWARE\scrin\service");
    Ok(())
}

// ---- service main -----------------------------------------------------------

define_windows_service!(ffi_service_main, service_main);

/// Hands the process to the service control manager (blocks until stop).
pub fn run_dispatcher() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

enum Msg {
    Stop,
    SessionChange,
    Pipe(Decision),
}

fn service_main(args: Vec<OsString>) {
    // Arguments come from the service config: `run <agent path>`.
    let agent = std::env::args_os()
        .nth(2)
        .or_else(|| args.into_iter().nth(1))
        .map(PathBuf::from);
    if let Err(e) = run_service(agent) {
        tracing::error!(error = %e, "service failed");
    }
}

fn run_service(agent: Option<PathBuf>) -> Result<()> {
    let agent = agent.ok_or("missing agent path")?;
    let (tx, rx) = mpsc::channel::<Msg>();
    let ctl_tx = tx.clone();
    let handle = service_control_handler::register(SERVICE_NAME, move |ev| match ev {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = ctl_tx.send(Msg::Stop);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::SessionChange(_) => {
            let _ = ctl_tx.send(Msg::SessionChange);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let set = |state: ServiceState, accept: ServiceControlAccept| {
        handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(5),
            process_id: None,
        })
    };
    set(
        ServiceState::Running,
        ServiceControlAccept::STOP
            | ServiceControlAccept::SHUTDOWN
            | ServiceControlAccept::SESSION_CHANGE,
    )?;

    let agent_pid: Arc<Mutex<Option<u32>>> = Arc::default();
    spawn_pipe_server(tx, agent_pid.clone());
    supervise(&agent, &rx, &agent_pid);

    set(ServiceState::Stopped, ServiceControlAccept::empty())?;
    Ok(())
}

struct Agent {
    session: u32,
    pid: u32,
    process: Owned,
}

fn supervise(agent_exe: &Path, rx: &mpsc::Receiver<Msg>, agent_pid: &Mutex<Option<u32>>) {
    let start = Instant::now();
    let now_ms = || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut sup = Supervisor::new();
    let mut agent: Option<Agent> = None;
    let mut actions = sup.on_console(console_session(), now_ms());
    loop {
        for a in actions.drain(..) {
            match a {
                Action::Launch(session) => match launch_agent(agent_exe, session) {
                    Ok(a) => {
                        tracing::info!(session, pid = a.pid, "agent started");
                        *agent_pid.lock().unwrap_or_else(PoisonError::into_inner) = Some(a.pid);
                        agent = Some(a);
                    }
                    Err(e) => {
                        tracing::warn!(session, error = %e, "agent launch failed");
                        sup.on_launch_failed(session, now_ms());
                    }
                },
                Action::Kill(session) => {
                    if let Some(a) = agent.take().filter(|a| a.session == session) {
                        // SAFETY: valid process handle we own; exit code 0.
                        unsafe {
                            let _ = TerminateProcess(a.process.0, 0);
                        }
                        *agent_pid.lock().unwrap_or_else(PoisonError::into_inner) = None;
                    }
                }
            }
        }
        match rx.recv_timeout(TICK) {
            Ok(Msg::Stop) => {
                for a in sup.stop() {
                    if let (Action::Kill(_), Some(a)) = (a, agent.take()) {
                        // SAFETY: valid process handle we own.
                        unsafe {
                            let _ = TerminateProcess(a.process.0, 0);
                        }
                    }
                }
                return;
            }
            Ok(Msg::SessionChange) => actions = sup.on_console(console_session(), now_ms()),
            Ok(Msg::Pipe(Decision::SendSas)) => send_sas(),
            Ok(Msg::Pipe(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if let Some(a) = &agent
            // SAFETY: valid process handle; zero timeout only polls.
            && unsafe { WaitForSingleObject(a.process.0, 0) } == WAIT_OBJECT_0
        {
            let session = a.session;
            agent = None;
            *agent_pid.lock().unwrap_or_else(PoisonError::into_inner) = None;
            tracing::info!(session, "agent exited");
            sup.on_agent_exit(session, now_ms());
        }
        actions.extend(sup.on_tick(now_ms()));
    }
}

fn console_session() -> u32 {
    // SAFETY: no arguments; returns 0xFFFFFFFF when no session is attached.
    unsafe { WTSGetActiveConsoleSessionId() }
}

/// Starts `exe` in `session` with a copy of the service's SYSTEM token whose
/// session id is changed, on the interactive desktop. The agent can then
/// follow the input desktop to the lock screen and elevation prompts.
fn launch_agent(exe: &Path, session: u32) -> Result<Agent> {
    let mut own = HANDLE::default();
    // SAFETY: current-process pseudo handle; `own` receives a new handle.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &raw mut own)? };
    let own = Owned(own);
    let mut dup = HANDLE::default();
    // SAFETY: `own.0` is a valid token handle; `dup` receives a new one.
    unsafe {
        DuplicateTokenEx(
            own.0,
            TOKEN_ALL_ACCESS,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &raw mut dup,
        )?;
    }
    let token = Owned(dup);
    // SAFETY: valid primary token; the buffer is a u32 of the stated size.
    unsafe {
        SetTokenInformation(
            token.0,
            TokenSessionId,
            (&raw const session).cast(),
            u32::try_from(size_of::<u32>()).unwrap_or(4),
        )?;
    }
    let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: valid token; `env` receives a block freed below.
    unsafe { CreateEnvironmentBlock(&raw mut env, Some(token.0), false)? };
    let mut desktop = wide(OsStr::new(r"winsta0\default"));
    let mut line = OsString::from("\"");
    line.push(exe.as_os_str());
    line.push("\" --agent");
    let mut cmd = wide(&line);
    let si = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(0),
        lpDesktop: PWSTR(desktop.as_mut_ptr()),
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: every pointer refers to a live local for the duration of the
    // call; `cmd` is mutable as CreateProcessW requires.
    let created = unsafe {
        CreateProcessAsUserW(
            Some(token.0),
            PCWSTR::null(),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NEW_PROCESS_GROUP,
            Some(env),
            PCWSTR::null(),
            &raw const si,
            &raw mut pi,
        )
    };
    // SAFETY: `env` came from CreateEnvironmentBlock.
    unsafe {
        let _ = DestroyEnvironmentBlock(env);
    }
    created?;
    drop(Owned(pi.hThread));
    Ok(Agent {
        session,
        pid: pi.dwProcessId,
        process: Owned(pi.hProcess),
    })
}

/// Secure attention sequence, as if Ctrl+Alt+Del were pressed locally.
/// Requires `SoftwareSASGeneration` (set by [`install`]).
fn send_sas() {
    #[link(name = "sas")]
    unsafe extern "system" {
        fn SendSAS(as_user: i32);
    }
    tracing::info!("sending the secure attention sequence");
    // SAFETY: documented export of sas.dll; FALSE = called from a service.
    unsafe { SendSAS(0) };
}

// ---- pipe -------------------------------------------------------------------

fn spawn_pipe_server(tx: mpsc::Sender<Msg>, agent_pid: Arc<Mutex<Option<u32>>>) {
    std::thread::spawn(move || {
        loop {
            if let Err(e) = serve_one(&tx, &agent_pid) {
                tracing::debug!(error = %e, "pipe client ended");
            }
            if tx.send(Msg::Pipe(Decision::Deny)).is_err() {
                return;
            }
        }
    });
}

fn serve_one(tx: &mpsc::Sender<Msg>, agent_pid: &Mutex<Option<u32>>) -> Result<()> {
    let pipe = Owned(create_pipe()?);
    // SAFETY: valid pipe handle; blocking connect (no OVERLAPPED).
    unsafe { ConnectNamedPipe(pipe.0, None)? };
    let mut client_pid = 0u32;
    // SAFETY: valid connected pipe; `client_pid` receives the OS-reported pid.
    unsafe { GetNamedPipeClientProcessId(pipe.0, &raw mut client_pid)? };
    let mut greeted = false;
    loop {
        let mut len = [0u8; 1];
        read_exact(&pipe, &mut len)?;
        let n = ipc::body_len(len[0])?;
        let mut body = vec![0u8; n];
        read_exact(&pipe, &mut body)?;
        let req = Request::decode(&body)?;
        let pid = *agent_pid.lock().unwrap_or_else(PoisonError::into_inner);
        let decision = ipc::authorize(req, client_pid, greeted, pid);
        let reply = if decision == Decision::Deny {
            tracing::warn!(client_pid, "pipe request denied");
            Reply::Denied
        } else {
            greeted |= decision == Decision::Greet;
            Reply::Ack
        };
        write_all(&pipe, &reply.encode())?;
        if matches!(decision, Decision::SendSas | Decision::Shutdown) {
            tx.send(Msg::Pipe(decision))?;
        }
        if decision == Decision::Deny {
            // SAFETY: valid pipe handle.
            unsafe {
                let _ = DisconnectNamedPipe(pipe.0);
            }
            return Ok(());
        }
    }
}

fn create_pipe() -> Result<HANDLE> {
    let sddl = wide(OsStr::new(PIPE_SDDL));
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: NUL-terminated SDDL; `sd` receives a LocalAlloc'd descriptor.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &raw mut sd,
            None,
        )?;
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    let name = wide(OsStr::new(ipc::PIPE_NAME));
    // SAFETY: valid name and attributes for the duration of the call.
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_DUPLEX.0),
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64,
            64,
            0,
            Some(&raw const sa),
        )
    };
    // SAFETY: `sd` was allocated by the SDDL conversion above.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    if pipe.is_invalid() {
        return Err(windows::core::Error::from_thread().into());
    }
    Ok(pipe)
}

fn read_exact(pipe: &Owned, buf: &mut [u8]) -> Result<()> {
    let mut off = 0;
    while off < buf.len() {
        let mut n = 0u32;
        // SAFETY: valid pipe; the slice outlives the synchronous call.
        unsafe { ReadFile(pipe.0, Some(&mut buf[off..]), Some(&raw mut n), None)? };
        if n == 0 {
            return Err("pipe closed".into());
        }
        off += usize::try_from(n)?;
    }
    Ok(())
}

fn write_all(pipe: &Owned, buf: &[u8]) -> Result<()> {
    let mut n = 0u32;
    // SAFETY: valid pipe; the slice outlives the synchronous call.
    unsafe { WriteFile(pipe.0, Some(buf), Some(&raw mut n), None)? };
    Ok(())
}
