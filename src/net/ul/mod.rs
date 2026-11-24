#![cfg_attr(not(feature = "net"), allow(dead_code))]
#[cfg(feature = "net")]
mod admission;
#[cfg(feature = "net")]
mod aetitle;
#[cfg(feature = "net")]
mod association;
#[cfg(feature = "net")]
mod association_config;
#[cfg(feature = "net")]
mod called_ae;
#[cfg(feature = "net")]
mod common_ext;
mod conn;
mod error;
#[cfg(feature = "net")]
mod linger;
#[cfg(feature = "net")]
mod local_endpoint;
#[cfg(feature = "net")]
mod negotiation;
#[cfg(feature = "net")]
mod observability;
mod outcome;
mod pdu;
#[cfg(feature = "net")]
mod reader;
#[cfg(feature = "net")]
mod rules;
#[cfg(feature = "net")]
mod server;
#[cfg(feature = "net")]
mod socket_buffers;
#[cfg(feature = "tls")]
mod tls_conn;
#[cfg(feature = "tls")]
mod tls_crl;
#[cfg(feature = "tls")]
mod tls_profile;
#[cfg(feature = "tls")]
mod tls_system_roots;
#[cfg(feature = "net")]
mod transport;
#[cfg(feature = "net")]
mod writer;
#[cfg(feature = "net")]
pub use admission::*;
#[cfg(feature = "net")]
pub use aetitle::*;
#[cfg(feature = "net")]
pub use association::*;
#[cfg(feature = "net")]
pub use association_config::*;
#[cfg(feature = "net")]
pub use called_ae::*;
#[cfg(feature = "net")]
pub use common_ext::{CommonExtendedNegotiationHandler, accept_related_general_sop_classes};
#[cfg(feature = "net")]
pub use conn::Conn;
pub use conn::{CancelToken, Ctx, Nanos};
pub use error::{
    Detail, Error, Kind, Result, TlsFailureClass, TlsRevocationError, TlsRevocationKind,
    rejection_description, rejection_text,
};
#[cfg(feature = "net")]
pub use linger::{DEFAULT_ARTIM_TIMEOUT, provider_abort_reason, unexpected_pdu_error};
#[cfg(feature = "net")]
pub use local_endpoint::{
    LocalEndpointKind, SOURCE_PORT_RETRY_DELAYS, SourcePortDialer, classify_local_endpoint_error,
    local_endpoint_unavailable, retry_unavailable_source_port, source_port_retryable,
    valid_remote_address, with_source_port_collision,
};
#[cfg(feature = "net")]
pub use negotiation::{
    AcceptOptions, AcceptedContext, DialOptions, EXTENDED_NEGOTIATION_ACTION_ACCEPT,
    EXTENDED_NEGOTIATION_ACTION_IGNORE, EXTENDED_NEGOTIATION_ACTION_REJECT,
    ExtendedNegotiationAction, ExtendedNegotiationHandler, ExtendedNegotiationResult,
    RejectionCallback, USER_IDENTITY_ACTION_ACCEPT, USER_IDENTITY_ACTION_IGNORE,
    USER_IDENTITY_ACTION_REJECT, UserIdentityAction, UserIdentityHandler, UserIdentityRequest,
    UserIdentityResult,
};
#[cfg(feature = "net")]
pub use observability::{
    ObservabilityOptions, RawPduCapture, RawPduDispatcher, RawPduDispatcherStats, RawPduSink,
};
pub use outcome::PresentationContextOutcome;
pub use pdu::*;
#[cfg(feature = "net")]
pub use reader::{ByteRead, parse_pdu_body, read_pdu, read_pdu_with_association_limit};
#[cfg(feature = "net")]
pub use rules::{PresentationContextRule, RequestorRoles};
#[cfg(feature = "net")]
pub use server::*;
#[cfg(feature = "net")]
pub use socket_buffers::{MAX_SOCKET_BUFFER_BYTES, SocketBuffers, effective_socket_buffers};
#[cfg(feature = "tls")]
#[cfg_attr(not(feature = "dicomweb"), allow(unused_imports))]
pub(crate) use tls_conn::tls_failure_of;
#[cfg(feature = "tls")]
pub use tls_conn::{TlsConnectionState, TlsHandshakeFailure};
#[cfg(feature = "tls")]
pub use tls_crl::{
    MAX_TLS_CRL_BYTES, MAX_TLS_CRL_ENTRIES, MAX_TLS_CRL_LISTS, TlsCrlMode, TlsRevocationOptions,
    TlsRevocationSet, preflight_tls_crl_der, validate_tls_crls,
};
#[cfg(feature = "tls")]
pub use tls_profile::*;
#[cfg(feature = "tls")]
#[cfg_attr(not(feature = "dicomweb"), allow(unused_imports))]
pub(crate) use tls_profile::{
    client_certificate_resolver, https_server_verifier, reset_handshake_state,
};
#[cfg(feature = "tls")]
pub use tls_system_roots::{system_root_certificates, system_root_store};
#[cfg(feature = "net")]
pub use transport::{DialContextFn, HttpConnectOptions, TransportOptions};
#[cfg(feature = "net")]
pub use writer::{encode_pdu, write_pdu};
