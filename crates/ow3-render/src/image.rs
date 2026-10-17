//! A plain RGBA image and PNG output.

use std::io::Write;

/// 8-bit RGBA pixels, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Image {
        Image {
            width,
            height,
            rgba: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    pub fn set(&mut self, x: u32, y: u32, color: [u8; 4]) {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba[i..i + 4].copy_from_slice(&color);
    }

    /// Draws `color` over the pixel with its alpha.
    pub fn blend(&mut self, x: u32, y: u32, color: [u8; 4]) {
        let under = self.pixel(x, y);
        let a = u32::from(color[3]);
        let mix = |top: u8, bottom: u8| {
            ((u32::from(top) * a + u32::from(bottom) * (255 - a)) / 255) as u8
        };
        let alpha = (a + u32::from(under[3]) * (255 - a) / 255) as u8;
        self.set(
            x,
            y,
            [
                mix(color[0], under[0]),
                mix(color[1], under[1]),
                mix(color[2], under[2]),
                alpha,
            ],
        );
    }

    /// The `size` x `size` square at (`x`, `y`) scaled to `out` x `out`
    /// pixels by averaging.
    pub fn scaled_square(&self, x: u32, y: u32, size: u32, out: u32) -> Vec<[u8; 4]> {
        let step = size / out;
        let mut pixels = Vec::with_capacity((out * out) as usize);
        for oy in 0..out {
            for ox in 0..out {
                let mut sum = [0u32; 4];
                for dy in 0..step {
                    for dx in 0..step {
                        let p = self.pixel(x + ox * step + dx, y + oy * step + dy);
                        for c in 0..4 {
                            sum[c] += u32::from(p[c]);
                        }
                    }
                }
                let n = step * step;
                pixels.push(sum.map(|s| (s / n) as u8));
            }
        }
        pixels
    }

    pub fn write_png(&self, out: impl Write) -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)
    }
}

impl From<ow3_formats::blp::Image> for Image {
    fn from(image: ow3_formats::blp::Image) -> Image {
        Image {
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        }
    }
}
