use crate::support::{self, Outcome, Plane, Report};
use dicom_rust::codec::jpeg2000::j2kcore;
use std::path::Path;
type Bounds = &'static [(i64, f64)];
const EXACT: Bounds = &[];
const PROFILE0: &[(u32, i64, Bounds)] = &[
    (1, 0, EXACT),
    (2, 0, EXACT),
    (3, 0, EXACT),
    (4, 0, &[(5, 0.776), (4, 0.626), (6, 1.070)]),
    (5, 0, &[(2, 0.319), (2, 0.323), (2, 0.317), (0, 0.0)]),
    (
        6,
        0,
        &[(635, 11287.0), (403, 6124.0), (378, 3968.0), (0, 0.0)],
    ),
    (7, 0, EXACT),
    (8, 1, EXACT),
    (9, 0, EXACT),
    (10, 0, EXACT),
    (11, 0, EXACT),
    (12, 0, EXACT),
    (13, 0, EXACT),
    (14, 0, EXACT),
    (15, 0, EXACT),
    (16, 0, EXACT),
];
const PROFILE1: &[(u32, i64, Bounds)] = &[
    (1, 0, EXACT),
    (2, 0, &[(5, 0.765), (4, 0.616), (6, 1.051)]),
    (3, 0, &[(2, 0.311), (2, 0.280), (1, 0.267), (0, 0.0)]),
    (4, 0, &[(624, 3080.0)]),
    (5, 0, &[(40, 8.458), (40, 9.716), (40, 10.154)]),
    (6, 0, &[(2, 0.600), (2, 0.600), (2, 0.600)]),
    (7, 0, EXACT),
];
const HIGH_FIDELITY: &[(&str, Bounds)] = &[
    ("hifi_p1_02.j2k", &[(43, 80.0), (33, 62.0), (38, 72.0)]),
    ("hifi_ht1_02.j2k", &[(47, 82.0), (36, 65.0), (42, 86.0)]),
];
const HT_ALLOWANCE: &[(&str, Bounds)] = &[
    ("ds0_ht_02_b11.j2k", &[(1, 0.001)]),
    ("ds0_ht_03_b11.j2k", &[(17, 0.15)]),
    ("ds0_ht_04_b11.j2k", &[(2, 0.1), (2, 0.1), (2, 0.1)]),
    (
        "ds0_ht_05_b11.j2k",
        &[(0, 0.0), (1, 0.001), (1, 0.001), (2, 0.001)],
    ),
    (
        "ds0_ht_06_b11.j2k",
        &[(2500, 75000.0), (0, 0.0), (0, 0.0), (200, 2000.0)],
    ),
    (
        "ds0_ht_06_b15.j2k",
        &[(0, 0.0), (0, 0.0), (0, 0.0), (12, 10.0)],
    ),
    (
        "ds0_hm_06_b11.j2k",
        &[(0, 0.0), (0, 0.0), (0, 0.0), (200, 2000.0)],
    ),
    ("ds0_ht_07_b11.j2k", &[(40, 25.0), (40, 25.0), (50, 25.0)]),
    ("ds0_ht_07_b15.j2k", &[(2, 0.075), (2, 0.05), (2, 0.075)]),
    ("ds0_ht_08_b11.j2k", &[(40, 45.0), (40, 30.0), (40, 45.0)]),
    ("ds0_ht_08_b15.j2k", &[(0, 0.0), (1, 0.001), (0, 0.0)]),
    ("ds0_ht_15_b11.j2k", &[(17, 0.15)]),
    ("ds1_ht_01_b11.j2k", &[(1, 0.001)]),
    ("ds1_ht_02_b11.j2k", &[(2, 0.1), (2, 0.1), (2, 0.1)]),
    (
        "ds1_ht_03_b11.j2k",
        &[(0, 0.0), (1, 0.03), (2, 0.05), (2, 0.001)],
    ),
];
fn reason(_name: &str) -> Option<&'static str> {
    None
}
fn references(dir: &Path, prefix: &str) -> Vec<Plane> {
    let mut planes = Vec::new();
    loop {
        let path = dir.join(format!("{prefix}-{}.pgx", planes.len()));
        if !path.is_file() {
            return planes;
        }
        planes.push(
            support::read_pgx(&support::read(&path))
                .unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        );
    }
}
fn decode(stream: &[u8], reduce: i64) -> Result<Vec<Plane>, String> {
    let c = j2kcore::parse_codestream(stream).map_err(|e| e.to_string())?;
    let mut limits = j2kcore::Limits::defaults();
    limits.max_components = limits.max_components.max(16384);
    let options = j2kcore::DecodeOptions {
        reduce,
        ..Default::default()
    };
    let image = j2kcore::decode(&c, limits, options).map_err(|e| e.to_string())?;
    let ceil = |a: i64, b: i64| (a + b - 1).div_euclid(b);
    let finest = |pick: fn(&j2kcore::Component) -> i64| {
        c.components.iter().map(pick).min().unwrap_or(1).max(1) << reduce
    };
    let (sx, sy) = (finest(|k| k.dx), finest(|k| k.dy));
    let (x0, y0) = (i64::from(c.x_offset), i64::from(c.y_offset));
    let (x1, y1) = (i64::from(c.width), i64::from(c.height));
    let mut planes = Vec::new();
    for (component, samples) in c.components.iter().zip(&image.components) {
        let (dx, dy) = (component.dx << reduce, component.dy << reduce);
        let columns: Vec<usize> = (ceil(x0, dx)..ceil(x1, dx))
            .map(|cx| (ceil(cx * dx, sx) - ceil(x0, sx)).clamp(0, image.width - 1) as usize)
            .collect();
        let rows: Vec<usize> = (ceil(y0, dy)..ceil(y1, dy))
            .map(|cy| (ceil(cy * dy, sy) - ceil(y0, sy)).clamp(0, image.height - 1) as usize)
            .collect();
        let mut plane = Plane {
            width: columns.len(),
            height: rows.len(),
            samples: Vec::with_capacity(columns.len() * rows.len()),
        };
        for row in &rows {
            let line = &samples[row * image.width as usize..][..image.width as usize];
            plane.samples.extend(columns.iter().map(|x| line[*x]));
        }
        planes.push(plane);
    }
    Ok(planes)
}
fn run(report: &mut Report, path: &Path, reduce: i64, want: &[Plane], bounds: &[(i64, f64)]) {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!want.is_empty(), "{name}: no reference image");
    let outcome = match decode(&support::read(path), reduce) {
        Err(e) => Outcome::Refused(e),
        Ok(got) => {
            let peak: Vec<i64> = bounds.iter().map(|b| b.0).collect();
            let mse: Vec<f64> = bounds.iter().map(|b| b.1).collect();
            support::compare(&got, want, &peak, &mse)
        }
    };
    report.case(&name, outcome, reason(&name));
}
fn sum(base: &[(i64, f64)], extra: &[(i64, f64)], components: usize) -> Vec<(i64, f64)> {
    (0..components)
        .map(|i| {
            let b = base.get(i).copied().unwrap_or((0, 0.0));
            let e = extra.get(i).copied().unwrap_or((0, 0.0));
            (b.0 + e.0, b.1 + e.1)
        })
        .collect()
}
fn profile(report: &mut Report, dir: &Path, number: u32, table: &[(u32, i64, Bounds)]) {
    for (n, reduce, bounds) in table {
        let want = references(
            &dir.join(format!("reference_class1_profile{number}")),
            &format!("c1p{number}_{n:02}"),
        );
        run(
            report,
            &dir.join(format!("codestreams_profile{number}/p{number}_{n:02}.j2k")),
            *reduce,
            &want,
            bounds,
        );
        let bset = dir.join(format!("htj2k_bsets_profile{number}/p{number}_{n:02}_bset"));
        let mut derived: Vec<_> = std::fs::read_dir(&bset)
            .unwrap_or_else(|e| panic!("{}: {e}", bset.display()))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "j2k"))
            .collect();
        derived.sort();
        assert!(!derived.is_empty(), "{}: no codestream", bset.display());
        for path in derived {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let extra = HT_ALLOWANCE
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(EXACT, |(_, extra)| *extra);
            run(
                report,
                &path,
                *reduce,
                &want,
                &sum(bounds, extra, want.len()),
            );
        }
    }
}
#[test]
#[ignore = "needs the conformance cache: run scripts/ci.sh conformance"]
fn itu_t803_jpeg_2000() {
    let Some(dir) = support::set_dir("itu-t803") else {
        return;
    };
    let dir = dir.join("files");
    let mut report = Report::new("itu-t803 (JPEG 2000 and HTJ2K, class 1)");
    profile(&mut report, &dir, 0, PROFILE0);
    profile(&mut report, &dir, 1, PROFILE1);
    let want = references(&dir.join("reference_class1HF_profile1"), "hifi_02");
    for (name, bounds) in HIGH_FIDELITY {
        run(
            &mut report,
            &dir.join("codestreams_hifi").join(name),
            0,
            &want,
            bounds,
        );
    }
    report.finish();
}
