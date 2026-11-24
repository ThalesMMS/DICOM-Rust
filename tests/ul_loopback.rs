use dicom_rust::net::ul::{self, Pdu};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
const VERIFY: &str = "1.2.840.10008.1.1";
const CT: &str = "1.2.840.10008.5.1.4.1.1.2";
const MR: &str = "1.2.840.10008.5.1.4.1.1.4";
const IVR: &str = "1.2.840.10008.1.2";
const EVR: &str = "1.2.840.10008.1.2.1";
fn dial_options() -> ul::DialOptions {
    ul::DialOptions {
        called_ae_title: "SCP".into(),
        calling_ae_title: "SCU".into(),
        contexts: vec![
            ul::PresentationContext::new(VERIFY, &[IVR]),
            ul::PresentationContext::new(CT, &[EVR, IVR]),
        ],
        artim_timeout: ul::Nanos::from_secs(2),
        ..Default::default()
    }
}
fn accept_options() -> ul::AcceptOptions {
    ul::AcceptOptions {
        ae_title: "SCP".into(),
        supported_abstract_syntaxes: vec![VERIFY.into(), CT.into(), MR.into()],
        supported_transfer_syntaxes: vec![EVR.into(), IVR.into()],
        artim_timeout: ul::Nanos::from_secs(2),
        ..Default::default()
    }
}
fn pair<A: Send + 'static, B>(
    dial: ul::DialOptions,
    accept: ul::AcceptOptions,
    scp: impl FnOnce(ul::Result<ul::Association>) -> A + Send + 'static,
    scu: impl FnOnce(ul::Result<ul::Association>) -> B,
) -> (B, A) {
    let listener = ul::listen(ul::ListenOptions::default()).expect("listen");
    let addr = listener.local_addr().expect("addr").to_string();
    let server = thread::spawn(move || scp(listener.accept_association(accept)));
    let scu_out = scu(ul::dial(&addr, dial));
    (scu_out, server.join().expect("scp thread"))
}
fn pdata(pc: u8, command: bool, last: bool, data: Vec<u8>) -> Pdu {
    Pdu::PData(ul::PDataTf {
        values: vec![ul::PDataValue {
            presentation_context_id: pc,
            is_command: command,
            is_last: last,
            data,
        }],
    })
}
#[test]
fn exchange_and_release() {
    let (scu, scp) = pair(
        dial_options(),
        accept_options(),
        |a| {
            let a = a.expect("accept");
            assert_eq!(a.send_pdu_limit(), 16378);
            let got = a.receive(None).expect("recv");
            assert_eq!(got, pdata(1, true, true, (0..10).collect()));
            a.send(None, &pdata(3, false, true, (0..5).collect()))
                .unwrap();
            assert_eq!(a.receive(None).unwrap(), Pdu::ReleaseRq);
            a.send(None, &Pdu::ReleaseRp).unwrap();
            a.telemetry_snapshot()
        },
        |a| {
            let a = a.expect("dial");
            a.send(None, &pdata(1, true, true, (0..10).collect()))
                .unwrap();
            assert_eq!(
                a.receive(None).unwrap(),
                pdata(3, false, true, (0..5).collect())
            );
            a.release(None).expect("release");
            a.telemetry_snapshot()
        },
    );
    assert_eq!(
        (scu.established, scu.released, scu.aborted, scu.closed),
        (1, 1, 0, 1)
    );
    assert_eq!(
        (scp.established, scp.released, scp.aborted, scp.closed),
        (1, 1, 0, 0)
    );
}
#[test]
fn release_collision() {
    let (scu, scp) = pair(
        dial_options(),
        accept_options(),
        |a| {
            let a = a.unwrap();
            a.release(None).map(|_| a.telemetry_snapshot())
        },
        |a| {
            let a = a.unwrap();
            a.release(None).map(|_| a.telemetry_snapshot())
        },
    );
    for m in [scu.expect("scu release"), scp.expect("scp release")] {
        assert_eq!((m.released, m.aborted, m.closed), (1, 0, 1));
    }
}
#[test]
fn unexpected_pdu_while_releasing_aborts() {
    let (scu, scp) = pair(
        dial_options(),
        accept_options(),
        |a| {
            let a = a.unwrap();
            assert_eq!(a.receive(None).unwrap(), Pdu::ReleaseRq);
            a.send(
                None,
                &Pdu::Unknown(ul::UnknownPdu {
                    pdu_type: 9,
                    data: vec![1, 2],
                }),
            )
            .unwrap();
            a.receive(None).unwrap_err()
        },
        |a| a.unwrap().release(None).unwrap_err(),
    );
    assert_eq!(
        scu.to_string(),
        "dicom ul: unexpected PDU: got unknown PDU while waiting for A-RELEASE-RP"
    );
    assert_eq!(ul::provider_abort_reason(&scu), Some(2));
    assert_eq!(
        scp.to_string(),
        "dicom ul: association aborted: source=0 reason=2"
    );
}
#[test]
fn abort_reaches_the_peer() {
    let (scu, scp) = pair(
        dial_options(),
        accept_options(),
        |a| {
            let a = a.unwrap();
            let err = a.receive(None).unwrap_err();
            (err, a.telemetry_snapshot())
        },
        |a| {
            let a = a.unwrap();
            a.abort(0).unwrap();
            a.telemetry_snapshot()
        },
    );
    assert_eq!(
        scp.0.to_string(),
        "dicom ul: association aborted: source=0 reason=0"
    );
    assert!(scp.0.is(ul::Kind::AssociationAborted));
    assert_eq!((scu.aborted, scu.closed), (1, 1));
    assert_eq!((scp.1.aborted, scp.1.closed), (1, 1));
}
#[test]
fn rejection_reasons() {
    let cases = [
        (
            (1, 1, 1),
            "rejected-permanent, service-user, no-reason-given (result=1 source=1 reason=1)",
        ),
        (
            (2, 1, 3),
            "rejected-transient, service-user, calling-AE-title-not-recognized (result=2 source=1 reason=3)",
        ),
        (
            (1, 1, 7),
            "rejected-permanent, service-user, called-AE-title-not-recognized (result=1 source=1 reason=7)",
        ),
        (
            (1, 2, 2),
            "rejected-permanent, service-provider (ACSE), protocol-version-not-supported (result=1 source=2 reason=2)",
        ),
        (
            (2, 3, 1),
            "rejected-transient, service-provider (presentation), temporary-congestion (result=2 source=3 reason=1)",
        ),
        (
            (2, 3, 2),
            "rejected-transient, service-provider (presentation), local-limit-exceeded (result=2 source=3 reason=2)",
        ),
    ];
    for ((result, source, reason), text) in cases {
        let mut accept = accept_options();
        accept.admission = Some(Arc::new(move |_: &ul::Ctx, _: &ul::AdmissionRequest| {
            Some(ul::AssociationRj {
                result,
                source,
                reason,
            })
        }));
        let (scu, scp) = pair(
            dial_options(),
            accept,
            |a| a.unwrap_err(),
            |a| a.unwrap_err(),
        );
        let want = format!("dicom ul: association rejected: {text}");
        assert_eq!(scu.to_string(), want);
        assert_eq!(scp.to_string(), want);
    }
}
#[test]
fn no_accepted_contexts() {
    let mut accept = accept_options();
    accept.supported_abstract_syntaxes = vec![MR.into()];
    accept.supported_transfer_syntaxes.clear();
    let (scu, scp) = pair(
        dial_options(),
        accept,
        |a| a.unwrap_err(),
        |a| a.unwrap_err(),
    );
    assert_eq!(
        scu.to_string(),
        "dicom ul: association rejected: rejected-permanent, service-user, no-reason-given (result=1 source=1 reason=1)"
    );
    assert_eq!(
        scp.to_string(),
        "dicom ul: no accepted presentation contexts: presentation context 1 abstract-syntax=1.2.840.10008.1.1 proposed-transfer-syntaxes=[1.2.840.10008.1.2] result=abstract-syntax-not-supported; presentation context 3 abstract-syntax=1.2.840.10008.5.1.4.1.1.2 proposed-transfer-syntaxes=[1.2.840.10008.1.2.1,1.2.840.10008.1.2] result=abstract-syntax-not-supported"
    );
}
fn send_fragmented(a: &ul::Association, size: usize) -> usize {
    let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8 ^ 7).collect();
    let room = (a.send_pdu_limit() - ul::PDV_HEADER_SIZE) as usize;
    let mut pdus = 0;
    for (i, chunk) in data.chunks(room).enumerate() {
        let last = (i + 1) * room >= data.len();
        a.send(None, &pdata(1, false, last, chunk.to_vec()))
            .unwrap();
        pdus += 1;
    }
    pdus
}
fn receive_all(a: &ul::Association) -> (usize, Vec<u8>) {
    let (mut pdus, mut data) = (0, Vec::new());
    loop {
        let Pdu::PData(p) = a.receive(None).unwrap() else {
            panic!("expected P-DATA");
        };
        pdus += 1;
        let last = p.values.iter().any(|v| v.is_last);
        p.values.into_iter().for_each(|v| data.extend(v.data));
        if last {
            return (pdus, data);
        }
    }
}
#[test]
fn fragmentation_by_peer_maximum_length() {
    for (peer_max, want_pdus) in [(1024, 69), (4096, 18), (16384, 5)] {
        let mut accept = accept_options();
        accept.max_pdu = peer_max;
        let (scu, scp) = pair(
            dial_options(),
            accept,
            |a| {
                let a = a.unwrap();
                let got = receive_all(&a);
                a.send(None, &Pdu::ReleaseRp).unwrap();
                got
            },
            |a| {
                let a = a.unwrap();
                assert_eq!(a.send_pdu_limit(), peer_max);
                let pdus = send_fragmented(&a, 70000);
                assert_eq!(a.receive(None).unwrap(), Pdu::ReleaseRp);
                pdus
            },
        );
        assert_eq!(scu, want_pdus);
        assert_eq!(scp.0, want_pdus);
        assert_eq!(
            scp.1,
            (0..70000).map(|i| (i % 251) as u8 ^ 7).collect::<Vec<u8>>()
        );
    }
}
#[test]
fn oversized_pdata_is_refused() {
    let mut accept = accept_options();
    accept.max_pdu = 4096;
    let (_, scp) = pair(
        dial_options(),
        accept,
        |a| a.unwrap().receive(None).unwrap_err(),
        |a| {
            let a = a.unwrap();
            a.send(None, &pdata(1, true, true, vec![0; 20000])).unwrap();
            let _ = a.receive(None);
        },
    );
    assert_eq!(
        scp.to_string(),
        "dicom ul: PDU too large: length 20006 exceeds max 4096"
    );
    assert_eq!(ul::provider_abort_reason(&scp), Some(6));
}
#[test]
fn idle_timeout() {
    let mut accept = accept_options();
    accept.idle_timeout = ul::Nanos::from_millis(100);
    let (_, scp) = pair(
        dial_options(),
        accept,
        |a| a.unwrap().receive(None).unwrap_err(),
        |a| {
            let a = a.unwrap();
            thread::sleep(Duration::from_millis(400));
            let _ = a.send(None, &pdata(1, true, true, vec![0]));
        },
    );
    assert_eq!(
        scp.to_string(),
        "dicom ul: association timeout: context deadline exceeded"
    );
    assert!(scp.is(ul::Kind::AssociationTimeout));
}
#[test]
fn release_timeout_aborts() {
    let mut dial = dial_options();
    dial.release_timeout = ul::Nanos::from_millis(100);
    let (scu, scp) = pair(
        dial,
        accept_options(),
        |a| {
            let a = a.unwrap();
            assert_eq!(a.receive(None).unwrap(), Pdu::ReleaseRq);
            thread::sleep(Duration::from_millis(500));
            a.receive(None).unwrap_err()
        },
        |a| {
            let a = a.unwrap();
            let err = a.release(None).unwrap_err();
            (err, a.telemetry_snapshot())
        },
    );
    assert_eq!(
        scu.0.to_string(),
        "dicom ul: association timeout: context deadline exceeded"
    );
    assert_eq!((scu.1.released, scu.1.aborted, scu.1.closed), (0, 1, 1));
    assert_eq!(
        scp.to_string(),
        "dicom ul: association aborted: source=0 reason=0"
    );
}
#[test]
fn peer_closing_early() {
    let (_, scp) = pair(
        dial_options(),
        accept_options(),
        |a| {
            let a = a.unwrap();
            (a.receive(None).unwrap_err(), a.receive(None).unwrap_err())
        },
        |a| a.unwrap().close(),
    );
    assert_eq!(scp.0.to_string(), "EOF");
    assert_eq!(scp.1.to_string(), "EOF");
    assert!(scp.0.is(ul::Kind::Eof));
}
