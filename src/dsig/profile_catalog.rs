use super::profile::Profile;
pub(super) struct ProfileModule {
    pub name: &'static str,
    pub table: &'static str,
    pub profiles: &'static [Profile],
    pub tags: Option<&'static [u32]>,
}
const C: Profile = Profile::CreatorRsa;
const A: Profile = Profile::AuthorizationRsa;
const S: Profile = Profile::SrRsa;
#[rustfmt::skip]
pub(super) static PROFILE_MODULES: &[ProfileModule] = &[
    ProfileModule { name: "General Equipment Module", table: "C.7-8", profiles: &[C, S], tags: None },
    ProfileModule { name: "General Image Module", table: "C.7-9", profiles: &[C, A], tags: None },
    ProfileModule { name: "Image Pixel Module", table: "C.7-11a", profiles: &[C, A], tags: None },
    ProfileModule { name: "Graphic Annotation Module", table: "C.10-5", profiles: &[C, A], tags: None },
    ProfileModule { name: "SR Document General Module", table: "C.17-2", profiles: &[C, A], tags: None },
    ProfileModule { name: "SR Document Content Module", table: "C.17-4", profiles: &[C, A, S], tags: None },
    ProfileModule { name: "Waveform Module", table: "C.10-9", profiles: &[C, A], tags: None },
    ProfileModule { name: "Waveform Annotation Module", table: "C.10-11", profiles: &[C, A], tags: None },
    ProfileModule { name: "Multi-frame Functional Groups Module", table: "C.7.6.16-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Enhanced MR Image Module", table: "C.8-79", profiles: &[C, A], tags: None },
    ProfileModule { name: "MR Spectroscopy Module", table: "C.8-102", profiles: &[C, A], tags: None },
    ProfileModule { name: "Raw Data Module", table: "C.19-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Enhanced CT Image Module", table: "C.8-114", profiles: &[C, A], tags: None },
    ProfileModule { name: "Enhanced XA/XRF Image Module", table: "C.8.19.2-1", profiles: &[C, A], tags: Some(&[0x00080008, 0x0008002A, 0x0008114A, 0x00089092, 0x00089154, 0x00089410, 0x00109431, 0x00180022, 0x00189004, 0x00189410, 0x00189423, 0x00189424, 0x00189457, 0x00200012, 0x00204000, 0x00280002, 0x00280004, 0x00280100, 0x00280101, 0x00280102, 0x00280103, 0x00280300, 0x00280301, 0x00280302, 0x00282110, 0x00282112, 0x00282114, 0x00540410, 0x00540414, 0x00880200, 0x20500020]) },
    ProfileModule { name: "Segmentation Image Module", table: "C.8.20-2", profiles: &[C, A], tags: None },
    ProfileModule { name: "Encapsulated Document Series Module", table: "C.24-1", profiles: &[C], tags: None },
    ProfileModule { name: "Encapsulated Document Module", table: "C.24-2", profiles: &[A], tags: None },
    ProfileModule { name: "X-Ray 3D Image Module", table: "C.8.21.1-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Enhanced PET Image Module", table: "C.8.22-3", profiles: &[C, A], tags: None },
    ProfileModule { name: "Enhanced US Image Module", table: "C.8.24.3-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Surface Segmentation Module", table: "C.8.23-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Surface Mesh Module", table: "C.27-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Structured Display Module", table: "C.11.16-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Structured Display Image Box Module", table: "C.11.17-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Structured Display Annotation Module", table: "C.11.18-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Generic Implant Template Description Module", table: "C.29.1.1-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Generic Implant Template 2D Drawings Module", table: "C.29.1.2-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Generic Implant Template 3D Models Module", table: "C.29.1.3-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Generic Implant Template Mating Features Module", table: "C.29.1.4-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Generic Implant Template Planning Landmarks Module", table: "C.29.1.5-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Implant Assembly Template Module", table: "C.29.2-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Implant Template Group Module", table: "C.29.3-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Point Cloud Module", table: "C.27.5-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Enhanced Mammography Image Module", table: "C.8.31-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Tractography Results Series Module", table: "C.8.33-1", profiles: &[C], tags: None },
    ProfileModule { name: "Tractography Results Module", table: "C.8.33-2", profiles: &[C], tags: None },
    ProfileModule { name: "Volumetric Graphic Annotation Module", table: "C.11.28-1", profiles: &[C, A], tags: None },
    ProfileModule { name: "Microscopy Bulk Simple Annotations Module", table: "C.37.1-2", profiles: &[C], tags: None },
    ProfileModule { name: "Waveform Presentation State Relationship Module", table: "C.39.1-1", profiles: &[C], tags: Some(&[0x00081115]) },
    ProfileModule { name: "Structured Waveform Annotation Module", table: "C.39.2-1", profiles: &[C], tags: None },
    ProfileModule { name: "Textual Waveform Annotation Module", table: "C.39.3-1", profiles: &[C], tags: None },
    ProfileModule { name: "Displayed Waveform Segment Module", table: "C.39.4-1", profiles: &[C], tags: None },
    ProfileModule { name: "Montage Activation Module", table: "C.39.5-1", profiles: &[C], tags: None },
    ProfileModule { name: "Waveform Presentation Montage Module", table: "C.39.6-1", profiles: &[C], tags: None },
    ProfileModule { name: "MR Spectroscopy Pulse Sequence Module", table: "C.8-103", profiles: &[A], tags: None },
    ProfileModule { name: "MR Spectroscopy Data Module", table: "C.8-106", profiles: &[A], tags: None },
];
#[rustfmt::skip]
pub(super) const OVERLAY_PROFILE_ELEMENTS: &[u16] = &[0x0010, 0x0011, 0x0022, 0x0040, 0x0045, 0x0050, 0x0100, 0x0102, 0x1301, 0x1302, 0x1303, 0x1500, 0x3000];
#[rustfmt::skip]
pub(super) const CURVE_PROFILE_ELEMENTS: &[u16] = &[0x0005, 0x0010, 0x0020, 0x0103, 0x3000, 0x0022, 0x0030, 0x0040, 0x0104, 0x0105, 0x0106, 0x0110, 0x0112, 0x0114, 0x2500, 0x2600];
