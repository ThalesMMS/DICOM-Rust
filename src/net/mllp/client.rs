use super::stream::Conn;
use super::{
    Error, ExchangePhase, IoWriter, MLLP2, Reader, Release, RequestTransportState, Sentinel,
    exchange_error, io_error, release_valid, transport_phase_error, validate_frame_payload,
    write_confirmation, write_frame_count,
};
use crate::hl7::{self, FieldExt, Message, ParseOptions};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
#[derive(Debug, Clone, Default)]
pub struct ClientOptions {
    pub release: Release,
    pub transport_ack_timeout: Duration,
    pub max_frame_bytes: i64,
    pub parse_options: ParseOptions,
    pub response_timeout: Duration,
}
pub struct Client {
    conn: Conn,
    reader: Reader<Conn>,
    options: ClientOptions,
    broken: Option<Error>,
}
impl Client {
    pub fn dial(address: impl ToSocketAddrs, options: ClientOptions) -> Result<Client, Error> {
        if !release_valid(options.release) {
            return Err(Error::Sentinel(Sentinel::UnsupportedRelease));
        }
        let conn = TcpStream::connect(address).map_err(io_error)?;
        Client::new(conn, options)
    }
    pub fn new(conn: TcpStream, options: ClientOptions) -> Result<Client, Error> {
        Client::over(Conn::plain(conn), options)
    }
    #[cfg(feature = "tls")]
    #[doc(hidden)]
    pub fn dial_tls(
        address: impl ToSocketAddrs,
        options: ClientOptions,
        tls: &super::ClientTls,
    ) -> Result<Client, Error> {
        if !release_valid(options.release) {
            return Err(Error::Sentinel(Sentinel::UnsupportedRelease));
        }
        let conn = TcpStream::connect(address).map_err(io_error)?;
        Client::new_tls(conn, options, tls)
    }
    #[cfg(feature = "tls")]
    pub fn new_tls(
        conn: TcpStream,
        options: ClientOptions,
        tls: &super::ClientTls,
    ) -> Result<Client, Error> {
        let raw = conn.try_clone().map_err(io_error)?;
        match super::stream::client_handshake(conn, tls) {
            Ok(conn) => Client::over(conn, options),
            Err(e) => {
                let _ = raw.shutdown(std::net::Shutdown::Both);
                let e = io_error(e);
                Err(Error::Wrapped {
                    text: format!("mllp: TLS handshake: {e}"),
                    wrapped: vec![e],
                })
            }
        }
    }
    fn over(conn: Conn, mut options: ClientOptions) -> Result<Client, Error> {
        if options.max_frame_bytes <= 0 {
            options.max_frame_bytes = super::DEFAULT_MAX_FRAME_BYTES;
        }
        if options.parse_options.max_bytes <= 0
            || options.parse_options.max_bytes > options.max_frame_bytes
        {
            options.parse_options.max_bytes = options.max_frame_bytes;
        }
        if options.response_timeout.is_zero() {
            options.response_timeout = Duration::from_secs(30);
        }
        if options.transport_ack_timeout.is_zero() {
            options.transport_ack_timeout = Duration::from_secs(30);
        }
        let reader = Reader::new(conn.try_clone().map_err(io_error)?, options.max_frame_bytes);
        let broken = (!release_valid(options.release))
            .then_some(Error::Sentinel(Sentinel::UnsupportedRelease));
        Ok(Client {
            conn,
            reader,
            options,
            broken,
        })
    }
    fn mllp2(&self) -> bool {
        self.options.release == MLLP2
    }
    fn local_failure(&self, err: Error) -> Error {
        if self.mllp2() {
            exchange_error(
                ExchangePhase::WriteRequest,
                RequestTransportState::NotSent,
                err,
            )
        } else {
            err
        }
    }
    fn phase_failure(
        &mut self,
        phase: ExchangePhase,
        state: RequestTransportState,
        err: Error,
    ) -> Error {
        let err = if self.mllp2() {
            exchange_error(phase, state, err)
        } else {
            transport_phase_error(phase.as_str(), err)
        };
        self.broken = Some(err.clone());
        let _ = self.conn.shutdown();
        err
    }
    fn set_timeout(&self, d: Duration) -> Result<(), Error> {
        let d = Some(d.max(Duration::from_millis(1)));
        self.conn.set_read_timeout(d).map_err(io_error)?;
        self.conn.set_write_timeout(d).map_err(io_error)
    }
    pub fn send(&mut self, message: &Message) -> (Option<Message>, Option<Error>) {
        if let Some(b) = &self.broken {
            return (None, Some(b.clone()));
        }
        let payload = if self.mllp2() {
            message
                .marshal_with_limit(self.options.max_frame_bytes)
                .map_err(|e| {
                    if e.is(hl7::Sentinel::Limit) {
                        Error::join(Error::Sentinel(Sentinel::FrameTooLarge), Error::Hl7(e))
                    } else {
                        Error::Hl7(e)
                    }
                })
        } else {
            message.marshal().map_err(Error::Hl7)
        };
        let payload = match payload {
            Ok(p) => p,
            Err(e) => return (None, Some(self.local_failure(e))),
        };
        use ExchangePhase as P;
        use RequestTransportState as S;
        if self.mllp2() {
            if payload.len() as i64 > self.options.max_frame_bytes {
                return (
                    None,
                    Some(exchange_error(
                        P::WriteRequest,
                        S::NotSent,
                        Error::Sentinel(Sentinel::FrameTooLarge),
                    )),
                );
            }
            if let Err(e) = validate_frame_payload(&payload) {
                return (None, Some(exchange_error(P::WriteRequest, S::NotSent, e)));
            }
        }
        if let Err(e) = self.set_timeout(self.options.response_timeout) {
            return (
                None,
                Some(self.phase_failure(P::WriteRequest, S::NotSent, e)),
            );
        }
        let mut conn = &self.conn;
        let (written, result) = write_frame_count(&mut IoWriter(&mut conn), &payload);
        if let Err(e) = result {
            let state = if written == 0 && !e.is_invalid_write_count() {
                S::NotSent
            } else {
                S::Unknown
            };
            return (None, Some(self.phase_failure(P::WriteRequest, state, e)));
        }
        let mut state = S::Unknown;
        let confirm = self
            .options
            .transport_ack_timeout
            .min(self.options.response_timeout);
        if self.mllp2() {
            if let Err(e) = self.set_timeout(confirm) {
                return (
                    None,
                    Some(self.phase_failure(P::RequestConfirmation, state, e)),
                );
            }
            if let Err(e) = self.reader.read_confirmation() {
                if e.is(Sentinel::TransportNak) {
                    state = S::Rejected;
                }
                return (
                    None,
                    Some(self.phase_failure(P::RequestConfirmation, state, e)),
                );
            }
            state = S::Acknowledged;
            if let Err(e) = self.set_timeout(self.options.response_timeout) {
                return (None, Some(self.phase_failure(P::ReadResponse, state, e)));
            }
        }
        let response_payload = match self.reader.read_frame() {
            Ok(p) => p,
            Err(e) => return (None, Some(self.phase_failure(P::ReadResponse, state, e))),
        };
        if self.mllp2() {
            if let Err(e) = self.set_timeout(confirm) {
                return (
                    None,
                    Some(self.phase_failure(P::ResponseConfirmation, state, e)),
                );
            }
            let mut conn = &self.conn;
            if let Err(e) = write_confirmation(&mut IoWriter(&mut conn), true) {
                return (
                    None,
                    Some(self.phase_failure(P::ResponseConfirmation, state, e)),
                );
            }
        }
        let response = match hl7::parse_with_options(&response_payload, &self.options.parse_options)
        {
            Ok(r) => r,
            Err(e) => {
                if self.mllp2() {
                    return (
                        None,
                        Some(exchange_error(P::ParseResponse, state, Error::Hl7(e))),
                    );
                }
                let e = Error::Hl7(e);
                return (
                    None,
                    Some(Error::Wrapped {
                        text: format!("mllp: response: {e}"),
                        wrapped: vec![e],
                    }),
                );
            }
        };
        if let Some(msa) = response.segment("MSA")
            && msa.field(2).string() != message.header().control_id
        {
            let err = Error::Sentinel(Sentinel::ResponseMismatch);
            if self.mllp2() {
                return (
                    Some(response),
                    Some(exchange_error(P::CorrelateResponse, state, err)),
                );
            }
            return (Some(response), Some(err));
        }
        (Some(response), None)
    }
    pub fn close(&self) -> Result<(), Error> {
        self.conn.shutdown().map_err(io_error)
    }
}
