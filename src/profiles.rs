#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ContainerFormat {
    Best,
    Matroska,
    Mpeg,
    WebM,
    GifContainer,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ContainerSelection {
    Same,
    Format(ContainerFormat),
}

#[derive(Debug, Copy, Clone)]
pub enum VideoEncoding {
    Av1,
    Vp8,
    Vp9,
    H264,
    H265,
    Gif,
}

#[derive(Debug, Copy, Clone)]
pub enum AudioEncoding {
    Aac,
    Ac3,
    Opus,
    Vorbis,
    Flac,
}

use AudioEncoding::{Aac, Ac3, Flac, Opus, Vorbis};
use ContainerFormat::{Best, GifContainer, Matroska, Mpeg, WebM};
use VideoEncoding::{Av1, Gif, H264, H265, Vp8, Vp9};
use gettextrs::gettext;
use gst::prelude::*;

impl ContainerFormat {
    pub fn viable_video_encodings(self) -> Vec<VideoEncoding> {
        let video = match self {
            Best => vec![Av1],
            Matroska | Mpeg => vec![Av1, Vp9, Vp8, H264, H265],
            WebM => vec![Av1, Vp8, Vp9],
            GifContainer => vec![VideoEncoding::Gif],
        };
        video.into_iter().filter(|v| v.is_available()).collect()
    }

    pub fn viable_audio_encodings(self) -> Vec<AudioEncoding> {
        match self {
            Best => vec![Opus],
            Matroska => vec![Vorbis, Opus, Aac, Ac3, Flac],
            Mpeg => vec![Opus, Aac, Ac3, Flac],
            WebM => vec![Vorbis, Opus],
            GifContainer => vec![],
        }
    }

    pub const fn format(self) -> &'static str {
        match self {
            Matroska => "video/x-matroska",
            Mpeg => "video/quicktime",
            Best | WebM => "video/webm",
            GifContainer => "image/gif",
        }
    }

    pub fn container_caps(self) -> gst::Caps {
        let mut builder = gst::Caps::builder(self.format());
        if matches!(self, Mpeg) {
            // Bare "video/quicktime" lets encodebin pick qtmux, which writes a
            // QuickTime/MOV file under our .mp4 extension. variant=iso forces
            // mp4mux instead, so the container actually matches the extension.
            builder = builder.field("variant", "iso");
        }
        builder.build()
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Matroska => "mkv",
            Mpeg => "mp4",
            Best | WebM => "webm",
            GifContainer => "gif",
        }
    }

    pub fn for_display(self) -> String {
        match self {
            Best => gettext("Recommended (WEBM, AV1, Opus)"),
            Matroska => "MKV".to_owned(),
            Mpeg => "MP4".to_owned(),
            WebM => "WEBM".to_owned(),
            GifContainer => "GIF".to_owned(),
        }
    }

    /// Stable identifier used to persist the selection. Unlike list indices,
    /// this survives codec availability and display-ordering changes.
    pub const fn settings_key(self) -> &'static str {
        match self {
            Best => "best",
            Matroska => "matroska",
            Mpeg => "mpeg",
            WebM => "webm",
            GifContainer => "gif",
        }
    }

    pub fn from_settings_key(key: &str) -> Option<Self> {
        [Best, Matroska, Mpeg, WebM, GifContainer]
            .into_iter()
            .find(|c| c.settings_key() == key)
    }
}

impl ContainerSelection {
    const fn display_priority(self) -> u8 {
        match self {
            Self::Format(Best) => 0,
            Self::Same => 1,
            Self::Format(_) => 2,
        }
    }

    pub fn all() -> Vec<Self> {
        let mut selections: Vec<Self> = [Best, Matroska, Mpeg, WebM, GifContainer]
            .into_iter()
            .filter(|c| !c.viable_video_encodings().is_empty())
            .map(Self::Format)
            .chain(std::iter::once(Self::Same))
            .collect();
        selections.sort_by_key(|s| s.display_priority());
        selections
    }

    pub fn for_display(self) -> String {
        match self {
            Self::Same => gettext("Keep as-is"),
            Self::Format(f) => f.for_display(),
        }
    }

    /// Stable identifier used to persist the selection across sessions.
    pub const fn settings_key(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Format(f) => f.settings_key(),
        }
    }

    pub fn from_settings_key(key: &str) -> Option<Self> {
        if key == "same" {
            Some(Self::Same)
        } else {
            ContainerFormat::from_settings_key(key).map(Self::Format)
        }
    }
}

impl VideoEncoding {
    pub const ALL: &[Self] = &[Av1, Vp8, Vp9, H264, H265, Gif];

    pub const fn format(&self) -> &str {
        match self {
            Av1 => "video/x-av1",
            Vp8 => "video/x-vp8",
            Vp9 => "video/x-vp9",
            H264 => "video/x-h264",
            H265 => "video/x-h265",
            Gif => "image/gif",
        }
    }

    pub fn available_encoders(self) -> Vec<gst::ElementFactory> {
        let caps = gst::Caps::builder(self.format()).build();
        let mut factories: Vec<gst::ElementFactory> = gst::ElementFactory::factories_with_type(
            gst::ElementFactoryType::ENCODER | gst::ElementFactoryType::VIDEO_ENCODER,
            gst::Rank::MARGINAL,
        )
        .into_iter()
        .filter(|factory| {
            factory.static_pad_templates().iter().any(|tmpl| {
                tmpl.direction() == gst::PadDirection::Src && tmpl.caps().can_intersect(&caps)
            })
        })
        .collect();
        factories.sort_by_key(|f| std::cmp::Reverse(f.rank()));
        factories
    }

    pub fn is_available(self) -> bool {
        !self.available_encoders().is_empty()
    }

    pub fn encoding_profile(self) -> gstreamer_pbutils::EncodingVideoProfile {
        let caps = gst::Caps::builder(self.format()).build();
        gstreamer_pbutils::EncodingVideoProfile::builder(&caps).build()
    }

    pub const fn max_framerate(self) -> f64 {
        match self {
            Vp8 => 60.,
            Av1 | Vp9 => 240.,
            H264 | H265 => 300.,
            Gif => 50.,
        }
    }

    pub const fn for_display(&self) -> &str {
        match self {
            Av1 => "AV1",
            Vp8 => "VP8",
            Vp9 => "VP9",
            H264 => "H264",
            H265 => "H265",
            Gif => "GIF",
        }
    }

    pub const fn settings_key(self) -> &'static str {
        match self {
            Av1 => "av1",
            Vp8 => "vp8",
            Vp9 => "vp9",
            H264 => "h264",
            H265 => "h265",
            Gif => "gif",
        }
    }

    pub fn from_settings_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| e.settings_key() == key)
    }
}

impl AudioEncoding {
    pub const ALL: &[Self] = &[Aac, Ac3, Opus, Vorbis, Flac];

    pub const fn format(&self) -> &str {
        match self {
            Aac => "audio/mpeg",
            Ac3 => "audio/x-ac3",
            Opus => "audio/x-opus",
            Vorbis => "audio/x-vorbis",
            Flac => "audio/x-flac",
        }
    }

    pub fn caps(self) -> gst::Caps {
        let mut builder = gst::Caps::builder(self.format());
        if matches!(self, Aac) {
            // Bare "audio/mpeg" also matches the MP3 and MP2 encoders (all rank
            // primary), so the chosen codec is registry-order dependent.
            // mpegversion=4 pins it to AAC.
            builder = builder.field("mpegversion", 4i32);
        }
        builder.build()
    }

    pub const fn for_display(&self) -> &str {
        match self {
            Aac => "AAC",
            Ac3 => "AC3",
            Opus => "Opus",
            Vorbis => "Vorbis",
            Flac => "FLAC",
        }
    }

    pub const fn settings_key(self) -> &'static str {
        match self {
            Aac => "aac",
            Ac3 => "ac3",
            Opus => "opus",
            Vorbis => "vorbis",
            Flac => "flac",
        }
    }

    pub fn from_settings_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| e.settings_key() == key)
    }
}

#[derive(Debug)]
pub struct OutputFormat {
    pub container_selection: ContainerSelection,
    pub video_encoding: Option<VideoEncoding>,
    pub audio_encoding: Option<AudioEncoding>,
}
