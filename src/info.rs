use std::time::Duration;

use glib::translate::IntoGlib;
use gst::prelude::*;
use gstreamer_pbutils::DiscovererInfo;
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

    pub const fn as_even_dimensions(self) -> Self {
        Self {
            // Clear the least significant bit to make the dimension even
            width: self.width & !1,
            height: self.height & !1,
        }
    }
}

impl<T: Copy> Dimensions<T> {
    pub const fn swap(&self) -> Self {
        Self {
            width: self.height,
            height: self.width,
        }
    }
}

#[derive(Debug)]
pub struct MediaInfo {
    pub dimensions: Dimensions<u32>,
    /// `None` for sources without a meaningful constant rate (e.g. still images),
    /// in which case callers fall back to a default.
    pub framerate: Option<Framerate>,
    pub duration: Duration,
    pub has_audio: bool,
}

/// Extracts dimensions, framerate, duration, and audio presence from an already-discovered media file.
///
/// Returns `None` if the file has no video stream.
pub fn media_info(info: &DiscovererInfo) -> Option<MediaInfo> {
    let video = info.video_streams().into_iter().next()?;

    let dimensions = Dimensions {
        width: video.width(),
        height: video.height(),
    };

    let fraction = video.framerate();
    let framerate = (fraction.numer() > 0 && fraction.denom() > 0).then(|| Framerate {
        nominator: fraction.numer().cast_unsigned(),
        denominator: fraction.denom().cast_unsigned(),
    });

    let duration = info
        .duration()
        .map_or(Duration::ZERO, |d| Duration::from_millis(d.mseconds()));

    Some(MediaInfo {
        dimensions,
        framerate,
        duration,
        has_audio: !info.audio_streams().is_empty(),
    })
}

pub fn log_debug_info() {
    let registry = gst::Registry::get();
    for plugin in registry.plugins() {
        info!("{} ({})", plugin.plugin_name(), plugin.version());
        for feature in registry.features_by_plugin(&plugin.plugin_name()) {
            info!("  {}", feature.name());
        }
    }

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
