//! Draws the app's icon from the logo. `icon OUTPUT.png PIXELS`
use olc_app::ui::logo::render;
use std::fs::File;
use std::io::BufWriter;

const ANGLE: f32 = 0.4;

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let pixels: usize = words[1].parse().expect("a size");
    let image = render(ANGLE, pixels);
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect();
    let file = BufWriter::new(File::create(&words[0]).expect("a file"));
    let mut encoder = png::Encoder::new(file, pixels as u32, pixels as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a header");
    writer.write_image_data(&bytes).expect("the image");
}
