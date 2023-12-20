use crate::cli::ResizeStrategy;
use crate::imgproc::resize;
use fast_image_resize::FilterType;
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, DynamicImage, RgbImage};
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;
use utils::comp_decomp::BitPack;

#[inline]
pub fn frame_to_rgb(frame: image::Frame) -> RgbImage {
    DynamicImage::ImageRgba8(frame.into_buffer()).into_rgb8()
}

pub fn compress_frames(
    gif: GifDecoder<BufReader<File>>,
    dim: (u32, u32),
    filter: FilterType,
    resize: ResizeStrategy,
    color: &[u8; 3],
) -> Result<Vec<(BitPack, Duration)>, String> {
    let mut compressed_frames = Vec::new();
    let mut frames = gif.into_frames();

    // The first frame should always exist
    let first = frames.next().unwrap().unwrap();
    let first_duration = first.delay().numer_denom_ms();
    let first_duration = Duration::from_millis((first_duration.0 / first_duration.1).into());
    let first_img = match resize {
        ResizeStrategy::No => resize::img_pad(frame_to_rgb(first), dim, color)?,
        ResizeStrategy::Crop => resize::img_resize_crop(frame_to_rgb(first), dim, filter)?,
        ResizeStrategy::Fit => resize::img_resize_fit(frame_to_rgb(first), dim, filter, color)?,
    };

    let mut canvas: Option<Vec<u8>> = None;
    while let Some(Ok(frame)) = frames.next() {
        let (dur_num, dur_div) = frame.delay().numer_denom_ms();
        let duration = Duration::from_millis((dur_num / dur_div).into());

        let img = match resize {
            ResizeStrategy::No => resize::img_pad(frame_to_rgb(frame), dim, color)?,
            ResizeStrategy::Crop => resize::img_resize_crop(frame_to_rgb(frame), dim, filter)?,
            ResizeStrategy::Fit => resize::img_resize_fit(frame_to_rgb(frame), dim, filter, color)?,
        };

        compressed_frames.push((
            BitPack::pack(canvas.as_ref().unwrap_or(&first_img), &img)?,
            duration,
        ));
        canvas = Some(img);
    }
    //Add the first frame we got earlier:
    compressed_frames.push((BitPack::pack(&canvas.unwrap(), &first_img)?, first_duration));
    Ok(compressed_frames)
}
