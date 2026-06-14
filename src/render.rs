use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fraction::ToPrimitive;
use ges::prelude::*;
use gst::{ClockTime, PadProbeData, PadProbeType};
use gstreamer_pbutils::Discoverer;
use log::{error, info};
use ordered_float::NotNan;

use crate::{
    info::{Dimensions, Framerate},
    orientation::VideoOrientation,
    profiles::{ContainerFormat, ContainerSelection, OutputFormat},
};

/// Render progress reported back to the UI, in milliseconds. A message where `position == total`
/// signals completion.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub position: u64,
    pub total: u64,
}

pub struct InputSettings {
    pub uri: url::Url,
    pub framerate: Framerate,
    pub scaled_dimension: Dimensions<u32>,
    pub orientation: VideoOrientation,
    pub full_scaled_width: NotNan<f64>,
    pub full_scaled_height: NotNan<f64>,
    pub crop_left: NotNan<f64>,
    pub crop_top: NotNan<f64>,
    pub inpoint: ClockTime,
    pub duration: ClockTime,
}

pub struct RenderJob {
    pub input_settings: InputSettings,
    pub output_path: PathBuf,
    pub output_format: OutputFormat,
    pub mute: bool,
    pub sender: async_channel::Sender<Result<Progress, ()>>,
    pub running_flag: Arc<AtomicBool>,
}

pub fn run_render(
    RenderJob {
        input_settings,
        output_path,
        output_format,
        mute,
        sender,
        running_flag,
    }: RenderJob,
) {
    // When output == input, write to a temporary file to avoid truncating
    // the source before the pipeline reads it.
    let same_file = output_path == input_settings.uri.to_file_path().unwrap();
    let render_path = if same_file {
        let mut temp = output_path.clone();
        temp.set_extension(format!(
            "tmp.{}",
            output_path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        info!(
            "Output path matches input path, rendering to temporary file: {}",
            temp.display()
        );
        temp
    } else {
        output_path.clone()
    };

    let timeline = build_timeline(&input_settings);

    let pipeline = ges::Pipeline::new();
    pipeline.set_timeline(&timeline).unwrap();

    set_render_settings_for_pipeline(
        &output_format,
        &input_settings.uri,
        &render_path,
        mute,
        &pipeline,
    );

    pipeline.set_mode(ges::PipelineFlags::RENDER).unwrap();

    setup_progress_event(
        &timeline,
        sender.clone(),
        input_settings.duration,
        running_flag.clone(),
    );

    pipeline.set_state(gst::State::Playing).unwrap();

    let bus = pipeline
        .bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    info!("Starting pipeline");

    let success = run_pipeline(&bus, &pipeline, &sender, &running_flag);

    pipeline.set_state(gst::State::Null).unwrap();

    cleanup_render_job(same_file, success, &render_path, &output_path);
}

fn build_timeline(
    InputSettings {
        uri,
        framerate,
        scaled_dimension,
        orientation,
        full_scaled_width,
        full_scaled_height,
        crop_left,
        crop_top,
        inpoint,
        duration,
    }: &InputSettings,
) -> ges::Timeline {
    let clip = ges::UriClip::new(uri.as_str()).unwrap();

    let timeline = ges::Timeline::new_audio_video();

    let layer = timeline.append_layer();
    layer.add_clip(&clip).unwrap();

    if let Some(track) = timeline.tracks().first() {
        track.set_restriction_caps(
            &gst::Caps::builder("video/x-raw")
                .field("framerate", framerate.as_gst_fraction())
                .field("width", scaled_dimension.width.cast_signed())
                .field("height", scaled_dimension.height.cast_signed())
                .build(),
        );
        track.elements().into_iter().for_each(|track_element| {
            ges::prelude::TrackElementExt::set_child_property(
                &track_element,
                "video-direction",
                &orientation.to_gst_video_orientation_method().to_value(),
            )
            .unwrap();

            let set_dimension = |name: &str, value: NotNan<f64>| {
                ges::prelude::TrackElementExt::set_child_property(
                    &track_element,
                    name,
                    &value
                        .to_i32()
                        .unwrap_or_else(|| panic!("{name} cannot be over i32::MAX"))
                        .to_value(),
                )
                .unwrap();
            };

            set_dimension("width", *full_scaled_width);
            set_dimension("height", *full_scaled_height);
            set_dimension("posx", -crop_left * full_scaled_width);
            set_dimension("posy", -crop_top * full_scaled_height);
        });
    }

    clip.add_top_effect(&ges::Effect::new("videorate").unwrap(), 0)
        .ok();

    clip.set_inpoint(*inpoint);
    clip.set_duration(Some(*duration));

    timeline
}

fn set_render_settings_for_pipeline(
    output_format: &OutputFormat,
    input_uri: &url::Url,
    render_path: &Path,
    mute: bool,
    pipeline: &ges::Pipeline,
) {
    let profile: gstreamer_pbutils::EncodingProfile = match output_format.container_selection {
        ContainerSelection::Same => same_container_profile(input_uri, mute).upcast(),
        ContainerSelection::Format(ContainerFormat::GifContainer) => output_format
            .video_encoding
            .unwrap()
            .encoding_profile()
            .upcast(),
        ContainerSelection::Format(container) => {
            format_container_profile(output_format, container, mute).upcast()
        }
    };

    pipeline
        .set_render_settings(
            url::Url::from_file_path(render_path).unwrap().as_str(),
            &profile,
        )
        .unwrap();
}

/// Builds a container profile that mirrors the source's codecs, used when the output container is "Same".
fn same_container_profile(
    input_uri: &url::Url,
    mute: bool,
) -> gstreamer_pbutils::EncodingContainerProfile {
    let profile = gstreamer_pbutils::EncodingProfile::from_discoverer(
        &Discoverer::new(gst::ClockTime::SECOND)
            .unwrap()
            .discover_uri(input_uri.as_str())
            .unwrap(),
    )
    .unwrap();

    let (video_caps, audio_caps): (Vec<_>, Vec<_>) = profile
        .input_caps()
        .iter()
        .map(|discovered_caps| {
            let discovered_caps = discovered_caps.to_owned();
            let is_video = discovered_caps.name().starts_with("video");

            if is_video {
                let mut discovered_caps = discovered_caps;
                discovered_caps.remove_fields(["width", "height", "framerate"]);

                let mut caps = gst::Caps::builder(discovered_caps.name());
                for (name, value) in &*discovered_caps {
                    caps = caps.field(name, value.clone());
                }
                caps.build()
            } else {
                // For audio, only keep the codec name to avoid
                // over-constraining encoder selection.
                gst::Caps::builder(discovered_caps.name()).build()
            }
        })
        .partition(|c| c.to_string().starts_with("video"));

    let profile_format = profile.format();

    let mut container_profile =
        gstreamer_pbutils::EncodingContainerProfile::builder(&profile_format).name("container");

    if let Some(video_cap) = video_caps.first() {
        let video_profile = gstreamer_pbutils::EncodingVideoProfile::builder(video_cap).build();

        container_profile = container_profile.add_profile(video_profile);
    }

    if !mute && let Some(audio_cap) = audio_caps.first() {
        let audio_profile = gstreamer_pbutils::EncodingAudioProfile::builder(audio_cap).build();

        container_profile = container_profile.add_profile(audio_profile);
    }

    container_profile.build()
}

/// Builds a container profile for an explicit output container plus the selected video/audio encodings.
fn format_container_profile(
    output_format: &OutputFormat,
    container: ContainerFormat,
    mute: bool,
) -> gstreamer_pbutils::EncodingContainerProfile {
    let video_profile = output_format.video_encoding.unwrap().encoding_profile();

    let container_caps = gst::Caps::builder(container.format()).build();

    let mut container_profile =
        gstreamer_pbutils::EncodingContainerProfile::builder(&container_caps)
            .name("container")
            .add_profile(video_profile);

    if !mute {
        let audio_profile = gstreamer_pbutils::EncodingAudioProfile::builder(
            &gst::Caps::builder(output_format.audio_encoding.unwrap().format()).build(),
        )
        .build();
        container_profile = container_profile.add_profile(audio_profile);
    }

    container_profile.build()
}

fn setup_progress_event(
    timeline: &ges::Timeline,
    sender: async_channel::Sender<Result<Progress, ()>>,
    duration: ClockTime,
    running_flag: Arc<AtomicBool>,
) {
    timeline
        .pads()
        .first()
        .unwrap()
        .add_probe(PadProbeType::DATA_DOWNSTREAM, move |_, info| {
            if let Some(PadProbeData::Buffer(data)) = &info.data
                && let Some(pts) = data.pts()
                && sender
                    .send_blocking(Ok(Progress {
                        position: pts.mseconds(),
                        total: duration.mseconds(),
                    }))
                    .is_err()
            {
                return gst::PadProbeReturn::Drop;
            }

            if !running_flag.load(Ordering::SeqCst) {
                send_cancel(&sender, "cancellation from pad probe");
                return gst::PadProbeReturn::Drop;
            }

            gst::PadProbeReturn::Ok
        });
}

/// Signals the receiver that the render was cancelled or failed, logging if the channel is gone.
fn send_cancel(sender: &async_channel::Sender<Result<Progress, ()>>, context: &str) {
    if let Err(e) = sender.send_blocking(Err(())) {
        error!("Failed to send {context}: {e}");
    }
}

fn run_pipeline(
    bus: &gst::Bus,
    pipeline: &ges::Pipeline,
    sender: &async_channel::Sender<Result<Progress, ()>>,
    running_flag: &Arc<AtomicBool>,
) -> bool {
    let mut success = false;

    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => {
                success = true;
                if let Err(e) = sender.send_blocking(Ok(Progress {
                    position: 1,
                    total: 1,
                })) {
                    error!("Failed to send EOS: {e}");
                }
                break;
            }
            MessageView::Error(e) => {
                error!(
                    "Error from {:?}: {} ({:?})",
                    e.src().map(gst::prelude::GstObjectExt::path_string),
                    e.error(),
                    e.debug()
                );
                pipeline.set_state(gst::State::Null).unwrap();

                send_cancel(sender, "error");
                break;
            }
            _ => {
                if !running_flag.load(Ordering::SeqCst) {
                    pipeline.set_state(gst::State::Null).unwrap();

                    send_cancel(sender, "cancellation");
                    break;
                }
            }
        }
    }

    success
}

fn cleanup_render_job(same_file: bool, success: bool, render_path: &Path, output_path: &Path) {
    if same_file {
        if success {
            info!(
                "Renaming temporary file to output: {}",
                output_path.display()
            );
            if let Err(e) = std::fs::rename(render_path, output_path) {
                error!("Failed to rename temporary file to output: {e}");
            }
        } else {
            info!("Removing temporary file: {}", render_path.display());
            if let Err(e) = std::fs::remove_file(render_path) {
                error!("Failed to remove temporary file: {e}");
            }
        }
    }
}
