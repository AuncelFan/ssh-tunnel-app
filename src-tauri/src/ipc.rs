use crate::backend::Backend;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use socket2::{Domain, SockAddr, Socket, Type};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

type Reply = Result<Value, String>;
const MAX_FRAME: usize = 32 * 1024 * 1024;

fn send(socket: &mut Socket, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("Local message too large"));
    }
    socket.write_all(&(bytes.len() as u32).to_be_bytes())?;
    socket.write_all(&bytes)
}
fn receive<T: DeserializeOwned>(socket: &mut Socket) -> io::Result<T> {
    let mut size = [0; 4];
    socket.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size > MAX_FRAME {
        return Err(io::Error::other("Local message too large"));
    }
    let mut bytes = vec![0; size];
    socket.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn canonical_anchor(path: &Path) -> io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(e)?;
            Ok(canonical_anchor(parent)?.join(
                path.file_name()
                    .ok_or_else(|| io::Error::other("Invalid configuration filename"))?,
            ))
        }
        Err(e) => Err(e),
    }
}
// Unix-domain addresses are limited to about 100 bytes, including on Windows.
// Different spellings of the same filesystem socket can be used by each client.
fn local_address(path: &Path) -> io::Result<SockAddr> {
    if let Ok(address) = SockAddr::unix(path) {
        return Ok(address);
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing socket directory"))?;
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::other("Missing socket filename"))?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
        // The directory already exists, so Windows can supply its DOS alias when enabled.
        let wide: Vec<u16> = parent.as_os_str().encode_wide().chain(Some(0)).collect();
        let length = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
        if length > 0 {
            let mut buffer = vec![0; length as usize];
            let written = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), length) };
            if written > 0 && written < length {
                let short =
                    PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written as usize]))
                        .join(filename);
                if let Ok(address) = SockAddr::unix(short) {
                    return Ok(address);
                }
            }
        }
    }
    let cwd = std::env::current_dir()?.canonicalize()?;
    let parent = parent.canonicalize()?;
    for ancestor in cwd.ancestors() {
        if let Ok(suffix) = parent.strip_prefix(ancestor) {
            let mut relative = PathBuf::new();
            for _ in cwd.strip_prefix(ancestor).unwrap().components() {
                relative.push("..");
            }
            return SockAddr::unix(relative.join(suffix).join(filename));
        }
    }
    Err(io::Error::other(
        "Local socket path exceeds the platform limit",
    ))
}
#[derive(Clone)]
pub struct Client {
    path: PathBuf,
}
impl Client {
    pub fn new(anchor: &Path) -> io::Result<Self> {
        let directory = dirs::data_local_dir()
            .ok_or_else(|| io::Error::other("Cannot find per-user data directory"))?
            .join("ssh-tunnel-ipc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&directory)?;
            if directory.symlink_metadata()?.permissions().mode() & 0o077 != 0 {
                return Err(io::Error::other("IPC directory must be private (0700)"));
            }
        }
        #[cfg(windows)]
        fs::create_dir_all(&directory)?;
        let canonical = canonical_anchor(anchor)?;
        let key = canonical.as_os_str().as_encoded_bytes();
        #[cfg(windows)]
        let key = key.to_ascii_lowercase();
        let hash = format!("{:x}", Sha256::digest(key));
        let profile = directory.join(&hash[..24]);
        fs::create_dir_all(&profile)?;
        let path = profile.join("s");
        local_address(&path)?;
        Ok(Self { path })
    }
    fn socket(&self) -> io::Result<Socket> {
        let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
        socket.connect(&local_address(&self.path)?)?;
        Ok(socket)
    }
    pub fn ensure(&self, anchor: &Path, executable: &Path) -> io::Result<()> {
        if self.socket().is_ok() {
            return Ok(());
        }
        let mut command = Command::new(executable);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::{
                Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation},
                System::Console::{
                    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
                },
            };
            // A caller may itself be a child with inheritable pipes. Even when the
            // daemon has its own stdio, inheriting a spare writer prevents EOF.
            for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                // These borrowed handles remain owned by the process. Only change inheritance.
                unsafe {
                    let handle = GetStdHandle(stream);
                    if !handle.is_null()
                        && handle != INVALID_HANDLE_VALUE
                        && SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0
                    {
                        let error = io::Error::last_os_error();
                        // Console pseudo-handles on older Windows lack handle metadata.
                        if error.raw_os_error() != Some(6) {
                            return Err(error);
                        }
                    }
                }
            }
            command.creation_flags(0x08000000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .arg("--backend")
            .arg("--config")
            .arg(anchor)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "Cannot launch {}: {e}. Keep the GUI and CLI executables together.",
                        executable.display()
                    ),
                )
            })?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut exited = false;
        loop {
            if self.socket().is_ok() {
                return Ok(());
            }
            if !exited && let Some(status) = child.try_wait()? {
                exited = true;
                if !status.success() {
                    let mut detail = String::new();
                    child.stderr.take().unwrap().read_to_string(&mut detail)?;
                    return Err(io::Error::other(format!(
                        "Cannot start backend: {}",
                        detail.trim()
                    )));
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("Timed out connecting to local backend"));
            }
            thread::sleep(Duration::from_millis(40));
        }
    }
    pub fn call(&self, command: &str, args: Value) -> Reply {
        let mut socket = self
            .socket()
            .map_err(|e| format!("Cannot reach local backend: {e}"))?;
        send(&mut socket, &(command, args)).map_err(|e| e.to_string())?;
        receive::<Reply>(&mut socket).map_err(|e| format!("Lost backend response: {e}"))?
    }
    pub fn config_path(&self) -> Result<PathBuf, String> {
        serde_json::from_value(self.call("get_config_path", json!({}))?).map_err(|e| e.to_string())
    }
    pub fn events(&self, mut event: impl FnMut(Value)) -> io::Result<()> {
        let mut socket = self.socket()?;
        send(&mut socket, &("subscribe", json!({})))?;
        loop {
            event(receive(&mut socket)?);
        }
    }
}
pub fn serve(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Client::new(path)?;
    let owner = fs::File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(endpoint.path.with_extension("lock"))?;
    match owner.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Ok(()),
        Err(fs::TryLockError::Error(e)) => return Err(e.into()),
    }
    match fs::remove_file(&endpoint.path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let listener = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    listener.bind(&local_address(&endpoint.path)?)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let backend = Arc::new(runtime.block_on(Backend::open(path))?);
    listener.listen(128)?;
    let (stop, done) = std::sync::mpsc::channel();
    let server = Arc::new(Server {
        backend,
        stop,
        closing: false.into(),
        requests: Default::default(),
    });
    let handle = runtime.handle().clone();
    thread::spawn(move || {
        while let Ok((socket, _)) = listener.accept() {
            let server = server.clone();
            handle.spawn_blocking(move || server.handle(socket));
        }
    });
    let mut last_client = done.recv()?;
    fs::remove_file(endpoint.path)?;
    drop(owner);
    let result = send(&mut last_client, &Reply::Ok(Value::Null));
    // Event subscribers have a blocking reader for the lifetime of their GUI.
    runtime.shutdown_background();
    Ok(result?)
}

struct Server {
    backend: Arc<Backend>,
    stop: std::sync::mpsc::Sender<Socket>,
    closing: std::sync::atomic::AtomicBool,
    requests: std::sync::RwLock<()>,
}
impl Server {
    fn handle(&self, mut socket: Socket) -> io::Result<()> {
        use std::sync::atomic::Ordering;
        let (command, args): (String, Value) = receive(&mut socket)?;
        let runtime = tokio::runtime::Handle::current();
        if command == "subscribe" {
            let mut events = self.backend.controller.subscribe();
            loop {
                match runtime.block_on(events.recv()) {
                    Ok(event) => send(&mut socket, &event)?,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        send(&mut socket, &Value::Null)?
                    }
                    Err(_) => return Ok(()),
                }
            }
        }
        if command == "shutdown" && !self.closing.swap(true, Ordering::SeqCst) {
            // Interrupt a running probe before waiting for outstanding commands.
            runtime.block_on(self.backend.controller.stop_all());
            let _drained = self.requests.write().unwrap();
            runtime.block_on(self.backend.controller.stop_all());
            self.stop
                .send(socket)
                .map_err(|_| io::Error::other("Backend has exited"))?;
            return Ok(());
        }
        let _active = self.requests.read().unwrap();
        let reply = if self.closing.load(Ordering::SeqCst) {
            Err("Backend is shutting down".into())
        } else {
            runtime.block_on(self.backend.execute(&command, args))
        };
        send(&mut socket, &reply)
    }
}
