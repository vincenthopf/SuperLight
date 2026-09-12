use crate::{Paths, Request, Response, PROTOCOL_VERSION, store};
use interprocess::{ConnectWaitMode, local_socket::{prelude::*, ConnectOptions, Listener, ListenerOptions, Stream}};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{fs, io::{self, Read, Write}, time::{Duration, Instant}};

pub const FRAME_LIMIT: usize = superlight_core::CONFIG_LIMIT + 65_536;
pub const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub protocol: u8,
    pub name: String,
    pub token: String,
    pub instance: String,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    protocol: u8,
    token: String,
    request: Request,
}

pub struct Server {
    listener: Listener,
    pub endpoint: Endpoint,
    paths: Paths,
    #[cfg(unix)]
    _directory: tempfile::TempDir,
}

pub struct Connection {
    stream: Stream,
    pub request: Request,
}

impl Connection {
    pub fn respond(self, response: &Response) -> io::Result<()> {
        write_frame(&self.stream, response, Instant::now() + TIMEOUT)
    }
}

fn random_hex<const N: usize>() -> io::Result<String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    let mut text = String::with_capacity(N * 2);
    for byte in bytes {
        text.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        text.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    Ok(text)
}

impl Server {
    pub fn bind(paths: Paths) -> io::Result<Self> {
        paths.prepare()?;
        let instance = random_hex::<16>()?;
        #[cfg(unix)]
        let directory = tempfile::Builder::new().prefix("superlight-").tempdir_in("/tmp")?;
        #[cfg(unix)]
        let name = directory.path().join("control.sock").to_str().ok_or_else(|| io::Error::other("Invalid socket path"))?.to_owned();
        #[cfg(windows)]
        let name = format!("superlight.{instance}");
        let endpoint = Endpoint { protocol: PROTOCOL_VERSION, name, token: random_hex::<32>()?, instance };
        let listener = ListenerOptions::new().name(socket_name(&endpoint.name)?).create_sync()?;
        let bytes = serde_json::to_vec(&endpoint).map_err(invalid)?;
        store::atomic_write(&paths.endpoint, &bytes)?;
        Ok(Self {
            listener, endpoint, paths,
            #[cfg(unix)]
            _directory: directory,
        })
    }

    pub fn accept(&self) -> io::Result<Connection> {
        let stream = self.listener.accept()?;
        let envelope: Envelope = read_frame(&stream, Instant::now() + TIMEOUT)?;
        if envelope.protocol != PROTOCOL_VERSION || !token_matches(&envelope.token, &self.endpoint.token) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Unauthorized local request"));
        }
        Ok(Connection { stream, request: envelope.request })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(bytes) = store::read_limited(&self.paths.endpoint, 4096)
            && let Ok(endpoint) = serde_json::from_slice::<Endpoint>(&bytes)
            && endpoint.instance == self.endpoint.instance {
            let _ = fs::remove_file(&self.paths.endpoint);
        }
    }
}

fn token_matches(left: &str, right: &str) -> bool {
    if left.len() != 64 || right.len() != 64 { return false; }
    left.bytes().zip(right.bytes()).fold(0u8, |difference, (left, right)| difference | (left ^ right)) == 0
}

fn socket_name(name: &str) -> io::Result<interprocess::local_socket::Name<'_>> {
    #[cfg(unix)] {
        use interprocess::local_socket::GenericFilePath;
        name.to_fs_name::<GenericFilePath>()
    }
    #[cfg(windows)] {
        use interprocess::local_socket::GenericNamespaced;
        if !name.starts_with("superlight.") || name.len() > 64 || name.contains(['\\', '/']) {
            return Err(invalid("Invalid local pipe name"));
        }
        name.to_ns_name::<GenericNamespaced>()
    }
}

impl Endpoint {
    pub fn load(paths: &Paths) -> io::Result<Self> {
        let endpoint: Self = serde_json::from_slice(&store::read_limited(&paths.endpoint, 4096)?).map_err(invalid)?;
        if endpoint.protocol != PROTOCOL_VERSION || endpoint.token.len() != 64 || endpoint.instance.len() != 32 {
            return Err(invalid("Unsupported or malformed SuperLight endpoint"));
        }
        Ok(endpoint)
    }

    fn connect(&self) -> io::Result<Stream> {
        ConnectOptions::new().name(socket_name(&self.name)?).wait_mode(ConnectWaitMode::Timeout(TIMEOUT)).connect_sync()
    }

    pub fn wake(&self) {
        let _ = self.connect();
    }
}

pub fn call(paths: &Paths, request: &Request) -> io::Result<Response> {
    let endpoint = Endpoint::load(paths)?;
    call_endpoint(&endpoint, request)
}

pub fn call_endpoint(endpoint: &Endpoint, request: &Request) -> io::Result<Response> {
    let stream = endpoint.connect()?;
    let deadline = Instant::now() + TIMEOUT;
    write_frame(&stream, &Envelope { protocol: PROTOCOL_VERSION, token: endpoint.token.clone(), request: request.clone() }, deadline)?;
    let response: Response = read_frame(&stream, deadline)?;
    if response.protocol != PROTOCOL_VERSION { return Err(invalid("Unsupported response protocol")); }
    Ok(response)
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline.checked_duration_since(Instant::now()).filter(|duration| !duration.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Local request timed out"))
}

fn read_exact(mut stream: &Stream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_recv_timeout(Some(remaining(deadline)?))?;
        match stream.read(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Local connection closed")),
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_all(mut stream: &Stream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_send_timeout(Some(remaining(deadline)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "Local connection stopped accepting data")),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_frame(stream: &Stream, value: &impl Serialize, deadline: Instant) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() > FRAME_LIMIT { return Err(invalid("Local request exceeds the size limit")); }
    write_all(stream, &(bytes.len() as u32).to_le_bytes(), deadline)?;
    write_all(stream, &bytes, deadline)
}

fn read_frame<T: DeserializeOwned>(stream: &Stream, deadline: Instant) -> io::Result<T> {
    let mut header = [0; 4];
    read_exact(stream, &mut header, deadline)?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > FRAME_LIMIT { return Err(invalid("Invalid local request size")); }
    let mut bytes = vec![0; len];
    read_exact(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(invalid)
}

fn invalid(error: impl ToString) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, error.to_string()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snapshot;
    use std::thread;

    #[test]
    fn authenticated_round_trip_and_endpoint_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let paths = Paths::in_dir(directory.path());
        let server = Server::bind(paths.clone()).unwrap();
        let endpoint = server.endpoint.clone();
        let join = thread::spawn(move || {
            let connection = server.accept().unwrap();
            assert!(matches!(connection.request, Request::Get));
            connection.respond(&Response::success(Snapshot { revision: 7, ..Snapshot::default() })).unwrap();
        });
        let response = call_endpoint(&endpoint, &Request::Get).unwrap();
        assert!(response.ok);
        assert_eq!(response.snapshot.unwrap().revision, 7);
        join.join().unwrap();
        assert!(!paths.endpoint.exists());
    }

    #[test]
    fn wrong_authentication_never_dispatches_a_command() {
        let directory = tempfile::tempdir().unwrap();
        let server = Server::bind(Paths::in_dir(directory.path())).unwrap();
        let mut endpoint = server.endpoint.clone();
        endpoint.token = "0".repeat(64);
        let join = thread::spawn(move || assert_eq!(server.accept().err().unwrap().kind(), io::ErrorKind::PermissionDenied));
        assert!(call_endpoint(&endpoint, &Request::Quit).is_err());
        join.join().unwrap();
    }

    #[test]
    fn oversized_frame_is_rejected_before_reading_its_body() {
        let directory = tempfile::tempdir().unwrap();
        let server = Server::bind(Paths::in_dir(directory.path())).unwrap();
        let endpoint = server.endpoint.clone();
        let join = thread::spawn(move || assert_eq!(server.accept().err().unwrap().kind(), io::ErrorKind::InvalidData));
        let stream = endpoint.connect().unwrap();
        write_all(&stream, &u32::MAX.to_le_bytes(), Instant::now() + TIMEOUT).unwrap();
        join.join().unwrap();
    }

    #[test]
    fn incomplete_frame_does_not_poison_the_next_connection() {
        let directory = tempfile::tempdir().unwrap();
        let server = Server::bind(Paths::in_dir(directory.path())).unwrap();
        let endpoint = server.endpoint.clone();
        let join = thread::spawn(move || {
            assert!(server.accept().is_err());
            server.accept().unwrap().respond(&Response::success(Snapshot::default())).unwrap();
        });
        endpoint.wake();
        assert!(call_endpoint(&endpoint, &Request::Get).unwrap().ok);
        join.join().unwrap();
    }

    #[test]
    fn tokens_are_distinct_and_fixed_length() {
        let one = random_hex::<32>().unwrap();
        let two = random_hex::<32>().unwrap();
        assert_eq!(one.len(), 64);
        assert_ne!(one, two);
        assert!(token_matches(&one, &one));
        assert!(!token_matches(&one, &two));
        assert!(!token_matches("", ""));
    }

    #[test]
    fn total_frame_deadline_is_enforced() {
        let directory = tempfile::tempdir().unwrap();
        let server = Server::bind(Paths::in_dir(directory.path())).unwrap();
        let endpoint = server.endpoint.clone();
        let join = thread::spawn(move || {
            let stream = server.listener.accept().unwrap();
            let start = Instant::now();
            assert!(read_frame::<Envelope>(&stream, start + Duration::from_millis(100)).is_err());
            assert!(start.elapsed() < Duration::from_secs(2));
        });
        let stream = endpoint.connect().unwrap();
        thread::sleep(Duration::from_millis(200));
        drop(stream);
        join.join().unwrap();
    }
}
