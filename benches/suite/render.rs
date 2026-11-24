use crate::harness::{Runner, Work};
use crate::inputs::Inputs;
use crate::volume;
use dicom_rust::render::frame::{Frame, render_frame};
use dicom_rust::render::mpr::{MprPlane, Plane};
use dicom_rust::render::mpr_render::{
    MprLimits, MprRenderOptions, SlabMode, render_mpr_plane, render_slab, reslice_oblique,
    reslice_oblique_slab,
};
use dicom_rust::render::vec3::Vec3;
use dicom_rust::render::vr_camera::VrCamera;
use dicom_rust::render::vr_quality::VrQuality;
use dicom_rust::render::vr_raycast::{render_vr_prepared, render_vr_prepared_preview};
use dicom_rust::render::vr_transfer::{VrPrefilterId, default_vr_preset};
use dicom_rust::render::{Rescale, WindowLevel};
use std::sync::Arc;
pub const NAMES: &[&str] = &[
    "render/window_level/ct512",
    "render/window_level/dx2048",
    "render/mpr_plane/coronal/ct512x300",
    "render/mpr_plane/sagittal/ct512x300",
    "render/reslice_oblique/512/ct512x300",
    "render/slab_mip_oblique/512x20/ct512x300",
    "render/slab_mip_axial/512x20/ct512x300",
    "render/vr_prepare_gaussian/ct512x300",
    "render/vr_first_preview/256/ct512x300",
    "render/vr_shaded/512/ct512x300",
    "render/vr_preview/256/ct512x300",
];
fn soft_tissue() -> WindowLevel {
    WindowLevel::new(40.0, 400.0)
}
fn oblique(v: &dicom_rust::render::volume::Volume) -> Plane {
    let c = v.center();
    let (a, b) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let width = v.cols as f64 * v.col_spacing;
    let height = v.depth as f64 * v.slice_spacing;
    let u = Vec3::new(width, 0.0, 0.0);
    let w = Vec3::new(0.0, height * b, height * a);
    Plane {
        origin: Vec3::new(
            c.x - u.x / 2.0 - w.x / 2.0,
            c.y - u.y / 2.0 - w.y / 2.0,
            c.z - u.z / 2.0 - w.z / 2.0,
        ),
        u,
        v: w,
    }
}
pub fn run(r: &mut Runner, inp: &Inputs) {
    if !r.any_selected(NAMES) {
        return;
    }
    let stack = volume::ct_stack(inp);
    let ct_frame = stack.frames[stack.frames.len() / 2].clone().expect("frame");
    let wl = soft_tissue();
    r.bench("render/window_level/ct512", Work::Pixels(512 * 512), || {
        render_frame(Some(&ct_frame), &wl)
    });
    let dx = inp.dx();
    let dx_frame = Frame {
        metadata: crate::codecs::pixel_md(&crate::codecs::md(2048, 2048, 1, 12, "MONOCHROME2")),
        pixel_bytes: Arc::new(dx.le_bytes()),
        rescale: Rescale {
            slope: 1.0,
            intercept: 0.0,
        },
        default_window: WindowLevel::new(2048.0, 4096.0),
        ..Frame::default()
    };
    let dx_wl = WindowLevel::new(2200.0, 1800.0);
    r.bench(
        "render/window_level/dx2048",
        Work::Pixels(2048 * 2048),
        || render_frame(Some(&dx_frame), &dx_wl),
    );
    let opts = MprRenderOptions {
        limits: MprLimits::defaults(),
        ..Default::default()
    };
    r.bench(
        "render/mpr_plane/coronal/ct512x300",
        Work::Pixels(512 * 300),
        || render_mpr_plane(&stack, MprPlane::Coronal, 256, &wl, opts),
    );
    r.bench(
        "render/mpr_plane/sagittal/ct512x300",
        Work::Pixels(512 * 300),
        || render_mpr_plane(&stack, MprPlane::Sagittal, 256, &wl, opts),
    );
    let vol = volume::volume_with_voxels(&stack);
    let plane = oblique(&vol);
    r.bench(
        "render/reslice_oblique/512/ct512x300",
        Work::Pixels(512 * 512),
        || reslice_oblique(Some(&vol), &plane, 512, 512, &wl, MprLimits::defaults()),
    );
    r.bench(
        "render/slab_mip_oblique/512x20/ct512x300",
        Work::Pixels(512 * 512),
        || {
            reslice_oblique_slab(
                Some(&vol),
                &plane,
                512,
                512,
                dicom_rust::render::mpr_render::ObliqueSlab {
                    thickness: 20,
                    mode: SlabMode::Mip,
                },
                &wl,
                MprLimits::defaults(),
            )
        },
    );
    r.bench(
        "render/slab_mip_axial/512x20/ct512x300",
        Work::Pixels(512 * 512),
        || render_slab(&stack, MprPlane::Axial, 150, 20, SlabMode::Mip, &wl, opts),
    );
    let preset = default_vr_preset();
    r.bench(
        "render/vr_prepare_gaussian/ct512x300",
        Work::Pixels(512 * 512 * 300),
        || {
            vol.prepare_vr_volume(None, VrPrefilterId::GAUSSIAN_1MM)
                .expect("prepare")
        },
    );
    let mut cam = VrCamera::new(1.0);
    cam.fit_volume(Some(&vol));
    let vr_wl = WindowLevel::default();
    let q = VrQuality::preview(256, 256);
    r.bench(
        "render/vr_first_preview/256/ct512x300",
        Work::Pixels(256 * 256),
        || {
            let prepared = vol
                .prepare_vr_volume(None, preset.prefilter)
                .expect("prepare");
            render_vr_prepared_preview(None, Some(&prepared), &cam, &preset, &vr_wl, None, &q)
        },
    );
    let prepared = vol
        .prepare_vr_volume(None, preset.prefilter)
        .expect("prepare");
    let q = VrQuality::default_quality(512, 512);
    r.bench(
        "render/vr_shaded/512/ct512x300",
        Work::Pixels(512 * 512),
        || {
            render_vr_prepared(
                None,
                Some(&prepared),
                &dicom_rust::render::VrRenderSettings {
                    camera: &cam,
                    preset: &preset,
                    window: &vr_wl,
                    shading: true,
                    clip: None,
                    quality: &q,
                },
            )
        },
    );
    let q = VrQuality::preview(256, 256);
    r.bench(
        "render/vr_preview/256/ct512x300",
        Work::Pixels(256 * 256),
        || render_vr_prepared_preview(None, Some(&prepared), &cam, &preset, &vr_wl, None, &q),
    );
}
pub fn check(inp: &Inputs) -> Result<(), String> {
    let stack = volume::ct_stack(inp);
    let frame = stack.frames[stack.frames.len() / 2]
        .clone()
        .ok_or("no frame")?;
    let (img, err) = render_frame(Some(&frame), &soft_tissue());
    if let Some(e) = err {
        return Err(format!("window/level: {e}"));
    }
    if img.size() != (512, 512) {
        return Err(format!("window/level image {:?}", img.size()));
    }
    let opts = MprRenderOptions {
        limits: MprLimits::defaults(),
        ..Default::default()
    };
    let (img, err) = render_mpr_plane(&stack, MprPlane::Coronal, 256, &soft_tissue(), opts);
    if let Some(e) = err {
        return Err(format!("coronal: {e}"));
    }
    if img.pixels().iter().all(|&p| p == img.pixels()[0]) {
        return Err("coronal plane is uniform".into());
    }
    let (img, err) = render_slab(
        &stack,
        MprPlane::Axial,
        150,
        20,
        SlabMode::Mip,
        &soft_tissue(),
        opts,
    );
    if let Some(e) = err {
        return Err(format!("mip slab: {e}"));
    }
    if img.size() != (512, 512) {
        return Err(format!("mip image {:?}", img.size()));
    }
    let vol = volume::volume_with_voxels(&stack);
    let (_, err) = reslice_oblique(
        Some(&vol),
        &oblique(&vol),
        512,
        512,
        &soft_tissue(),
        MprLimits::defaults(),
    );
    if let Some(e) = err {
        return Err(format!("oblique: {e}"));
    }
    let preset = default_vr_preset();
    let prepared = vol
        .prepare_vr_volume(None, preset.prefilter)
        .map_err(|e| e.to_string())?;
    let mut cam = VrCamera::new(1.0);
    cam.fit_volume(Some(&vol));
    let (img, err) = render_vr_prepared_preview(
        None,
        Some(&prepared),
        &cam,
        &preset,
        &WindowLevel::default(),
        None,
        &VrQuality::preview(64, 64),
    );
    if let Some(e) = err {
        return Err(format!("vr: {e}"));
    }
    if img.is_none() {
        return Err("vr: no image".into());
    }
    Ok(())
}
