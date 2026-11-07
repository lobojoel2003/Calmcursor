//! Small 2D math helpers.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// A 2D point or vector in pointer coordinates (pixels, or device units on Linux).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    #[inline]
    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    #[inline]
    pub fn distance(self, other: Vec2) -> f64 {
        (self - other).length()
    }

    #[inline]
    pub fn lerp(self, other: Vec2, t: f64) -> Vec2 {
        self + (other - self) * t
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Rounds both components to the nearest integer.
    #[inline]
    pub fn round(self) -> Vec2 {
        Vec2::new(self.x.round(), self.y.round())
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    #[inline]
    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    #[inline]
    fn add_assign(&mut self, rhs: Vec2) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    #[inline]
    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Vec2 {
    #[inline]
    fn sub_assign(&mut self, rhs: Vec2) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn mul(self, rhs: f64) -> Vec2 {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<f64> for Vec2 {
    type Output = Vec2;
    #[inline]
    fn div(self, rhs: f64) -> Vec2 {
        Vec2::new(self.x / rhs, self.y / rhs)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    #[inline]
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// An axis-aligned rectangle, used for the bounds of the desktop.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    /// Builds a rectangle from two corners in any order.
    pub fn from_corners(a: Vec2, b: Vec2) -> Self {
        Self {
            min: Vec2::new(a.x.min(b.x), a.y.min(b.y)),
            max: Vec2::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// Builds a rectangle from an origin and a size.
    pub fn from_origin_size(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self::from_corners(Vec2::new(x, y), Vec2::new(x + w, y + h))
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Moves `p` to the closest point inside the rectangle.
    pub fn clamp(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            p.x.clamp(self.min.x, self.max.x),
            p.y.clamp(self.min.y, self.max.y),
        )
    }

    /// The smallest rectangle containing both.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            min: Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Vec2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_ops() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(a.length(), 5.0);
        assert_eq!(a + a, Vec2::new(6.0, 8.0));
        assert_eq!(a - a, Vec2::ZERO);
        assert_eq!(a * 2.0, Vec2::new(6.0, 8.0));
        assert_eq!(a / 2.0, Vec2::new(1.5, 2.0));
        assert_eq!(-a, Vec2::new(-3.0, -4.0));
        assert_eq!(Vec2::ZERO.lerp(a, 0.5), Vec2::new(1.5, 2.0));
        assert_eq!(Vec2::new(1.4, -1.6).round(), Vec2::new(1.0, -2.0));
    }

    #[test]
    fn rect_clamp_and_union() {
        let r = Rect::from_corners(Vec2::new(10.0, 10.0), Vec2::new(0.0, 0.0));
        assert_eq!(r.min, Vec2::ZERO);
        assert_eq!(r.clamp(Vec2::new(-5.0, 20.0)), Vec2::new(0.0, 10.0));
        assert!(r.contains(Vec2::new(5.0, 5.0)));
        let u = r.union(&Rect::from_origin_size(-10.0, 5.0, 5.0, 50.0));
        assert_eq!(u.min, Vec2::new(-10.0, 0.0));
        assert_eq!(u.max, Vec2::new(10.0, 55.0));
        assert_eq!(u.width(), 20.0);
        assert_eq!(u.height(), 55.0);
    }
}
