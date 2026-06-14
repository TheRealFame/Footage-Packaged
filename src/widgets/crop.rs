// https://gitlab.gnome.org/YaLTeR/video-trimmer/-/blob/master/src/timeline.rs

use enum_map::{Enum, EnumMap, enum_map};
use fraction::ToPrimitive;
use gtk::gdk::Key;
use gtk::{gdk, prelude::*, subclass::prelude::*};
use gtk::{gio, glib};
use num_traits::real::Real;
use ordered_float::NotNan;

use crate::orientation::VideoOrientationTransformation;

#[derive(Debug, Clone, Copy, Default)]
pub struct Selection {
    pub top: NotNan<f64>,
    pub right: NotNan<f64>,
    pub bottom: NotNan<f64>,
    pub left: NotNan<f64>,
}

impl Selection {
    /// Convert the normalized 0–1 insets into pixel insets (top, right, bottom, left) for a region of the given size.
    pub fn for_dimensions(
        &self,
        width: NotNan<f64>,
        height: NotNan<f64>,
    ) -> (NotNan<f64>, NotNan<f64>, NotNan<f64>, NotNan<f64>) {
        (
            self.top * height,
            self.right * width,
            self.bottom * height,
            self.left * width,
        )
    }

    /// Same as [`Self::for_dimensions`], rounded and cast to `f32`.
    pub fn for_dimensions_f32(
        &self,
        width: NotNan<f32>,
        height: NotNan<f32>,
    ) -> (NotNan<f32>, NotNan<f32>, NotNan<f32>, NotNan<f32>) {
        let (top, right, bottom, left) =
            self.for_dimensions(NotNan::from(width), NotNan::from(height));

        (
            Real::round(top.as_f32()),
            Real::round(right.as_f32()),
            Real::round(bottom.as_f32()),
            Real::round(left.as_f32()),
        )
    }

    /// Same as [`Self::for_dimensions`], rounded and cast to `i32`.
    pub fn for_dimensions_i32(&self, width: i32, height: i32) -> (i32, i32, i32, i32) {
        let (top, right, bottom, left) =
            self.for_dimensions(NotNan::from(width), NotNan::from(height));

        (
            top.to_i32()
                .expect("since height is i32, and top is clamped to [0, height], this can't fail"),
            right
                .to_i32()
                .expect("since width is i32, and right is clamped to [0, width], this can't fail"),
            bottom.to_i32().expect(
                "since height is i32, and bottom is clamped to [0, height], this can't fail",
            ),
            left.to_i32()
                .expect("since width is i32, and left is clamped to [0, width], this can't fail"),
        )
    }

    pub const fn flipped_horizontal(&self) -> Self {
        Self {
            left: self.right,
            right: self.left,
            ..*self
        }
    }

    pub const fn flipped_vertical(&self) -> Self {
        Self {
            top: self.bottom,
            bottom: self.top,
            ..*self
        }
    }

    pub const fn rotated_right(&self) -> Self {
        Self {
            top: self.left,
            right: self.top,
            bottom: self.right,
            left: self.bottom,
        }
    }

    pub const fn rotated_left(&self) -> Self {
        Self {
            top: self.right,
            right: self.bottom,
            bottom: self.left,
            left: self.top,
        }
    }

    pub const fn transformed(&self, transformation: VideoOrientationTransformation) -> Self {
        match transformation {
            VideoOrientationTransformation::RotateRight => self.rotated_right(),
            VideoOrientationTransformation::RotateLeft => self.rotated_left(),
            VideoOrientationTransformation::HorizontalFlip => self.flipped_horizontal(),
            VideoOrientationTransformation::VerticalFlip => self.flipped_vertical(),
        }
    }

    /// Returns a copy with `side` nudged by `amount`, clamped so the crop box can't invert or exceed the frame.
    fn move_side(
        &self,
        side: Side,
        direction: MoveDirection,
        amount: NotNan<f64>,
        min_size: NotNan<f64>,
    ) -> Self {
        let amount = match direction {
            MoveDirection::Positive => amount,
            MoveDirection::Negative => -amount,
        };

        match side {
            Side::Top => Self {
                top: (self.top + amount)
                    .clamp(NotNan::from(0), NotNan::from(1) - self.bottom - min_size),
                ..*self
            },
            Side::Right => Self {
                // Note the minus sign, because moving right means reducing the right crop.
                right: (self.right - amount)
                    .clamp(NotNan::from(0), NotNan::from(1) - self.left - min_size),
                ..*self
            },
            Side::Bottom => Self {
                // Note the minus sign, because moving down means reducing the bottom crop.
                bottom: (self.bottom - amount)
                    .clamp(NotNan::from(0), NotNan::from(1) - self.top - min_size),
                ..*self
            },
            Side::Left => Self {
                left: (self.left + amount)
                    .clamp(NotNan::from(0), NotNan::from(1) - self.right - min_size),
                ..*self
            },
        }
    }
}

enum SideOrientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Enum)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    const fn orientation(self) -> SideOrientation {
        match self {
            Self::Top | Self::Bottom => SideOrientation::Vertical,
            Self::Left | Self::Right => SideOrientation::Horizontal,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// The two sides that meet at this corner, returned as `(vertical, horizontal)`.
    const fn sides(self) -> (Side, Side) {
        match self {
            Self::TopLeft => (Side::Top, Side::Left),
            Self::TopRight => (Side::Top, Side::Right),
            Self::BottomLeft => (Side::Bottom, Side::Left),
            Self::BottomRight => (Side::Bottom, Side::Right),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum DragType {
    Side(Side),
    Corner(Corner),
    Whole,
}

impl DragType {
    /// GDK cursor name to display for this hover state.
    const fn gtk_cursor_name(self) -> &'static str {
        match self {
            Self::Side(Side::Top) => "n-resize",
            Self::Side(Side::Bottom) => "s-resize",
            Self::Side(Side::Left) => "w-resize",
            Self::Side(Side::Right) => "e-resize",
            Self::Corner(Corner::TopRight) => "ne-resize",
            Self::Corner(Corner::BottomLeft) => "sw-resize",
            Self::Corner(Corner::TopLeft) => "nw-resize",
            Self::Corner(Corner::BottomRight) => "se-resize",
            Self::Whole => "move",
        }
    }

    fn sides(self) -> Vec<Side> {
        match self {
            Self::Side(side) => vec![side],
            Self::Corner(corner) => vec![corner.sides().0, corner.sides().1],
            Self::Whole => vec![Side::Top, Side::Right, Side::Bottom, Side::Left],
        }
    }
}

// Positive means moving down for top and bottom, and right for left and right. Negative means the opposite.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum MoveDirection {
    Positive,
    Negative,
}

impl Selection {}

struct RelativeCoordinates {
    x: NotNan<f64>,
    y: NotNan<f64>,
}

struct RelativeSelectionCoordinates {
    top_left: RelativeCoordinates,
    bottom_right: RelativeCoordinates,
}

impl RelativeSelectionCoordinates {
    fn does_contain(&self, x: NotNan<f64>, y: NotNan<f64>) -> bool {
        x >= self.top_left.x
            && x <= self.bottom_right.x
            && y >= self.top_left.y
            && y <= self.bottom_right.y
    }
}

struct DragDelta(EnumMap<Side, NotNan<f64>>);

impl DragDelta {
    fn new(
        x: NotNan<f64>,
        y: NotNan<f64>,
        relative_selection_coordinates: &RelativeSelectionCoordinates,
    ) -> Self {
        Self(enum_map! {
            Side::Top => num_traits::Signed::abs(&(y - relative_selection_coordinates.top_left.y)),
            Side::Right => num_traits::Signed::abs(&(x - relative_selection_coordinates.bottom_right.x)),
            Side::Bottom => num_traits::Signed::abs(&(y - relative_selection_coordinates.bottom_right.y)),
            Side::Left => num_traits::Signed::abs(&(x - relative_selection_coordinates.top_left.x)),
        })
    }

    fn closest_sides(&self, threshold: NotNan<f64>) -> Vec<Side> {
        self.0
            .iter()
            .filter_map(|(side, delta)| (*delta <= threshold).then_some(side))
            .collect()
    }
}

trait KeyMovement {
    fn move_side(&self) -> Option<(SideOrientation, MoveDirection)>;
}

impl KeyMovement for Key {
    fn move_side(&self) -> Option<(SideOrientation, MoveDirection)> {
        match *self {
            Self::Up => Some((SideOrientation::Vertical, MoveDirection::Negative)),
            Self::Down => Some((SideOrientation::Vertical, MoveDirection::Positive)),
            Self::Left => Some((SideOrientation::Horizontal, MoveDirection::Negative)),
            Self::Right => Some((SideOrientation::Horizontal, MoveDirection::Positive)),
            _ => None,
        }
    }
}

mod imp {
    use super::*;
    use glib::{clone, subclass::Signal};
    use gtk::{
        CompositeTemplate,
        gdk::RGBA,
        gsk::{self, FillRule},
    };
    use log::error;
    use once_cell::unsync::OnceCell;
    use ordered_float::NotNan;
    use std::cell::Cell;

    const TOLERANCE: NotNan<f64> = unsafe { NotNan::new_unchecked(15.) };
    const PIXEL_KEYBOARD_MOVE: NotNan<f64> = unsafe { NotNan::new_unchecked(6.) };
    const MIN_SIZE: NotNan<f64> = unsafe { NotNan::new_unchecked(0.05) };

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

            let (width, height) = (
                NotNan::from(self.obj().width()).as_f32(),
                NotNan::from(self.obj().height()).as_f32(),
            );

            let crop = self.current_selection.get();

            let (top, right, bottom, left) = crop.for_dimensions_f32(width, height);

            let outer_crop_box_path = {
                let outer_crop_box_builder = gsk::PathBuilder::new();
                // Draw the outer rectangle covering the whole widget.
                outer_crop_box_builder.move_to(0., 0.);
                outer_crop_box_builder.line_to(*width, 0.);
                outer_crop_box_builder.line_to(*width, *height);
                outer_crop_box_builder.line_to(0., *height);
                outer_crop_box_builder.close();
                // Draw the inner rectangle representing the crop box.
                // EvenOdd fill rule will make sure that the area between the inner and outer rectangles is filled,
                // while the area inside the inner rectangle is not.
                outer_crop_box_builder.move_to(*left, *top);
                outer_crop_box_builder.line_to(*(width - right), *top);
                outer_crop_box_builder.line_to(*(width - right), *(height - bottom));
                outer_crop_box_builder.line_to(*left, *(height - bottom));
                outer_crop_box_builder.close();
                outer_crop_box_builder.to_path()
            };

            snapshot.append_fill(&outer_crop_box_path, FillRule::EvenOdd, &gray);
            self.obj()
                .snapshot_child(&self.obj().first_child().unwrap(), snapshot);
        }
    }

    impl Crop {
        /// Installs the pointer-drag gesture that resizes or moves the crop box.
        fn setup_drag_gesture(&self) {
            let obj = self.obj();

            let gesture_drag = gtk::GestureDrag::new();
            gesture_drag.connect_drag_begin({
                let obj = obj.downgrade();
                move |_, x, y| {
                    if let Ok(x) = NotNan::new(x)
                        && let Ok(y) = NotNan::new(y)
                    {
                        let obj = obj.upgrade().unwrap();
                        let imp = obj.imp();
                        imp.on_drag_start(x, y);
                    }
                }
            });
            gesture_drag.connect_drag_update({
                let obj = obj.downgrade();
                move |_, offset_x, offset_y| {
                    if let Ok(offset_x) = NotNan::new(offset_x)
                        && let Ok(offset_y) = NotNan::new(offset_y)
                    {
                        let obj = obj.upgrade().unwrap();
                        let imp = obj.imp();
                        imp.on_drag_update(offset_x, offset_y);
                    }
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

        /// Installs the motion controller that updates the cursor shape as the pointer nears an edge.
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

        /// Attaches an arrow-key controller to each corner handle so keyboard users can nudge the adjacent sides.
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

        /// Key controller for a corner handle: vertical arrows move its vertical side, horizontal arrows its horizontal side.
        fn get_event_controller_key(&self, corner: Corner) -> gtk::EventControllerKey {
            let event_controller_keyboard = gtk::EventControllerKey::new();
            let (vertical_side, horizontal_side) = corner.sides();

            event_controller_keyboard.connect_key_pressed(clone!(
                #[weak(rename_to=this)]
                self,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |_, key, _, _| {
                    match key.move_side() {
                        Some((side, direction)) => {
                            this.move_crop_box(
                                match side {
                                    SideOrientation::Vertical => vertical_side,
                                    SideOrientation::Horizontal => horizontal_side,
                                },
                                direction,
                            );
                            glib::Propagation::Stop
                        }
                        None => glib::Propagation::Proceed,
                    }
                }
            ));

            event_controller_keyboard
        }

        /// Pixel coordinates of the four crop edges in widget space
        fn positons(&self) -> RelativeSelectionCoordinates {
            let crop = self.current_selection.get();
            let (width, height) = (self.obj().width(), self.obj().height());
            RelativeSelectionCoordinates {
                top_left: RelativeCoordinates {
                    x: crop.left * NotNan::from(width),
                    y: crop.top * NotNan::from(height),
                },
                bottom_right: RelativeCoordinates {
                    x: (NotNan::from(1) - crop.right) * NotNan::from(width),
                    y: (NotNan::from(1) - crop.bottom) * NotNan::from(height),
                },
            }
        }

        /// Classifies `(x, y)` as a side, corner, whole-box, or no-op drag based on which edges fall within [`TOLERANCE`] pixels.
        fn calculate_drag_type(&self, x: NotNan<f64>, y: NotNan<f64>) -> Option<DragType> {
            let relative_selection_coordinates = self.positons();
            let drag_delta = DragDelta::new(x, y, &relative_selection_coordinates);

            match drag_delta.closest_sides(TOLERANCE).as_slice() {
                [] => {
                    if relative_selection_coordinates.does_contain(x, y) {
                        Some(DragType::Whole)
                    } else {
                        None
                    }
                }
                [side] => Some(DragType::Side(*side)),
                [side1, side2, ..] => Some(match (side1, side2) {
                    (Side::Top, Side::Left) | (Side::Left, Side::Top) => {
                        DragType::Corner(Corner::TopLeft)
                    }
                    (Side::Top, Side::Right) | (Side::Right, Side::Top) => {
                        DragType::Corner(Corner::TopRight)
                    }
                    (Side::Bottom, Side::Left) | (Side::Left, Side::Bottom) => {
                        DragType::Corner(Corner::BottomLeft)
                    }
                    (Side::Bottom, Side::Right) | (Side::Right, Side::Bottom) => {
                        DragType::Corner(Corner::BottomRight)
                    }
                    (Side::Top, _) => DragType::Side(Side::Top),
                    (Side::Bottom, _) => DragType::Side(Side::Bottom),
                    (Side::Left, _) => DragType::Side(Side::Left),
                    (Side::Right, _) => DragType::Side(Side::Right),
                }),
            }
        }

        /// Captures the initial selection and the drag mode at `(x, y)` so later updates can be applied relatively.
        fn on_drag_start(&self, x: NotNan<f64>, y: NotNan<f64>) {
            let drag_type = self.calculate_drag_type(x, y);

            if drag_type.is_some() {
                self.drag_start.set(self.current_selection.get());
                self.drag_type.set(drag_type);
                self.on_drag_update(NotNan::from(0), NotNan::from(0));
            }
        }

        /// Applies the pointer offset to the active side(s), clamped to the widget and to a minimum crop size.
        fn on_drag_update(&self, offset_x: NotNan<f64>, offset_y: NotNan<f64>) {
            if self.drag_type.get().is_none() {
                return;
            }

            let current_selection = self.current_selection.get();
            let old_selection = self.drag_start.get();

            let width =
                NotNan::from(1) - current_selection.right - current_selection.left - MIN_SIZE;
            let height =
                NotNan::from(1) - current_selection.top - current_selection.bottom - MIN_SIZE;

            let offset_x = offset_x / NotNan::from(self.obj().width());
            let offset_y = offset_y / NotNan::from(self.obj().height());

            let actual_offset_y = offset_y - (current_selection.top - old_selection.top)
                + (current_selection.bottom - old_selection.bottom);
            let actual_offset_x = offset_x - (current_selection.left - old_selection.left)
                + (current_selection.right - old_selection.right);

            let drag_type = self.drag_type.get().unwrap();

            match drag_type {
                DragType::Whole => {
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
                other => {
                    // We couldn't use Whole for this because the clamping logic is different when resizing from edges vs moving the whole box.
                    // Think of the case where two edges are nearby and we're moving the whole box. Since we are moving both edges together,
                    // if we clamped the offset for each edge independently, the delta will be clamped to the distance between them,
                    // instead of allowing them to move together until they hit the widget edge or minimum size.
                    for side in other.sides() {
                        self.current_selection
                            .set(self.current_selection.get().move_side(
                                side,
                                MoveDirection::Positive,
                                match side.orientation() {
                                    SideOrientation::Vertical => actual_offset_y,
                                    SideOrientation::Horizontal => actual_offset_x,
                                },
                                MIN_SIZE,
                            ));
                    }
                }
            }

            self.emit_crop_box_changed();

            self.obj().queue_allocate();
        }

        /// Clears the active drag so pointer motion resumes updating only the cursor.
        fn on_drag_end(&self) {
            self.drag_type.set(None);
        }

        /// Emits `crop-box-changed` with the current selection; does not mutate state.
        pub fn emit_crop_box_changed(&self) {
            let current_selection = self.current_selection.get();

            let crop_box_selection: [&dyn ToValue; 4] = [
                &current_selection.top.into_inner(),
                &current_selection.right.into_inner(),
                &current_selection.bottom.into_inner(),
                &current_selection.left.into_inner(),
            ];

            self.obj()
                .emit_by_name::<()>("crop-box-changed", &crop_box_selection);
        }

        /// Keyboard-nudges `side` by [`PIXEL_KEYBOARD_MOVE`] pixels (converted to the normalized scale) and emits the change.
        fn move_crop_box(&self, side: Side, direction: MoveDirection) {
            let (width, height) = (self.obj().width(), self.obj().height());

            let current_selection = self.current_selection.get();

            let amount = PIXEL_KEYBOARD_MOVE
                / match side {
                    Side::Top | Side::Bottom => NotNan::from(height),
                    Side::Left | Side::Right => NotNan::from(width),
                };

            self.current_selection
                .set(current_selection.move_side(side, direction, amount, MIN_SIZE));

            self.emit_crop_box_changed();

            self.obj().queue_allocate();
        }

        /// Updates the widget cursor to match the drag that a press at `(x, y)` would initiate.
        fn on_motion(&self, x: f64, y: f64) {
            let (Ok(x), Ok(y)) = (NotNan::new(x), NotNan::new(y)) else {
                error!("Motion event had invalid coordinates: ({x}, {y})");
                return;
            };
            let drag_type = self.calculate_drag_type(x, y);

            let cursor = gdk::Cursor::from_name(
                drag_type.map_or("default", DragType::gtk_cursor_name),
                None,
            )
            .unwrap();
            self.obj().set_cursor(Some(&cursor));
        }
    }
}

glib::wrapper! {
    pub struct Crop(ObjectSubclass<imp::Crop>)
        @extends gtk::Widget,
        @implements gtk::Buildable, gtk::Accessible, gtk::ConstraintTarget, gio::ActionMap, gio::ActionGroup, gtk::Root;
}

impl Crop {
    /// Current crop as normalized 0–1 insets from each edge of the source frame.
    pub fn proportions(&self) -> Selection {
        self.imp().current_selection.get()
    }

    /// Replace the selection, emit `crop-box-changed`, and trigger a re-layout.
    pub fn set_proportions(&self, proportions: Selection) {
        self.imp().current_selection.set(proportions);
        self.imp().emit_crop_box_changed();
        self.queue_allocate();
    }

    /// Selection remapped for the given orientation change so the cropped region tracks the same pixels after the transform.
    pub fn proportions_transformed(
        &self,
        transformation: VideoOrientationTransformation,
    ) -> Selection {
        self.proportions().transformed(transformation)
    }

    /// Clear the crop back to the full frame.
    pub fn reset(&self) {
        self.set_proportions(Selection::default());
    }
}
