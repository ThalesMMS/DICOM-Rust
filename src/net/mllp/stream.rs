use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
#[cfg(feature = "tls")]
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(feature = "tls")]
enum TlsStream {
    Client(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
    Server(rustls::StreamOwned<rustls::ServerConnection, TcpStream>),
}
#[cfg(feature = "tls")]
impl TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let r = match self {
            TlsStream::Client(s) => s.read(buf),
            TlsStream::Server(s) => s.read(buf),
        };
        match r {
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(0),
            r => r,
        }
    }
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = match self {
            TlsStream::Client(s) => s.write(data)?,
            TlsStream::Server(s) => s.write(data)?,
        };
        self.flush()?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            TlsStream::Client(s) => s.flush(),
            TlsStream::Server(s) => s.flush(),
        }
    }
    fn close_notify(&mut self) {
        match self {
            TlsStream::Client(s) => s.conn.send_close_notify(),
            TlsStream::Server(s) => s.conn.send_close_notify(),
        }
        let _ = self.flush();
    }
}
pub(crate) struct Conn {
    tcp: TcpStream,
    #[cfg(feature = "tls")]
    tls: Option<Arc<Mutex<TlsStream>>>,
}
impl Conn {
    pub fn plain(tcp: TcpStream) -> Conn {
        Conn {
            tcp,
            #[cfg(feature = "tls")]
            tls: None,
        }
    }
    pub fn try_clone(&self) -> io::Result<Conn> {
        Ok(Conn {
            tcp: self.tcp.try_clone()?,
            #[cfg(feature = "tls")]
            tls: self.tls.clone(),
        })
    }
    pub fn set_read_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        self.tcp.set_read_timeout(d)
    }
    pub fn set_write_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        self.tcp.set_write_timeout(d)
    }
    pub fn shutdown(&self) -> io::Result<()> {
        #[cfg(feature = "tls")]
        if let Some(tls) = &self.tls
            && let Ok(mut s) = tls.try_lock()
        {
            s.close_notify();
        }
        self.tcp.shutdown(Shutdown::Both)
    }
}
impl Read for &Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(feature = "tls")]
        if let Some(tls) = &self.tls {
            let mut s = tls.lock().unwrap_or_else(|e| e.into_inner());
            return s.read(buf);
        }
        (&self.tcp).read(buf)
    }
}
impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}
impl Write for &Conn {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        #[cfg(feature = "tls")]
        if let Some(tls) = &self.tls {
            let mut s = tls.lock().unwrap_or_else(|e| e.into_inner());
            return s.write(data);
        }
        (&self.tcp).write(data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(feature = "tls")]
#[derive(Clone)]
pub struct ClientTls {
    pub config: Arc<rustls::ClientConfig>,
    pub server_name: String,
}
#[cfg(feature = "tls")]
impl ClientTls {
    pub fn with_system_roots(server_name: &str) -> io::Result<ClientTls> {
        let (roots, _) = crate::net::ul::system_root_store()
            .map_err(|e| io::Error::other(e.text().to_string()))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(ClientTls {
            config: Arc::new(config),
            server_name: server_name.to_string(),
        })
    }
}
#[cfg(feature = "tls")]
fn connection_state(
    common: &rustls::CommonState,
    server_name: Option<&str>,
) -> crate::net::ul::TlsConnectionState {
    crate::net::ul::TlsConnectionState {
        version: common.protocol_version().map_or(0, |v| v.into()),
        cipher_suite: common
            .negotiated_cipher_suite()
            .map_or(0, |s| s.suite().into()),
        handshake_complete: !common.is_handshaking(),
        did_resume: false,
        peer_certificates: common
            .peer_certificates()
            .map(|c| c.iter().map(|c| c.clone().into_owned()).collect())
            .unwrap_or_default(),
        server_name: server_name.unwrap_or_default().to_string(),
    }
}
#[cfg(feature = "tls")]
pub(crate) fn client_handshake(mut tcp: TcpStream, tls: &ClientTls) -> io::Result<Conn> {
    let name = rustls::pki_types::ServerName::try_from(tls.server_name.clone())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut conn = rustls::ClientConnection::new(tls.config.clone(), name)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)?;
    }
    let raw = tcp.try_clone()?;
    Ok(Conn {
        tcp: raw,
        tls: Some(Arc::new(Mutex::new(TlsStream::Client(
            rustls::StreamOwned::new(conn, tcp),
        )))),
    })
}
#[cfg(feature = "tls")]
pub(crate) fn server_handshake(
    mut tcp: TcpStream,
    config: &Arc<rustls::ServerConfig>,
) -> io::Result<(Conn, crate::net::ul::TlsConnectionState)> {
    let mut conn = rustls::ServerConnection::new(config.clone())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut tcp) {
            let _ = conn.write_tls(&mut tcp);
            return Err(e);
        }
    }
    let state = connection_state(&conn, conn.server_name());
    let raw = tcp.try_clone()?;
    Ok((
        Conn {
            tcp: raw,
            tls: Some(Arc::new(Mutex::new(TlsStream::Server(
                rustls::StreamOwned::new(conn, tcp),
            )))),
        },
        state,
    ))
}
#[cfg(all(test, feature = "tls"))]
mod system_roots_tests {
    use super::*;
    #[test]
    fn builds_the_client_configuration_with_the_system_roots() {
        let tls = ClientTls::with_system_roots("hl7.example.org").expect("configuration");
        assert_eq!(tls.server_name, "hl7.example.org");
        assert!(tls.config.alpn_protocols.is_empty());
    }
}
