use super::websocket::Conn;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
const CLOSE_NOTIFY_TIMEOUT: Duration = Duration::from_secs(1);
struct Session {
    conn: Mutex<rustls::Connection>,
    sock: TcpStream,
}
impl Session {
    fn lock(&self) -> io::Result<MutexGuard<'_, rustls::Connection>> {
        self.conn
            .lock()
            .map_err(|_| io::Error::other("tls: session lock poisoned"))
    }
}
fn flush(conn: &mut rustls::Connection, mut sock: &TcpStream) -> io::Result<()> {
    while conn.wants_write() {
        conn.write_tls(&mut sock)?;
    }
    Ok(())
}
struct ReadHalf {
    session: Arc<Session>,
    sock: TcpStream,
    inbox: Vec<u8>,
    eof: bool,
}
impl Read for ReadHalf {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            {
                let mut conn = self.session.lock()?;
                match conn.reader().read(buf) {
                    Ok(n) => return Ok(n),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "unexpected EOF",
                        ));
                    }
                    Err(e) => return Err(e),
                }
                if !self.inbox.is_empty() || self.eof {
                    let mut pending = &self.inbox[..];
                    let taken = conn.read_tls(&mut pending)?;
                    self.inbox.drain(..taken);
                    if let Err(e) = conn.process_new_packets() {
                        let _ = flush(&mut conn, &self.session.sock);
                        return Err(io::Error::new(io::ErrorKind::InvalidData, e));
                    }
                    let _ = flush(&mut conn, &self.session.sock);
                    continue;
                }
            }
            let mut chunk = [0u8; 8192];
            let mut sock = &self.sock;
            match sock.read(&mut chunk)? {
                0 => self.eof = true,
                n => self.inbox.extend_from_slice(&chunk[..n]),
            }
        }
    }
}
struct WriteHalf {
    session: Arc<Session>,
}
impl Write for WriteHalf {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut conn = self.session.lock()?;
        let mut sent = 0;
        while sent < data.len() {
            sent += conn.writer().write(&data[sent..])?;
            flush(&mut conn, &self.session.sock)?;
        }
        Ok(sent)
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut conn = self.session.lock()?;
        flush(&mut conn, &self.session.sock)
    }
}
impl Conn {
    pub fn new_tls(
        stream: TcpStream,
        session: rustls::Connection,
        buffered: Vec<u8>,
        client: bool,
        max_message_bytes: i64,
    ) -> io::Result<Arc<Conn>> {
        let shared = Arc::new(Session {
            conn: Mutex::new(session),
            sock: stream.try_clone()?,
        });
        let read_half = ReadHalf {
            session: shared.clone(),
            sock: stream.try_clone()?,
            inbox: Vec::new(),
            eof: false,
        };
        let write_half = WriteHalf {
            session: shared.clone(),
        };
        let closing = shared;
        let on_close = move || {
            if let Ok(mut conn) = closing.conn.try_lock() {
                let _ = closing.sock.set_write_timeout(Some(CLOSE_NOTIFY_TIMEOUT));
                conn.send_close_notify();
                let _ = flush(&mut conn, &closing.sock);
            }
        };
        Conn::with_transport(
            stream,
            Box::new(read_half),
            Box::new(write_half),
            Some(Box::new(on_close)),
            buffered,
            client,
            max_message_bytes,
        )
    }
}
