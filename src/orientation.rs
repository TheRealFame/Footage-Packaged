#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoOrientation {
    #[default]
    Identity,
    R90,
    R180,
    R270,
    FlippedIdentity,
    FR90,
    FR180,
    FR270,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoOrientationTransformation {
    RotateRight,
    RotateLeft,
    HorizontalFlip,
    VerticalFlip,
}

impl VideoOrientationTransformation {
    pub fn does_swap_width_height(self) -> bool {
        matches!(self, Self::RotateRight | Self::RotateLeft)
    }
}

use VideoOrientation::{FR90, FR180, FR270, FlippedIdentity, Identity, R90, R180, R270};

impl VideoOrientation {
    pub fn transform(self, transformation: VideoOrientationTransformation) -> Self {
        match transformation {
            VideoOrientationTransformation::RotateRight => self.rotate_right(),
            VideoOrientationTransformation::RotateLeft => self.rotate_left(),
            VideoOrientationTransformation::HorizontalFlip => self.horizontal_flip(),
            VideoOrientationTransformation::VerticalFlip => self.vertical_flip(),
        }
    }

    fn rotate_right(self) -> Self {
        match self {
            Identity => R90,
            R90 => R180,
            R180 => R270,
            R270 => Identity,
            FlippedIdentity => FR90,
            FR90 => FR180,
            FR180 => FR270,
            FR270 => FlippedIdentity,
        }
    }

    fn rotate_left(self) -> Self {
        match self {
            R90 => Identity,
            R180 => R90,
            R270 => R180,
            Identity => R270,
            FR90 => FlippedIdentity,
            FR180 => FR90,
            FR270 => FR180,
            FlippedIdentity => FR270,
        }
    }

    fn horizontal_flip(self) -> Self {
        match self {
            Identity => FlippedIdentity,
            FlippedIdentity => Identity,
            R90 => FR270,
            FR270 => R90,
            R180 => FR180,
            FR180 => R180,
            R270 => FR90,
            FR90 => R270,
        }
    }

    fn vertical_flip(self) -> Self {
        match self {
            Identity => FR180,
            FR180 => Identity,
            R90 => FR90,
            FR90 => R90,
            R180 => FlippedIdentity,
            FlippedIdentity => R180,
            R270 => FR270,
            FR270 => R270,
        }
    }

    pub fn is_width_height_swapped(self) -> bool {
        matches!(self, R90 | R270 | FR90 | FR270)
    }

    pub fn to_gst_video_orientation_method(self) -> gstreamer_video::VideoOrientationMethod {
        match self {
            Identity => gstreamer_video::VideoOrientationMethod::Identity,
            R90 => gstreamer_video::VideoOrientationMethod::_90r,
            R180 => gstreamer_video::VideoOrientationMethod::_180,
            R270 => gstreamer_video::VideoOrientationMethod::_90l,
            FlippedIdentity => gstreamer_video::VideoOrientationMethod::Horiz,
            FR180 => gstreamer_video::VideoOrientationMethod::Vert,
            FR90 => gstreamer_video::VideoOrientationMethod::UrLl,
            FR270 => gstreamer_video::VideoOrientationMethod::UlLr,
        }
    }
}
