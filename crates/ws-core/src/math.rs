//! Minimal, allocation-free 3D math used by all solvers.

/// A 3-component double-precision vector.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub const fn splat(v: f64) -> Self {
        Self::new(v, v, v)
    }

    #[inline]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    #[inline]
    pub fn scale(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }

    #[inline]
    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    #[inline]
    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    #[inline]
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    #[inline]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    #[inline]
    pub fn normalized(self) -> Self {
        let l = self.length();
        if l > 0.0 {
            self.scale(1.0 / l)
        } else {
            Self::zero()
        }
    }

    #[inline]
    pub fn lerp(self, o: Self, t: f64) -> Self {
        self.add(o.sub(self).scale(t))
    }

    /// Component-wise minimum (useful for AABB computation).
    #[inline]
    pub fn min_components(self, o: Self) -> Self {
        Self::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    /// Component-wise maximum.
    #[inline]
    pub fn max_components(self, o: Self) -> Self {
        Self::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
}

impl core::ops::Add for Vec3 {
    type Output = Vec3;
    #[inline]
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::add(self, o)
    }
}

impl core::ops::Sub for Vec3 {
    type Output = Vec3;
    #[inline]
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::sub(self, o)
    }
}

impl core::ops::Mul<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn mul(self, s: f64) -> Vec3 {
        Vec3::scale(self, s)
    }
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    #[inline]
    pub fn size(&self) -> Vec3 {
        self.max.sub(self.min)
    }

    #[inline]
    pub fn center(&self) -> Vec3 {
        self.min.lerp(self.max, 0.5)
    }

    #[inline]
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    #[inline]
    pub fn clamp_point(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            p.x.clamp(self.min.x, self.max.x),
            p.y.clamp(self.min.y, self.max.y),
            p.z.clamp(self.min.z, self.max.z),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec3_basics() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(a.add(b), Vec3::new(5.0, 7.0, 9.0));
        assert_eq!(a.dot(b), 32.0);
        assert_eq!(
            a.cross(b),
            Vec3::new(
                2.0 * 6.0 - 3.0 * 5.0,
                3.0 * 4.0 - 1.0 * 6.0,
                1.0 * 5.0 - 2.0 * 4.0
            )
        );
        assert!((a.length() - (14.0f64).sqrt()).abs() < 1e-15);
        let n = a.normalized();
        assert!((n.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn aabb_basics() {
        let box_ = Aabb::new(Vec3::zero(), Vec3::splat(2.0));
        assert!(box_.contains(Vec3::new(1.0, 1.0, 1.0)));
        assert!(!box_.contains(Vec3::new(3.0, 1.0, 1.0)));
        let c = box_.clamp_point(Vec3::new(-5.0, 1.0, 9.0));
        assert_eq!(c, Vec3::new(0.0, 1.0, 2.0));
        assert_eq!(box_.center(), Vec3::splat(1.0));
    }
}
