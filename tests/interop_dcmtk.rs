use dicom_rust::net::dimse as d;
use dicom_rust::net::ul::{self, Ctx};
use dicom_rust::object::{self, Element, ElementHeader, Object, Tag, Value, Vr};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
const VERIFY: &str = "1.2.840.10008.1.1";
const SC: &str = "1.2.840.10008.5.1.4.1.1.7";
const IVR: &str = "1.2.840.10008.1.2";
const EVR: &str = "1.2.840.10008.1.2.1";
const STUDY_UID: &str = "1.2.826.0.1.3680043.10.1337.136.1";
const SERIES_UID: &str = "1.2.826.0.1.3680043.10.1337.136.1.1";
const INSTANCE_UID: &str = "1.2.826.0.1.3680043.10.1337.136.1.1.1";
const PATIENT_NAME: &str = "INTEROP^CASE";
const PATIENT_ID: &str = "INTEROP-136";
const TAG_QR_LEVEL: Tag = Tag::new(0x0008, 0x0052);
const TAG_SOP_INSTANCE: Tag = Tag::new(0x0008, 0x0018);
const TAG_STUDY_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_PATIENT_NAME: Tag = Tag::new(0x0010, 0x0010);
fn tool(name: &str) -> PathBuf {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    match std::env::var_os("DCMTK_BIN") {
        Some(dir) => Path::new(&dir).join(exe),
        None => PathBuf::from(exe),
    }
}
fn runs(program: &Path, arg: &str) -> bool {
    Command::new(program)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}
fn tools_present(tls: bool) -> bool {
    let mut missing: Vec<String> = [
        "echoscu", "storescu", "findscu", "movescu", "getscu", "storescp", "dcmqrscp",
    ]
    .iter()
    .filter(|t| !runs(&tool(t), "--version"))
    .map(|t| t.to_string())
    .collect();
    if tls && !runs(Path::new("openssl"), "version") {
        missing.push("openssl".into());
    }
    if missing.is_empty() {
        return true;
    }
    let note = format!("interop: not run, missing {}", missing.join(", "));
    assert!(
        std::env::var_os("DICOM_RUST_REQUIRE_DCMTK").is_none(),
        "{note}"
    );
    eprintln!("{note}");
    false
}
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("dicom-rust-interop-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Scratch(dir)
    }
    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        std::fs::create_dir_all(&dir).expect("scratch subdirectory");
        dir
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("free port")
        .port()
}
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn start(program: &str, args: &[String], port: u16) -> Server {
    let child = Command::new(tool(program))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("start {program}: {e}"));
    let server = Server(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "{program} does not listen");
        thread::sleep(Duration::from_millis(50));
    }
    server
}
fn run(program: &str, args: &[String]) -> Output {
    let out = Command::new(tool(program))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("run {program}: {e}"));
    assert!(
        out.status.success(),
        "{program} {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
struct Pki {
    ca: PathBuf,
    tool_key: PathBuf,
    tool_cert: PathBuf,
    #[cfg_attr(not(feature = "tls"), allow(dead_code))]
    own_key: PathBuf,
    #[cfg_attr(not(feature = "tls"), allow(dead_code))]
    own_cert: PathBuf,
}
fn openssl(dir: &Path, args: &[&str]) {
    let out = Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("openssl");
    assert!(
        out.status.success(),
        "openssl {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn make_pki(dir: &Path) -> Pki {
    std::fs::write(
        dir.join("leaf.cnf"),
        "subjectAltName=DNS:localhost,IP:127.0.0.1\n\
         keyUsage=critical,digitalSignature\n\
         extendedKeyUsage=serverAuth,clientAuth\n",
    )
    .expect("leaf.cnf");
    let key = |name: &str| {
        openssl(
            dir,
            &[
                "ecparam",
                "-name",
                "prime256v1",
                "-genkey",
                "-noout",
                "-out",
                name,
            ],
        )
    };
    key("ca.key");
    openssl(
        dir,
        &[
            "req",
            "-x509",
            "-new",
            "-key",
            "ca.key",
            "-sha256",
            "-days",
            "2",
            "-subj",
            "/CN=Interop Test Authority",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-out",
            "ca.pem",
        ],
    );
    for side in ["tool", "own"] {
        let (k, csr, pem) = (
            format!("{side}.key"),
            format!("{side}.csr"),
            format!("{side}.pem"),
        );
        key(&k);
        openssl(
            dir,
            &[
                "req",
                "-new",
                "-key",
                &k,
                "-subj",
                "/CN=localhost",
                "-out",
                &csr,
            ],
        );
        openssl(
            dir,
            &[
                "x509",
                "-req",
                "-in",
                &csr,
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca.key",
                "-CAcreateserial",
                "-days",
                "2",
                "-sha256",
                "-extfile",
                "leaf.cnf",
                "-out",
                &pem,
            ],
        );
    }
    Pki {
        ca: dir.join("ca.pem"),
        tool_key: dir.join("tool.key"),
        tool_cert: dir.join("tool.pem"),
        own_key: dir.join("own.key"),
        own_cert: dir.join("own.pem"),
    }
}
struct Transport {
    pki: Option<Pki>,
}
impl Transport {
    fn new(tls: bool, scratch: &Scratch) -> Transport {
        Transport {
            pki: tls.then(|| make_pki(&scratch.dir("pki"))),
        }
    }
    fn tool_args(&self) -> Vec<String> {
        match &self.pki {
            None => Vec::new(),
            Some(p) => vec![
                "+tls".into(),
                slashed(&p.tool_key),
                slashed(&p.tool_cert),
                "+cf".into(),
                slashed(&p.ca),
                "--require-peer-cert".into(),
            ],
        }
    }
    #[cfg(feature = "tls")]
    fn profile(&self, p: &Pki) -> ul::TlsProfileOptions {
        let read = |path: &Path| std::fs::read(path).expect("PEM file");
        ul::TlsProfileOptions {
            certificates: vec![
                ul::TlsCertificate::from_pem(&read(&p.own_cert), &read(&p.own_key))
                    .expect("own certificate"),
            ],
            trusted_cas: Some(ul::certificates_from_pem(&read(&p.ca)).expect("authority")),
            peer_authentication: ul::TlsPeerAuthentication::RequirePeerCertificate,
            ..Default::default()
        }
    }
    fn dial(&self, mut opts: ul::DialOptions) -> ul::DialOptions {
        #[cfg(feature = "tls")]
        if let Some(p) = &self.pki {
            let mut profile = self.profile(p);
            profile.server_name = "localhost".into();
            opts.tls_config = Some(ul::client_tls_config(profile).expect("client TLS"));
        }
        opts.negotiation_timeout = ul::Nanos(20_000_000_000);
        opts
    }
    fn accept(&self, opts: ul::AcceptOptions) -> ul::AcceptOptions {
        #[cfg(feature = "tls")]
        if let Some(p) = &self.pki {
            let mut opts = opts;
            opts.tls_config = Some(ul::server_tls_config(self.profile(p)).expect("server TLS"));
            return opts;
        }
        opts
    }
    fn check(&self, assoc: &ul::Association) {
        #[cfg(feature = "tls")]
        assert_eq!(
            assoc.tls_connection_state().is_some(),
            self.pki.is_some(),
            "transport of the association"
        );
        #[cfg(not(feature = "tls"))]
        let _ = assoc;
    }
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
fn pixels() -> Vec<u8> {
    (0..64u32 * 48).map(|i| (i * 7 % 251) as u8).collect()
}
fn instance() -> Object {
    let mut o = Object::from_elements(vec![
        text(Tag::new(0x0008, 0x0016), Vr::UI, SC),
        text(TAG_SOP_INSTANCE, Vr::UI, INSTANCE_UID),
        text(Tag::new(0x0008, 0x0020), Vr::DA, "20260102"),
        text(Tag::new(0x0008, 0x0030), Vr::TM, "030405"),
        text(Tag::new(0x0008, 0x0050), Vr::SH, "ACC136"),
        text(Tag::new(0x0008, 0x0060), Vr::CS, "OT"),
        text(Tag::new(0x0008, 0x0064), Vr::CS, "WSD"),
        text(TAG_PATIENT_NAME, Vr::PN, PATIENT_NAME),
        text(Tag::new(0x0010, 0x0020), Vr::LO, PATIENT_ID),
        text(TAG_STUDY_UID, Vr::UI, STUDY_UID),
        text(Tag::new(0x0020, 0x000E), Vr::UI, SERIES_UID),
        text(Tag::new(0x0020, 0x0010), Vr::SH, "1"),
        text(Tag::new(0x0020, 0x0011), Vr::IS, "1"),
        text(Tag::new(0x0020, 0x0013), Vr::IS, "1"),
        text(Tag::new(0x0028, 0x0004), Vr::CS, "MONOCHROME2"),
    ]);
    for (element, value) in [
        (0x0002u16, 1u16),
        (0x0010, 48),
        (0x0011, 64),
        (0x0100, 8),
        (0x0101, 8),
        (0x0102, 7),
        (0x0103, 0),
    ] {
        o.set_us(Tag::new(0x0028, element), &[value]);
    }
    let raw = pixels();
    o.put(Element {
        header: ElementHeader::new(Tag::PIXEL_DATA, Vr::OB, raw.len() as u32),
        value: Value::Raw(raw),
    });
    o
}
fn write_instance(dir: &Path) -> PathBuf {
    let file = object::File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset: instance(),
        transfer_syntax: object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    let bytes = object::writer::write_file(&file).expect("write the instance");
    let path = dir.join("instance.dcm");
    std::fs::write(&path, bytes).expect("instance file");
    path
}
fn assert_instance(o: &Object, what: &str) {
    assert_eq!(
        o.get_uid(TAG_SOP_INSTANCE).as_deref(),
        Some(INSTANCE_UID),
        "{what}"
    );
    assert_eq!(
        o.get_string(TAG_PATIENT_NAME).as_deref(),
        Some(PATIENT_NAME),
        "{what}"
    );
    assert_eq!(o.get_raw(Tag::PIXEL_DATA), Some(pixels()), "{what}: pixels");
}
fn files_in(dir: &Path) -> Vec<object::File> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("output directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| object::open_file(p).unwrap_or_else(|(_, e)| panic!("read {}: {e}", p.display())))
        .collect()
}
fn query() -> Object {
    Object::from_elements(vec![
        text(TAG_QR_LEVEL, Vr::CS, "STUDY"),
        text(TAG_PATIENT_NAME, Vr::PN, ""),
        text(TAG_STUDY_UID, Vr::UI, STUDY_UID),
    ])
}
struct Provider {
    listener: Arc<ul::Listener>,
    port: u16,
    stored: Arc<Mutex<Vec<Object>>>,
    errors: Arc<Mutex<Vec<String>>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Provider {
    fn start(transport: &Arc<Transport>, move_port: u16) -> Provider {
        let listener = Arc::new(ul::listen(ul::ListenOptions::default()).expect("listen"));
        let port = listener.local_addr().expect("address").port();
        let stored = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let seen = stored.clone();
        let store: d::CStoreHandler = Arc::new(move |_: &Ctx, req: &d::CStoreRequestContext| {
            match &req.data_set {
                Some(ds) => seen.lock().unwrap().push(ds.clone()),
                None => return (0xC000, None),
            }
            (0, None)
        });
        let find: d::CFindHandler = Arc::new(|_: &Ctx, req: &d::CFindRequestContext| {
            let wanted = req.identifier.get_uid(TAG_STUDY_UID).unwrap_or_default();
            if req.query_retrieve_level != "STUDY" || (!wanted.is_empty() && wanted != STUDY_UID) {
                return Ok(Vec::new());
            }
            Ok(vec![d::CFindMatch {
                identifier: Some(Object::from_elements(vec![
                    text(TAG_QR_LEVEL, Vr::CS, "STUDY"),
                    text(TAG_PATIENT_NAME, Vr::PN, PATIENT_NAME),
                    text(TAG_STUDY_UID, Vr::UI, STUDY_UID),
                ])),
                status: 0,
                before_send: None,
            }])
        });
        let mover = transport.clone();
        let c_move: d::CMoveHandler = Arc::new(move |_: &Ctx, req: &d::CMoveRequestContext| {
            let transport = mover.clone();
            let destination = req.request.move_destination.clone();
            let originator = req.request.message_id;
            Ok(vec![d::CMoveSubOperation {
                affected_sop_class_uid: SC.into(),
                affected_sop_instance_uid: INSTANCE_UID.into(),
                store: Some(Arc::new(move |_: &Ctx| {
                    match forward(&transport, move_port, &destination, originator) {
                        Ok(status) => d::CMoveSubOperationResult { status, err: None },
                        Err(e) => d::CMoveSubOperationResult {
                            status: 0xA702,
                            err: Some(d::Error::new(e)),
                        },
                    }
                })),
            }])
        });
        let c_get: d::CGetHandler = Arc::new(|_: &Ctx, _: &d::CGetRequestContext| {
            Ok(vec![d::CGetSubOperation {
                affected_sop_class_uid: SC.into(),
                affected_sop_instance_uid: INSTANCE_UID.into(),
                load_data_set: Some(Arc::new(|_: &Ctx| Ok(instance()))),
                ..Default::default()
            }])
        });
        let scp = Arc::new(d::AssociationScpOptions {
            storage: d::StorageScpOptions {
                store_handler: Some(store),
                ..Default::default()
            },
            c_find_handler: Some(find),
            c_move_handler: Some(c_move),
            c_get_handler: Some(c_get),
            ..Default::default()
        });
        let accept = transport.accept(ul::AcceptOptions {
            ae_title: "OWNSCP".into(),
            supported_abstract_syntaxes: strings(&[
                VERIFY,
                SC,
                d::STUDY_ROOT_FIND_SOP_CLASS_UID,
                d::STUDY_ROOT_MOVE_SOP_CLASS_UID,
                d::STUDY_ROOT_GET_SOP_CLASS_UID,
            ]),
            supported_transfer_syntaxes: strings(&[EVR, IVR]),
            role_selections: vec![ul::RoleSelectionItem {
                sop_class_uid: SC.into(),
                scu_role: false,
                scp_role: true,
            }],
            ..Default::default()
        });
        let (l2, e2, t2) = (listener.clone(), errors.clone(), transport.clone());
        let thread = thread::spawn(move || {
            let mut serving = Vec::new();
            while let Ok(assoc) = l2.accept_association(accept.clone()) {
                let (scp, errors, transport) = (scp.clone(), e2.clone(), t2.clone());
                serving.push(thread::spawn(move || {
                    transport.check(&assoc);
                    if let Err(e) = d::serve_association(None, &assoc, &scp) {
                        errors.lock().unwrap().push(e.text().to_string());
                    }
                    assoc.close();
                }));
            }
            for s in serving {
                let _ = s.join();
            }
        });
        Provider {
            listener,
            port,
            stored,
            errors,
            thread: Some(thread),
        }
    }
    fn finish(mut self) -> (Vec<Object>, Vec<String>) {
        self.listener.close();
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            t.join().expect("provider thread");
        }
        let stored = self.stored.lock().unwrap().clone();
        let errors = self.errors.lock().unwrap().clone();
        (stored, errors)
    }
}
fn forward(
    transport: &Transport,
    port: u16,
    destination: &str,
    originator_message_id: u16,
) -> Result<u16, String> {
    let dial = transport.dial(ul::DialOptions {
        called_ae_title: destination.to_string(),
        calling_ae_title: "OWNSCP".into(),
        contexts: vec![ul::PresentationContext::new(SC, &[EVR, IVR])],
        ..Default::default()
    });
    let assoc = ul::dial(&format!("127.0.0.1:{port}"), dial).map_err(|e| e.to_string())?;
    let mut client = d::StoreClient::new(&assoc);
    let result = client.store_with_options(
        None,
        &instance(),
        d::CStoreOptions {
            move_originator_ae_title: "DCMTKSCU".into(),
            move_originator_message_id: Some(originator_message_id),
            ..Default::default()
        },
    );
    let status = match result {
        Ok(r) => r.response.map_or(0xC000, |r| r.status),
        Err((_, e)) => {
            assoc.close();
            return Err(e.text().to_string());
        }
    };
    assoc.release(None).map_err(|e| e.to_string())?;
    assoc.close();
    Ok(status)
}
fn provider_against_dcmtk_requestors(tls: bool) {
    if !tools_present(tls) {
        return;
    }
    let scratch = Scratch::new(if tls { "scp-tls" } else { "scp-tcp" });
    let transport = Arc::new(Transport::new(tls, &scratch));
    let file = write_instance(&scratch.dir("in"));
    let move_port = free_port();
    let provider = Provider::start(&transport, move_port);
    let port = provider.port.to_string();
    let common = |extra: &[&str]| {
        let mut args = strings(&["-aet", "DCMTKSCU", "-aec", "OWNSCP"]);
        args.extend(transport.tool_args());
        args.extend(strings(extra));
        args
    };
    let peer = |mut args: Vec<String>| {
        args.extend(strings(&["127.0.0.1", &port]));
        args
    };
    run("echoscu", &peer(common(&[])));
    let mut args = peer(common(&[]));
    args.push(slashed(&file));
    run("storescu", &args);
    let found = scratch.dir("found");
    run(
        "findscu",
        &peer(common(&[
            "-S",
            "-k",
            "0008,0052=STUDY",
            "-k",
            "0010,0010",
            "-k",
            &format!("0020,000D={STUDY_UID}"),
            "-X",
            "-od",
            &slashed(&found),
        ])),
    );
    let responses = files_in(&found);
    assert_eq!(responses.len(), 1, "C-FIND responses written by findscu");
    assert_eq!(
        responses[0].dataset.get_uid(TAG_STUDY_UID).as_deref(),
        Some(STUDY_UID)
    );
    assert_eq!(
        responses[0].dataset.get_string(TAG_PATIENT_NAME).as_deref(),
        Some(PATIENT_NAME)
    );
    let moved = scratch.dir("moved");
    run(
        "movescu",
        &peer(common(&[
            "-S",
            "-aem",
            "DCMTKSCU",
            "+P",
            &move_port.to_string(),
            "-k",
            "0008,0052=STUDY",
            "-k",
            &format!("0020,000D={STUDY_UID}"),
            "-od",
            &slashed(&moved),
        ])),
    );
    let files = files_in(&moved);
    assert_eq!(files.len(), 1, "instances received by movescu");
    assert_instance(&files[0].dataset, "C-MOVE to movescu");
    let got = scratch.dir("got");
    run(
        "getscu",
        &peer(common(&[
            "-S",
            "-k",
            "0008,0052=STUDY",
            "-k",
            &format!("0020,000D={STUDY_UID}"),
            "-od",
            &slashed(&got),
        ])),
    );
    let files = files_in(&got);
    assert_eq!(files.len(), 1, "instances received by getscu");
    assert_instance(&files[0].dataset, "C-GET to getscu");
    let (stored, errors) = provider.finish();
    assert_eq!(errors, Vec::<String>::new(), "provider associations");
    assert_eq!(stored.len(), 1, "instances stored by storescu");
    assert_instance(&stored[0], "C-STORE from storescu");
}
fn dial(transport: &Transport, port: u16, called: &str, contexts: &[&str]) -> ul::Association {
    let opts = transport.dial(ul::DialOptions {
        called_ae_title: called.to_string(),
        calling_ae_title: "OWNSCU".into(),
        contexts: contexts
            .iter()
            .map(|a| ul::PresentationContext::new(a, &[EVR, IVR]))
            .collect(),
        role_selections: if contexts.contains(&d::STUDY_ROOT_GET_SOP_CLASS_UID) {
            vec![ul::RoleSelectionItem {
                sop_class_uid: SC.into(),
                scu_role: false,
                scp_role: true,
            }]
        } else {
            Vec::new()
        },
        ..Default::default()
    });
    let assoc = ul::dial(&format!("127.0.0.1:{port}"), opts)
        .unwrap_or_else(|e| panic!("dial {called}: {e}"));
    transport.check(&assoc);
    assoc
}
fn store(assoc: &ul::Association) -> u16 {
    let mut client = d::StoreClient::new(assoc);
    match client.store_with_options(None, &instance(), Default::default()) {
        Ok(r) => r.response.expect("C-STORE response").status,
        Err((_, e)) => panic!("C-STORE: {}", e.text()),
    }
}
fn requestor_against_dcmtk_providers(tls: bool) {
    if !tools_present(tls) {
        return;
    }
    let scratch = Scratch::new(if tls { "scu-tls" } else { "scu-tcp" });
    let transport = Arc::new(Transport::new(tls, &scratch));
    let received = scratch.dir("received");
    let port = free_port();
    let mut args = strings(&["-aet", "DCMTKSCP", "-od", &slashed(&received)]);
    args.extend(transport.tool_args());
    args.push(port.to_string());
    let storescp = start("storescp", &args, port);
    let assoc = dial(&transport, port, "DCMTKSCP", &[VERIFY, SC]);
    let pc = d::accepted_verification_context(&assoc).expect("verification context");
    assert_eq!(d::send_c_echo(None, &assoc, pc.id, 1).unwrap().status, 0);
    assert_eq!(store(&assoc), 0, "C-STORE status of storescp");
    assoc.release(None).expect("release");
    assoc.close();
    drop(storescp);
    let files = files_in(&received);
    assert_eq!(files.len(), 1, "instances written by storescp");
    assert_instance(&files[0].dataset, "C-STORE to storescp");
    let destination = Arc::new(ul::listen(ul::ListenOptions::default()).expect("listen"));
    let destination_port = destination.local_addr().expect("address").port();
    let port = free_port();
    let database = scratch.dir("database");
    let config = scratch.0.join("dcmqrscp.cfg");
    std::fs::write(
        &config,
        format!(
            "NetworkTCPPort = {port}\nMaxPDUSize = 16384\nMaxAssociations = 16\n\n\
             HostTable BEGIN\nowndest = (OWNDEST, localhost, {destination_port})\n\
             ownscu = (OWNSCU, localhost, {destination_port})\nHostTable END\n\n\
             VendorTable BEGIN\nVendorTable END\n\n\
             AETable BEGIN\nDCMTKQR {} RW (100, 256mb) ANY\nAETable END\n",
            slashed(&database)
        ),
    )
    .expect("configuration file");
    let mut args = strings(&["-c", &slashed(&config)]);
    args.extend(transport.tool_args());
    args.push(port.to_string());
    let dcmqrscp = start("dcmqrscp", &args, port);
    let assoc = dial(&transport, port, "DCMTKQR", &[VERIFY, SC]);
    assert_eq!(store(&assoc), 0, "C-STORE status of dcmqrscp");
    assoc.release(None).expect("release");
    assoc.close();
    let assoc = dial(
        &transport,
        port,
        "DCMTKQR",
        &[d::STUDY_ROOT_FIND_SOP_CLASS_UID],
    );
    let client = d::FindClient::for_sop_class(&assoc, d::STUDY_ROOT_FIND_SOP_CLASS_UID).unwrap();
    let mut results = Vec::new();
    client
        .find_with_options(
            Default::default(),
            d::CFindRequest {
                message_id: 1,
                ..Default::default()
            },
            Some(&query()),
            &mut |r| results.push((r.status(), r.identifier)),
        )
        .expect("C-FIND");
    assoc.release(None).expect("release");
    assoc.close();
    let statuses: Vec<u16> = results.iter().map(|r| r.0).collect();
    assert_eq!(statuses, vec![0xFF00, 0x0000], "C-FIND statuses");
    let matched = results[0].1.as_ref().expect("C-FIND identifier");
    assert_eq!(matched.get_uid(TAG_STUDY_UID).as_deref(), Some(STUDY_UID));
    assert_eq!(
        matched.get_string(TAG_PATIENT_NAME).as_deref(),
        Some(PATIENT_NAME)
    );
    let assoc = dial(
        &transport,
        port,
        "DCMTKQR",
        &[d::STUDY_ROOT_GET_SOP_CLASS_UID, SC],
    );
    let pc = d::accepted_context_for_sop_class(&assoc, d::STUDY_ROOT_GET_SOP_CLASS_UID).unwrap();
    let syntax = d::accepted_transfer_syntax(&assoc, pc.id).unwrap();
    let got = Arc::new(Mutex::new(Vec::new()));
    let g2 = got.clone();
    let handler: d::CGetStoreHandler = Arc::new(move |_: &Ctx, r: &d::CGetStoreRequestContext| {
        g2.lock().unwrap().push(r.data_set.clone());
        (0, None)
    });
    let last = d::send_c_get_with_progress(
        None,
        &assoc,
        pc.id,
        &d::CGetRequest {
            affected_sop_class_uid: d::STUDY_ROOT_GET_SOP_CLASS_UID.into(),
            message_id: 2,
            priority: 0,
        },
        Some(&query()),
        &syntax,
        dicom_rust::net::dimse::CGetHandlers {
            store: Some(&handler),
            on_progress: &mut |_| {},
        },
    )
    .expect("C-GET");
    assoc.release(None).expect("release");
    assoc.close();
    assert_eq!(last.status, 0, "C-GET final status");
    assert_eq!(last.counts.completed, Some(1), "C-GET completed");
    let got = got.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "instances received by C-GET");
    assert_instance(&got[0], "C-GET from dcmqrscp");
    let accept = transport.accept(ul::AcceptOptions {
        ae_title: "OWNDEST".into(),
        supported_abstract_syntaxes: strings(&[VERIFY, SC]),
        supported_transfer_syntaxes: strings(&[EVR, IVR]),
        ..Default::default()
    });
    let moved = Arc::new(Mutex::new(Vec::new()));
    let (m2, l2, t2) = (moved.clone(), destination.clone(), transport.clone());
    let receiving = thread::spawn(move || -> Result<(), String> {
        let assoc = l2.accept_association(accept).map_err(|e| e.to_string())?;
        t2.check(&assoc);
        let storage = d::StorageScpOptions {
            store_handler: Some(Arc::new(move |_: &Ctx, req: &d::CStoreRequestContext| {
                if let Some(ds) = &req.data_set {
                    m2.lock().unwrap().push(ds.clone());
                }
                (0, None)
            })),
            ..Default::default()
        };
        let served =
            d::serve_storage_association(None, &assoc, &storage).map_err(|e| e.text().to_string());
        assoc.close();
        served
    });
    let assoc = dial(
        &transport,
        port,
        "DCMTKQR",
        &[d::STUDY_ROOT_MOVE_SOP_CLASS_UID],
    );
    let pc = d::accepted_context_for_sop_class(&assoc, d::STUDY_ROOT_MOVE_SOP_CLASS_UID).unwrap();
    let syntax = d::accepted_transfer_syntax(&assoc, pc.id).unwrap();
    let last = d::send_c_move_with_options(
        Default::default(),
        &assoc,
        pc.id,
        &d::CMoveRequest {
            affected_sop_class_uid: d::STUDY_ROOT_MOVE_SOP_CLASS_UID.into(),
            message_id: 3,
            move_destination: "OWNDEST".into(),
            ..Default::default()
        },
        Some(&query()),
        &syntax,
        &mut |_| {},
    );
    destination.close();
    let _ = TcpStream::connect(("127.0.0.1", destination_port));
    let received = receiving.join().expect("destination thread");
    let last = last.expect("C-MOVE");
    assoc.release(None).expect("release");
    assoc.close();
    drop(dcmqrscp);
    assert_eq!(received, Ok(()), "destination association");
    assert_eq!(last.status, 0, "C-MOVE final status");
    assert_eq!(last.counts.completed, Some(1), "C-MOVE completed");
    let moved = moved.lock().unwrap().clone();
    assert_eq!(moved.len(), 1, "instances received by C-MOVE");
    assert_instance(&moved[0], "C-MOVE from dcmqrscp");
}
#[test]
#[ignore = "needs the DCMTK tools: run scripts/ci.sh conformance"]
fn provider_against_dcmtk_requestors_tcp() {
    provider_against_dcmtk_requestors(false);
}
#[test]
#[ignore = "needs the DCMTK tools: run scripts/ci.sh conformance"]
fn requestor_against_dcmtk_providers_tcp() {
    requestor_against_dcmtk_providers(false);
}
#[cfg(feature = "tls")]
#[test]
#[ignore = "needs the DCMTK tools and openssl: run scripts/ci.sh conformance"]
fn provider_against_dcmtk_requestors_tls() {
    provider_against_dcmtk_requestors(true);
}
#[cfg(feature = "tls")]
#[test]
#[ignore = "needs the DCMTK tools and openssl: run scripts/ci.sh conformance"]
fn requestor_against_dcmtk_providers_tls() {
    requestor_against_dcmtk_providers(true);
}
