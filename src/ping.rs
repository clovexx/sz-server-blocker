use crate::firewall::PING_SK_MARK;
use crate::server::Server;
use std::io::ErrorKind;
use std::net::SocketAddr;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpSocket, UdpSocket};
use tokio::sync::{mpsc, Semaphore};
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(2);
const UDP_PROBE: Duration = Duration::from_millis(400);
const CONCURRENCY: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeResult {
    Ok(u64),
    /// UDP-порт принимает пакеты, ответа нет (норма для игрового порта).
    Open,
    Timeout,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingResult {
    pub tcp: ProbeResult,
    pub udp: ProbeResult,
}

pub async fn ping_server(server: &Server) -> PingResult {
    let (tcp, udp) = tokio::join!(ping_tcp(server), ping_udp(server));
    PingResult { tcp, udp }
}

async fn ping_tcp(server: &Server) -> ProbeResult {
    let addr: SocketAddr = match format!("{}:{}", server.ip, server.port).parse() {
        Ok(addr) => addr,
        Err(_) => return ProbeResult::Error,
    };

    let socket = match addr {
        SocketAddr::V4(_) => TcpSocket::new_v4(),
        SocketAddr::V6(_) => TcpSocket::new_v6(),
    };

    let socket = match socket {
        Ok(socket) => socket,
        Err(_) => return ProbeResult::Error,
    };

    let _ = try_mark_socket(socket.as_raw_fd());

    let start = Instant::now();
    match timeout(TIMEOUT, socket.connect(addr)).await {
        Ok(Ok(stream)) => {
            drop(stream);
            ProbeResult::Ok(start.elapsed().as_millis() as u64)
        }
        Ok(Err(_)) => ProbeResult::Error,
        Err(_) => ProbeResult::Timeout,
    }
}

async fn ping_udp(server: &Server) -> ProbeResult {
    let addr: SocketAddr = match format!("{}:{}", server.ip, server.port).parse() {
        Ok(addr) => addr,
        Err(_) => return ProbeResult::Error,
    };

    let bind_addr = match addr {
        SocketAddr::V4(_) => "0.0.0.0:0",
        SocketAddr::V6(_) => "[::]:0",
    };

    let std_socket = match std::net::UdpSocket::bind(bind_addr) {
        Ok(socket) => socket,
        Err(_) => return ProbeResult::Error,
    };

    #[cfg(unix)]
    enable_ip_recverr(std_socket.as_raw_fd());

    let _ = try_mark_socket(std_socket.as_raw_fd());

    if std_socket.connect(addr).is_err() {
        return ProbeResult::Error;
    }

    if std_socket.set_nonblocking(true).is_err() {
        return ProbeResult::Error;
    }

    let socket = match UdpSocket::from_std(std_socket) {
        Ok(socket) => socket,
        Err(_) => return ProbeResult::Error,
    };

    let start = Instant::now();
    let deadline = start + UDP_PROBE;
    let mut sends_ok = 0u8;

    while Instant::now() < deadline {
        match socket.send(&[0]).await {
            Ok(_) => sends_ok += 1,
            Err(err) if err.kind() == ErrorKind::ConnectionRefused => {
                return ProbeResult::Ok(start.elapsed().as_millis() as u64);
            }
            Err(_) => return ProbeResult::Error,
        }

        if let Ok(Some(err)) = socket.take_error() {
            if err.kind() == ErrorKind::ConnectionRefused {
                return ProbeResult::Ok(start.elapsed().as_millis() as u64);
            }
        }

        if sends_ok >= 2 {
            return ProbeResult::Open;
        }

        tokio::time::sleep(Duration::from_millis(40)).await;
    }

    if sends_ok > 0 {
        ProbeResult::Open
    } else {
        ProbeResult::Timeout
    }
}

#[cfg(unix)]
fn enable_ip_recverr(fd: std::os::unix::io::RawFd) {
    let on: libc::c_int = 1;
    unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IP,
            libc::IP_RECVERR,
            &on as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

#[cfg(unix)]
fn try_mark_socket(fd: std::os::unix::io::RawFd) -> std::io::Result<()> {
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_MARK,
            &PING_SK_MARK as *const _ as *const libc::c_void,
            std::mem::size_of::<u32>() as libc::socklen_t,
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn try_mark_socket(_fd: i32) -> std::io::Result<()> {
    Ok(())
}

pub async fn ping_servers(servers: &[Server]) -> Vec<PingResult> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let items: Vec<_> = servers
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, server)| (idx, server))
        .collect();

    ping_servers_progress(items, tx).await;

    let mut results = vec![
        PingResult {
            tcp: ProbeResult::Error,
            udp: ProbeResult::Error,
        };
        servers.len()
    ];
    while let Some((idx, result)) = rx.recv().await {
        if let Some(slot) = results.get_mut(idx) {
            *slot = result;
        }
    }
    results
}

pub async fn ping_servers_progress(
    items: Vec<(usize, Server)>,
    tx: mpsc::UnboundedSender<(usize, PingResult)>,
) {
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
    let mut handles = Vec::with_capacity(items.len());

    for (idx, server) in items {
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };
        let tx = tx.clone();

        handles.push(tokio::spawn(async move {
            let result = ping_server(&server).await;
            let _ = tx.send((idx, result));
            drop(permit);
        }));
    }

    drop(tx);

    for handle in handles {
        let _ = handle.await;
    }
}

/// UDP-пинг: свой RTT, иначе TCP если порт UDP открыт, иначе ошибка.
pub fn format_latency(result: Option<PingResult>) -> String {
    match result {
        None => "—".into(),
        Some(ping) => match ping.udp {
            ProbeResult::Ok(ms) => ms.to_string(),
            ProbeResult::Open => match ping.tcp {
                ProbeResult::Ok(ms) => ms.to_string(),
                _ => "—".into(),
            },
            ProbeResult::Timeout => "тайм".into(),
            ProbeResult::Error => match ping.tcp {
                ProbeResult::Ok(ms) => ms.to_string(),
                ProbeResult::Timeout => "тайм".into(),
                _ => "нет".into(),
            },
        },
    }
}

pub fn format_ping(result: Option<PingResult>) -> String {
    format_latency(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{Server, DEFAULT_PORT};

    #[tokio::test]
    async fn ping_unreachable_returns_timeout_or_error() {
        let server = Server {
            name: "test".into(),
            ip: "192.0.2.1".into(),
            port: DEFAULT_PORT,
            pool: "TEST".into(),
            region: "UNK".into(),
        };

        let result = ping_server(&server).await;
        assert!(!matches!(result.tcp, ProbeResult::Ok(_)));
    }

    #[test]
    fn formats_dual_ping() {
        let ping = PingResult {
            tcp: ProbeResult::Ok(47),
            udp: ProbeResult::Ok(15),
        };
        assert_eq!(format_ping(Some(ping)), "15");
    }

    #[test]
    fn udp_open_falls_back_to_tcp() {
        let ping = PingResult {
            tcp: ProbeResult::Ok(42),
            udp: ProbeResult::Open,
        };
        assert_eq!(format_latency(Some(ping)), "42");
    }

    #[test]
    fn udp_open_without_tcp() {
        let ping = PingResult {
            tcp: ProbeResult::Timeout,
            udp: ProbeResult::Open,
        };
        assert_eq!(format_latency(Some(ping)), "—");
    }

    #[test]
    fn formats_failed_ping() {
        let ping = PingResult {
            tcp: ProbeResult::Timeout,
            udp: ProbeResult::Error,
        };
        assert_eq!(format_ping(Some(ping)), "тайм");
    }
}
