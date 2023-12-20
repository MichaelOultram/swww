use image::codecs::{gif::GifDecoder, webp::WebPDecoder};
use image::io::Reader;
use image::{AnimationDecoder, DynamicImage, Frames, ImageError, ImageFormat, ImageResult};
use std::io::Stdin;
use std::{
    fs::File,
    io::{stdin, BufReader, Read},
    path::Path,
};

pub enum ImgBuf {
    Stdin(BufReader<Stdin>),
    File(Reader<BufReader<File>>),
}

impl ImgBuf {
    pub fn new(path: &Path) -> Result<Self, String> {
        Ok(if let Some("-") = path.to_str() {
            let reader = BufReader::new(stdin());
            Self::Stdin(reader)
        } else {
            let reader = Reader::open(path)
                .map_err(|e| format!("failed to open image: {e}"))?
                .with_guessed_format()
                .map_err(|e| format!("failed to detect the image's format: {e}"))?;
            Self::File(reader)
        })
    }

    fn format(&self) -> Option<ImageFormat> {
        match self {
            Self::Stdin(_) => None, // Not seekable
            Self::File(reader) => reader.format(),
        }
    }

    pub fn is_animated(&self) -> bool {
        matches!(
            self.format(),
            Some(ImageFormat::Gif) | Some(ImageFormat::WebP)
        )
    }

    pub fn decode(self) -> ImageResult<DynamicImage> {
        match self {
            Self::Stdin(mut reader) => {
                let mut buffer = Vec::new();
                reader
                    .read_to_end(&mut buffer)
                    .map_err(ImageError::IoError)?;
                image::load_from_memory(&buffer)
            }
            Self::File(reader) => reader.decode(),
        }
    }

    pub fn into_frames<'a>(self) -> Result<Frames<'a>, String> {
        fn decode<'a>(
            reader: impl Read + 'a,
            img_format: Option<ImageFormat>,
        ) -> Result<Frames<'a>, String> {
            match img_format {
                Some(ImageFormat::Gif) => Ok(GifDecoder::new(reader)
                    .map_err(|_| "GifDecoder failed")?
                    .into_frames()),
                Some(ImageFormat::WebP) => Ok(WebPDecoder::new(reader)
                    .map_err(|_| "WebPDecoder failed")?
                    .into_frames()),
                _ => Err(format!(
                    "Unsupported image format for animations {img_format:#?}"
                )),
            }
        }

        let img_format = self.format();
        match self {
            Self::Stdin(reader) => decode(reader, img_format),
            Self::File(reader) => decode(reader.into_inner(), img_format),
        }
    }
}
