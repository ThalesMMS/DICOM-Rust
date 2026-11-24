pub const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
pub const CT_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.2";
pub const ENHANCED_CT_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.2.1";
pub const RT_STRUCTURE_SET_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.3";
pub fn uid(kind: u32, a: u64, b: u64) -> String {
    format!("2.25.3141592653589793238{kind:02}{a:06}{b:06}")
}
pub type El = (u32, Vec<u8>);
fn long_vr(vr: &str) -> bool {
    matches!(
        vr,
        "OB" | "OD" | "OF" | "OL" | "OV" | "OW" | "SQ" | "SV" | "UC" | "UN" | "UR" | "UT" | "UV"
    )
}
fn header(tag: u32, vr: &str, len: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(&((tag >> 16) as u16).to_le_bytes());
    out.extend_from_slice(&(tag as u16).to_le_bytes());
    out.extend_from_slice(vr.as_bytes());
    if long_vr(vr) {
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&len.to_le_bytes());
    } else {
        out.extend_from_slice(&(len as u16).to_le_bytes());
    }
    out
}
pub fn raw(tag: u32, vr: &str, value: &[u8]) -> El {
    let mut v = value.to_vec();
    if v.len() % 2 == 1 {
        v.push(0);
    }
    let mut out = header(tag, vr, v.len() as u32);
    out.extend_from_slice(&v);
    (tag, out)
}
pub fn text(tag: u32, vr: &str, value: &str) -> El {
    let mut v = value.as_bytes().to_vec();
    if v.len() % 2 == 1 {
        v.push(if vr == "UI" { 0 } else { b' ' });
    }
    let mut out = header(tag, vr, v.len() as u32);
    out.extend_from_slice(&v);
    (tag, out)
}
pub fn us(tag: u32, value: u16) -> El {
    raw(tag, "US", &value.to_le_bytes())
}
pub fn ul(tag: u32, value: u32) -> El {
    raw(tag, "UL", &value.to_le_bytes())
}
pub fn body(mut els: Vec<El>) -> Vec<u8> {
    els.sort_by_key(|e| e.0);
    let mut out = Vec::with_capacity(els.iter().map(|e| e.1.len()).sum());
    for (_, b) in els {
        out.extend_from_slice(&b);
    }
    out
}
pub fn seq(tag: u32, items: Vec<Vec<El>>) -> El {
    let mut content = Vec::new();
    for item in items {
        let b = body(item);
        content.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0]);
        content.extend_from_slice(&(b.len() as u32).to_le_bytes());
        content.extend_from_slice(&b);
    }
    let mut out = header(tag, "SQ", content.len() as u32);
    out.extend_from_slice(&content);
    (tag, out)
}
pub fn encapsulated(frames: &[Vec<u8>]) -> El {
    let mut out = Vec::new();
    out.extend_from_slice(&[0xE0, 0x7F, 0x10, 0x00]);
    out.extend_from_slice(b"OB\0\0");
    out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    out.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0, 0, 0, 0, 0]);
    for f in frames {
        let mut f = f.clone();
        if f.len() % 2 == 1 {
            f.push(0);
        }
        out.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0]);
        out.extend_from_slice(&(f.len() as u32).to_le_bytes());
        out.extend_from_slice(&f);
    }
    out.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]);
    (0x7FE0_0010, out)
}
pub fn part10(sop_class: &str, sop_instance: &str, syntax: &str, dataset: &[u8]) -> Vec<u8> {
    let meta = body(vec![
        raw(0x0002_0001, "OB", &[0, 1]),
        text(0x0002_0002, "UI", sop_class),
        text(0x0002_0003, "UI", sop_instance),
        text(0x0002_0010, "UI", syntax),
        text(0x0002_0012, "UI", "2.25.271828182845904523536028747135"),
        text(0x0002_0013, "SH", "BENCH_SUITE"),
    ]);
    let mut out = vec![0u8; 128];
    out.extend_from_slice(b"DICM");
    out.extend_from_slice(&ul(0x0002_0000, meta.len() as u32).1);
    out.extend_from_slice(&meta);
    out.extend_from_slice(dataset);
    out
}
#[derive(Clone, Copy, Debug)]
pub struct CtPlace {
    pub patient: u64,
    pub study: u64,
    pub series: u64,
    pub instance: u64,
}
impl CtPlace {
    pub fn sop_instance(&self) -> String {
        uid(4, self.series * 1000 + self.study, self.instance)
    }
}
pub fn ct_header(p: CtPlace, rows: u16, cols: u16, z_mm: f64) -> Vec<El> {
    let ds = |v: f64| {
        format!("{v:.6}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };
    let code = |value: &str, scheme: &str, meaning: &str| {
        vec![
            text(0x0008_0100, "SH", value),
            text(0x0008_0102, "SH", scheme),
            text(0x0008_0104, "LO", meaning),
        ]
    };
    vec![
        text(0x0008_0005, "CS", "ISO_IR 100"),
        text(0x0008_0008, "CS", "ORIGINAL\\PRIMARY\\AXIAL"),
        text(0x0008_0012, "DA", "20250101"),
        text(0x0008_0013, "TM", "101500"),
        text(0x0008_0016, "UI", CT_IMAGE_STORAGE),
        text(0x0008_0018, "UI", &p.sop_instance()),
        text(0x0008_0020, "DA", "20250101"),
        text(0x0008_0021, "DA", "20250101"),
        text(0x0008_0022, "DA", "20250101"),
        text(0x0008_0023, "DA", "20250101"),
        text(0x0008_0030, "TM", "100000"),
        text(0x0008_0031, "TM", "100500"),
        text(0x0008_0032, "TM", "100510.250000"),
        text(0x0008_0033, "TM", "100511.500000"),
        text(0x0008_0050, "SH", &format!("ACC{:06}", p.study)),
        text(0x0008_0060, "CS", "CT"),
        text(0x0008_0070, "LO", "SYNTHETIC"),
        text(0x0008_0080, "LO", "BENCH HOSPITAL"),
        text(0x0008_0090, "PN", "REFERRER^ANY"),
        text(0x0008_1010, "SH", "STATION01"),
        text(0x0008_1030, "LO", "CHEST ABDOMEN PELVIS W CONTRAST"),
        seq(
            0x0008_1032,
            vec![code("CTCAP", "LOCAL", "CT CHEST ABDOMEN PELVIS")],
        ),
        text(0x0008_103E, "LO", "BODY 1.0 SOFT TISSUE"),
        text(0x0008_1090, "LO", "SYNTHETIC SCANNER"),
        seq(
            0x0008_1140,
            vec![vec![
                text(0x0008_1150, "UI", CT_IMAGE_STORAGE),
                text(0x0008_1155, "UI", &uid(9, p.study, 1)),
            ]],
        ),
        text(0x0010_0010, "PN", &format!("PHANTOM^BENCH{:03}", p.patient)),
        text(0x0010_0020, "LO", &format!("PID{:06}", p.patient)),
        text(0x0010_0030, "DA", "19700101"),
        text(0x0010_0040, "CS", "O"),
        text(0x0010_1010, "AS", "055Y"),
        text(0x0018_0015, "CS", "CHEST"),
        text(0x0018_0022, "CS", "HELICAL MODE"),
        text(0x0018_0050, "DS", "1"),
        text(0x0018_0060, "DS", "120"),
        text(0x0018_0088, "DS", "1"),
        text(0x0018_0090, "DS", "500"),
        text(0x0018_1020, "LO", "1.0.0"),
        text(0x0018_1030, "LO", "CAP_ROUTINE"),
        text(0x0018_1100, "DS", "358.4"),
        text(0x0018_1110, "DS", "1085.6"),
        text(0x0018_1111, "DS", "595"),
        text(0x0018_1120, "DS", "0"),
        text(0x0018_1130, "DS", "152"),
        text(0x0018_1140, "CS", "CW"),
        text(0x0018_1150, "IS", "500"),
        text(0x0018_1151, "IS", "212"),
        text(0x0018_1152, "IS", "106"),
        text(0x0018_1160, "SH", "BODY FILTER"),
        text(0x0018_1170, "IS", "24"),
        text(0x0018_1190, "DS", "0.7\\0.7"),
        text(0x0018_1210, "SH", "STANDARD"),
        text(0x0018_5100, "CS", "FFS"),
        text(0x0020_000D, "UI", &uid(2, p.patient, p.study)),
        text(0x0020_000E, "UI", &uid(3, p.study, p.series)),
        text(0x0020_0010, "SH", &format!("{}", p.study)),
        text(0x0020_0011, "IS", &format!("{}", p.series)),
        text(0x0020_0012, "IS", "1"),
        text(0x0020_0013, "IS", &format!("{}", p.instance + 1)),
        text(0x0020_0032, "DS", &format!("-179.2\\-179.2\\{}", ds(z_mm))),
        text(0x0020_0037, "DS", "1\\0\\0\\0\\1\\0"),
        text(0x0020_0052, "UI", &uid(5, p.study, p.series)),
        text(0x0020_1040, "LO", ""),
        text(0x0020_1041, "DS", &ds(z_mm)),
        us(0x0028_0002, 1),
        text(0x0028_0004, "CS", "MONOCHROME2"),
        us(0x0028_0010, rows),
        us(0x0028_0011, cols),
        text(0x0028_0030, "DS", "0.7\\0.7"),
        us(0x0028_0100, 16),
        us(0x0028_0101, 12),
        us(0x0028_0102, 11),
        us(0x0028_0103, 0),
        text(0x0028_1050, "DS", "40\\400"),
        text(0x0028_1051, "DS", "400\\1500"),
        text(0x0028_1052, "DS", "-1024"),
        text(0x0028_1053, "DS", "1"),
        text(0x0028_1054, "LO", "HU"),
        text(0x0028_1055, "LO", "WINDOW1\\WINDOW2"),
    ]
}
pub fn ct_native_file(p: CtPlace, rows: u16, cols: u16, z_mm: f64, pixels: &[u8]) -> Vec<u8> {
    let mut els = ct_header(p, rows, cols, z_mm);
    els.push(raw(0x7FE0_0010, "OW", pixels));
    part10(CT_IMAGE_STORAGE, &p.sop_instance(), EXPLICIT_LE, &body(els))
}
pub fn ct_encapsulated_file(
    p: CtPlace,
    rows: u16,
    cols: u16,
    z_mm: f64,
    ts: &str,
    codestream: &[u8],
) -> Vec<u8> {
    let mut els = ct_header(p, rows, cols, z_mm);
    els.push(encapsulated(&[codestream.to_vec()]));
    part10(CT_IMAGE_STORAGE, &p.sop_instance(), ts, &body(els))
}
pub fn enhanced_ct_file(frames: usize, frame_pixels: &[u8]) -> Vec<u8> {
    let p = CtPlace {
        patient: 1,
        study: 1,
        series: 900,
        instance: 0,
    };
    let mut els: Vec<El> = ct_header(p, 512, 512, 0.0)
        .into_iter()
        .filter(|e| {
            !matches!(
                e.0,
                0x0020_0032
                    | 0x0020_0037
                    | 0x0028_0030
                    | 0x0018_0050
                    | 0x0020_1041
                    | 0x0028_1050
                    | 0x0028_1051
                    | 0x0028_1052
                    | 0x0028_1053
                    | 0x0028_1054
                    | 0x0028_1055
                    | 0x0008_0016
                    | 0x0008_0008
            )
        })
        .collect();
    els.push(text(0x0008_0016, "UI", ENHANCED_CT_STORAGE));
    els.push(text(0x0008_0008, "CS", "ORIGINAL\\PRIMARY\\AXIAL\\NONE"));
    els.push(text(0x0028_0008, "IS", &frames.to_string()));
    els.push(seq(
        0x5200_9229,
        vec![vec![
            seq(
                0x0020_9116,
                vec![vec![text(0x0020_0037, "DS", "1\\0\\0\\0\\1\\0")]],
            ),
            seq(
                0x0028_9110,
                vec![vec![
                    text(0x0018_0050, "DS", "1"),
                    text(0x0028_0030, "DS", "0.7\\0.7"),
                ]],
            ),
        ]],
    ));
    let per_frame = (0..frames)
        .map(|i| {
            vec![
                seq(
                    0x0020_9111,
                    vec![vec![
                        ul(0x0020_9157, i as u32 + 1),
                        text(0x0018_9074, "DT", "20250101100510.250000"),
                        text(0x0020_9156, "US", ""),
                    ]],
                ),
                seq(
                    0x0020_9113,
                    vec![vec![text(
                        0x0020_0032,
                        "DS",
                        &format!("-179.2\\-179.2\\{}", i),
                    )]],
                ),
                seq(
                    0x0028_9132,
                    vec![vec![
                        text(0x0028_1050, "DS", "40"),
                        text(0x0028_1051, "DS", "400"),
                    ]],
                ),
                seq(
                    0x0028_9145,
                    vec![vec![
                        text(0x0028_1052, "DS", "-1024"),
                        text(0x0028_1053, "DS", "1"),
                        text(0x0028_1054, "LO", "HU"),
                    ]],
                ),
            ]
        })
        .collect();
    els.push(seq(0x5200_9230, per_frame));
    let mut pixels = Vec::with_capacity(frame_pixels.len() * frames);
    for _ in 0..frames {
        pixels.extend_from_slice(frame_pixels);
    }
    els.push(raw(0x7FE0_0010, "OW", &pixels));
    part10(
        ENHANCED_CT_STORAGE,
        &p.sop_instance(),
        EXPLICIT_LE,
        &body(els),
    )
}
pub fn rtstruct_file(rois: usize, contours: usize, points: usize) -> Vec<u8> {
    let p = CtPlace {
        patient: 1,
        study: 1,
        series: 800,
        instance: 0,
    };
    let frame_of_ref = uid(5, 1, 1);
    let mut els = vec![
        text(0x0008_0005, "CS", "ISO_IR 100"),
        text(0x0008_0016, "UI", RT_STRUCTURE_SET_STORAGE),
        text(0x0008_0018, "UI", &p.sop_instance()),
        text(0x0008_0060, "CS", "RTSTRUCT"),
        text(0x0010_0010, "PN", "PHANTOM^BENCH001"),
        text(0x0010_0020, "LO", "PID000001"),
        text(0x0020_000D, "UI", &uid(2, 1, 1)),
        text(0x0020_000E, "UI", &uid(3, 1, 800)),
        text(0x3006_0002, "SH", "BENCH"),
        text(0x3006_0008, "DA", "20250101"),
    ];
    let mut set_rois = Vec::new();
    let mut roi_contours = Vec::new();
    let mut observations = Vec::new();
    for r in 0..rois {
        set_rois.push(vec![
            text(0x3006_0022, "IS", &(r + 1).to_string()),
            text(0x3006_0024, "UI", &frame_of_ref),
            text(0x3006_0026, "LO", &format!("STRUCTURE_{r:02}")),
            text(0x3006_0036, "CS", "MANUAL"),
        ]);
        let mut items = Vec::new();
        for c in 0..contours {
            let z = c as f64 * 2.5 - 60.0;
            let rad = 20.0 + 3.0 * r as f64 + 5.0 * ((c as f64) * 0.3).sin();
            let mut data = String::with_capacity(points * 30);
            for k in 0..points {
                let a = k as f64 / points as f64 * std::f64::consts::TAU;
                if k > 0 {
                    data.push('\\');
                }
                data.push_str(&format!(
                    "{:.3}\\{:.3}\\{:.2}",
                    rad * a.cos() + r as f64 * 4.0,
                    rad * a.sin() - 10.0,
                    z
                ));
            }
            items.push(vec![
                seq(
                    0x3006_0016,
                    vec![vec![
                        text(0x0008_1150, "UI", CT_IMAGE_STORAGE),
                        text(0x0008_1155, "UI", &uid(4, 1, c as u64)),
                    ]],
                ),
                text(0x3006_0042, "CS", "CLOSED_PLANAR"),
                text(0x3006_0046, "IS", &points.to_string()),
                text(0x3006_0050, "DS", &data),
            ]);
        }
        roi_contours.push(vec![
            text(
                0x3006_002A,
                "IS",
                &format!("{}\\{}\\{}", (r * 37) % 256, (r * 91) % 256, (r * 53) % 256),
            ),
            seq(0x3006_0040, items),
            text(0x3006_0084, "IS", &(r + 1).to_string()),
        ]);
        observations.push(vec![
            text(0x3006_0082, "IS", &(r + 1).to_string()),
            text(0x3006_0084, "IS", &(r + 1).to_string()),
            text(0x3006_00A4, "CS", "ORGAN"),
        ]);
    }
    els.push(seq(0x3006_0020, set_rois));
    els.push(seq(0x3006_0039, roi_contours));
    els.push(seq(0x3006_0080, observations));
    part10(
        RT_STRUCTURE_SET_STORAGE,
        &p.sop_instance(),
        EXPLICIT_LE,
        &body(els),
    )
}
