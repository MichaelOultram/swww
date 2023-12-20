use image::RgbImage;
use std::io::{stdin, BufReader, Read};
use std::path::Path;

pub fn read_img(path: &Path) -> Result<(RgbImage, bool), String> {
    if let Some("-") = path.to_str() {
        let mut reader = BufReader::new(stdin());
        let mut buffer = Vec::new();
        if let Err(e) = reader.read_to_end(&mut buffer) {
            return Err(format!("failed to read stdin: {e}"));
        }

        return match image::load_from_memory(&buffer) {
            Ok(img) => Ok((img.into_rgb8(), false)),
            Err(e) => return Err(format!("failed load image from memory: {e}")),
        };
    }

    let imgbuf = match image::io::Reader::open(path) {
        Ok(img) => img,
        Err(e) => return Err(format!("failed to open image: {e}")),
    };

    let imgbuf = match imgbuf.with_guessed_format() {
        Ok(img) => img,
        Err(e) => return Err(format!("failed to detect the image's format: {e}")),
    };

    let is_gif = imgbuf.format() == Some(image::ImageFormat::Gif);
    match imgbuf.decode() {
        Ok(img) => Ok((img.into_rgb8(), is_gif)),
        Err(e) => Err(format!("failed to decode image: {e}")),
    }
}
