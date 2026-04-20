use std::process::Command;

use glib::translate::IntoGlib;
use gst::prelude::*;
use itertools::Itertools;
use log::info;

#[derive(Debug)]
pub struct Framerate {
    pub nominator: u32,
    pub denominator: u32,
}

impl Framerate {
    pub fn value(&self) -> f64 {
        f64::from(self.nominator) / f64::from(self.denominator)
    }

    pub fn as_gst_fraction(&self) -> gst::Fraction {
        gst::Fraction::new(self.nominator.cast_signed(), self.denominator.cast_signed())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Dimensions<T> {
    pub width: T,
    pub height: T,
}

impl Dimensions<u32> {
    pub fn width_f64(self) -> f64 {
        f64::from(self.width)
    }

    pub fn height_f64(self) -> f64 {
        f64::from(self.height)
    }

    pub fn as_even_dimensions(self) -> Dimensions<u32> {
        Dimensions {
            // Clear the least significant bit to make the dimension even
            width: self.width & !1,
            height: self.height & !1,
        }
    }
}
impl<T: Copy> Dimensions<T> {
    pub fn swap(&self) -> Dimensions<T> {
        Dimensions {
            width: self.height,
            height: self.width,
        }
    }
}

pub fn get_info(path: String) -> Option<(Dimensions<u32>, Option<Framerate>, bool)> {
    let video_info = get_video_info(path.clone())?;
    let audio_info = get_audio_info(path)?;
    Some((video_info.0, video_info.1, audio_info))
}

fn get_audio_info(path: String) -> Option<bool> {
    let o = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-show_entries", "stream=codec_type"])
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;

    let s = std::str::from_utf8(&o.stdout).ok()?;

    Some(s.lines().any(|x| x == "audio"))
}

fn get_video_info(path: String) -> Option<(Dimensions<u32>, Option<Framerate>)> {
    let ffprobe_output = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args(["-show_entries", "stream=width,height,r_frame_rate"])
        .args(["-of", "csv=s=x:p=0"])
        .arg(path)
        .output()
        .ok()?;

    let ffprobe_stdout = std::str::from_utf8(&ffprobe_output.stdout).ok()?;

    match ffprobe_stdout.trim().split('x').collect_vec()[..] {
        [width, height, framerate] => Some((
            Dimensions {
                width: width.trim().parse().ok()?,
                height: height.trim().parse().ok()?,
            },
            {
                let (x, y) = framerate.split('/').collect_tuple()?;
                Some(Framerate {
                    nominator: x.trim().parse().ok()?,
                    denominator: y.trim().parse().ok()?,
                })
            },
        )),
        [width, height] => Some((
            Dimensions {
                width: width.trim().parse().ok()?,
                height: height.trim().parse().ok()?,
            },
            None,
        )),
        _ => None,
    }
}

pub fn get_debug_info() {
    let gst_inspect_output = Command::new("gst-inspect-1.0").output().unwrap();

    let gst_inspect_stdout = std::str::from_utf8(&gst_inspect_output.stdout).unwrap();

    info!("{gst_inspect_stdout}");

    info!(
        "GStreamer version: {}.{}.{}.{}",
        gst::version().0,
        gst::version().1,
        gst::version().2,
        gst::version().3,
    );

    info!("Encoder selection priority:");
    for encoding in crate::profiles::VideoEncoding::ALL {
        let encoders = encoding.available_encoders();
        info!("{}:", encoding.for_display());
        if encoders.is_empty() {
            info!("  (none)");
        }
        for factory in &encoders {
            info!("  {:>6}  {}", factory.rank().into_glib(), factory.name());
        }
    }
}
