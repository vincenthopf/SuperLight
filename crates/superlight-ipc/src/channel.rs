use crate::{PROTOCOL_VERSION, Paths, Request, Response, store};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{self, Read, Write},
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::net::{TcpListener as Listener, TcpStream as Stream};
#[cfg(unix)]
use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

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
        let directory = tempfile::Builder::new()
            .prefix("superlight-")
            .tempdir_in("/tmp")?;
        #[cfg(unix)]
        let name = directory
            .path()
            .join("control.sock")
            .to_str()
            .ok_or_else(|| io::Error::other("Invalid socket path"))?
            .to_owned();
        #[cfg(unix)]
        let listener = Listener::bind(&name)?;
        #[cfg(windows)]
        let listener = Listener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        #[cfg(windows)]
        let name = listener.local_addr()?.to_string();
        let endpoint = Endpoint {
            protocol: PROTOCOL_VERSION,
            name,
            token: random_hex::<32>()?,
            instance,
        };
        store::atomic_write(
            &paths.endpoint,
            &serde_json::to_vec(&endpoint).map_err(invalid)?,
        )?;
        Ok(Self {
            listener,
            endpoint,
            paths,
            #[cfg(unix)]
            _directory: directory,
        })
    }

    pub fn accept(&self) -> io::Result<Connection> {
        let (stream, _) = self.listener.accept()?;
        configure(&stream)?;
        let envelope: Envelope = read_frame(&stream, Instant::now() + TIMEOUT)?;
        if envelope.protocol != PROTOCOL_VERSION
            || !token_matches(&envelope.token, &self.endpoint.token)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unauthorized local request",
            ));
        }
        Ok(Connection {
            stream,
            request: envelope.request,
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(bytes) = store::read_limited(&self.paths.endpoint, 4096)
            && let Ok(endpoint) = serde_json::from_slice::<Endpoint>(&bytes)
            && endpoint.instance == self.endpoint.instance
        {
            let _ = fs::remove_file(&self.paths.endpoint);
        }
    }
}

fn token_matches(left: &str, right: &str) -> bool {
    if left.len() != 64 || right.len() != 64 {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn configure(stream: &Stream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    #[cfg(windows)]
    stream.set_nodelay(true)?;
    Ok(())
}

impl Endpoint {
    pub fn load(paths: &Paths) -> io::Result<Self> {
        let endpoint: Self = serde_json::from_slice(&store::read_limited(&paths.endpoint, 4096)?)
            .map_err(invalid)?;
        if endpoint.protocol != PROTOCOL_VERSION
            || endpoint.token.len() != 64
            || endpoint.instance.len() != 32
        {
            return Err(invalid("Unsupported or malformed SuperLight endpoint"));
        }
        Ok(endpoint)
    }

    fn connect(&self) -> io::Result<Stream> {
        #[cfg(unix)]
        let stream = Stream::connect(&self.name)?;
        #[cfg(windows)]
        let stream = {
            let address: std::net::SocketAddr = self.name.parse().map_err(invalid)?;
            if address.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "The control endpoint must be on IPv4 loopback",
                ));
            }
            Stream::connect_timeout(&address, TIMEOUT)?
        };
        configure(&stream)?;
        Ok(stream)
    }

    pub fn wake(&self) {
        let _ = self.connect();
    }
}

pub fn call(paths: &Paths, request: &Request) -> io::Result<Response> {
    call_endpoint(&Endpoint::load(paths)?, request)
}

pub fn call_endpoint(endpoint: &Endpoint, request: &Request) -> io::Result<Response> {
    let stream = endpoint.connect()?;
    let deadline = Instant::now() + TIMEOUT;
    write_frame(
        &stream,
        &Envelope {
            protocol: PROTOCOL_VERSION,
            token: endpoint.token.clone(),
            request: request.clone(),
        },
        deadline,
    )?;
    let response: Response = read_frame(&stream, deadline)?;
    if response.protocol != PROTOCOL_VERSION {
        return Err(invalid("Unsupported response protocol"));
    }
    Ok(response)
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Local request timed out"))
}

fn ignore_shut_down(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() != io::ErrorKind::InvalidInput => Err(error),
        _ => Ok(()),
    }
}

fn read_exact(mut stream: &Stream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        ignore_shut_down(stream.set_read_timeout(Some(remaining(deadline)?)))?;
        match stream.read(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Local connection closed",
                ));
            }
            Ok(count) => bytes = &mut bytes[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_all(mut stream: &Stream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        ignore_shut_down(stream.set_write_timeout(Some(remaining(deadline)?)))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Local connection stopped accepting data",
                ));
            }
            Ok(count) => bytes = &bytes[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_frame(stream: &Stream, value: &impl Serialize, deadline: Instant) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() > FRAME_LIMIT {
        return Err(invalid("Local request exceeds the size limit"));
    }
    write_all(stream, &(bytes.len() as u32).to_le_bytes(), deadline)?;
    write_all(stream, &bytes, deadline)
}

fn read_frame<T: DeserializeOwned>(stream: &Stream, deadline: Instant) -> io::Result<T> {
    let mut header = [0; 4];
    read_exact(stream, &mut header, deadline)?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > FRAME_LIMIT {
        return Err(invalid("Invalid local request size"));
    }
    let mut bytes = vec![0; len];
    read_exact(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(invalid)
}

fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
