use crate::imgproc;
use fast_image_resize::{FilterType, PixelType, Resizer};
use image::RgbImage;
use std::num::NonZeroU32;

pub fn img_pad(
    mut img: RgbImage,
    dimensions: (u32, u32),
    color: &[u8; 3],
) -> Result<Vec<u8>, String> {
    let (padded_w, padded_h) = dimensions;
    let (padded_w, padded_h) = (padded_w as usize, padded_h as usize);
    let mut padded = Vec::with_capacity(padded_h * padded_w * 3);

    let img = image::imageops::crop(&mut img, 0, 0, dimensions.0, dimensions.1).to_image();
    let (img_w, img_h) = img.dimensions();
    let (img_w, img_h) = (img_w as usize, img_h as usize);
    let raw_img = img.into_vec();

    for _ in 0..(((padded_h - img_h) / 2) * padded_w) {
        padded.push(color[2]);
        padded.push(color[1]);
        padded.push(color[0]);
    }

    // Calculate left and right border widths. `u32::div` rounds toward 0, so, if `img_w` is odd,
    // add an extra pixel to the right border to ensure the row is the correct width.
    let left_border_w = (padded_w - img_w) / 2;
    let right_border_w = left_border_w + (img_w % 2);

    for row in 0..img_h {
        for _ in 0..left_border_w {
            padded.push(color[2]);
            padded.push(color[1]);
            padded.push(color[0]);
        }

        for pixel in raw_img[(row * img_w * 3)..((row + 1) * img_w * 3)].chunks_exact(3) {
            padded.push(pixel[2]);
            padded.push(pixel[1]);
            padded.push(pixel[0]);
        }
        for _ in 0..right_border_w {
            padded.push(color[2]);
            padded.push(color[1]);
            padded.push(color[0]);
        }
    }

    while padded.len() < (padded_h * padded_w * 3) {
        padded.push(color[2]);
        padded.push(color[1]);
        padded.push(color[0]);
    }

    Ok(padded)
}

/// Resize an image to fit within the given dimensions, covering as much space as possible without
/// cropping.
pub fn img_resize_fit(
    img: RgbImage,
    dimensions: (u32, u32),
    filter: FilterType,
    padding_color: &[u8; 3],
) -> Result<Vec<u8>, String> {
    let (width, height) = dimensions;
    let (img_w, img_h) = img.dimensions();
    if (img_w, img_h) != (width, height) {
        // if our image is already scaled to fit, skip resizing it and just pad it directly
        if img_w == width || img_h == height {
            return img_pad(img, dimensions, padding_color);
        }

        let ratio = width as f32 / height as f32;
        let img_r = img_w as f32 / img_h as f32;

        let (trg_w, trg_h) = if ratio > img_r {
            let scale = height as f32 / img_h as f32;
            ((img_w as f32 * scale) as u32, height)
        } else {
            let scale = width as f32 / img_w as f32;
            (width, (img_h as f32 * scale) as u32)
        };

        let src = match fast_image_resize::Image::from_vec_u8(
            // We unwrap below because we know the images's dimensions should never be 0
            NonZeroU32::new(img_w).unwrap(),
            NonZeroU32::new(img_h).unwrap(),
            img.into_raw(),
            PixelType::U8x3,
        ) {
            Ok(i) => i,
            Err(e) => return Err(e.to_string()),
        };

        // We unwrap below because we know the outputs's dimensions should never be 0
        let new_w = NonZeroU32::new(trg_w).unwrap();
        let new_h = NonZeroU32::new(trg_h).unwrap();

        let mut dst = fast_image_resize::Image::new(new_w, new_h, PixelType::U8x3);
        let mut dst_view = dst.view_mut();

        let mut resizer = Resizer::new(fast_image_resize::ResizeAlg::Convolution(filter));
        if let Err(e) = resizer.resize(&src.view(), &mut dst_view) {
            return Err(e.to_string());
        }

        img_pad(
            image::RgbImage::from_raw(trg_w, trg_h, dst.into_vec()).unwrap(),
            dimensions,
            padding_color,
        )
    } else {
        let mut res = img.into_vec();
        // The ARGB is 'little endian', so here we must  put the order
        // of bytes 'in reverse', so it needs to be BGRA.
        imgproc::rgb_to_brg(&mut res);
        Ok(res)
    }
}

pub fn img_resize_crop(
    img: RgbImage,
    dimensions: (u32, u32),
    filter: FilterType,
) -> Result<Vec<u8>, String> {
    let (width, height) = dimensions;
    let (img_w, img_h) = img.dimensions();
    let mut resized_img = if (img_w, img_h) != (width, height) {
        let src = match fast_image_resize::Image::from_vec_u8(
            // We unwrap below because we know the images's dimensions should never be 0
            NonZeroU32::new(img_w).unwrap(),
            NonZeroU32::new(img_h).unwrap(),
            img.into_raw(),
            PixelType::U8x3,
        ) {
            Ok(i) => i,
            Err(e) => return Err(e.to_string()),
        };

        // We unwrap below because we know the outputs's dimensions should never be 0
        let new_w = NonZeroU32::new(width).unwrap();
        let new_h = NonZeroU32::new(height).unwrap();
        let mut src_view = src.view();
        src_view.set_crop_box_to_fit_dst_size(new_w, new_h, Some((0.5, 0.5)));

        let mut dst = fast_image_resize::Image::new(new_w, new_h, PixelType::U8x3);
        let mut dst_view = dst.view_mut();

        let mut resizer = Resizer::new(fast_image_resize::ResizeAlg::Convolution(filter));
        if let Err(e) = resizer.resize(&src_view, &mut dst_view) {
            return Err(e.to_string());
        }

        dst.into_vec()
    } else {
        img.into_vec()
    };

    // The ARGB is 'little endian', so here we must  put the order
    // of bytes 'in reverse', so it needs to be BGRA.
    imgproc::rgb_to_brg(&mut resized_img);

    Ok(resized_img)
}
