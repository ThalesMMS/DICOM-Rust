#![cfg(feature = "render")]
#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::object::{File, Vr};
use dicom_rust::pixel::extract::render_frame;
use dicom_rust::pixel::frame::FrameImage;
use dicom_rust::pixel::render_single::render_single_frame;
use dicom_rust::render::window::{WindowLevel, windowed_gray};
fn gray(img: FrameImage) -> (usize, usize, Vec<u8>) {
    match img {
        FrameImage::Gray(g) => (g.width, g.height, g.pixels),
        FrameImage::Rgba(_) => panic!("colour output for a grayscale frame"),
    }
}
fn with_window(
    mut ds: dicom_rust::object::Object,
    center: &str,
    width: &str,
    slope: &str,
    intercept: &str,
) -> File {
    ds.put(text(0x0028, 0x1050, Vr::DS, center));
    ds.put(text(0x0028, 0x1051, Vr::DS, width));
    ds.put(text(0x0028, 0x1053, Vr::DS, slope));
    ds.put(text(0x0028, 0x1052, Vr::DS, intercept));
    write_and_read(ds, EXPLICIT_LE).1
}
#[test]
fn grayscale_frames_follow_the_window() {
    for (allocated, bits, center, width, slope, intercept) in [
        (8, 8, 128.0, 256.0, 1.0, 0.0),
        (8, 8, 60.0, 40.0, 1.0, 0.0),
        (16, 12, 1000.0, 2000.0, 1.0, -1024.0),
        (16, 12, 40.0, 400.0, 2.0, -2048.0),
    ] {
        let (rows, cols) = (7, 9);
        let f = with_window(
            image(rows, cols, 1, allocated, bits),
            &center.to_string(),
            &width.to_string(),
            &slope.to_string(),
            &intercept.to_string(),
        );
        let window = WindowLevel::new(center, width);
        let stored = samples(rows, cols, 1, bits);
        for img in [render_frame(&f, 0), render_single_frame(&f, 0)] {
            let (w, h, pixels) = gray(img.expect("render"));
            assert_eq!((w, h), (cols, rows));
            let want: Vec<u8> = stored
                .iter()
                .map(|&s| windowed_gray(s as f64 * slope + intercept, &window))
                .collect();
            assert_eq!(
                pixels, want,
                "bits {bits} window {center}/{width} rescale {slope}/{intercept}"
            );
        }
    }
    let w = WindowLevel::new(100.0, 50.0);
    assert_eq!(windowed_gray(0.0, &w), 0);
    assert_eq!(windowed_gray(1000.0, &w), 255);
    assert!(
        (129..=131).contains(&windowed_gray(100.0, &w)),
        "{}",
        windowed_gray(100.0, &w)
    );
}
#[test]
fn monochrome1_is_inverted() {
    let mut ds = image(4, 6, 1, 8, 8);
    let normal = gray(render_frame(&with_window(ds.clone(), "128", "256", "1", "0"), 0).unwrap()).2;
    ds.put(text(0x0028, 0x0004, Vr::CS, "MONOCHROME1"));
    let inverted = gray(render_frame(&with_window(ds, "128", "256", "1", "0"), 0).unwrap()).2;
    for (a, b) in normal.iter().zip(&inverted) {
        assert!((255 - *a as i32 - *b as i32).abs() <= 1, "{a} {b}");
    }
}
#[test]
fn colour_frames_keep_their_samples() {
    let (rows, cols) = (5, 3);
    let f = write_and_read(image(rows, cols, 3, 8, 8), EXPLICIT_LE).1;
    let rgb = samples(rows, cols, 3, 8);
    match render_frame(&f, 0).expect("render") {
        FrameImage::Rgba(img) => {
            assert_eq!((img.width, img.height), (cols, rows));
            for (i, px) in img.pixels.chunks(4).enumerate() {
                assert_eq!(
                    &px[..3],
                    &[rgb[3 * i] as u8, rgb[3 * i + 1] as u8, rgb[3 * i + 2] as u8]
                );
                assert_eq!(px[3], 255);
            }
        }
        FrameImage::Gray(_) => panic!("gray output for RGB"),
    }
}
#[test]
fn each_frame_of_a_multi_frame_file_renders() {
    let (rows, cols) = (4, 4);
    let one = samples(rows, cols, 1, 8);
    let mut pixels: Vec<u8> = one.iter().map(|&s| s as u8).collect();
    pixels.extend(one.iter().map(|&s| 255 - s as u8));
    let mut ds = image(rows, cols, 1, 8, 8);
    ds.put(raw(0x7FE0, 0x0010, Vr::OB, pixels.clone()));
    ds.put(text(0x0028, 0x0008, Vr::IS, "2"));
    let f = with_window(ds, "127.5", "256", "1", "0");
    let first = gray(render_frame(&f, 0).unwrap()).2;
    let second = gray(render_frame(&f, 1).unwrap()).2;
    assert_ne!(first, second);
    for (a, b) in first.iter().zip(&second) {
        assert!((*a as i32 + *b as i32 - 255).abs() <= 2, "{a} {b}");
    }
    assert!(render_frame(&f, 2).is_err(), "no third frame");
}
