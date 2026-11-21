//! Cameras: a view rotation and translation and a pinhole projection, the GTE's model
//! (`screen = offset + H * (x, y) / z`, psx-spx "GTE Coordinate Calculation Commands").
//!
//! View space has `x` right, `y` down and `z` into the screen, as the GTE's.

use crate::math::{self, Matrix};

/// The PlayStation frame the game's projection is set for, in pixels.
pub const PSX_FRAME: [f32; 2] = [320.0, 240.0];

/// A view transform and its projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Model space to view space: `view = rotation * model + translation`.
    pub rotation: Matrix,
    pub translation: [f32; 3],
    /// The projection distance `H`, in pixels of a frame [`Camera::frame_width`] wide.
    pub focal: f32,
    /// The frame width `focal` is measured in; a wider frame scales the picture with it.
    pub frame_width: f32,
    /// The nearest view depth drawn.
    pub near: f32,
}

impl Camera {
    /// A model point in view space.
    pub fn view(&self, model: [f32; 3]) -> [f32; 3] {
        math::add(math::apply(&self.rotation, model), self.translation)
    }

    /// The pixel of a view-space point in a `width` x `height` frame. The point must be past
    /// [`Camera::near`].
    pub fn project(&self, view: [f32; 3], width: usize, height: usize) -> [f32; 2] {
        let focal = self.focal * width as f32 / self.frame_width;
        [width as f32 / 2.0 + focal * view[0] / view[2], height as f32 / 2.0 + focal * view[1] / view[2]]
    }

    /// The model point's pixel, or `None` when it is behind the near plane.
    pub fn screen(&self, model: [f32; 3], width: usize, height: usize) -> Option<[f32; 2]> {
        let view = self.view(model);
        (view[2] > self.near).then(|| self.project(view, width, height))
    }

    /// A camera at the origin looking down `+z`, `focal` pixels from a 64-pixel frame.
    pub fn looking_down_z(focal: f32) -> Self {
        Self { rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], translation: [0.0; 3], focal, frame_width: 64.0, near: 1.0 }
    }

    /// A camera at `eye` looking at `target`, with model `up` pointing up on screen and a horizontal
    /// field of view of `fov` radians.
    pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3], fov: f32) -> Self {
        let forward = math::normalize(math::sub(target, eye));
        let right = math::normalize(math::cross(forward, up));
        let down = math::cross(forward, right);
        let rotation = [right, down, forward];
        let translation = math::scale(math::apply(&rotation, eye), -1.0);
        let frame_width = PSX_FRAME[0];
        Self { rotation, translation, focal: frame_width / 2.0 / (fov / 2.0).tan(), frame_width, near: 8.0 }
    }

    /// A camera orbiting `target` at `distance`, turned `yaw` about the model's up axis and raised
    /// `pitch` above the horizon (radians). The model's `y` is up.
    pub fn orbit(target: [f32; 3], yaw: f32, pitch: f32, distance: f32, fov: f32) -> Self {
        let pitch = pitch.clamp(-1.55, 1.55);
        let offset = [pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos()];
        let eye = math::add(target, math::scale(offset, distance));
        Self::look_at(eye, target, [0.0, 1.0, 0.0], fov)
    }
}
