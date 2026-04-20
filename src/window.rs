use std::{
    num::ParseIntError,
    os::fd::AsFd,
    path::{Path, PathBuf},
    time::Duration,
};

use adw::prelude::*;
use fraction::Ratio;
use gettextrs::gettext;
use glib::clone;
use gtk::{gio, glib, subclass::prelude::*};
use itertools::Itertools;
use log::{error, warn};

use crate::{
    Listable,
    info::{Dimensions, Framerate},
    orientation::VideoOrientationTransformation,
    profiles::{AudioEncoding, ContainerSelection, OutputFormat, VideoEncoding},
    runtime, spawn,
};

mod imp {

    use std::{
        cell::{Cell, RefCell},
        sync::{Arc, atomic::AtomicBool},
    };

    use crate::{
        config::{APP_ID, PKGDATADIR},
        widgets::{preview::VideoPreview, timeline::Timeline},
    };

    use super::*;

    use adw::subclass::prelude::AdwApplicationWindowImpl;
    use derivative::Derivative;
    use gtk::CompositeTemplate;

    #[derive(CompositeTemplate, Derivative)]
    #[derivative(Default)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/window.ui")]
    pub struct AppWindow {
        #[template_child]
        pub video_preview: TemplateChild<VideoPreview>,
        #[template_child]
        pub rotate_left_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub rotate_right_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub horizontal_flip_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub vertical_flip_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub audio_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub save_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub progress_bar: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub try_again_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub done_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub open_result: TemplateChild<gtk::Button>,
        #[template_child]
        pub container_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub video_encoding: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub audio_encoding: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub framerate_row: TemplateChild<adw::SpinRow>,
        // #[template_child]
        // pub link_axis: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub resize_type: TemplateChild<gtk::DropDown>,
        #[template_child]
        pub resize_width_multiplier_percentage: TemplateChild<gtk::Entry>,
        #[template_child]
        pub resize_height_multiplier_percentage: TemplateChild<gtk::Entry>,
        #[template_child]
        pub resize_width_value: TemplateChild<gtk::Entry>,
        #[template_child]
        pub resize_height_value: TemplateChild<gtk::Entry>,
        #[template_child]
        pub cancel_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub back_edit: TemplateChild<gtk::Button>,
        #[template_child]
        pub success_status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub timeline: TemplateChild<Timeline>,
        #[template_child]
        pub play_pause: TemplateChild<gtk::Button>,
        #[template_child]
        pub help_overlay: TemplateChild<adw::ShortcutsDialog>,

        pub running_flag: Arc<AtomicBool>,
        pub video_dimensions: Cell<Option<Dimensions<u32>>>,
        pub selected_video_dimensions: Cell<Option<Dimensions<u32>>>,
        pub selected_video_path: RefCell<Option<PathBuf>>,
        pub result_video_path: RefCell<Option<PathBuf>>,
        pub provider: gtk::CssProvider,
        #[derivative(Default(value = "gio::Settings::new(APP_ID)"))]
        pub settings: gio::Settings,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppWindow {
        const NAME: &'static str = "AppWindow";
        type Type = super::AppWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            super::AppWindow::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }

        fn new() -> Self {
            Self::default()
        }
    }

    impl ObjectImpl for AppWindow {
        fn constructed(&self) {
            self.parent_constructed();

            let theme = gtk::IconTheme::for_display(
                &gtk::gdk::Display::default().expect("cannot find display"),
            );
            theme.add_search_path(PKGDATADIR.to_owned() + "/icons");

            let obj = self.obj();
            obj.load_window_size();
            obj.setup_gactions();
        }
    }

    impl WidgetImpl for AppWindow {}
    impl WindowImpl for AppWindow {
        fn close_request(&self) -> glib::Propagation {
            let obj = self.obj();

            if let Err(err) = obj.save_window_size() {
                warn!("Failed to save window state: {}", &err);
            }

            if self.running_flag.load(std::sync::atomic::Ordering::SeqCst) {
                self.obj().convert_cancel(true);
                glib::Propagation::Stop
            } else {
                // Pass close request on to the parent
                self.parent_close_request()
            }
        }
    }

    impl ApplicationWindowImpl for AppWindow {}
    impl AdwApplicationWindowImpl for AppWindow {}
}

glib::wrapper! {
    pub struct AppWindow(ObjectSubclass<imp::AppWindow>)
        @extends gtk::Widget, gtk::Window,  gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionMap, gio::ActionGroup,
                    gtk::Root, gtk::Native, gtk::ShortcutManager,
                    gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl AppWindow {
    pub fn new<P: glib::prelude::IsA<gtk::Application>>(app: &P) -> Self {
        let win = glib::Object::builder::<AppWindow>()
            .property("application", app)
            .build();

        win.setup_crop_box_listener();

        win.imp().container_row.set_model(Some(
            &ContainerSelection::get_all()
                .into_iter()
                .map(super::profiles::ContainerSelection::for_display)
                .collect_vec()
                .to_list(),
        ));

        win
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("close")
                .activate(clone!(
                    #[weak(rename_to=window)]
                    self,
                    move |_, _, _| {
                        window.close();
                    }
                ))
                .build(),
            gio::ActionEntry::builder("about")
                .activate(clone!(
                    #[weak(rename_to=window)]
                    self,
                    move |_, _, _| {
                        window.show_about();
                    }
                ))
                .build(),
            gio::ActionEntry::builder("show-help-overlay")
                .activate(clone!(
                    #[weak(rename_to=window)]
                    self,
                    move |_, _, _| {
                        window.imp().help_overlay.present(Some(&window));
                    }
                ))
                .build(),
            gio::ActionEntry::builder("open")
                .activate(clone!(
                    #[weak(rename_to=window)]
                    self,
                    move |_, _, _| {
                        spawn!(async move {
                            window.open_dialog().await;
                        });
                    }
                ))
                .build(),
        ]);
    }

    fn setup_crop_box_listener(&self) {
        self.imp().video_preview.crop_box().connect_local(
            "crop-box-changed",
            true,
            clone!(
                #[weak(rename_to=this)]
                self,
                #[upgrade_or]
                None,
                move |v| {
                    let (t, r, b, l): (f64, f64, f64, f64) = (
                        v.get(1)?.get().ok()?,
                        v.get(2)?.get().ok()?,
                        v.get(3)?.get().ok()?,
                        v.get(4)?.get().ok()?,
                    );

                    let video_dimensions = this.imp().video_dimensions.get()?;

                    let selected_height =
                        (video_dimensions.height_f64() * (1. - t - b)) as u32 / 2 * 2;
                    let selected_width =
                        (video_dimensions.width_f64() * (1. - l - r)) as u32 / 2 * 2;

                    this.imp().selected_video_dimensions.set(Some(Dimensions {
                        width: selected_width,
                        height: selected_height,
                    }));

                    this.imp()
                        .resize_height_value
                        .set_text(&selected_height.to_string());
                    this.imp()
                        .resize_width_value
                        .set_text(&selected_width.to_string());

                    None
                }
            ),
        );
    }

    #[template_callback]
    fn on_rotate_left(&self) {
        self.imp()
            .video_preview
            .transform_orientation(VideoOrientationTransformation::RotateLeft);
    }

    #[template_callback]
    fn on_rotate_right(&self) {
        self.imp()
            .video_preview
            .transform_orientation(VideoOrientationTransformation::RotateRight);
    }

    #[template_callback]
    fn on_horizontal_flip(&self) {
        self.imp()
            .video_preview
            .transform_orientation(VideoOrientationTransformation::HorizontalFlip);
    }

    #[template_callback]
    fn on_vertical_flip(&self) {
        self.imp()
            .video_preview
            .transform_orientation(VideoOrientationTransformation::VerticalFlip);
    }

    #[template_callback]
    fn on_audio_toggled(&self) {
        let b = &self.imp().audio_button;
        if b.is_active() {
            b.set_icon_name("audio-volume-muted-symbolic");
            b.set_tooltip_text(Some(&gettext("Enable Audio")));
        } else {
            b.set_icon_name("audio-volume-high-symbolic");
            b.set_tooltip_text(Some(&gettext("Disable Audio")));
        }
        // don't think about it
        if b.is_visible() {
            self.imp().video_preview.set_mute(b.is_active());
        }
    }

    #[template_callback]
    fn on_save_clicked(&self) {
        spawn!(clone!(
            #[weak(rename_to=this)]
            self,
            async move {
                this.save_dialog().await;
            }
        ));
    }

    #[template_callback]
    fn on_try_again(&self) {
        self.imp().video_preview.refresh_ui();
    }

    #[template_callback]
    fn on_done(&self) {
        self.imp().stack.set_visible_child_name("welcome");
        self.imp().back_edit.set_visible(false);
    }

    #[template_callback]
    fn on_cancel(&self) {
        self.convert_cancel(false);
    }

    #[template_callback]
    fn on_back_edit(&self) {
        self.imp().video_preview.refresh_ui();
        self.imp().stack.set_visible_child_name("editing");
        self.imp().back_edit.set_visible(false);
    }

    #[template_callback]
    fn on_open_result(&self) {
        let file =
            std::fs::File::open(self.imp().result_video_path.borrow().as_ref().unwrap()).unwrap();
        runtime().spawn(async move {
            ashpd::desktop::open_uri::OpenFileRequest::default()
                .ask(true)
                .send_file(&file.as_fd())
                .await
                .ok();
        });
    }

    #[template_callback]
    fn on_container_changed(&self) {
        self.update_options();
    }

    #[template_callback]
    fn on_video_encoding_changed(&self) {
        self.update_framerate_limit();
    }

    #[template_callback]
    fn on_resize_type_changed(&self) {
        let imp = self.imp();
        match imp.resize_type.selected() {
            0 => {
                imp.resize_width_value.set_visible(false);
                imp.resize_height_value.set_visible(false);
                imp.resize_width_multiplier_percentage.set_visible(true);
                imp.resize_height_multiplier_percentage.set_visible(true);
            }
            1 => {
                imp.resize_width_value.set_visible(true);
                imp.resize_height_value.set_visible(true);
                imp.resize_width_multiplier_percentage.set_visible(false);
                imp.resize_height_multiplier_percentage.set_visible(false);
            }
            _ => unreachable!(),
        }
    }

    #[template_callback]
    fn on_resize_width_changed(&self) {
        self.update_height_from_width();
    }

    #[template_callback]
    fn on_resize_height_changed(&self) {
        self.update_width_from_height();
    }

    #[template_callback]
    fn on_resize_height_multiplier_percentage_changed(&self) {
        let imp = self.imp();
        let old_value = imp
            .resize_width_multiplier_percentage
            .text()
            .as_str()
            .to_owned();
        let new_value = imp
            .resize_height_multiplier_percentage
            .text()
            .as_str()
            .to_owned();
        if old_value != new_value && !new_value.is_empty() {
            imp.resize_width_multiplier_percentage.set_text(&new_value);
        }
    }

    #[template_callback]
    fn on_resize_width_multiplier_percentage_changed(&self) {
        let imp = self.imp();
        let old_value = imp
            .resize_height_multiplier_percentage
            .text()
            .as_str()
            .to_owned();
        let new_value = imp
            .resize_width_multiplier_percentage
            .text()
            .as_str()
            .to_owned();
        if old_value != new_value && !new_value.is_empty() {
            imp.resize_height_multiplier_percentage.set_text(&new_value);
        }
    }

    #[template_callback]
    fn on_preview_ready(&self) {
        self.mark_ui_as_ready();
    }

    #[template_callback]
    fn on_orientation_flipped(&self) {
        if let Some(video_dimensions) = self.imp().video_dimensions.get() {
            self.imp()
                .video_dimensions
                .set(Some(video_dimensions.swap()));
        }
    }

    #[template_callback]
    fn on_preview_mode_changed(&self, playing: bool) {
        if playing {
            self.imp().play_pause.set_icon_name("pause-symbolic");
            self.imp()
                .play_pause
                .set_tooltip_text(Some(&gettext("Pause")));
        } else {
            self.imp().play_pause.set_icon_name("play-symbolic");
            self.imp()
                .play_pause
                .set_tooltip_text(Some(&gettext("Play")));
        }
    }

    #[template_callback]
    fn on_preview_set_position(&self, position: u64) {
        self.imp()
            .timeline
            .set_position(Duration::from_millis(position));
    }

    #[template_callback]
    fn on_timeline_set_range(&self, start: f64, end: f64) {
        let start = Duration::from_secs_f64(start);
        let end = Duration::from_secs_f64(end);
        if self.imp().video_preview.inpoint() != start || self.imp().video_preview.outpoint() != end
        {
            self.imp().video_preview.set_range(start, end);
        }
    }

    #[template_callback]
    fn on_timeline_moving(&self) {
        self.imp().video_preview.set_playing(false);
    }

    #[template_callback]
    fn on_timeline_set_position(&self, position: f64) {
        self.imp()
            .video_preview
            .seek(Duration::from_secs_f64(position));
    }

    #[template_callback]
    fn on_play_pause(&self) {
        let icon = self.imp().play_pause.icon_name().unwrap();
        self.imp()
            .video_preview
            .set_playing(icon == "play-symbolic");
    }

    fn update_width_from_height(&self) {
        // if self.imp().link_axis.is_active() && self.imp().link_axis.is_visible() {
        if let Some(video_dimensions) = self.imp().selected_video_dimensions.get() {
            let old_value = self.imp().resize_width_value.text().as_str().to_owned();
            let other_text = self.imp().resize_height_value.text().as_str().to_owned();
            if other_text.is_empty() {
                return;
            }

            let other_way =
                generate_height_from_width(old_value.parse().unwrap_or(0), video_dimensions)
                    .to_string();

            if other_way == other_text {
                return;
            }

            let new_value =
                generate_width_from_height(other_text.parse().unwrap_or(0), video_dimensions)
                    .to_string();

            if old_value != new_value && new_value != "0" {
                self.imp().resize_width_value.set_text(&new_value);
            }
        }
        // }
    }

    fn update_height_from_width(&self) {
        // if self.imp().link_axis.is_active() && self.imp().link_axis.is_visible() {
        if let Some(dimensions) = self.imp().selected_video_dimensions.get() {
            let old_value = self.imp().resize_height_value.text().as_str().to_owned();
            let other_text = self.imp().resize_width_value.text().as_str().to_owned();
            if other_text.is_empty() {
                return;
            }

            let other_way =
                generate_width_from_height(old_value.parse().unwrap_or(0), dimensions).to_string();

            if other_way == other_text {
                return;
            }

            let new_value =
                generate_height_from_width(other_text.parse().unwrap_or(0), dimensions).to_string();

            if old_value != new_value && new_value != "0" {
                self.imp().resize_height_value.set_text(&new_value);
            }
        }
        // }
    }

    fn convert_cancel(&self, closing: bool) {
        let stop_converting_dialog = adw::AlertDialog::new(
            Some(&gettext("Stop rendering?")),
            Some(&gettext("You will lose all progress.")),
        );

        stop_converting_dialog
            .add_responses(&[("cancel", &gettext("_Cancel")), ("stop", &gettext("_Stop"))]);
        stop_converting_dialog
            .set_response_appearance("stop", adw::ResponseAppearance::Destructive);

        stop_converting_dialog.connect_response(
            None,
            clone!(
                #[weak(rename_to=this)]
                self,
                move |_, response_id| {
                    if response_id == "stop" {
                        this.imp()
                            .running_flag
                            .store(false, std::sync::atomic::Ordering::SeqCst);

                        if closing {
                            this.close();
                        } else {
                            this.imp().stack.set_visible_child_name("failure");
                        }
                    }
                }
            ),
        );

        stop_converting_dialog.present(Some(self));
    }

    async fn open_dialog(&self) {
        let filter = gtk::FileFilter::new();
        filter.add_mime_type("video/*");
        filter.set_name(Some(&gettext("Video Files")));

        let model = gio::ListStore::new::<gtk::FileFilter>();
        model.append(&filter);

        if let Ok(file) = gtk::FileDialog::builder()
            .modal(true)
            .filters(&model)
            .build()
            .open_future(Some(self))
            .await
        {
            let path = file.path().unwrap();

            self.open_file(&path);
        }
    }

    async fn save_dialog(&self) {
        let input_path = self.imp().selected_video_path.borrow().to_owned().unwrap();

        let input_path_stem = input_path.file_stem().unwrap().to_str().unwrap().to_owned();

        let extension = match self.selected_container() {
            ContainerSelection::Same => {
                input_path.extension().unwrap().to_str().unwrap().to_owned()
            }
            ContainerSelection::Format(f) => f.extension().to_owned(),
        };

        if let Ok(file) = gtk::FileDialog::builder()
            .modal(true)
            .initial_name(format!("{input_path_stem}.{extension}"))
            .build()
            .save_future(Some(self))
            .await
        {
            self.save_file(file.path().unwrap());
        }
    }

    fn selected_container(&self) -> ContainerSelection {
        ContainerSelection::get_all()[self.imp().container_row.selected() as usize]
    }

    fn selected_video_encoding(&self) -> Option<VideoEncoding> {
        let list = match self.selected_container() {
            ContainerSelection::Same => return None,
            ContainerSelection::Format(f) => f.viable_video_encodings(),
        };
        if list.is_empty() {
            None
        } else {
            Some(list[self.imp().video_encoding.selected() as usize])
        }
    }

    fn selected_audio_encoding(&self) -> Option<AudioEncoding> {
        let list = match self.selected_container() {
            ContainerSelection::Same => return None,
            ContainerSelection::Format(f) => f.viable_audio_encodings(),
        };
        if list.is_empty() {
            None
        } else {
            Some(list[self.imp().audio_encoding.selected() as usize])
        }
    }

    fn update_options(&self) {
        let imp = self.imp();

        let (available_video, available_audio) = match self.selected_container() {
            ContainerSelection::Same => (vec![], vec![]),
            ContainerSelection::Format(f) => {
                (f.viable_video_encodings(), f.viable_audio_encodings())
            }
        };

        imp.audio_encoding.set_visible(available_audio.len() > 1);
        imp.audio_encoding.set_model(Some(
            &available_audio
                .into_iter()
                .map(|e| e.for_display().to_owned())
                .collect_vec()
                .to_list(),
        ));

        imp.video_encoding.set_visible(available_video.len() > 1);
        imp.video_encoding.set_model(Some(
            &available_video
                .into_iter()
                .map(|e| e.for_display().to_owned())
                .collect_vec()
                .to_list(),
        ));

        self.update_framerate_limit();
    }

    fn update_framerate_limit(&self) {
        let max_fps = self
            .selected_video_encoding()
            .map_or(480., super::profiles::VideoEncoding::max_framerate);
        let adj = self.imp().framerate_row.adjustment();
        adj.set_upper(max_fps);
        if adj.value() > max_fps {
            adj.set_value(max_fps);
        }
    }

    fn get_desired_dimensions(&self) -> Result<Dimensions<u32>, ParseIntError> {
        let imp = self.imp();

        Ok(match imp.resize_type.selected() {
            0 => {
                let width_multiplier_percentage: u32 =
                    imp.resize_width_multiplier_percentage.text().parse()?;

                let height_multiplier_percentage: u32 =
                    imp.resize_height_multiplier_percentage.text().parse()?;

                let selected_video_dimensions = imp.selected_video_dimensions.get().unwrap();

                Dimensions {
                    width: selected_video_dimensions.width * width_multiplier_percentage / 100,
                    height: selected_video_dimensions.height * height_multiplier_percentage / 100,
                }
            }
            1 => Dimensions {
                width: imp.resize_width_value.text().parse::<u32>()?,
                height: imp.resize_height_value.text().parse::<u32>()?,
            },
            _ => unreachable!(),
        }
        .as_even_dimensions())
    }

    fn save_file(&self, path: PathBuf) {
        self.imp().result_video_path.replace(Some(path.clone()));

        let file_name = path.file_name().unwrap().to_str().unwrap().to_owned();

        self.imp()
            .success_status
            .set_description(Some(&gettext("Saved as {}").replace("{}", &file_name)));

        self.imp()
            .stack
            .set_transition_type(gtk::StackTransitionType::None);
        self.imp().stack.set_visible_child_name("exporting");
        glib::MainContext::default().iteration(true);
        self.imp()
            .stack
            .set_transition_type(gtk::StackTransitionType::Crossfade);

        let running_flag = self.imp().running_flag.clone();
        let receiver_running_flag = running_flag.clone();
        running_flag.store(true, std::sync::atomic::Ordering::SeqCst);

        self.imp().progress_bar.set_fraction(0.);

        let (sender, receiver) = async_channel::unbounded();
        self.imp().video_preview.save(
            path,
            sender,
            OutputFormat {
                container_selection: self.selected_container(),
                video_encoding: self.selected_video_encoding(),
                audio_encoding: self.selected_audio_encoding(),
            },
            {
                let f = Ratio::<i32>::approximate_float(self.imp().framerate_row.value());

                match f {
                    Some(ratio) => Framerate {
                        nominator: ratio.numer().cast_unsigned(),
                        denominator: ratio.denom().cast_unsigned(),
                    },
                    _ => Framerate {
                        nominator: 30,
                        denominator: 1,
                    },
                }
            },
            self.get_desired_dimensions().unwrap(),
            running_flag,
        );

        glib::spawn_future_local(clone!(
            #[weak(rename_to=this)]
            self,
            async move {
                let mut most_done = 0;
                while let Ok(p) = receiver.recv().await {
                    if !receiver_running_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        this.imp().stack.set_visible_child_name("failure");
                        break;
                    }
                    match p {
                        Ok((done, total)) if done == total => {
                            this.imp().stack.set_visible_child_name("success");
                            this.imp().back_edit.set_visible(true);
                            this.imp()
                                .running_flag
                                .store(false, std::sync::atomic::Ordering::SeqCst);
                            break;
                        }
                        Ok((done, total)) => {
                            most_done = std::cmp::max(done, most_done);
                            this.imp()
                                .progress_bar
                                .set_fraction(most_done as f64 / total as f64);
                        }
                        Err(()) => {
                            this.imp().stack.set_visible_child_name("failure");
                            this.imp()
                                .running_flag
                                .store(false, std::sync::atomic::Ordering::SeqCst);
                            break;
                        }
                    }
                }
            }
        ));
    }

    fn create_ui(&self, path: &Path) {
        self.imp().video_preview.reset();
        let (dimensions, duration, framerate, has_audio) =
            match self.imp().video_preview.load_path(path) {
                Ok(result) => result,
                Err(err) => {
                    error!("Failed to load video: {err}");
                    self.imp().stack.set_visible_child_name("invalid");
                    return;
                }
            };
        if has_audio {
            if self.imp().audio_button.is_active() {
                // don't think about it
                self.imp().audio_button.set_visible(false);
                self.imp().audio_button.set_active(false);
            }
            self.imp().audio_button.set_visible(true);
        } else {
            self.imp().audio_button.set_visible(false);
        }
        self.imp().timeline.set_position(Duration::ZERO);
        self.imp().timeline.set_duration(duration);
        self.imp()
            .timeline
            .set_range(Some((Duration::ZERO, duration)));
        self.imp().video_dimensions.set(Some(dimensions));
        self.imp().selected_video_dimensions.set(Some(dimensions));
        self.imp()
            .resize_height_multiplier_percentage
            .set_text("100");
        self.imp()
            .resize_width_multiplier_percentage
            .set_text("100");
        self.imp()
            .resize_height_value
            .set_text(&dimensions.height.to_string());
        self.imp()
            .resize_width_value
            .set_text(&dimensions.width.to_string());
        let max_fps = self
            .selected_video_encoding()
            .map_or(240., super::profiles::VideoEncoding::max_framerate);
        self.imp()
            .framerate_row
            .set_value(framerate.map_or(30., |x| x.value().min(max_fps)));
    }

    pub fn mark_ui_as_ready(&self) {
        self.imp()
            .stack
            .set_transition_type(gtk::StackTransitionType::Crossfade);
        self.imp().stack.set_visible_child_name("editing");
        self.imp().play_pause.grab_focus();
    }

    pub fn open_file(&self, path: &Path) {
        self.imp()
            .selected_video_path
            .replace(Some(path.to_path_buf()));

        self.imp()
            .stack
            .set_transition_type(gtk::StackTransitionType::None);
        self.imp().stack.set_visible_child_name("loading");

        self.create_ui(path);
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::from_appdata(
            "/io/gitlab/adhami3310/Footage/io.gitlab.adhami3310.Footage.metainfo.xml",
            Some("1.3"),
        );

        about.set_developers(&["Khaleel Al-Adhami"]);
        about.set_artists(&["kramo https://kramo.hu"]);

        // Translators: Replace "translator-credits" with your names, one name per line
        about.set_translator_credits(&gettext("translator-credits"));

        about.present(Some(self));
    }
}

trait SettingsStore {
    fn save_window_size(&self) -> Result<(), glib::BoolError>;
    fn load_window_size(&self);
}

impl SettingsStore for AppWindow {
    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let imp = self.imp();

        let (width, height) = self.default_size();

        imp.settings.set_int("window-width", width)?;
        imp.settings.set_int("window-height", height)?;

        imp.settings
            .set_boolean("is-maximized", self.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let imp = self.imp();

        let width = imp.settings.int("window-width");
        let height = imp.settings.int("window-height");
        let is_maximized = imp.settings.boolean("is-maximized");

        self.set_default_size(width, height);

        if is_maximized {
            self.maximize();
        }
    }
}

fn generate_width_from_height(height: u32, image_dim: Dimensions<u32>) -> u32 {
    ((f64::from(height) * (image_dim.width_f64()) / (image_dim.height_f64())).round() as i32)
        .cast_unsigned()
}

fn generate_height_from_width(width: u32, image_dim: Dimensions<u32>) -> u32 {
    ((f64::from(width) * (image_dim.height_f64()) / (image_dim.width_f64())).round() as i32)
        .cast_unsigned()
}
