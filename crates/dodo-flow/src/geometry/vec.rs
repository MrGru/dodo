//! [`Vec2`] — the two-component `f32` vector the whole engine measures in.
//!
//! **Why not gpui's `Point<f32>`.** It would work, and it would put a UI
//! framework in the type signature of every pure function in `models/` and
//! `geometry/`, which is exactly the boundary `lib.rs` says is very hard to
//! recover once lost. `Vec2` is twenty lines and costs nothing at runtime;
//! `views/` converts at the render boundary and nowhere else does.
//!
//! **Why `f32` and not `f64`.** The values end up in a `PathVertex<Pixels>`,
//! which is `f32` — a wider world space would be truncated at the last step
//! anyway. Translating a cached tessellation instead of rebuilding it was
//! measured as exact to 0.000122 px of maximum deviation, which is `f32`
//! rounding and is the accuracy floor this engine works to.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// A point, a size or a delta in world space.
///
/// One type for all three on purpose: a size is a vector from the origin, a
/// delta is a difference of two positions, and keeping them apart would need
/// three newtypes and a conversion at every call site for no error class that
/// has ever bitten a canvas.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    /// The origin, and the zero delta.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    /// The unit vector; also the identity scale.
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };

    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    /// The vector with both components equal — a square size, most often.
    pub const fn splat(v: f32) -> Vec2 {
        Vec2 { x: v, y: v }
    }

    /// Component-wise multiplication. Distinct from `self * scalar`, which is
    /// the uniform case; a non-uniform scale is rare enough to be spelled out.
    pub fn scale(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x * other.x, self.y * other.y)
    }

    pub fn min(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x.min(other.x), self.y.min(other.y))
    }

    pub fn max(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x.max(other.x), self.y.max(other.y))
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// The squared length, for comparisons that do not need the square root —
    /// hit-testing a handle radius, mostly.
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// Every component is finite. The document format lets any `f32` through
    /// serde, so a loaded document is checked with this before it can poison a
    /// bounds union with a `NaN`.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Vec2) {
        *self = *self + rhs;
    }
}

impl Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Vec2) {
        *self = *self - rhs;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: f32) -> Vec2 {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<f32> for Vec2 {
    type Output = Vec2;

    fn div(self, rhs: f32) -> Vec2 {
        Vec2::new(self.x / rhs, self.y / rhs)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;

    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::Vec2;

    #[test]
    fn arithmetic_is_component_wise() {
        let a = Vec2::new(3.0, 4.0);
        let b = Vec2::new(1.0, 2.0);

        assert_eq!(a + b, Vec2::new(4.0, 6.0));
        assert_eq!(a - b, Vec2::new(2.0, 2.0));
        assert_eq!(a * 2.0, Vec2::new(6.0, 8.0));
        assert_eq!(a / 2.0, Vec2::new(1.5, 2.0));
        assert_eq!(-a, Vec2::new(-3.0, -4.0));
        assert_eq!(a.scale(b), Vec2::new(3.0, 8.0));
    }

    #[test]
    fn length_uses_the_euclidean_norm() {
        let v = Vec2::new(3.0, 4.0);

        assert_eq!(v.length_squared(), 25.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(Vec2::ZERO.length(), 0.0);
    }

    #[test]
    fn min_and_max_are_component_wise() {
        let a = Vec2::new(1.0, 9.0);
        let b = Vec2::new(5.0, 2.0);

        assert_eq!(a.min(b), Vec2::new(1.0, 2.0));
        assert_eq!(a.max(b), Vec2::new(5.0, 9.0));
    }

    #[test]
    fn non_finite_components_are_detected() {
        assert!(Vec2::new(1.0, 2.0).is_finite());
        assert!(!Vec2::new(f32::NAN, 0.0).is_finite());
        assert!(!Vec2::new(0.0, f32::INFINITY).is_finite());
    }
}
