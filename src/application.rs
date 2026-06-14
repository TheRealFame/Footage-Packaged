use glib::{ExitCode, clone};
use gst::prelude::*;
use log::{debug, info};

use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::config::{APP_ID, PKGDATADIR, PROFILE, VERSION};
use crate::info::log_debug_info;
use crate::window::AppWindow;

mod imp {
    use super::*;
    use adw::subclass::prelude::AdwApplicationImpl;

    #[derive(Debug)]
    pub struct App {
        pub settings: gio::Settings,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for App {
        const NAME: &'static str = "DarkroomApp";
        type Type = super::App;
        type ParentType = adw::Application;

        fn new() -> Self {
            Self {
                settings: gio::Settings::new(APP_ID),
            }
        }
    }

    impl ObjectImpl for App {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.setup_gactions();
            obj.setup_accels();
        }
    }

    impl ApplicationImpl for App {
        fn activate(&self) {
            debug!("Application::activate");

            self.obj().present_main_window();
        }

        fn startup(&self) {
            debug!("Application::startup");
            self.parent_startup();

            // Set icons for shell
            gtk::Window::set_default_icon_name(APP_ID);
        }

        fn open(&self, files: &[gio::File], _hint: &str) {
            if let Some(file) = files.first() {
                let application = self.obj();
                application.present_main_window();
                if let Some(window) = application.active_window() {
                    let file_path = file.path().unwrap();
                    window
                        .downcast_ref::<AppWindow>()
                        .unwrap()
                        .open_file(&file_path);
                }
            }
            debug!("Application::open");
        }
    }

    impl gtk::subclass::prelude::GtkApplicationImpl for App {}
    impl AdwApplicationImpl for App {}
}

glib::wrapper! {
    pub struct App(ObjectSubclass<imp::App>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionMap, gio::ActionGroup;
}

impl Default for App {
    fn default() -> Self {
        glib::Object::builder::<Self>()
            .property("application-id", Some(APP_ID))
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .property("resource-base-path", "/io/gitlab/adhami3310/Footage/")
            .build()
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("quit")
                .activate(clone!(
                    #[weak(rename_to=app)]
                    self,
                    move |_, _, _| {
                        app.quit();
                    }
                ))
                .build(),
            gio::ActionEntry::builder("new-window")
                .activate(clone!(
                    #[weak(rename_to=app)]
                    self,
                    move |_, _, _| {
                        app.present_main_window();
                    }
                ))
                .build(),
        ]);
    }

    // Sets up keyboard shortcuts
    fn setup_accels(&self) {
        self.set_accels_for_action("app.new-window", &["<Control>n"]);
        self.set_accels_for_action("app.quit", &["<Control>q"]);
        self.set_accels_for_action("win.close", &["<Control>w"]);
        self.set_accels_for_action("win.open", &["<Control>o"]);
        self.set_accels_for_action("win.show-help-overlay", &["<Control>question"]);
    }

    fn present_main_window(&self) {
        log_debug_info();

        let window = AppWindow::new(self);
        let window: gtk::Window = window.upcast();
        window.present();
    }

    pub fn run(&self) -> ExitCode {
        info!("Footage ({APP_ID})");
        info!("Version: {VERSION} ({PROFILE})");
        info!("Datadir: {PKGDATADIR}");

        gst::init().unwrap();
        ges::init().unwrap();

        // svtav1enc is much faster than av1enc (libaom) but ships with lower
        // rank. Boost it above av1enc so encodebin prefers it, while still
        // letting hardware encoders (rank 257+) win when available.
        if let Some(factory) = gst::ElementFactory::find("svtav1enc") {
            factory.set_rank(gst::Rank::PRIMARY + 1);
        }

        ApplicationExtManual::run(self)
    }
}
