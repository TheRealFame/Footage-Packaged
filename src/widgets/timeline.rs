// https://gitlab.gnome.org/YaLTeR/video-trimmer/-/blob/master/src/timeline.rs

use std::time::Duration;

use glib::subclass::prelude::*;
use gtk::glib;

/// The trimmed-in/out selection on the timeline.
#[derive(Debug, Clone, Copy)]
pub struct TimeRange {
    pub start: Duration,
    pub end: Duration,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum DragType {
    Playback,
    Start,
    End,
}

/// Which edge of the selection a keyboard nudge moves.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Handle {
    Start,
    End,
}

/// Direction of a keyboard nudge along the timeline.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Nudge {
    Back,
    Forward,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CursorType {
    Normal,
    StartEnd,
}

impl CursorType {
    const fn gtk_cursor_name(self) -> &'static str {
        match self {
            Self::Normal => "default",
            Self::StartEnd => "col-resize",
        }
    }
}

mod imp {
    use super::*;
    use glib::{clone, subclass::Signal};
    use gtk::{
        CompositeTemplate,
        gdk::{self, Key},
        prelude::*,
        subclass::prelude::*,
    };
    use once_cell::unsync::OnceCell;
    use std::cell::Cell;

    const TOLERANCE: f64 = 12.;
    const TIMELINE_KEYBOARD_MOVE: Duration = Duration::from_millis(250);

    #[derive(Debug, CompositeTemplate)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/timeline.ui")]
    pub struct Timeline {
        #[template_child]
        box_timeline_position: TemplateChild<gtk::Box>,
        #[template_child]
        box_timeline_selection: TemplateChild<gtk::Box>,
        #[template_child]
        box_wrapper: TemplateChild<gtk::Box>,
        #[template_child]
        left_handle: TemplateChild<gtk::Button>,
        #[template_child]
        right_handle: TemplateChild<gtk::Button>,

        position: Cell<Duration>,
        duration: Cell<Duration>,
        range: Cell<Option<TimeRange>>,
        gesture_drag: OnceCell<gtk::GestureDrag>,
        drag_start: Cell<f64>,
        drag_type: Cell<Option<DragType>>,
        cursor_type: Cell<CursorType>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Timeline {
        const NAME: &'static str = "Timeline";
        type Type = super::Timeline;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("timeline");
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }

        fn new() -> Self {
            Self {
                box_timeline_position: TemplateChild::default(),
                box_timeline_selection: TemplateChild::default(),
                box_wrapper: TemplateChild::default(),
                left_handle: TemplateChild::default(),
                right_handle: TemplateChild::default(),

                position: Cell::new(Duration::ZERO),
                duration: Cell::new(Duration::ZERO),
                range: Cell::new(Some(TimeRange {
                    start: Duration::ZERO,
                    end: Duration::ZERO,
                })),
                gesture_drag: OnceCell::new(),
                drag_start: Cell::new(0.),
                drag_type: Cell::new(None),
                cursor_type: Cell::new(CursorType::Normal),
            }
        }
    }

    impl ObjectImpl for Timeline {
        fn signals() -> &'static [Signal] {
            use once_cell::sync::Lazy;
            static SIGNALS: Lazy<[Signal; 3]> = Lazy::new(|| {
                [
                    Signal::builder("set-range")
                        .param_types([glib::Type::F64, glib::Type::F64])
                        .build(),
                    Signal::builder("set-position")
                        .param_types([glib::Type::F64])
                        .build(),
                    Signal::builder("moving").build(),
                ]
            });

            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            let obj = self.obj();
            self.parent_constructed();

            // Invisible until we get duration.
            self.box_timeline_position.set_child_visible(false);
            self.box_timeline_selection.set_child_visible(false);

            // For some reason doesn't work from the .ui file.
            obj.set_overflow(gtk::Overflow::Hidden);

            self.setup_drag_gesture();
            self.setup_motion_event();
            self.setup_keyboard_events();
        }

        fn dispose(&self) {
            let obj = self.obj();
            while let Some(child) = obj.first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Timeline {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let duration = self.duration.get();
            if duration.is_zero() {
                return;
            }

            let duration_secs = duration.as_secs_f64();
            let time_to_x = |t: Duration| {
                ((t.as_secs_f64() / duration_secs).clamp(0., 1.) * f64::from(width)) as i32
            };

            let position = self.position.get();
            let x = time_to_x(position);
            let position_width = self
                .box_timeline_position
                .measure(gtk::Orientation::Horizontal, -1)
                .0;
            let position_height = self
                .box_timeline_position
                .measure(gtk::Orientation::Vertical, position_width)
                .0
                .max(height);

            self.box_timeline_position.size_allocate(
                &gtk::Allocation::new(x - position_width / 2, 0, position_width, position_height),
                baseline,
            );

            if let Some(TimeRange { start, end }) = self.range.get() {
                let x = time_to_x(start);
                let x_end = time_to_x(end);

                let selection_width = self
                    .box_timeline_selection
                    .measure(gtk::Orientation::Horizontal, -1)
                    .0
                    .max(x_end - x);
                let selection_height = self
                    .box_timeline_selection
                    .measure(gtk::Orientation::Vertical, selection_width)
                    .0
                    .max(height);

                self.box_timeline_selection.size_allocate(
                    &gtk::Allocation::new(x, 0, selection_width, selection_height),
                    baseline,
                );
            }
        }
    }

    impl Timeline {
        /// Installs the pointer-drag gesture that seeks playback or resizes the selection.
        fn setup_drag_gesture(&self) {
            let obj = self.obj();

            let gesture_drag = gtk::GestureDrag::new();
            gesture_drag.connect_drag_begin({
                let obj = obj.downgrade();
                move |_, x, y| {
                    obj.upgrade().unwrap().imp().on_drag_start(x, y);
                }
            });
            gesture_drag.connect_drag_update({
                let obj = obj.downgrade();
                move |_, offset_x, offset_y| {
                    obj.upgrade()
                        .unwrap()
                        .imp()
                        .on_drag_update(offset_x, offset_y);
                }
            });
            gesture_drag.connect_drag_end({
                let obj = obj.downgrade();
                move |_, _, _| {
                    obj.upgrade().unwrap().imp().on_drag_end();
                }
            });
            obj.add_controller(gesture_drag.clone());
            self.gesture_drag.set(gesture_drag).unwrap();
        }

        /// Installs the motion controller that switches the cursor to a resize arrow near a handle.
        fn setup_motion_event(&self) {
            let obj = self.obj();

            let event_controller_motion = gtk::EventControllerMotion::new();
            event_controller_motion.connect_motion({
                let obj = obj.downgrade();
                move |_, x, y| {
                    obj.upgrade().unwrap().imp().on_motion(x, y);
                }
            });
            obj.add_controller(event_controller_motion);
        }

        /// Attaches an arrow-key controller to each handle so keyboard users can nudge the selection.
        fn setup_keyboard_events(&self) {
            self.left_handle
                .add_controller(self.event_controller_key(Handle::Start));
            self.right_handle
                .add_controller(self.event_controller_key(Handle::End));
        }

        /// Key controller for a handle: left/right arrows nudge it back/forward along the timeline.
        fn event_controller_key(&self, handle: Handle) -> gtk::EventControllerKey {
            let event_controller_keyboard = gtk::EventControllerKey::new();
            event_controller_keyboard.connect_key_pressed(clone!(
                #[weak(rename_to = this)]
                self,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |_, key, _, _| {
                    let direction = match key {
                        Key::Left => Nudge::Back,
                        Key::Right => Nudge::Forward,
                        _ => return glib::Propagation::Proceed,
                    };
                    this.nudge(handle, direction);
                    glib::Propagation::Stop
                }
            ));
            event_controller_keyboard
        }

        pub fn set_range(&self, range: Option<TimeRange>) {
            self.range.set(range);
            self.refresh();
        }

        pub fn refresh(&self) {
            let obj = self.obj();

            let duration = self.duration.get();
            if duration.is_zero() {
                self.box_timeline_position.set_child_visible(false);
                self.box_timeline_selection.set_child_visible(false);
                obj.queue_allocate();
                return;
            }

            self.box_timeline_position.set_child_visible(true);
            self.box_timeline_selection
                .set_child_visible(self.range.get().is_some());

            obj.queue_allocate();
        }

        /// The x-coordinates of the selection's start and end handles in widget space,
        /// or `None` when no range is set.
        fn selection_edges(&self) -> Option<(f64, f64)> {
            self.range.get()?;
            let allocation = self
                .box_timeline_selection
                .compute_bounds(&self.box_timeline_selection.parent().unwrap())
                .unwrap();
            let start = f64::from(allocation.x());
            let end = f64::from(allocation.x() + allocation.width());
            Some((start, end))
        }

        fn on_drag_start(&self, x: f64, _y: f64) {
            self.emit_moving();
            self.drag_start.set(x);
            self.drag_type.set(Some(DragType::Playback));

            if let Some((start, end)) = self.selection_edges() {
                if (x - end).abs() <= TOLERANCE {
                    self.drag_type.set(Some(DragType::End));
                    self.drag_start.set(end);
                } else if (x - start).abs() <= TOLERANCE {
                    self.drag_type.set(Some(DragType::Start));
                    self.drag_start.set(start);
                }
            }

            self.on_drag_update(0., 0.);
        }

        fn on_drag_update(&self, offset_x: f64, _offset_y: f64) {
            let obj = self.obj();

            let x = self.drag_start.get() + offset_x;
            let width = f64::from(obj.width());

            // Sanitize (this can get weird values when resizing the window while dragging).
            let x = x.clamp(0., width);
            let value = x / width;

            let duration = self.duration.get();

            if !duration.is_zero() {
                let time = Duration::from_secs_f64(duration.as_secs_f64() * value);

                // Update the position for responsive seeking.
                self.set_position(time);
                obj.queue_allocate();

                let Some(TimeRange { start, end }) = self.range.get() else {
                    return;
                };

                let (start, end) = match self.drag_type.get().unwrap() {
                    DragType::Start => {
                        if time <= end {
                            (time, end)
                        } else {
                            self.drag_type.set(Some(DragType::End));
                            (end, time)
                        }
                    }
                    DragType::End => {
                        if time >= start {
                            // self.set_position(start);
                            (start, time)
                        } else {
                            self.drag_type.set(Some(DragType::Start));
                            (time, start)
                        }
                    }
                    DragType::Playback => return,
                };

                self.range.set(Some(TimeRange { start, end }));
                self.refresh();
            }
        }

        /// Moves one handle of the selection by [`TIMELINE_KEYBOARD_MOVE`], clamped so it can't cross
        /// the other handle or leave the clip, then seeks playback to the moved handle.
        fn nudge(&self, handle: Handle, direction: Nudge) {
            let TimeRange { start, end } = self.range.get().unwrap();

            let (range, anchor) = match handle {
                Handle::Start => {
                    let start = match direction {
                        Nudge::Forward => (start + TIMELINE_KEYBOARD_MOVE).min(end),
                        Nudge::Back => start.saturating_sub(TIMELINE_KEYBOARD_MOVE),
                    };
                    (TimeRange { start, end }, start)
                }
                Handle::End => {
                    let end = match direction {
                        Nudge::Forward => (end + TIMELINE_KEYBOARD_MOVE).min(self.duration.get()),
                        Nudge::Back => end.saturating_sub(TIMELINE_KEYBOARD_MOVE).max(start),
                    };
                    (TimeRange { start, end }, end)
                }
            };

            self.range.set(Some(range));
            self.commit(anchor);
        }

        /// Seeks playback to `anchor` and emits the current range and position.
        fn commit(&self, anchor: Duration) {
            self.set_position(anchor);
            let TimeRange { start, end } = self.range.get().unwrap();
            self.emit_set_range(start, end);
            self.emit_set_position(self.position.get());
        }

        fn on_drag_end(&self) {
            let TimeRange { start, end } = self.range.get().unwrap();
            self.emit_set_range(start, end);
            self.emit_set_position(self.position.get());
        }

        fn emit_moving(&self) {
            self.obj().emit_by_name::<()>("moving", &[]);
        }

        fn emit_set_range(&self, start: Duration, end: Duration) {
            self.obj()
                .emit_by_name::<()>("set-range", &[&start.as_secs_f64(), &end.as_secs_f64()]);
        }

        fn emit_set_position(&self, position: Duration) {
            self.obj()
                .emit_by_name::<()>("set-position", &[&position.as_secs_f64()]);
        }

        pub fn set_duration(&self, duration: Duration) {
            self.duration.set(duration);
            self.refresh();
        }

        pub fn set_position(&self, position: Duration) {
            let TimeRange { start, end } = self.range.get().unwrap();
            let position = position.clamp(start, end);
            self.position.set(position);
            self.refresh();
        }

        fn on_motion(&self, x: f64, _y: f64) {
            let obj = self.obj();

            // Don't change the cursor while in drag.
            if self.gesture_drag.get().unwrap().is_active() {
                return;
            }

            let resizing_cursor = self.selection_edges().is_some_and(|(start, end)| {
                (x - end).abs() <= TOLERANCE || (x - start).abs() <= TOLERANCE
            });

            let cursor_type = if resizing_cursor {
                CursorType::StartEnd
            } else {
                CursorType::Normal
            };

            if self.cursor_type.get() != cursor_type {
                let cursor = gdk::Cursor::from_name(cursor_type.gtk_cursor_name(), None).unwrap();
                obj.set_cursor(Some(&cursor));
                self.cursor_type.set(cursor_type);
            }
        }
    }
}

glib::wrapper! {
    pub struct Timeline(ObjectSubclass<imp::Timeline>)
        @extends gtk::Widget,
        @implements gtk::Root, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Timeline {
    pub fn set_range(&self, range: Option<TimeRange>) {
        self.imp().set_range(range);
    }

    pub fn set_duration(&self, duration: Duration) {
        self.imp().set_duration(duration);
    }

    pub fn set_position(&self, position: Duration) {
        self.imp().set_position(position);
    }
}
