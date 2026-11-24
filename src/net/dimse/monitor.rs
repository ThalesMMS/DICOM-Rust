#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Error, Kind, Result};
use super::message::receive_command_set_any;
use crate::net::ul::{self, Association, CancelToken, Ctx};
use crate::object::Object;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{Scope, ScopedJoinHandle};
use std::time::Duration;
pub struct Interleaved {
    pub pc_id: u8,
    pub command: Option<Object>,
    pub err: Option<Error>,
}
#[derive(Default)]
struct State {
    canceled: bool,
    err: Option<Error>,
}
pub struct ScpCancelMonitor<'scope> {
    cancel_err: Error,
    op_cancel: CancelToken,
    op_cause: CancelCause,
    op_ctx: Ctx,
    read_cancel: CancelToken,
    commands: Mutex<Receiver<Interleaved>>,
    state: Arc<Mutex<State>>,
    handle: Mutex<Option<ScopedJoinHandle<'scope, ()>>>,
}
impl<'scope> ScpCancelMonitor<'scope> {
    pub fn start<'env>(
        scope: &'scope Scope<'scope, 'env>,
        parent: Option<&Ctx>,
        assoc: &'env Association,
        pc_id: u8,
        message_id: u16,
        cancel_err: Error,
        allow_store_rsp: bool,
    ) -> ScpCancelMonitor<'scope> {
        let parent = parent.cloned().unwrap_or_default();
        let op_cancel = CancelToken::new();
        let op_cause = CancelCause::default();
        let op_ctx = parent.with_cancel(&op_cancel).with_value(op_cause.clone());
        let read_cancel = CancelToken::new();
        let read_ctx = parent.with_cancel(&read_cancel).without_idle_timeout();
        let (tx, rx) = mpsc::sync_channel::<Interleaved>(1);
        let state = Arc::new(Mutex::new(State::default()));
        let worker = Worker {
            target_pc: pc_id,
            target_msg: message_id,
            cancel_err: cancel_err.clone(),
            allow_store_rsp,
            op_cancel: op_cancel.clone(),
            op_cause: op_cause.clone(),
            state: state.clone(),
            tx,
        };
        let handle = scope.spawn(move || worker.run(&read_ctx, assoc));
        ScpCancelMonitor {
            cancel_err,
            op_cancel,
            op_cause,
            op_ctx,
            read_cancel,
            commands: Mutex::new(rx),
            state,
            handle: Mutex::new(Some(handle)),
        }
    }
    pub fn context(&self) -> &Ctx {
        &self.op_ctx
    }
    pub fn operation_error(&self) -> Option<Error> {
        let (canceled, err) = {
            let s = self.state.lock().unwrap();
            (s.canceled, s.err.clone())
        };
        if let Some(err) = err {
            if canceled {
                return Error::join([Some(self.cancel_err.clone()), Some(err)]);
            }
            return Some(err);
        }
        if canceled {
            return Some(self.cancel_err.clone());
        }
        context_cause(&self.op_ctx)
    }
    pub fn stop(&self) -> Option<Error> {
        self.read_cancel.cancel();
        if let Some(h) = self.handle.lock().unwrap().take() {
            let _ = h.join();
        }
        let err = self.operation_error();
        self.op_cause
            .set(Error::from(ul::Error::sentinel(ul::Kind::Canceled)));
        self.op_cancel.cancel();
        err
    }
    pub fn receive_c_store_response(&self, ctx: &Ctx, expected_pc: u8) -> Result<CStoreResponse> {
        let rx = self.commands.lock().unwrap();
        loop {
            match rx.recv_timeout(Duration::from_millis(5)) {
                Ok(event) => {
                    if let Some(err) = event.err {
                        return Err(err);
                    }
                    if event.pc_id != expected_pc {
                        return Err(Error::wrap(
                            Kind::PresentationContextMismatch,
                            format!("got {}, want {expected_pc}", event.pc_id),
                        ));
                    }
                    return parse_c_store_response(event.command.as_ref().expect("command"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    if let Some(err) = self.operation_error() {
                        return Err(err);
                    }
                    return Err(Error::new(
                        "dicom dimse: C-GET command monitor stopped before C-STORE response",
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(e) = context_err(ctx) {
                        return Err(e);
                    }
                }
            }
        }
    }
}
struct Worker {
    target_pc: u8,
    target_msg: u16,
    cancel_err: Error,
    allow_store_rsp: bool,
    op_cancel: CancelToken,
    op_cause: CancelCause,
    state: Arc<Mutex<State>>,
    tx: SyncSender<Interleaved>,
}
impl Worker {
    fn fail(&self, err: Error) {
        {
            let mut s = self.state.lock().unwrap();
            if s.err.is_none() {
                s.err = Some(err.clone());
            }
        }
        self.op_cause.set(err);
        self.op_cancel.cancel();
    }
    fn send(&self, ctx: &Ctx, event: Interleaved) -> bool {
        let mut event = event;
        loop {
            match self.tx.try_send(event) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
                Err(mpsc::TrySendError::Full(e)) => {
                    if ctx.err().is_some() {
                        return false;
                    }
                    event = e;
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
    fn run(self, ctx: &Ctx, assoc: &Association) {
        loop {
            let (pc_id, command) = match receive_command_set_any(Some(ctx), assoc) {
                Ok(v) => v,
                Err(err) => {
                    if ctx.err().is_some() {
                        return;
                    }
                    if err.is_ul(ul::Kind::AssociationTimeout) || err.is_deadline() {
                        self.op_cause
                            .set(Error::from(ul::Error::sentinel(ul::Kind::DeadlineExceeded)));
                        self.op_cancel.cancel();
                        return;
                    }
                    self.fail(err);
                    return;
                }
            };
            let field = match command_uint16(Some(&command), COMMAND_FIELD) {
                Ok(f) => f,
                Err(e) => {
                    self.fail(e);
                    return;
                }
            };
            match field {
                C_CANCEL_RQ => {
                    let cancel = match parse_c_cancel_request(&command) {
                        Ok(c) => c,
                        Err(e) => {
                            self.fail(e);
                            return;
                        }
                    };
                    if pc_id != self.target_pc
                        || cancel.message_id_being_responded_to != self.target_msg
                    {
                        continue;
                    }
                    self.state.lock().unwrap().canceled = true;
                    self.op_cause.set(self.cancel_err.clone());
                    self.op_cancel.cancel();
                    if self.allow_store_rsp
                        && !self.send(
                            ctx,
                            Interleaved {
                                pc_id: 0,
                                command: None,
                                err: Some(self.cancel_err.clone()),
                            },
                        )
                    {
                        return;
                    }
                }
                C_STORE_RSP => {
                    if !self.allow_store_rsp {
                        self.fail(Error::new(
                            "dicom dimse: unexpected C-STORE-RSP during active operation",
                        ));
                        return;
                    }
                    if !self.send(
                        ctx,
                        Interleaved {
                            pc_id,
                            command: Some(command),
                            err: None,
                        },
                    ) {
                        return;
                    }
                }
                _ => {
                    self.fail(Error::new(format!(
                        "dicom dimse: unexpected command field 0x{field:04X} during active operation"
                    )));
                    return;
                }
            }
        }
    }
}
