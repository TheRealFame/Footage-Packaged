// https://gitlab.gnome.org/YaLTeR/video-trimmer/-/blob/master/src/timeline.rs

use gtk::{gdk, prelude::*, subclass::prelude::*};
use gtk::{gio, glib};

use crate::orientation::VideoOrientationTransformation;

#[derive(Debug, Clone, Copy, Default)]
pub struct Selection {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Selection {
    pub fn for_dimensions(&self, width: f64, height: f64) -> (f64, f64, f64, f64) {
        (
            self.top * height,
            self.right * width,
            self.bottom * height,
            self.left * width,
        )
    }

    pub fn for_dimensions_f32(&self, width: f32, height: f32) -> (f32, f32, f32, f32) {
        let (top, right, bottom, left) = self.for_dimensions(f64::from(width), f64::from(height));

        #[allow(clippy::cast_possible_truncation)]
        (
            top.round() as f32,
            right.round() as f32,
            bottom.round() as f32,
            left.round() as f32,
        )
    }

    pub fn for_dimensions_i32(&self, width: i32, height: i32) -> (i32, i32, i32, i32) {
        let (top, right, bottom, left) = self.for_dimensions(f64::from(width), f64::from(height));

        #[allow(clippy::cast_possible_truncation)]
        (
            top.round() as i32,
            right.round() as i32,
            bottom.round() as i32,
            left.round() as i32,
        )
    }
}

mod imp {
    use super::*;
    use glib::{clone, subclass::Signal};
    use gtk::{
        CompositeTemplate,
        gdk::{Key, RGBA},
        gsk::{self, FillRule},
    };
    use itertools::Itertools;
    use once_cell::unsync::OnceCell;
    use ordered_float::NotNan;
    use std::cell::Cell;

    const TOLERANCE: f64 = 15.;
    const PIXEL_KEYBOARD_MOVE: f64 = 6.;

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum DragType {
        Top,
        Right,
        Bottom,
        Left,
        TopRight,
        BottomRight,
        BottomLeft,
        TopLeft,
        All,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
    enum CursorType {
        #[default]
        Normal,
        Top,
        Bottom,
        Left,
        Right,
        TopRight,
        BottomRight,
        BottomLeft,
        TopLeft,
        All,
    }

    impl CursorType {
        fn gtk_cursor_name(self) -> &'static str {
            match self {
                CursorType::Normal => "default",
                CursorType::Top => "n-resize",
                CursorType::Bottom => "s-resize",
                CursorType::Left => "w-resize",
                CursorType::Right => "e-resize",
                CursorType::TopRight => "ne-resize",
                CursorType::BottomLeft => "sw-resize",
                CursorType::TopLeft => "nw-resize",
                CursorType::BottomRight => "se-resize",
                CursorType::All => "move",
            }
        }
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum Side {
        Top,
        Right,
        Bottom,
        Left,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum Corner {
        TopLeft,
        TopRight,
        BottomLeft,
        BottomRight,
    }

    impl Corner {
        fn sides(self) -> (Side, Side) {
            match self {
                Corner::TopLeft => (Side::Top, Side::Left),
                Corner::TopRight => (Side::Top, Side::Right),
                Corner::BottomLeft => (Side::Bottom, Side::Left),
                Corner::BottomRight => (Side::Bottom, Side::Right),
            }
        }
    }

    // Positive means moving down for top and bottom, and right for left and right. Negative means the opposite.
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum MoveDirection {
        Positive,
        Negative,
    }

    impl Selection {
        fn move_side(&self, side: Side, direction: MoveDirection, amount: f64) -> Self {
            let amount = match direction {
                MoveDirection::Positive => amount,
                MoveDirection::Negative => -amount,
            };

            match side {
                Side::Top => Self {
                    top: (self.top + amount).clamp(0., 1. - self.bottom),
                    ..*self
                },
                Side::Right => Self {
                    // Note the minus sign, because moving right means reducing the right crop.
                    right: (self.right - amount).clamp(0., 1. - self.left),
                    ..*self
                },
                Side::Bottom => Self {
                    // Note the minus sign, because moving down means reducing the bottom crop.
                    bottom: (self.bottom - amount).clamp(0., 1. - self.top),
                    ..*self
                },
                Side::Left => Self {
                    left: (self.left + amount).clamp(0., 1. - self.right),
                    ..*self
                },
            }
        }
    }

    #[derive(Debug, CompositeTemplate, Default)]
    #[template(resource = "/io/gitlab/adhami3310/Footage/blueprints/crop.ui")]
    pub struct Crop {
        #[template_child]
        pub inner_crop_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub top: TemplateChild<gtk::Box>,
        #[template_child]
        pub bottom: TemplateChild<gtk::Box>,
        #[template_child]
        pub container: TemplateChild<gtk::Box>,
        #[template_child]
        pub top_left: TemplateChild<gtk::Button>,
        #[template_child]
        pub top_right: TemplateChild<gtk::Button>,
        #[template_child]
        pub bottom_left: TemplateChild<gtk::Button>,
        #[template_child]
        pub bottom_right: TemplateChild<gtk::Button>,

        gesture_drag: OnceCell<gtk::GestureDrag>,
        drag_start: Cell<Selection>,
        pub current_selection: Cell<Selection>,
        drag_type: Cell<Option<DragType>>,
        cursor_type: Cell<CursorType>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Crop {
        const NAME: &'static str = "Crop";
        type Type = super::Crop;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("cropbox");
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }

        fn new() -> Self {
            Self::default()
        }
    }

    impl ObjectImpl for Crop {
        fn constructed(&self) {
            self.parent_constructed();

            self.setup_drag_gesture();
            self.setup_motion_event();
            self.setup_keyboard_events();
        }

        fn signals() -> &'static [Signal] {
            use once_cell::sync::Lazy;
            static SIGNALS: Lazy<[Signal; 1]> = Lazy::new(|| {
                [Signal::builder("crop-box-changed")
                    .param_types([
                        glib::Type::F64,
                        glib::Type::F64,
                        glib::Type::F64,
                        glib::Type::F64,
                    ])
                    .build()]
            });

            SIGNALS.as_ref()
        }

        fn dispose(&self) {
            let obj = self.obj();
            while let Some(child) = obj.first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Crop {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let crop = self.current_selection.get();
            let (top, right, bottom, left) = crop.for_dimensions_i32(width, height);
            self.container.size_allocate(
                &gtk::Allocation::new(left, top, width - left - right, height - top - bottom),
                baseline,
            );
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let gray = RGBA::builder()
                .red(0.)
                .green(0.)
                .blue(0.)
                .alpha(0.5)
                .build();

            let (width, height) = (self.obj().width() as f32, self.obj().height() as f32);

            let crop = self.current_selection.get();

            let (top, right, bottom, left) = crop.for_dimensions_f32(width, height);

            let outer_crop_box_path = {
                let outer_crop_box_builder = gsk::PathBuilder::new();
                // Draw the outer rectangle covering the whole widget.
                outer_crop_box_builder.move_to(0., 0.);
                outer_crop_box_builder.line_to(width, 0.);
                outer_crop_box_builder.line_to(width, height);
                outer_crop_box_builder.line_to(0., height);
                outer_crop_box_builder.close();
                // Draw the inner rectangle representing the crop box.
                // EvenOdd fill rule will make sure that the area between the inner and outer rectangles is filled,
                // while the area inside the inner rectangle is not.
                outer_crop_box_builder.move_to(left, top);
                outer_crop_box_builder.line_to(width - right, top);
                outer_crop_box_builder.line_to(width - right, height - bottom);
                outer_crop_box_builder.line_to(left, height - bottom);
                outer_crop_box_builder.close();
                outer_crop_box_builder.to_path()
            };

            snapshot.append_fill(&outer_crop_box_path, FillRule::EvenOdd, &gray);
            self.obj()
                .snapshot_child(&self.obj().first_child().unwrap(), snapshot);
        }
    }

    impl Crop {
        fn setup_drag_gesture(&self) {
            let obj = self.obj();

            let gesture_drag = gtk::GestureDrag::new();
            gesture_drag.connect_drag_begin({
                let obj = obj.downgrade();
                move |_, x, y| {
                    let obj = obj.upgrade().unwrap();
                    let imp = obj.imp();
                    imp.on_drag_start(x, y);
                }
            });
            gesture_drag.connect_drag_update({
                let obj = obj.downgrade();
                move |_, offset_x, offset_y| {
                    let obj = obj.upgrade().unwrap();
                    let imp = obj.imp();
                    imp.on_drag_update(offset_x, offset_y);
                }
            });
            gesture_drag.connect_drag_end({
                let obj = obj.downgrade();
                move |_, _, _| {
                    let obj = obj.upgrade().unwrap();
                    let imp = obj.imp();
                    imp.on_drag_end();
                }
            });
            obj.add_controller(gesture_drag.clone());
            self.gesture_drag.set(gesture_drag).unwrap();
        }

        fn setup_motion_event(&self) {
            let obj = self.obj();
            let event_controller_motion = gtk::EventControllerMotion::new();
            event_controller_motion.connect_motion({
                let obj = obj.downgrade();
                move |_, x, y| {
                    let obj = obj.upgrade().unwrap();
                    let imp = obj.imp();
                    imp.on_motion(x, y);
                }
            });
            obj.add_controller(event_controller_motion);
        }

        fn setup_keyboard_events(&self) {
            self.top_left
                .add_controller(self.get_event_controller_key(Corner::TopLeft));
            self.bottom_left
                .add_controller(self.get_event_controller_key(Corner::BottomLeft));
            self.top_right
                .add_controller(self.get_event_controller_key(Corner::TopRight));
            self.bottom_right
                .add_controller(self.get_event_controller_key(Corner::BottomRight));
        }

        fn get_event_controller_key(&self, corner: Corner) -> gtk::EventControllerKey {
            let event_controller_keyboard = gtk::EventControllerKey::new();
            let (vertical_side, horizontal_side) = corner.sides();

            event_controller_keyboard.connect_key_pressed(clone!(
                #[weak(rename_to=this)]
                self,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |_, key, _, _| {
                    match key {
                        Key::Up => {
                            this.move_crop_box(vertical_side, MoveDirection::Negative);
                            glib::Propagation::Stop
                        }
                        Key::Down => {
                            this.move_crop_box(vertical_side, MoveDirection::Positive);
                            glib::Propagation::Stop
                        }
                        Key::Left => {
                            this.move_crop_box(horizontal_side, MoveDirection::Negative);
                            glib::Propagation::Stop
                        }
                        Key::Right => {
                            this.move_crop_box(horizontal_side, MoveDirection::Positive);
                            glib::Propagation::Stop
                        }
                        _ => glib::Propagation::Proceed,
                    }
                }
            ));

            event_controller_keyboard
        }

        fn positons(&self) -> (f64, f64, f64, f64) {
            let crop = self.current_selection.get();
            let (width, height) = (self.obj().width(), self.obj().height());
            (
                (f64::from(height) * crop.top),
                (f64::from(width) * (1. - crop.right)),
                (f64::from(height) * (1. - crop.bottom)),
                (f64::from(width) * crop.left),
            )
        }

        fn calculate_drag_type(&self, x: f64, y: f64) -> Option<DragType> {
            let (top, right, bottom, left) = self.positons();

            let (dt, dr, db, dl) = (
                (y - top).abs(),
                (x - right).abs(),
                (y - bottom).abs(),
                (x - left).abs(),
            );

            let ((i0, v0), (i1, v1)) = [dt, dr, db, dl]
                .into_iter()
                .flat_map(NotNan::new)
                .enumerate()
                .sorted_by_key(|(_, x)| *x)
                .take(2)
                .collect_tuple()
                .unwrap();

            if v0 > NotNan::new(TOLERANCE).unwrap() {
                if x >= left && x <= right && y >= top && y <= bottom {
                    return Some(DragType::All);
                }
                return None;
            }

            if v1 > NotNan::new(TOLERANCE).unwrap() {
                if v0 < NotNan::new(TOLERANCE).unwrap() {
                    let current_drag = Some(
                        [
                            DragType::Top,
                            DragType::Right,
                            DragType::Bottom,
                            DragType::Left,
                        ][i0],
                    );

                    return current_drag;
                }
                return None;
            }

            let current_drag = match (
                [
                    DragType::Top,
                    DragType::Right,
                    DragType::Bottom,
                    DragType::Left,
                ][i0],
                [
                    DragType::Top,
                    DragType::Right,
                    DragType::Bottom,
                    DragType::Left,
                ][i1],
            ) {
                (DragType::Top, DragType::Left) | (DragType::Left, DragType::Top) => {
                    DragType::TopLeft
                }
                (DragType::Top, DragType::Right) | (DragType::Right, DragType::Top) => {
                    DragType::TopRight
                }
                (DragType::Bottom, DragType::Left) | (DragType::Left, DragType::Bottom) => {
                    DragType::BottomLeft
                }
                (DragType::Bottom, DragType::Right) | (DragType::Right, DragType::Bottom) => {
                    DragType::BottomRight
                }
                (x, _) => x,
            };

            Some(current_drag)
        }

        fn on_drag_start(&self, x: f64, y: f64) {
            let drag_type = self.calculate_drag_type(x, y);

            if drag_type.is_some() {
                self.drag_start.set(self.current_selection.get());
                self.drag_type.set(drag_type);
                self.on_drag_update(0., 0.);
            }
        }

        fn on_drag_update(&self, offset_x: f64, offset_y: f64) {
            if self.drag_type.get().is_none() {
                return;
            }

            let current_selection = self.current_selection.get();
            let old_selection = self.drag_start.get();

            let min_size = 0.05;
            let width = 1. - current_selection.right - current_selection.left - min_size;
            let height = 1. - current_selection.top - current_selection.bottom - min_size;

            let offset_x = offset_x / f64::from(self.obj().width());
            let offset_y = offset_y / f64::from(self.obj().height());

            let actual_offset_y = offset_y - (current_selection.top - old_selection.top)
                + (current_selection.bottom - old_selection.bottom);
            let actual_offset_x = offset_x - (current_selection.left - old_selection.left)
                + (current_selection.right - old_selection.right);

            let drag_type = self.drag_type.get().unwrap();

            if matches!(drag_type, DragType::All) {
                let actual_offset_y = offset_y - (current_selection.top - old_selection.top);
                let actual_offset_x = offset_x - (current_selection.left - old_selection.left);
                let offset_y = actual_offset_y
                    .clamp(-current_selection.top, height)
                    .clamp(-height, current_selection.bottom);
                let offset_x = actual_offset_x
                    .clamp(-current_selection.left, width)
                    .clamp(-width, current_selection.right);

                self.current_selection.set(Selection {
                    top: offset_y + current_selection.top,
                    right: -offset_x + current_selection.right,
                    bottom: -offset_y + current_selection.bottom,
                    left: offset_x + current_selection.left,
                });
            }

            if matches!(
                drag_type,
                DragType::Top | DragType::TopLeft | DragType::TopRight
            ) {
                let offset_y = actual_offset_y.clamp(-current_selection.top, height);

                let current_selection = self.current_selection.get();

                self.current_selection.set(Selection {
                    top: offset_y + current_selection.top,
                    right: current_selection.right,
                    bottom: current_selection.bottom,
                    left: current_selection.left,
                });
            }
            if matches!(
                drag_type,
                DragType::Bottom | DragType::BottomLeft | DragType::BottomRight
            ) {
                let offset_y = actual_offset_y.clamp(-height, current_selection.bottom);

                let current_selection = self.current_selection.get();

                self.current_selection.set(Selection {
                    top: current_selection.top,
                    right: current_selection.right,
                    bottom: -offset_y + current_selection.bottom,
                    left: current_selection.left,
                });
            }
            if matches!(
                drag_type,
                DragType::Left | DragType::BottomLeft | DragType::TopLeft
            ) {
                let offset_x = actual_offset_x.clamp(-current_selection.left, width);

                let current_selection = self.current_selection.get();

                self.current_selection.set(Selection {
                    top: current_selection.top,
                    right: current_selection.right,
                    bottom: current_selection.bottom,
                    left: offset_x + current_selection.left,
                });
            }
            if matches!(
                drag_type,
                DragType::Right | DragType::BottomRight | DragType::TopRight
            ) {
                let offset_x = actual_offset_x.clamp(-width, current_selection.right);

                let current_selection = self.current_selection.get();

                self.current_selection.set(Selection {
                    top: current_selection.top,
                    right: -offset_x + current_selection.right,
                    bottom: current_selection.bottom,
                    left: current_selection.left,
                });
            }

            let current_selection = self.current_selection.get();

            self.obj().emit_by_name::<()>(
                "crop-box-changed",
                &[
                    &current_selection.top,
                    &current_selection.right,
                    &current_selection.bottom,
                    &current_selection.left,
                ],
            );

            self.obj().queue_allocate();
        }

        fn on_drag_end(&self) {
            self.drag_type.set(None);
        }

        pub fn emit_crop_box_changed(&self) {
            let current_selection = self.current_selection.get();

            self.obj().emit_by_name::<()>(
                "crop-box-changed",
                &[
                    &current_selection.top,
                    &current_selection.right,
                    &current_selection.bottom,
                    &current_selection.left,
                ],
            );
        }

        fn move_crop_box(&self, side: Side, direction: MoveDirection) {
            let (width, height) = (self.obj().width(), self.obj().height());

            let current_selection = self.current_selection.get();

            let amount = PIXEL_KEYBOARD_MOVE
                / match side {
                    Side::Top | Side::Bottom => f64::from(height),
                    Side::Left | Side::Right => f64::from(width),
                };

            self.current_selection
                .set(current_selection.move_side(side, direction, amount));

            self.emit_crop_box_changed();

            self.obj().queue_allocate();
        }

        fn on_motion(&self, x: f64, y: f64) {
            let drag_type = self.calculate_drag_type(x, y);

            let cursor_type = match drag_type {
                Some(DragType::Top) => CursorType::Top,
                Some(DragType::Left) => CursorType::Left,
                Some(DragType::Bottom) => CursorType::Bottom,
                Some(DragType::Right) => CursorType::Right,
                Some(DragType::TopLeft) => CursorType::TopLeft,
                Some(DragType::BottomLeft) => CursorType::BottomLeft,
                Some(DragType::BottomRight) => CursorType::BottomRight,
                Some(DragType::TopRight) => CursorType::TopRight,
                Some(DragType::All) => CursorType::All,
                None => CursorType::Normal,
            };
            if self.cursor_type.get() != cursor_type {
                let cursor = gdk::Cursor::from_name(cursor_type.gtk_cursor_name(), None).unwrap();
                self.obj().set_cursor(Some(&cursor));
                self.cursor_type.set(cursor_type);
            }
        }
    }
}

glib::wrapper! {
    pub struct Crop(ObjectSubclass<imp::Crop>)
        @extends gtk::Widget,
        @implements gtk::Buildable, gtk::Accessible, gtk::ConstraintTarget, gio::ActionMap, gio::ActionGroup, gtk::Root;
}

impl Crop {
    pub fn proportions(&self) -> Selection {
        self.imp().current_selection.get()
    }

    pub fn set_proportions(&self, proportions: Selection) {
        self.imp().current_selection.set(proportions);
        self.imp().emit_crop_box_changed();
        self.queue_allocate();
    }

    fn rotate_right_proportions(&self) -> Selection {
        let p = self.proportions();
        Selection {
            top: p.left,
            right: p.top,
            bottom: p.right,
            left: p.bottom,
        }
    }

    fn rotate_left_proportions(&self) -> Selection {
        let p = self.proportions();
        Selection {
            top: p.right,
            right: p.bottom,
            bottom: p.left,
            left: p.top,
        }
    }

    fn horizontal_flip_proportions(&self) -> Selection {
        let p = self.proportions();
        Selection {
            top: p.top,
            right: p.left,
            bottom: p.bottom,
            left: p.right,
        }
    }

    fn vertical_flip_proportions(&self) -> Selection {
        let p = self.proportions();
        Selection {
            top: p.bottom,
            right: p.right,
            bottom: p.top,
            left: p.left,
        }
    }

    pub fn orientation_transformation_proportions(
        &self,
        transformation: VideoOrientationTransformation,
    ) -> Selection {
        match transformation {
            VideoOrientationTransformation::RotateRight => self.rotate_right_proportions(),
            VideoOrientationTransformation::RotateLeft => self.rotate_left_proportions(),
            VideoOrientationTransformation::HorizontalFlip => self.horizontal_flip_proportions(),
            VideoOrientationTransformation::VerticalFlip => self.vertical_flip_proportions(),
        }
    }

    pub fn reset(&self) {
        self.set_proportions(Selection::default());
    }
}
