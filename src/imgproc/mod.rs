pub mod frames;
pub mod imgbuf;
pub mod resize;
pub mod transition;

use super::cli;

pub fn make_filter(filter: &cli::Filter) -> fast_image_resize::FilterType {
    match filter {
        cli::Filter::Nearest => fast_image_resize::FilterType::Box,
        cli::Filter::Bilinear => fast_image_resize::FilterType::Bilinear,
        cli::Filter::CatmullRom => fast_image_resize::FilterType::CatmullRom,
        cli::Filter::Mitchell => fast_image_resize::FilterType::Mitchell,
        cli::Filter::Lanczos3 => fast_image_resize::FilterType::Lanczos3,
    }
}

/// Convert an RGB &[u8] to BRG in-place by swapping bytes
#[inline]
fn rgb_to_brg(rgb: &mut [u8]) {
    for pixel in rgb.chunks_exact_mut(3) {
        pixel.swap(0, 2);
    }
}
