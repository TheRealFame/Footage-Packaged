use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use thiserror::Error;

use glib::clone;
use gst::{ClockTime, PadProbeData, PadProbeType, SeekFlags, prelude::*};
use gstreamer_pbutils::Discoverer;
use gtk::{gdk, gio, glib, subclass::prelude::*};

use log::{error, info};

use crate::{
    info::{Dimensions, Framerate, MediaInfo, media_info},
    orientation::{VideoOrientation, VideoOrientationTransformation},
    profiles::OutputFormat,
    render::{InputSettings, Progress, RenderJob, run_render},
    widgets::timeline::TimeRange,
};

/// State that only exists while a video is loaded and being previewed.
pub struct LoadedVideo {
    uri: url::Url,

    original_dimensions: Dimensions<u32>,

    current_dimensions: Dimensions<u32>,
    orientation: VideoOrientation,
    inpoint: Duration,
    outpoint: Duration,
    mute: bool,
    ended: bool,
    pipeline: gst::Element,
    videoflip: gst::Element,
    _bus_watch: gst::bus::BusWatchGuard,
}

fn duration_to_clocktime(duration: Duration) -> ClockTime {
    ClockTime::from_nseconds(
        duration
            .as_nanos()
            .try_into()
            .expect("Duration too large to convert to ClockTime"),
    )
}

mod imp {

    use crate::widgets::crop::Crop;

    use super::*;

    use adw::subclass::prelude::BinImpl;
    use glib::subclass::Signal;
    use gtk::CompositeTemplate;

    #[derive(CompositeTemplate, Default)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/video-preview.ui")]
    pub struct VideoPreview {
        #[template_child]
        pub paint: TemplateChild<gtk::Picture>,
        #[template_child]
        pub crop_box: TemplateChild<Crop>,

        pub loaded: RefCell<Option<LoadedVideo>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VideoPreview {
        const NAME: &'static str = "VideoPreview";
        type Type = super::VideoPreview;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }

        fn new() -> Self {
            Self::default()
        }
    }

    impl ObjectImpl for VideoPreview {
        fn signals() -> &'static [Signal] {
            use once_cell::sync::Lazy;
            static SIGNALS: Lazy<[Signal; 4]> = Lazy::new(|| {
                [
                    Signal::builder("orientation-flipped")
                        .param_types(std::iter::empty::<glib::Type>())
                        .build(),
                    Signal::builder("set-position")
                        .param_types([glib::Type::U64])
                        .build(),
                    Signal::builder("preview-ready")
                        .param_types(std::iter::empty::<glib::Type>())
                        .build(),
                    Signal::builder("mode-changed")
                        .param_types([glib::Type::BOOL])
                        .build(),
                ]
            });

            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for VideoPreview {}

    impl BinImpl for VideoPreview {}
}

glib::wrapper! {
pub struct VideoPreview(ObjectSubclass<imp::VideoPreview>)
    @extends adw::Bin, gtk::Widget,
    @implements gio::ActionMap, gio::ActionGroup, gtk::Root, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[derive(Debug, Error)]
pub enum VideoPreviewError {
    #[error("GStreamer error: {0}")]
    Glib(#[from] glib::Error),
    #[error("invalid file path")]
    InvalidPath,
    #[error("failed to get media info")]
    NoInfo,
}

#[gtk::template_callbacks]
impl VideoPreview {
    pub fn crop_box(&self) -> &crate::widgets::crop::Crop {
        &self.imp().crop_box
    }

    fn with_loaded<R>(&self, f: impl FnOnce(&LoadedVideo) -> R) -> Option<R> {
        self.imp().loaded.borrow().as_ref().map(f)
    }

    fn with_loaded_mut<R>(&self, f: impl FnOnce(&mut LoadedVideo) -> R) -> Option<R> {
        self.imp().loaded.borrow_mut().as_mut().map(f)
    }

    pub fn inpoint(&self) -> Duration {
        self.with_loaded(|v| v.inpoint)
            .unwrap_or(Duration::from_millis(0))
    }

    pub fn outpoint(&self) -> Duration {
        self.with_loaded(|v| v.outpoint)
            .unwrap_or(Duration::from_millis(0))
    }

    pub fn reset(&self) {
        self.imp().crop_box.reset();
        self.kill();
        self.imp().loaded.borrow_mut().take();
        self.imp().paint.set_paintable(None::<&gdk::Paintable>);
        self.emit_by_name::<()>("mode-changed", &[&false]);
    }

    pub fn load_path(&self, path: &Path) -> Result<MediaInfo, VideoPreviewError> {
        let uri = url::Url::from_file_path(path).map_err(|()| VideoPreviewError::InvalidPath)?;

        info!("Loading path: {}", uri.as_str());

        let discoverer = Discoverer::new(ClockTime::from_seconds(10))?;
        let info = discoverer.discover_uri(uri.as_str())?;

        let media_info = media_info(&info).ok_or(VideoPreviewError::NoInfo)?;

        self.imp().crop_box.reset();
        self.emit_by_name::<()>("mode-changed", &[&false]);

        self.build_pipeline(
            uri,
            media_info.dimensions,
            media_info.duration,
            !media_info.has_audio,
        );

        Ok(media_info)
    }

    fn build_pipeline(
        &self,
        uri: url::Url,
        dimensions: Dimensions<u32>,
        duration: Duration,
        mute: bool,
    ) {
        self.kill();

        let playbin = gst::ElementFactory::make("playbin3")
            .property("uri", uri.as_str())
            .build()
            .unwrap();

        // Video sink: videoconvertscale -> videoflip -> gtk4paintablesink
        let gtksink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .unwrap();

        let paintable = gtksink.property::<gdk::Paintable>("paintable");
        self.imp().paint.set_paintable(Some(&paintable));

        let video_sink = gst::Bin::default();
        let convert = gst::ElementFactory::make("videoconvertscale")
            .build()
            .unwrap();
        let flip = gst::ElementFactory::make("videoflip").build().unwrap();

        video_sink.add_many([&convert, &flip, &gtksink]).unwrap();
        gst::Element::link_many([&convert, &flip, &gtksink]).unwrap();

        let pad = gst::GhostPad::with_target(&convert.static_pad("sink").unwrap()).unwrap();

        let (sender, receiver) = async_channel::bounded(1);

        pad.add_probe(PadProbeType::DATA_DOWNSTREAM, move |_, info| {
            if let Some(PadProbeData::Buffer(data)) = &info.data
                && let Some(pts) = data.pts()
            {
                sender
                    .send_blocking(pts.mseconds())
                    .expect("Concurrency Issues");
            }

            gst::PadProbeReturn::Ok
        });

        video_sink.add_pad(&pad).unwrap();

        playbin.set_property("video-sink", &video_sink);

        if mute {
            playbin.set_property("mute", true);
        }

        let bus = playbin.bus().unwrap();

        playbin
            .set_state(gst::State::Paused)
            .expect("Unable to set the pipeline to the `Paused` state");

        let bus_watch = bus
            .add_watch_local(clone!(
                #[weak(rename_to=this)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move |_, msg| {
                    use gst::MessageView;

                    match msg.view() {
                        MessageView::Eos(..) => {
                            this.set_playing(false);
                            if let Some(loaded) = this.imp().loaded.borrow_mut().as_mut() {
                                loaded.ended = true;
                            }
                        }
                        MessageView::Error(err) => {
                            error!(
                                "Error from {:?}: {} ({:?})",
                                err.src().map(gst::prelude::GstObjectExt::path_string),
                                err.error(),
                                err.debug()
                            );
                        }
                        _ => (),
                    }

                    glib::ControlFlow::Continue
                }
            ))
            .expect("Failed to add bus watch");

        self.imp().loaded.replace(Some(LoadedVideo {
            uri,
            original_dimensions: dimensions,
            current_dimensions: dimensions,
            orientation: VideoOrientation::Identity,
            inpoint: Duration::ZERO,
            outpoint: duration,
            mute,
            ended: false,
            pipeline: playbin,
            videoflip: flip,
            _bus_watch: bus_watch,
        }));

        glib::spawn_future_local(clone!(
            #[weak(rename_to = this)]
            self,
            async move {
                let mut sent_ready = false;

                while let Ok(p) = receiver.recv().await {
                    if !sent_ready {
                        sent_ready = true;
                        this.emit_by_name::<()>("preview-ready", &[]);
                    }
                    let is_playing =
                        this.imp().loaded.borrow().as_ref().is_some_and(|v| {
                            matches!(v.pipeline.current_state(), gst::State::Playing)
                        });
                    if is_playing {
                        this.emit_by_name::<()>("set-position", &[&p]);
                    }
                }
            }
        ));
    }

    pub fn refresh_ui(&self) {
        let Some((uri, dimensions, mute, orientation, inpoint, outpoint)) =
            self.with_loaded(|loaded| {
                (
                    loaded.uri.clone(),
                    loaded.original_dimensions,
                    loaded.mute,
                    loaded.orientation,
                    loaded.inpoint,
                    loaded.outpoint,
                )
            })
        else {
            return;
        };

        let crop = self.imp().crop_box.proportions();

        self.build_pipeline(uri, dimensions, outpoint, mute);

        // Restore state that build_pipeline resets.
        self.with_loaded_mut(|loaded| {
            loaded.orientation = orientation;
            loaded.inpoint = inpoint;
            loaded.outpoint = outpoint;
            loaded.current_dimensions = if orientation.is_width_height_swapped() {
                dimensions.swap()
            } else {
                dimensions
            };
        });
        self.imp().crop_box.set_proportions(crop);
        self.update_videoflip();
    }

    pub fn seek(&self, position: Duration) {
        self.set_playing(false);
        self.quiet_seek(position);
    }

    fn quiet_seek(&self, position: Duration) {
        self.with_loaded_mut(|loaded| {
            if position == loaded.outpoint {
                loaded.ended = true;
            }
            loaded
                .pipeline
                .seek_simple(SeekFlags::FLUSH, duration_to_clocktime(position))
                .ok();
        });
    }

    pub fn set_playing(&self, playing: bool) {
        if self
            .with_loaded_mut(|loaded| {
                if playing {
                    if loaded.ended {
                        loaded.ended = false;
                        loaded
                            .pipeline
                            .seek_simple(SeekFlags::FLUSH, duration_to_clocktime(loaded.inpoint))
                            .ok();
                    }
                    loaded.pipeline.set_state(gst::State::Playing).unwrap();
                } else {
                    loaded.pipeline.set_state(gst::State::Paused).unwrap();
                }
            })
            .is_some()
        {
            self.emit_by_name::<()>("mode-changed", &[&playing]);
        }
    }

    pub fn set_range(&self, range: TimeRange) {
        self.with_loaded_mut(|loaded| {
            loaded.inpoint = range.start;
            loaded.outpoint = range.end;
        });
    }

    fn update_videoflip(&self) {
        self.with_loaded(|loaded| {
            loaded.videoflip.set_property(
                "video-direction",
                loaded.orientation.to_gst_video_orientation_method(),
            );
            // Force a frame refresh so the change is visible while paused.
            if let Some(position) = loaded.pipeline.query_position::<ClockTime>() {
                loaded.pipeline.seek_simple(SeekFlags::FLUSH, position).ok();
            }
        });
    }

    pub fn transform_orientation(&self, transformation: VideoOrientationTransformation) {
        let transformation_swaps_width_height = transformation.swaps_width_height();
        if self
            .with_loaded_mut(|loaded| {
                loaded.orientation = loaded.orientation.transformed(transformation);
                if transformation_swaps_width_height {
                    loaded.current_dimensions = loaded.current_dimensions.swap();
                }
            })
            .is_none()
        {
            return;
        }
        self.update_videoflip();
        if transformation_swaps_width_height {
            self.emit_by_name::<()>("orientation-flipped", &[]);
        }
        self.imp()
            .crop_box
            .set_proportions(self.imp().crop_box.proportions_transformed(transformation));
    }

    pub fn set_mute(&self, mute: bool) {
        self.with_loaded_mut(|loaded| {
            loaded.mute = mute;
            loaded.pipeline.set_property("mute", mute);
        });
    }

    fn kill(&self) {
        if let Some(loaded) = self.imp().loaded.borrow_mut().take() {
            loaded
                .pipeline
                .set_state(gst::State::Null)
                .expect("Unable to set the pipeline to the `Null` state");
        }
    }

    pub fn save(
        &self,
        output_path: PathBuf,
        sender: async_channel::Sender<Result<Progress, ()>>,
        output_format: OutputFormat,
        framerate: Framerate,
        scaled_dimension: Dimensions<u32>,
        running_flag: Arc<AtomicBool>,
    ) {
        let Some((input_uri, orientation, mute, inpoint, outpoint)) = self.with_loaded(|loaded| {
            (
                loaded.uri.clone(),
                loaded.orientation,
                loaded.mute,
                loaded.inpoint,
                loaded.outpoint,
            )
        }) else {
            error!("save called with no video loaded");
            return;
        };
        let crop = self.imp().crop_box.proportions();

        self.set_playing(false);

        info!(
            "Converting with output path: {output_path:?}, output format: {output_format:?}, framerate: {framerate:?}, scaled dimension: {scaled_dimension:?}",
            output_path = output_path.display(),
        );

        let duration = outpoint
            .checked_sub(inpoint)
            .expect("outpoint must be greater than or equal to inpoint");

        let job = RenderJob {
            input_settings: InputSettings {
                uri: input_uri,
                framerate,
                scaled_dimension,
                orientation,
                crop,
                inpoint: duration_to_clocktime(inpoint),
                duration: duration_to_clocktime(duration),
            },
            output_path,
            output_format,
            mute,
            sender,
            running_flag,
        };

        std::thread::spawn(move || run_render(job));
    }
}
