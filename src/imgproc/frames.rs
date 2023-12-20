use crate::imgproc::resize::ResizeOperation;
use image::{DynamicImage, Frame, RgbImage};
use std::time::Duration;
use utils::comp_decomp::BitPack;

#[inline]
pub fn frame_to_rgb(frame: Frame) -> RgbImage {
    DynamicImage::ImageRgba8(frame.into_buffer()).into_rgb8()
}

#[inline]
pub fn frame_duration(frame: &Frame) -> Duration {
    let (dur_num, dur_div) = frame.delay().numer_denom_ms();
    Duration::from_millis((dur_num / dur_div).into())
}

pub struct FrameCompressor {
    pub outputs: Vec<String>,
    pub first_img: Vec<u8>,
    first_duration: Duration,
    canvas: Option<Vec<u8>>,
    pub resize_operation: ResizeOperation,
    compressed_frames: Vec<(BitPack, Duration)>,
}

impl FrameCompressor {
    pub fn new(
        outputs: Vec<String>,
        first: &Frame,
        resize_operation: ResizeOperation,
    ) -> Result<Self, String> {
        Ok(Self {
            outputs,
            first_img: resize_operation.resize(&frame_to_rgb(first.clone()))?,
            first_duration: frame_duration(first),
            canvas: None,
            resize_operation,
            compressed_frames: Vec::new(),
        })
    }

    pub fn add_frame(&mut self, frame: &Frame) -> Result<(), String> {
        let duration = frame_duration(frame);
        let img = self.resize_operation.resize(&frame_to_rgb(frame.clone()))?;
        self.compressed_frames.push((
            BitPack::pack(self.canvas.as_ref().unwrap_or(&self.first_img), &img)?,
            duration,
        ));
        self.canvas = Some(img);
        Ok(())
    }

    pub fn done(mut self) -> Result<Vec<(BitPack, Duration)>, String> {
        //Add the first frame we got earlier:
        self.compressed_frames.push((
            BitPack::pack(&self.canvas.unwrap(), &self.first_img)?,
            self.first_duration,
        ));
        Ok(self.compressed_frames)
    }
}
