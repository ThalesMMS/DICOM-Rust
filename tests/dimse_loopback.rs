use dicom_rust::net::dimse::{self as d, Error, Kind};
use dicom_rust::net::ul::{self, Ctx};
use dicom_rust::object::{Element, ElementHeader, Object, Tag, Value, Vr};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
const VERIFY: &str = "1.2.840.10008.1.1";
const CT: &str = "1.2.840.10008.5.1.4.1.1.2";
const SC: &str = "1.2.840.10008.5.1.4.1.1.7";
const IVR: &str = "1.2.840.10008.1.2";
const EVR: &str = "1.2.840.10008.1.2.1";
fn dial(extra: &[(&str, &[&str])], roles: &[&str]) -> ul::DialOptions {
    let mut contexts = vec![ul::PresentationContext::new(VERIFY, &[IVR])];
    for (a, ts) in extra {
        contexts.push(ul::PresentationContext::new(a, ts));
    }
    ul::DialOptions {
        called_ae_title: "SCP".into(),
        calling_ae_title: "SCU".into(),
        contexts,
        role_selections: roles
            .iter()
            .map(|s| ul::RoleSelectionItem {
                sop_class_uid: s.to_string(),
                scu_role: false,
                scp_role: true,
            })
            .collect(),
        ..Default::default()
    }
}
fn accept(abstracts: &[&str], roles: &[&str]) -> ul::AcceptOptions {
    ul::AcceptOptions {
        ae_title: "SCP".into(),
        supported_abstract_syntaxes: abstracts.iter().map(|s| s.to_string()).collect(),
        supported_transfer_syntaxes: vec![IVR.into(), EVR.into()],
        role_selections: roles
            .iter()
            .map(|s| ul::RoleSelectionItem {
                sop_class_uid: s.to_string(),
                scu_role: false,
                scp_role: true,
            })
            .collect(),
        ..Default::default()
    }
}
fn pair<B>(
    dial: ul::DialOptions,
    accept: ul::AcceptOptions,
    opts: d::AssociationScpOptions,
    scu: impl FnOnce(&ul::Association) -> B,
) -> (B, Result<(), String>) {
    let listener = ul::listen(ul::ListenOptions::default()).expect("listen");
    let addr = listener.local_addr().expect("addr").to_string();
    let server = thread::spawn(move || {
        let assoc = listener.accept_association(accept).expect("accept");
        let r = d::serve_association(None, &assoc, &opts).map_err(|e| e.text().to_string());
        assoc.close();
        r
    });
    let assoc = ul::dial(&addr, dial).expect("dial");
    let out = scu(&assoc);
    assoc.close();
    (out, server.join().expect("scp thread"))
}
fn text(tag: Tag, vr: Vr, v: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![v.to_string()]),
    }
}
fn instance(class: &str, inst: &str, pixels: usize) -> Object {
    let mut e = vec![
        text(Tag::new(0x0008, 0x0016), Vr::UI, class),
        text(Tag::new(0x0008, 0x0018), Vr::UI, inst),
        text(Tag::new(0x0010, 0x0010), Vr::PN, "TEST^NAME"),
    ];
    if pixels > 0 {
        let raw: Vec<u8> = (0..pixels).map(|i| (i % 251) as u8).collect();
        e.push(Element {
            header: ElementHeader::new(Tag::PIXEL_DATA, Vr::OW, raw.len() as u32),
            value: Value::Raw(raw),
        });
    }
    Object::from_elements(e)
}
#[test]
fn command_set_round_trip() {
    let data = d::encode_command_set(&d::CEchoRequest { message_id: 7 }.command_set()).unwrap();
    let decoded = d::decode_command_set(&data).unwrap();
    assert_eq!(
        d::command_uint16(Some(&decoded), d::COMMAND_FIELD).unwrap(),
        d::C_ECHO_RQ
    );
    assert_eq!(d::command_uint16(Some(&decoded), d::MESSAGE_ID).unwrap(), 7);
    let rsp = d::parse_c_echo_response(&decoded).unwrap_err();
    assert!(rsp.is(Kind::ProtocolViolation));
    assert_eq!(
        rsp.text(),
        "dicom dimse: command field 0x0030, want C-ECHO-RSP 0x8030"
    );
}
#[test]
fn echo_and_release() {
    let (scu, scp) = pair(
        dial(&[], &[]),
        accept(&[VERIFY], &[]),
        Default::default(),
        |a| {
            let pc = d::accepted_verification_context(a).unwrap();
            let statuses: Vec<u16> = [1u16, 2, 65535]
                .iter()
                .map(|m| d::send_c_echo(None, a, pc.id, *m).unwrap().status)
                .collect();
            a.release(None).unwrap();
            statuses
        },
    );
    assert_eq!(scu, vec![0, 0, 0]);
    assert_eq!(scp, Ok(()));
}
#[test]
fn store_statuses_and_large_data_sets() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let opts = d::AssociationScpOptions {
        storage: d::StorageScpOptions {
            store_handler: Some(Arc::new(move |_: &Ctx, req: &d::CStoreRequestContext| {
                let ds = req.data_set.as_ref().unwrap();
                let pixels = ds
                    .get(Tag::PIXEL_DATA)
                    .and_then(|e| e.raw())
                    .map_or(0, |r| r.len());
                s2.lock()
                    .unwrap()
                    .push((req.request.affected_sop_instance_uid.clone(), pixels));
                match pixels {
                    100 => (0xB000, None),
                    200 => (
                        0,
                        Some(d::new_scp_status_error(
                            "C-STORE",
                            0xA701,
                            "over quota",
                            Some(Error::new("quota")),
                        )),
                    ),
                    _ => (0, None),
                }
            })),
            ..Default::default()
        },
        ..Default::default()
    };
    let (scu, scp) = pair(
        dial(&[(CT, &[IVR, EVR]), (SC, &[EVR])], &[]),
        accept(&[VERIFY, CT, SC], &[]),
        opts,
        |a| {
            let mut client = d::StoreClient::new(a);
            let mut out = Vec::new();
            for (class, inst, n) in [
                (CT, "1.2.3.1", 40000),
                (SC, "1.2.3.2", 100),
                (CT, "1.2.3.3", 200),
            ] {
                let r =
                    client.store_with_options(None, &instance(class, inst, n), Default::default());
                out.push(match r {
                    Ok(r) => format!(
                        "ok 0x{:04X} pc={}",
                        r.response.unwrap().status,
                        r.presentation_context.id
                    ),
                    Err((_, e)) => e.text().to_string(),
                });
            }
            a.release(None).unwrap();
            out
        },
    );
    assert_eq!(
        scu,
        vec![
            "ok 0x0000 pc=3".to_string(),
            "ok 0xB000 pc=5".to_string(),
            "dicom dimse: C-STORE failed with status 0xA701: over quota".to_string(),
        ]
    );
    assert_eq!(scp, Ok(()));
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            ("1.2.3.1".to_string(), 40000),
            ("1.2.3.2".to_string(), 100),
            ("1.2.3.3".to_string(), 200)
        ]
    );
}
#[test]
fn find_matches_and_cancel() {
    let opts = d::AssociationScpOptions {
        c_find_handler: Some(Arc::new(|ctx: &Ctx, req: &d::CFindRequestContext| {
            if req.request.message_id == 2 {
                while ctx.err().is_none() {
                    thread::sleep(Duration::from_millis(2));
                }
                return Err(d::context_err(ctx).unwrap());
            }
            Ok((0..3)
                .map(|i| d::CFindMatch {
                    identifier: Some(Object::from_elements(vec![
                        text(Tag::new(0x0008, 0x0052), Vr::CS, "STUDY"),
                        text(Tag::new(0x0020, 0x000D), Vr::UI, &format!("1.2.3.{i}")),
                    ])),
                    status: if i == 2 { d::STATUS_PENDING_WARNING } else { 0 },
                    before_send: None,
                })
                .collect())
        })),
        ..Default::default()
    };
    let (scu, scp) = pair(
        dial(&[(d::STUDY_ROOT_FIND_SOP_CLASS_UID, &[IVR])], &[]),
        accept(&[VERIFY, d::STUDY_ROOT_FIND_SOP_CLASS_UID], &[]),
        opts,
        |a| {
            let client = d::FindClient::for_sop_class(a, d::STUDY_ROOT_FIND_SOP_CLASS_UID).unwrap();
            let id = Object::from_elements(vec![text(Tag::new(0x0008, 0x0052), Vr::CS, "STUDY")]);
            let mut statuses = Vec::new();
            client
                .find_with_options(
                    Default::default(),
                    d::CFindRequest {
                        message_id: 1,
                        ..Default::default()
                    },
                    Some(&id),
                    &mut |r| statuses.push(r.status()),
                )
                .unwrap();
            let pc = client.presentation_ctx_id;
            let mut canceled = Vec::new();
            let err = thread::scope(|s| {
                s.spawn(|| {
                    thread::sleep(Duration::from_millis(150));
                    let cancel = d::CCancelRequest {
                        message_id_being_responded_to: 2,
                    };
                    d::send_command_set(None, a, pc, &cancel.command_set()).unwrap();
                });
                client
                    .find_with_options(
                        Default::default(),
                        d::CFindRequest {
                            message_id: 2,
                            ..Default::default()
                        },
                        Some(&id),
                        &mut |r| canceled.push((r.status(), r.response.error_comment.clone())),
                    )
                    .unwrap_err()
            });
            let echo = d::send_c_echo(None, a, 1, 3).unwrap().status;
            a.release(None).unwrap();
            (statuses, canceled, err.text().to_string(), echo)
        },
    );
    assert_eq!(scu.0, vec![0xFF00, 0xFF00, 0xFF01, 0x0000]);
    assert_eq!(scu.1, vec![(0xFE00, "C-FIND canceled".to_string())]);
    assert_eq!(
        scu.2,
        "dicom dimse: C-FIND failed with status 0xFE00: C-FIND canceled"
    );
    assert_eq!(scu.3, 0);
    assert_eq!(scp, Ok(()));
}
#[test]
fn move_sub_operation_counters() {
    let opts = d::AssociationScpOptions {
        c_move_handler: Some(Arc::new(|_: &Ctx, _: &d::CMoveRequestContext| {
            let op = |inst: &str, status: u16, err: Option<&str>| {
                let err = err.map(|e| e.to_string());
                d::CMoveSubOperation {
                    affected_sop_class_uid: CT.into(),
                    affected_sop_instance_uid: inst.into(),
                    store: Some(Arc::new(move |_: &Ctx| d::CMoveSubOperationResult {
                        status,
                        err: err.clone().map(Error::new),
                    })),
                }
            };
            Ok(vec![
                op("1.2.3.1", 0, None),
                op("1.2.3.2", 0xB000, None),
                op("1.2.3.3", 0xA700, None),
                op("1.2.3.4", 0, Some("dest down")),
            ])
        })),
        ..Default::default()
    };
    let (scu, scp) = pair(
        dial(&[(d::STUDY_ROOT_MOVE_SOP_CLASS_UID, &[IVR])], &[]),
        accept(&[VERIFY, d::STUDY_ROOT_MOVE_SOP_CLASS_UID], &[]),
        opts,
        |a| {
            let req = d::CMoveRequest {
                affected_sop_class_uid: d::STUDY_ROOT_MOVE_SOP_CLASS_UID.into(),
                message_id: 22,
                move_destination: "DEST".into(),
                ..Default::default()
            };
            let id = Object::from_elements(vec![text(Tag::new(0x0008, 0x0052), Vr::CS, "STUDY")]);
            let mut seen = Vec::new();
            let last = d::send_c_move_with_options(
                Default::default(),
                a,
                3,
                &req,
                Some(&id),
                &dicom_rust::object::transfer::IMPLICIT_VR_LITTLE_ENDIAN,
                &mut |p| seen.push((p.response.status, p.response.counts.remaining)),
            )
            .unwrap();
            a.release(None).unwrap();
            (seen, last)
        },
    );
    let (seen, last) = scu;
    assert_eq!(
        seen,
        vec![
            (0xFF00, Some(3)),
            (0xFF00, Some(2)),
            (0xFF00, Some(1)),
            (0xB000, None)
        ]
    );
    assert_eq!(last.counts.completed, Some(1));
    assert_eq!(last.counts.failed, Some(2));
    assert_eq!(last.counts.warning, Some(1));
    assert_eq!(
        last.failed_sop_instance_uid_list,
        Some(vec!["1.2.3.3".to_string(), "1.2.3.4".to_string()])
    );
    assert_eq!(scp, Ok(()));
}
#[test]
fn get_stores_on_the_same_association() {
    let opts = d::AssociationScpOptions {
        c_get_handler: Some(Arc::new(|_: &Ctx, _: &d::CGetRequestContext| {
            Ok(["1.2.3.1", "1.2.3.2"]
                .iter()
                .map(|inst| {
                    let inst = inst.to_string();
                    let i2 = inst.clone();
                    d::CGetSubOperation {
                        affected_sop_class_uid: CT.into(),
                        affected_sop_instance_uid: inst,
                        load_data_set: Some(Arc::new(move |_: &Ctx| Ok(instance(CT, &i2, 5000)))),
                        ..Default::default()
                    }
                })
                .collect())
        })),
        ..Default::default()
    };
    let (scu, scp) = pair(
        dial(
            &[(d::STUDY_ROOT_GET_SOP_CLASS_UID, &[IVR]), (CT, &[IVR])],
            &[CT],
        ),
        accept(&[VERIFY, d::STUDY_ROOT_GET_SOP_CLASS_UID, CT], &[CT]),
        opts,
        |a| {
            let stored = Arc::new(Mutex::new(Vec::new()));
            let s2 = stored.clone();
            let handler: d::CGetStoreHandler =
                Arc::new(move |_: &Ctx, r: &d::CGetStoreRequestContext| {
                    s2.lock().unwrap().push((
                        r.request.affected_sop_instance_uid.clone(),
                        r.request.message_id,
                    ));
                    (0, None)
                });
            let req = d::CGetRequest {
                affected_sop_class_uid: d::STUDY_ROOT_GET_SOP_CLASS_UID.into(),
                message_id: 40,
                priority: 0,
            };
            let id = Object::from_elements(vec![text(Tag::new(0x0008, 0x0052), Vr::CS, "STUDY")]);
            let last = d::send_c_get_with_progress(
                None,
                a,
                3,
                &req,
                Some(&id),
                &dicom_rust::object::transfer::IMPLICIT_VR_LITTLE_ENDIAN,
                dicom_rust::net::dimse::CGetHandlers {
                    store: Some(&handler),
                    on_progress: &mut |_| {},
                },
            )
            .unwrap();
            a.release(None).unwrap();
            let s = stored.lock().unwrap().clone();
            (s, last.status, last.counts.completed)
        },
    );
    assert_eq!(
        scu.0,
        vec![("1.2.3.1".to_string(), 1), ("1.2.3.2".to_string(), 2)]
    );
    assert_eq!(scu.1, 0);
    assert_eq!(scu.2, Some(2));
    assert_eq!(scp, Ok(()));
}
#[test]
fn missing_handler_ends_the_association() {
    let (scu, scp) = pair(
        dial(&[(CT, &[IVR])], &[]),
        accept(&[VERIFY, CT], &[]),
        Default::default(),
        |a| {
            let mut client = d::StoreClient::new(a);
            client
                .store_with_options(None, &instance(CT, "1.2.3", 0), Default::default())
                .unwrap_err()
                .1
                .is(Kind::AssociationStateUncertain)
        },
    );
    assert!(scu);
    assert_eq!(scp, Err("dicom dimse: missing C-STORE handler".to_string()));
}
#[test]
fn association_server_saturation_and_shutdown() {
    let listener = Arc::new(ul::listen(ul::ListenOptions::default()).expect("listen"));
    let addr = listener.local_addr().unwrap().to_string();
    let handler: ul::AssociationHandler = Arc::new(|ctx: &Ctx, a: &ul::Association| {
        d::serve_association(Some(ctx), a, &Default::default()).map_err(|e| e.to_ul())
    });
    let server = Arc::new(
        ul::AssociationServer::new(
            listener,
            ul::AssociationServerOptions {
                accept: accept(&[VERIFY], &[]),
                max_concurrent_associations: 1,
                saturation_policy: ul::SaturationPolicy::Reject,
                handler: Some(handler),
            },
        )
        .unwrap(),
    );
    let s2 = server.clone();
    let serving = thread::spawn(move || s2.serve(None));
    let first = ul::dial(&addr, dial(&[], &[])).unwrap();
    assert_eq!(d::send_c_echo(None, &first, 1, 1).unwrap().status, 0);
    let second = ul::dial(&addr, dial(&[], &[])).unwrap_err();
    assert_eq!(
        second.text(),
        "dicom ul: association rejected: rejected-transient, service-provider (presentation), local-limit-exceeded (result=2 source=3 reason=2)"
    );
    first.release(None).unwrap();
    server
        .shutdown(Some(
            &Ctx::background().with_timeout(Duration::from_secs(2)),
        ))
        .unwrap();
    serving.join().unwrap().unwrap();
    let m = server.snapshot();
    assert_eq!((m.accepted, m.rejected, m.released, m.active), (1, 1, 1, 0));
}
const MPPS: &str = "1.2.840.10008.3.1.2.3.3";
#[test]
fn normalized_get_set_and_unrecognized_operation() {
    let get: d::NormalizedHandler =
        Arc::new(|_: &Ctx, _: &d::NormalizedRequest, _: Option<&Object>| {
            (
                d::NormalizedScpResult {
                    response: d::NormalizedResponse::default(),
                    data_set: Some(Object::from_elements(vec![text(
                        Tag::new(0x0010, 0x0010),
                        Vr::PN,
                        "DOE^JOHN",
                    )])),
                },
                None,
            )
        });
    let opts = d::AssociationScpOptions {
        normalized_scp: Some(d::NormalizedScpOptions {
            get_handler: Some(get),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (scu, scp) = pair(
        dial(&[(MPPS, &[IVR])], &[]),
        accept(&[VERIFY, MPPS], &[]),
        opts,
        |a| {
            let client = d::NormalizedClient::new(a);
            let options = d::NormalizedOperationOptions::default();
            let req = d::NormalizedRequest {
                sop_class_uid: MPPS.into(),
                sop_instance_uid: "1.2.3.4".into(),
                ..Default::default()
            };
            let (got, r) = client.get(&options, req.clone());
            assert!(r.is_ok());
            let rsp = got.response.expect("response");
            assert_eq!((rsp.message_id_being_responded_to, rsp.status), (1, 0));
            assert_eq!(
                got.data_set
                    .and_then(|o| o.get_raw(Tag::new(0x0010, 0x0010))),
                Some(b"DOE^JOHN".to_vec())
            );
            let (set, r) = client.set(&options, req, Some(&Object::new()));
            let err = r.expect_err("unrecognized operation");
            assert!(err.is(Kind::NormalizedStatus));
            assert_eq!(
                set.response.map(|r| r.status),
                Some(d::STATUS_UNRECOGNIZED_OPERATION)
            );
            a.release(None).is_ok()
        },
    );
    assert!(scu);
    assert_eq!(scp, Ok(()));
}
#[test]
fn mpps_create_then_complete_is_final() {
    use dicom_rust::mpps;
    let repo = Arc::new(mpps::MemoryRepository::new(0).expect("repo"));
    let service = mpps::Service::new(repo, Default::default()).expect("service");
    let attrs = mpps::build_create(&mpps::CreateAttributes {
        scheduled_steps: vec![mpps::ScheduledStep {
            study_instance_uid: "1.2.3.4".into(),
            ..Default::default()
        }],
        performed_procedure_step_id: "PPS1".into(),
        performed_station_ae_title: "CT1".into(),
        start_date: "20261118".into(),
        start_time: "1015".into(),
        modality: "CT".into(),
        ..Default::default()
    })
    .expect("create attributes");
    let done = mpps::complete(
        "20261118",
        "1100",
        vec![mpps::PerformedSeries {
            protocol_name: "P".into(),
            series_instance_uid: "1.2.3.5".into(),
            ..Default::default()
        }],
    )
    .expect("complete");
    let (scu, scp) = pair(
        dial(&[(MPPS, &[IVR])], &[]),
        accept(&[VERIFY, MPPS], &[]),
        service.association_scp_options(),
        |a| {
            let client = mpps::Client::new(a);
            let (created, r) = client.create(None, "", Some(&attrs));
            assert!(r.is_ok());
            let uid = created.sop_instance_uid;
            assert!(uid.starts_with("2.25."));
            assert!(client.set(None, &uid, Some(&done)).1.is_ok());
            let (again, r) = client.set(None, &uid, Some(&done));
            assert!(r.is_err());
            assert_eq!(again.status, 0x0110);
            assert_eq!(again.status_fields.and_then(|f| f.error_id), Some(0xA710));
            a.release(None).is_ok()
        },
    );
    assert!(scu);
    assert_eq!(scp, Ok(()));
}
