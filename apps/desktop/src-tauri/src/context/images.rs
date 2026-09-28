//! Image scaling, annotation and encoding for context packs.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{ImageFormat, Rgba, RgbaImage};

use crate::platform::ImageLimit;

const MARK: Rgba<u8> = Rgba([255, 45, 85, 255]);
const HALO: Rgba<u8> = Rgba([255, 255, 255, 255]);

/// Makes sure `img` fits `limit`. Screenshots normally arrive at size; this
/// only guards against ones that don't.
pub fn fit_within(img: RgbaImage, limit: ImageLimit) -> RgbaImage {
    let (w, h) = limit.fit(f64::from(img.width()), f64::from(img.height()));
    if (w, h) == img.dimensions() {
        img
    } else {
        imageops::resize(&img, w, h, FilterType::Triangle)
    }
}

/// A 2 px high-contrast outline (red with a white halo) just outside `rect`,
/// given in pixels as (x, y, width, height).
pub fn outline(img: &mut RgbaImage, rect: (i64, i64, i64, i64)) {
    let (x, y, w, h) = rect;
    stroke(img, (x - 3, y - 3, w + 6, h + 6), 1, HALO);
    stroke(img, (x - 2, y - 2, w + 4, h + 4), 2, MARK);
}

/// A small dot with a white ring where the cursor was.
pub fn cursor_marker(img: &mut RgbaImage, x: i64, y: i64) {
    disc(img, x, y, 6, HALO);
    disc(img, x, y, 4, MARK);
}

pub fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .expect("encoding a PNG in memory can't fail");
    out.into_inner()
}

pub fn encode_jpeg(img: &RgbaImage, quality: u8) -> Vec<u8> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&rgb)
        .expect("encoding a JPEG in memory can't fail");
    out
}

fn stroke(img: &mut RgbaImage, rect: (i64, i64, i64, i64), thickness: i64, color: Rgba<u8>) {
    let (x, y, w, h) = rect;
    for t in 0..thickness {
        for i in x..x + w {
            put(img, i, y + t, color);
            put(img, i, y + h - 1 - t, color);
        }
        for j in y..y + h {
            put(img, x + t, j, color);
            put(img, x + w - 1 - t, j, color);
        }
    }
}

fn disc(img: &mut RgbaImage, cx: i64, cy: i64, radius: i64, color: Rgba<u8>) {
    for j in -radius..=radius {
        for i in -radius..=radius {
            if i * i + j * j <= radius * radius {
                put(img, cx + i, cy + j, color);
            }
        }
    }
}

fn put(img: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
        img.put_pixel(x as u32, y as u32, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_within_only_touches_images_over_the_limit() {
        let limit = ImageLimit {
            max_long_edge: 100,
            max_pixels: u64::MAX,
        };
        let small = fit_within(RgbaImage::new(80, 40), limit);
        assert_eq!(small.dimensions(), (80, 40));
        let big = fit_within(RgbaImage::new(400, 100), limit);
        assert_eq!(big.dimensions(), (100, 25));
    }

    #[test]
    fn annotations_stay_inside_the_image() {
        let mut img = RgbaImage::new(20, 20);
        outline(&mut img, (-50, -50, 200, 200));
        cursor_marker(&mut img, 19, 19);
        assert_eq!(*img.get_pixel(19, 19), MARK);
    }

    #[test]
    fn outline_marks_just_outside_the_rectangle() {
        let mut img = RgbaImage::new(40, 40);
        outline(&mut img, (10, 10, 20, 20));
        assert_eq!(*img.get_pixel(9, 15), MARK);
        assert_eq!(*img.get_pixel(7, 15), HALO);
        assert_eq!(
            *img.get_pixel(15, 15),
            Rgba([0, 0, 0, 0]),
            "inside is untouched"
        );
    }

    #[test]
    fn encoders_produce_their_formats() {
        let img = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        assert!(encode_png(&img).starts_with(b"\x89PNG"));
        assert!(encode_jpeg(&img, 80).starts_with(&[0xFF, 0xD8]));
    }
}
