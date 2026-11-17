//! HD movies decoded with Android's MediaCodec, the device's own H.264
//! decoder, for zakuro_gpu::movie: the player's HD clips arrive as raw
//! access units, one per picture, and come out as YUV in the codec's
//! buffers, handed on without a copy.

use std::time::Duration;

use ndk::media::media_codec::{DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;
use zakuro_gpu::movie::{Decoder, Yuv};

/// MediaCodec's buffer flags and the colour formats it gives YUV in.
const END_OF_STREAM: u32 = 4;
const YUV420_PLANAR: i32 = 19;
const YUV420_SEMI_PLANAR: i32 = 21;
const QCOM_SEMI_PLANAR_32M: i32 = 0x7FA3_0C04;
const YUV420_FLEXIBLE: i32 = 0x7F42_0888;
/// one picture every 30th of a second, as the clips are made.
const FRAME_US: u64 = 33_333;

/// lets zakuro_gpu decode HD movie clips with MediaCodec.
pub fn install() {
    zakuro_gpu::movie::set_decoder(open);
}

fn open(width: u32, height: u32) -> Result<Box<dyn Decoder>, String> {
    let codec = MediaCodec::from_decoder_type("video/avc").ok_or("no H.264 decoder on this device")?;
    let mut format = MediaFormat::new();
    format.set_str("mime", "video/avc");
    format.set_i32("width", width as i32);
    format.set_i32("height", height as i32);
    codec.configure(&format, None, MediaCodecDirection::Decoder).map_err(|e| format!("configuring the decoder: {e:?}"))?;
    codec.start().map_err(|e| format!("starting the decoder: {e:?}"))?;
    log::info!("HD movies: decoding with {}", codec.name().unwrap_or_else(|_| "MediaCodec".into()));
    Ok(Box::new(Codec { codec, layout: None, said: false }))
}

struct Codec {
    codec: MediaCodec,
    /// how the decoder lays its pictures out, from its output format.
    layout: Option<Layout>,
    said: bool,
}

#[derive(Clone, Copy, Debug)]
struct Layout {
    width: usize,
    height: usize,
    stride: usize,
    slice: usize,
    /// left and top of the picture in the buffer.
    left: usize,
    top: usize,
    planar: bool,
}

/// how the pictures come out, from the decoder's output format.
fn layout_of(codec: &MediaCodec) -> Layout {
    {
        let mut format = codec.output_format();
        let i = |key: &str| format.i32(key).unwrap_or(0).max(0) as usize;
        let (width, height) = (i("width"), i("height"));
        let (left, top, right, bottom) = format.rect("crop").map_or((0, 0, width as i32 - 1, height as i32 - 1), |r| r);
        let colour = format.i32("color-format").unwrap_or(0);
        let stride = i("stride").max(width);
        let slice = i("slice-height").max(height);
        let planar = match colour {
            YUV420_PLANAR => true,
            YUV420_SEMI_PLANAR | QCOM_SEMI_PLANAR_32M => false,
            other => {
                if other != YUV420_FLEXIBLE {
                    log::warn!("HD movies: the decoder gives colour format {other:#X}, read as NV12");
                }
                false
            }
        };
        let layout = Layout {
            width: (right - left + 1).max(0) as usize,
            height: (bottom - top + 1).max(0) as usize,
            stride,
            slice,
            left: left.max(0) as usize,
            top: top.max(0) as usize,
            planar,
        };
        log::info!("HD movies: pictures come out as {layout:?}, colour format {colour:#X}, {:?}", format.str("mime"));
        layout
    }
}

impl Codec {
    /// hands out the pictures done, waiting up to wait for the first.
    fn drain(&mut self, wait: Duration, out: &mut dyn FnMut(u32, &Yuv)) -> Result<bool, String> {
        let mut wait = wait;
        loop {
            let result = self.codec.dequeue_output_buffer(wait).map_err(|e| format!("decoding: {e:?}"))?;
            wait = Duration::ZERO;
            match result {
                DequeuedOutputBufferInfoResult::TryAgainLater => return Ok(false),
                DequeuedOutputBufferInfoResult::OutputFormatChanged => self.layout = Some(layout_of(&self.codec)),
                DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
                DequeuedOutputBufferInfoResult::Buffer(buffer) => {
                    let info = *buffer.info();
                    let end = info.flags() & END_OF_STREAM != 0;
                    if info.size() > 0 {
                        if self.layout.is_none() {
                            self.layout = Some(layout_of(&self.codec));
                        }
                        let index = ((info.presentation_time_us().max(0) as u64 + FRAME_US / 2) / FRAME_US) as u32;
                        let data = &buffer.buffer()[info.offset().max(0) as usize..][..info.size() as usize];
                        match self.layout.and_then(|layout| picture(data, layout)) {
                            Some(yuv) => out(index, &yuv),
                            None if !self.said => {
                                self.said = true;
                                log::warn!("HD movies: a picture of {} bytes doesn't fit {:?}", data.len(), self.layout);
                            }
                            None => {}
                        }
                    }
                    self.codec.release_output_buffer(buffer, false).map_err(|e| format!("releasing a picture: {e:?}"))?;
                    if end {
                        return Ok(true);
                    }
                }
            }
        }
    }

    /// queues an access unit, or the end of the stream when none, handing
    /// out pictures while the decoder has no room for it.
    fn queue(&mut self, unit: Option<&[u8]>, index: u32, out: &mut dyn FnMut(u32, &Yuv)) -> Result<(), String> {
        for _ in 0..200 {
            match self.codec.dequeue_input_buffer(Duration::from_millis(5)).map_err(|e| format!("decoding: {e:?}"))? {
                DequeuedInputBufferResult::Buffer(mut input) => {
                    let room = input.buffer_mut();
                    let size = unit.map_or(0, <[u8]>::len);
                    if size > room.len() {
                        return Err(format!("an access unit of {size} bytes is more than the decoder's {}", room.len()));
                    }
                    for (to, &from) in room.iter_mut().zip(unit.unwrap_or_default()) {
                        to.write(from);
                    }
                    let flags = if unit.is_none() { END_OF_STREAM } else { 0 };
                    return self
                        .codec
                        .queue_input_buffer(input, 0, size, index as u64 * FRAME_US, flags)
                        .map_err(|e| format!("decoding: {e:?}"));
                }
                DequeuedInputBufferResult::TryAgainLater => {
                    self.drain(Duration::ZERO, out)?;
                }
            }
        }
        Err("the decoder takes no more input".into())
    }
}

impl Decoder for Codec {
    fn decode(&mut self, unit: &[u8], index: u32, out: &mut dyn FnMut(u32, &Yuv)) -> Result<(), String> {
        self.queue(Some(unit), index, out)?;
        self.drain(Duration::ZERO, out).map(|_| ())
    }

    fn finish(&mut self, out: &mut dyn FnMut(u32, &Yuv)) -> Result<(), String> {
        self.queue(None, 0, out)?;
        for _ in 0..100 {
            if self.drain(Duration::from_millis(10), out)? {
                break;
            }
        }
        // after the end of a stream MediaCodec takes nothing more until flushed
        self.flush()
    }

    fn flush(&mut self) -> Result<(), String> {
        self.codec.flush().map_err(|e| format!("flushing the decoder: {e:?}"))
    }
}

/// a decoded picture's planes inside the buffer it came out in.
fn picture(data: &[u8], layout: Layout) -> Option<Yuv<'_>> {
    let Layout { width, height, stride, slice, left, top, planar } = layout;
    if width < 2 || height < 2 || stride < width {
        return None;
    }
    let luma = stride * slice;
    // what zakuro_gpu::movie::canvas reads of each plane: rows of the
    // picture, the last one only as wide as the picture
    let y = data.get(top * stride + left..)?;
    let rows = |count: usize, stride: usize, width: usize| (count - 1) * stride + width;
    if y.len() < rows(height, stride, width) {
        return None;
    }
    let (u, v, uv_stride, uv_step) = if planar {
        // I420: U then V, each a quarter of the luma at half the stride
        let half = stride / 2;
        let chroma = (top / 2) * half + left / 2;
        let u = data.get(luma + chroma..)?;
        let v = data.get(luma + half * (slice / 2) + chroma..)?;
        (u, v, half, 1)
    } else {
        // NV12: U and V interleaved at the luma's stride
        let chroma = luma + (top / 2) * stride + (left / 2) * 2;
        (data.get(chroma..)?, data.get(chroma + 1..)?, stride, 2)
    };
    let chroma_width = (width / 2 - 1) * uv_step + 1;
    if u.len() < rows(height / 2, uv_stride, chroma_width) || v.len() < rows(height / 2, uv_stride, chroma_width) {
        return None;
    }
    Some(Yuv { width, height, y, y_stride: stride, u, v, uv_stride, uv_step })
}
