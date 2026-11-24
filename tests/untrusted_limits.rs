use dicom_rust::limits::Limits;
use dicom_rust::net::ul;
use dicom_rust::object::{self, ReadFileOptions};
use dicom_rust::pixel::pixeldata::{self, CodecRecovery, PixelErrorKind};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
thread_local! {
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}
struct Counting;
fn grew(size: usize) {
    let _ = LIVE.try_with(|live| {
        let now = live.get().saturating_add(size);
        live.set(now);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
    });
}
fn shrank(size: usize) {
    let _ = LIVE.try_with(|live| live.set(live.get().saturating_sub(size)));
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grew(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grew(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrank(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        shrank(layout.size());
        grew(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn measured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(base));
    let out = f();
    (out, PEAK.with(Cell::get).saturating_sub(base))
}
const MIB: usize = 1 << 20;
fn element(group: u16, elem: u16, vr: &str, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(vr.as_bytes());
    if matches!(vr, "OB" | "OW" | "SQ" | "UN") {
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    } else {
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    }
    out.extend_from_slice(value);
    out
}
fn part10(uid: &str, body: &[u8]) -> Vec<u8> {
    let meta = [
        element(0x0002, 0x0001, "OB", &[0, 1]),
        element(0x0002, 0x0002, "UI", b"1.2.840.10008.5.1.4.1.1.7\0"),
        element(0x0002, 0x0003, "UI", b"1.2.3.4\0"),
        element(0x0002, 0x0010, "UI", &{
            let mut u = uid.as_bytes().to_vec();
            if u.len() % 2 == 1 {
                u.push(0);
            }
            u
        }),
    ]
    .concat();
    let mut out = vec![0u8; 128];
    out.extend_from_slice(b"DICM");
    out.extend(element(
        0x0002,
        0x0000,
        "UL",
        &(meta.len() as u32).to_le_bytes(),
    ));
    out.extend(meta);
    out.extend_from_slice(body);
    out
}
const EXPLICIT: &str = "1.2.840.10008.1.2.1";
const ITEM: [u8; 4] = [0xFE, 0xFF, 0x00, 0xE0];
const ITEM_END: [u8; 8] = [0xFE, 0xFF, 0x0D, 0xE0, 0, 0, 0, 0];
const SEQUENCE_END: [u8; 8] = [0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0];
const UNDEFINED: [u8; 4] = [0xFF; 4];
fn undefined_sequence_start(group: u16, elem: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&elem.to_le_bytes());
    out.extend_from_slice(b"SQ\0\0");
    out.extend_from_slice(&UNDEFINED);
    out
}
fn with(limits: Limits) -> ReadFileOptions {
    ReadFileOptions {
        limits,
        ..ReadFileOptions::default()
    }
}
fn read_error(data: &[u8], opts: ReadFileOptions) -> String {
    match object::read_file_with_options(data, opts) {
        Ok(_) => panic!("the read succeeded"),
        Err((_, err)) => err.to_string(),
    }
}
#[test]
fn element_length_beyond_the_bytes_present_is_refused_without_allocating() {
    let mut body = vec![0x10, 0x00, 0x20, 0x40, b'O', b'B', 0, 0];
    body.extend_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    body.extend_from_slice(&[0u8; 16]);
    let file = part10(EXPLICIT, &body);
    for limits in [Limits::default(), Limits::NONE] {
        let (text, peak) = measured(|| read_error(&file, with(limits)));
        assert!(
            text.contains("4294967280") || text.contains("exceed"),
            "{text}"
        );
        assert!(peak < MIB, "peak {peak}");
    }
    let (result, peak) = measured(|| {
        object::read_data_set_with_options(
            Box::new(Unsized(body.clone(), 0)),
            object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
            with(Limits::default()),
        )
    });
    assert!(result.is_err());
    assert!(peak < MIB, "peak {peak}");
}
struct Unsized(Vec<u8>, usize);
impl object::parser::source::ByteRead for Unsized {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<object::Error>) {
        let rest = &self.0[self.1..];
        if rest.is_empty() {
            return (0, Some(object::Error::eof()));
        }
        let n = p.len().min(rest.len());
        p[..n].copy_from_slice(&rest[..n]);
        self.1 += n;
        (n, None)
    }
}
#[test]
fn element_bytes_limit() {
    let file = part10(EXPLICIT, &element(0x0042, 0x0011, "OB", &[7u8; 64]));
    assert!(object::read_file_with_options(&file, with(Limits::default())).is_ok());
    let text = read_error(
        &file,
        with(Limits {
            max_element_bytes: 32,
            ..Limits::default()
        }),
    );
    assert!(
        text.contains("maximum element value size exceeded"),
        "{text}"
    );
    let opts = ReadFileOptions {
        max_element_bytes: 64,
        limits: Limits {
            max_element_bytes: 32,
            ..Limits::default()
        },
        ..ReadFileOptions::default()
    };
    assert!(object::read_file_with_options(&file, opts).is_ok());
}
#[test]
fn pixel_data_bytes_limit() {
    let file = part10(EXPLICIT, &element(0x7FE0, 0x0010, "OW", &[0u8; 64]));
    assert!(object::read_file_with_options(&file, with(Limits::default())).is_ok());
    let text = read_error(
        &file,
        with(Limits {
            max_pixel_data_bytes: 32,
            ..Limits::default()
        }),
    );
    assert!(text.contains("maximum Pixel Data size exceeded"), "{text}");
}
#[test]
fn data_set_bytes_limit() {
    let body: Vec<u8> = (0..64u16)
        .flat_map(|i| element(0x0011, 0x1000 + i, "LO", b"12345678"))
        .collect();
    let file = part10(EXPLICIT, &body);
    assert!(object::read_file_with_options(&file, with(Limits::default())).is_ok());
    let text = read_error(
        &file,
        with(Limits {
            max_data_set_bytes: 400,
            ..Limits::default()
        }),
    );
    assert!(text.contains("maximum total bytes exceeded"), "{text}");
}
fn nested_sequences(depth: usize) -> Vec<u8> {
    let mut body = Vec::new();
    for _ in 0..depth {
        body.extend(undefined_sequence_start(0x0008, 0x1140));
        body.extend_from_slice(&ITEM);
        body.extend_from_slice(&UNDEFINED);
    }
    for _ in 0..depth {
        body.extend_from_slice(&ITEM_END);
        body.extend_from_slice(&SEQUENCE_END);
    }
    part10(EXPLICIT, &body)
}
#[test]
fn sequence_depth_limit() {
    assert!(object::read_file_with_options(&nested_sequences(60), with(Limits::default())).is_ok());
    let deep = nested_sequences(20_000);
    let (text, peak) = measured(|| read_error(&deep, with(Limits::default())));
    assert!(
        text.contains("maximum sequence nesting depth exceeded") && text.contains("limit 128"),
        "{text}"
    );
    assert!(peak < 4 * MIB, "peak {peak}");
    let text = read_error(
        &nested_sequences(8),
        with(Limits {
            max_sequence_depth: 6,
            ..Limits::default()
        }),
    );
    assert!(text.contains("limit 6"), "{text}");
}
#[test]
fn item_count_limit() {
    let mut body = undefined_sequence_start(0x0008, 0x1140);
    for _ in 0..11 {
        body.extend_from_slice(&ITEM);
        body.extend_from_slice(&[0; 4]);
    }
    body.extend_from_slice(&SEQUENCE_END);
    let file = part10(EXPLICIT, &body);
    assert!(object::read_file_with_options(&file, with(Limits::default())).is_ok());
    let text = read_error(
        &file,
        with(Limits {
            max_items: 10,
            ..Limits::default()
        }),
    );
    assert!(
        text.contains("maximum sequence item count exceeded: got 11, limit 10"),
        "{text}"
    );
}
#[test]
fn element_count_limit() {
    let body: Vec<u8> = (0..6u16)
        .flat_map(|i| element(0x0011, 0x1000 + i, "LO", b"ab"))
        .collect();
    let file = part10(EXPLICIT, &body);
    let text = read_error(
        &file,
        with(Limits {
            max_elements: 5,
            ..Limits::default()
        }),
    );
    assert!(
        text.contains("maximum element count exceeded: got 6, limit 5"),
        "{text}"
    );
}
#[test]
fn fragment_count_limit() {
    let mut body = vec![0xE0, 0x7F, 0x10, 0x00, b'O', b'B', 0, 0];
    body.extend_from_slice(&UNDEFINED);
    body.extend_from_slice(&ITEM);
    body.extend_from_slice(&[0; 4]);
    for _ in 0..3 {
        body.extend_from_slice(&ITEM);
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(&[1, 2, 3, 4]);
    }
    body.extend_from_slice(&SEQUENCE_END);
    let file = part10("1.2.840.10008.1.2.4.80", &body);
    assert!(object::read_file_with_options(&file, with(Limits::default())).is_ok());
    let text = read_error(
        &file,
        with(Limits {
            max_fragments: 2,
            ..Limits::default()
        }),
    );
    assert!(
        text.contains("maximum fragment count exceeded: got 3, limit 2"),
        "{text}"
    );
}
fn deflate_bomb(prefix: &[u8], matches: usize) -> Vec<u8> {
    struct Bits(Vec<u8>, u32, u32);
    impl Bits {
        fn put(&mut self, value: u32, count: u32) {
            for i in 0..count {
                self.1 |= (value >> i & 1) << self.2;
                self.2 += 1;
                if self.2 == 8 {
                    self.0.push(self.1 as u8);
                    self.1 = 0;
                    self.2 = 0;
                }
            }
        }
        fn code(&mut self, code: u32, count: u32) {
            for i in (0..count).rev() {
                self.put(code >> i & 1, 1);
            }
        }
    }
    let mut b = Bits(Vec::new(), 0, 0);
    b.put(1, 1);
    b.put(1, 2);
    for &v in prefix.iter().chain(&[0u8, 0]) {
        if v < 144 {
            b.code(0x30 + u32::from(v), 8);
        } else {
            b.code(0x190 + u32::from(v - 144), 9);
        }
    }
    for _ in 0..matches {
        b.code(0b1100_0101, 8);
        b.code(0, 5);
    }
    b.code(0, 7);
    b.put(0, 7);
    b.0
}
#[test]
fn inflated_size_limit() {
    let zeros: u32 = 200_000 * 258 + 2;
    let mut header = vec![0xE0, 0x7F, 0x10, 0x00, b'O', b'W', 0, 0];
    header.extend_from_slice(&zeros.to_le_bytes());
    let bomb = deflate_bomb(&header, 200_000);
    assert!(bomb.len() < 400_000);
    let file = part10("1.2.840.10008.1.2.1.99", &bomb);
    let whole = object::read_file_with_options(&file, with(Limits::NONE));
    let value = whole
        .as_ref()
        .ok()
        .and_then(|f| f.dataset.get_raw(object::Tag::new(0x7FE0, 0x0010)));
    assert_eq!(value.map(|v| v.len()), Some(zeros as usize));
    drop(whole);
    let (text, peak) = measured(|| {
        read_error(
            &file,
            with(Limits {
                max_inflated_bytes: 1 << 20,
                ..Limits::default()
            }),
        )
    });
    assert!(
        text.contains("maximum inflated size exceeded: limit 1048576"),
        "{text}"
    );
    assert!(peak < 8 * MIB, "peak {peak}");
}
fn declared_image(rows: u16, columns: u16, frames: u32) -> object::File {
    let us = |v: u16| v.to_le_bytes();
    let mut body = [
        element(0x0028, 0x0002, "US", &us(1)),
        element(0x0028, 0x0004, "CS", b"MONOCHROME2 "),
        element(0x0028, 0x0008, "IS", format!("{frames:<8}").as_bytes()),
        element(0x0028, 0x0010, "US", &us(rows)),
        element(0x0028, 0x0011, "US", &us(columns)),
        element(0x0028, 0x0100, "US", &us(16)),
        element(0x0028, 0x0101, "US", &us(16)),
        element(0x0028, 0x0102, "US", &us(15)),
        element(0x0028, 0x0103, "US", &us(0)),
    ]
    .concat();
    body.extend_from_slice(&[0xE0, 0x7F, 0x10, 0x00, b'O', b'B', 0, 0]);
    body.extend_from_slice(&UNDEFINED);
    body.extend_from_slice(&ITEM);
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&ITEM);
    body.extend_from_slice(&4u32.to_le_bytes());
    body.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);
    body.extend_from_slice(&SEQUENCE_END);
    match object::read_file(&part10("1.2.840.10008.1.2.4.80", &body)) {
        Ok(file) => file,
        Err((_, err)) => panic!("{err}"),
    }
}
fn decode_limited(file: &object::File, limits: &Limits) -> (Result<(), String>, PixelErrorKind) {
    let pixel = pixeldata::extract_view(&file.dataset).expect("pixel data");
    match pixeldata::decode_frames_with_limits(
        file.transfer_syntax.uid,
        &pixel,
        &file.dataset,
        &CodecRecovery::default(),
        limits,
    ) {
        Ok(_) => (Ok(()), PixelErrorKind::Other),
        Err(e) => (Err(e.to_string()), e.kind),
    }
}
#[test]
fn rows_columns_frames_and_frame_bytes_limits() {
    let limits = Limits {
        max_rows: 1024,
        max_columns: 2048,
        max_frames: 10,
        max_frame_bytes: 1 << 20,
        ..Limits::default()
    };
    let cases = [
        (1025u16, 16u16, 1u32, "rows is 1025, limit 1024"),
        (16, 2049, 1, "columns is 2049, limit 2048"),
        (16, 16, 11, "frames is 11, limit 10"),
        (
            1024,
            1024,
            1,
            "decompressed frame bytes is 2097152, limit 1048576",
        ),
    ];
    for (rows, columns, frames, want) in cases {
        let file = declared_image(rows, columns, frames);
        let ((result, kind), peak) = measured(|| decode_limited(&file, &limits));
        let text = result.expect_err("refused");
        assert!(text.contains(want), "{text}");
        assert_eq!(kind, PixelErrorKind::ResourceLimit);
        assert!(peak < MIB, "peak {peak}");
    }
    let (result, kind) = decode_limited(&declared_image(16, 16, 1), &limits);
    assert!(result.is_err());
    assert_ne!(kind, PixelErrorKind::ResourceLimit);
}
#[test]
fn default_limits_refuse_the_largest_declarable_image_before_allocating() {
    let file = declared_image(65_535, 65_535, 1);
    let ((result, kind), peak) = measured(|| decode_limited(&file, &Limits::default()));
    assert!(result.is_err());
    assert_eq!(kind, PixelErrorKind::ResourceLimit);
    assert!(peak < MIB, "peak {peak}");
    let pixel = pixeldata::extract_view(&file.dataset).expect("pixel data");
    let (result, peak) =
        measured(|| pixeldata::decode_frames(file.transfer_syntax.uid, &pixel, &file.dataset));
    let err = result.expect_err("refused");
    assert_eq!(err.kind, PixelErrorKind::ResourceLimit);
    assert!(
        err.to_string()
            .contains("decompressed frame bytes is 8589672450, limit 1073741824"),
        "{err}"
    );
    assert!(peak < MIB, "peak {peak}");
    let many = declared_image(4, 4, 100_001);
    let pixel = pixeldata::extract_view(&many.dataset).expect("pixel data");
    let err = pixeldata::decode_frames(many.transfer_syntax.uid, &pixel, &many.dataset)
        .expect_err("refused");
    assert!(
        err.to_string().contains("frames is 100001, limit 100000"),
        "{err}"
    );
    let small = declared_image(16, 16, 1);
    let pixel = pixeldata::extract_view(&small.dataset).expect("pixel data");
    let err = pixeldata::decode_frames(small.transfer_syntax.uid, &pixel, &small.dataset)
        .expect_err("not a JPEG-LS stream");
    assert_ne!(err.kind, PixelErrorKind::ResourceLimit);
}
#[test]
fn derived_decoder_limits() {
    let limits = Limits {
        max_frames: 5,
        max_frame_bytes: 4096,
        ..Limits::default()
    };
    let d = limits.decompress_limits();
    assert_eq!((d.max_frames, d.max_pixels), (5, 4096));
    let g = limits.guard_limits();
    assert_eq!((g.max_frames, g.max_frame_bytes), (5, 4096));
}
#[test]
fn pdu_length_limit() {
    let limits = Limits {
        max_pdu_length: 16_384,
        max_association_pdu_length: 32_768,
        ..Limits::default()
    };
    let pdata = [4u8, 0, 0x7F, 0xFF, 0xFF, 0xFF];
    let (result, peak) = measured(|| {
        ul::read_pdu_with_association_limit(
            &mut &pdata[..],
            limits.max_pdu_length,
            limits.max_association_pdu_length,
        )
    });
    let text = result.expect_err("refused").to_string();
    assert!(
        text.contains("length 2147483647 exceeds max 16384"),
        "{text}"
    );
    assert!(peak < MIB, "peak {peak}");
    let associate = [1u8, 0, 0x00, 0x10, 0x00, 0x00];
    let (result, peak) = measured(|| {
        ul::read_pdu_with_association_limit(
            &mut &associate[..],
            limits.max_pdu_length,
            limits.max_association_pdu_length,
        )
    });
    let text = result.expect_err("refused").to_string();
    assert!(text.contains("length 1048576 exceeds max 32768"), "{text}");
    assert!(peak < MIB, "peak {peak}");
    let short = [4u8, 0, 0x00, 0x00, 0x40, 0x00, 1, 2, 3];
    let (result, peak) = measured(|| {
        ul::read_pdu_with_association_limit(
            &mut &short[..],
            limits.max_pdu_length,
            limits.max_association_pdu_length,
        )
    });
    assert!(result.is_err());
    assert!(peak < MIB, "peak {peak}");
    let mut accept = ul::AcceptOptions::default();
    limits.apply_to_accept(&mut accept);
    assert_eq!(
        (accept.max_pdu, accept.max_association_pdu),
        (16_384, 32_768)
    );
}
#[test]
fn association_count_limit() {
    const VERIFY: &str = "1.2.840.10008.1.1";
    const IVR: &str = "1.2.840.10008.1.2";
    let limits = Limits {
        max_associations: 1,
        ..Limits::default()
    };
    let listener = Arc::new(ul::listen(ul::ListenOptions::default()).expect("listen"));
    let addr = listener.local_addr().expect("address").to_string();
    let (hold, held) = std::sync::mpsc::channel::<()>();
    let held = Arc::new(std::sync::Mutex::new(held));
    let handler: ul::AssociationHandler = Arc::new(move |_ctx: &ul::Ctx, a: &ul::Association| {
        let _ = held
            .lock()
            .map(|rx| rx.recv_timeout(Duration::from_secs(10)));
        if let Ok(ul::Pdu::ReleaseRq) = a.receive(None) {
            a.send(None, &ul::Pdu::ReleaseRp)?;
        }
        Ok(())
    });
    let mut options = ul::AssociationServerOptions {
        accept: ul::AcceptOptions {
            ae_title: "SCP".into(),
            supported_abstract_syntaxes: vec![VERIFY.into()],
            supported_transfer_syntaxes: vec![IVR.into()],
            artim_timeout: ul::Nanos::from_secs(2),
            ..Default::default()
        },
        saturation_policy: ul::SaturationPolicy::Reject,
        handler: Some(handler),
        ..Default::default()
    };
    limits.apply_to_server(&mut options);
    assert_eq!(options.max_concurrent_associations, 1);
    let server = Arc::new(ul::AssociationServer::new(listener, options).expect("server"));
    let serving = {
        let server = server.clone();
        thread::spawn(move || server.serve(None))
    };
    let dial = || {
        ul::dial(
            &addr,
            ul::DialOptions {
                called_ae_title: "SCP".into(),
                calling_ae_title: "SCU".into(),
                contexts: vec![ul::PresentationContext::new(VERIFY, &[IVR])],
                artim_timeout: ul::Nanos::from_secs(2),
                ..Default::default()
            },
        )
    };
    let first = dial().expect("the first association");
    let second = dial().expect_err("the second association is over the limit");
    assert!(
        second.text().contains("local-limit-exceeded"),
        "{}",
        second.text()
    );
    hold.send(()).expect("handler waiting");
    first.release(None).expect("release");
    server
        .shutdown(Some(
            &ul::Ctx::background().with_timeout(Duration::from_secs(5)),
        ))
        .expect("shutdown");
    serving.join().expect("serve thread").expect("serve");
    let m = server.snapshot();
    assert_eq!((m.accepted, m.rejected), (1, 1));
}
#[test]
fn json_and_xml_nesting_is_bounded() {
    let json = [vec![b'['; 100_000], vec![b']'; 100_000]].concat();
    assert!(dicom_rust::dicomjson::unmarshal(&json, &Default::default()).is_err());
    let mut nested = b"{\"00081140\":{\"vr\":\"SQ\",\"Value\":[".repeat(5_000);
    nested.extend(b"{}".iter());
    nested.extend(b"]}}".repeat(5_000));
    assert!(dicom_rust::dicomjson::unmarshal(&nested, &Default::default()).is_err());
    let mut xml =
        b"<NativeDicomModel xmlns=\"http://dicom.nema.org/PS3.19/models/NativeDICOM\">".to_vec();
    for _ in 0..5_000 {
        xml.extend_from_slice(b"<DicomAttribute tag=\"00081140\" vr=\"SQ\" keyword=\"ReferencedImageSequence\"><Item number=\"1\">");
    }
    for _ in 0..5_000 {
        xml.extend_from_slice(b"</Item></DicomAttribute>");
    }
    xml.extend_from_slice(b"</NativeDicomModel>");
    assert!(dicom_rust::dicomxml::unmarshal(&xml, &mut Default::default()).is_err());
}
#[test]
fn memory_source_reads_like_the_file() {
    let body = element(0x0010, 0x0020, "LO", b"ID");
    let a = object::read_data_set_with_options(
        Box::new(Unsized(body.clone(), 0)),
        object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        ReadFileOptions::default(),
    )
    .expect("stream");
    let b = object::read_data_set_with_options(
        Box::new(object::parser::source::MemFile::new(Rc::from(&body[..]))),
        object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        ReadFileOptions::default(),
    )
    .expect("memory");
    assert_eq!(a.elements().len(), b.elements().len());
}
