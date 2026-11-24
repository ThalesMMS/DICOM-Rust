use dicom_rust::object;
use dicom_rust::pixel::extract::render_frame;
use dicom_rust::pixel::frame::FrameImage;
use dicom_rust::render::png_encode::{PngSource, encode_png};
use std::error::Error;
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let input = args.next().unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data/ct_small.dcm").to_string()
    });
    let output = args.next().unwrap_or_else(|| {
        std::env::temp_dir()
            .join("ct_small.png")
            .display()
            .to_string()
    });
    let file = object::open_file(&input).map_err(|(_, e)| e)?;
    let image = render_frame(&file, 0)?;
    let png = match &image {
        FrameImage::Gray(g) => {
            println!("frame 0: {} x {} gray", g.width, g.height);
            encode_png(PngSource::Gray(&g.pixels), g.width, g.height)?
        }
        FrameImage::Rgba(c) => {
            println!("frame 0: {} x {} RGBA", c.width, c.height);
            encode_png(PngSource::Nrgba(&c.pixels), c.width, c.height)?
        }
    };
    std::fs::write(&output, &png)?;
    println!("wrote {output} ({} bytes)", png.len());
    Ok(())
}
