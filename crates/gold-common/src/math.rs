use std::ops::{Add, AddAssign, Div, Index, IndexMut, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[inline]
pub const fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec3 {
    pub const ZERO: Vec3 = v3(0.0, 0.0, 0.0);
    pub const ONE: Vec3 = v3(1.0, 1.0, 1.0);

    #[inline]
    pub fn splat(v: f64) -> Vec3 {
        v3(v, v, v)
    }
    #[inline]
    pub fn from_f32(a: [f32; 3]) -> Vec3 {
        v3(a[0] as f64, a[1] as f64, a[2] as f64)
    }
    #[inline]
    pub fn to_f32(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
    #[inline]
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    #[inline]
    pub fn cross(self, o: Vec3) -> Vec3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    #[inline]
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    #[inline]
    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }
    #[inline]
    pub fn normalize(self) -> Vec3 {
        let l = self.length();
        if l > 1e-12 {
            self * (1.0 / l)
        } else {
            Vec3::ZERO
        }
    }
    #[inline]
    pub fn min(self, o: Vec3) -> Vec3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    #[inline]
    pub fn max(self, o: Vec3) -> Vec3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    #[inline]
    pub fn abs(self) -> Vec3 {
        v3(self.x.abs(), self.y.abs(), self.z.abs())
    }
    #[inline]
    pub fn mul_elem(self, o: Vec3) -> Vec3 {
        v3(self.x * o.x, self.y * o.y, self.z * o.z)
    }
    #[inline]
    pub fn max_elem(self) -> f64 {
        self.x.max(self.y).max(self.z)
    }
    #[inline]
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }
    pub fn dist(self, o: Vec3) -> f64 {
        (self - o).length()
    }
    pub fn major_axis(self) -> usize {
        let a = self.abs();
        if a.x >= a.y && a.x >= a.z {
            0
        } else if a.y >= a.z {
            1
        } else {
            2
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    #[inline]
    fn add(self, o: Vec3) -> Vec3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    #[inline]
    fn sub(self, o: Vec3) -> Vec3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn mul(self, s: f64) -> Vec3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Mul<Vec3> for f64 {
    type Output = Vec3;
    #[inline]
    fn mul(self, v: Vec3) -> Vec3 {
        v * self
    }
}
impl Div<f64> for Vec3 {
    type Output = Vec3;
    #[inline]
    fn div(self, s: f64) -> Vec3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    #[inline]
    fn neg(self) -> Vec3 {
        v3(-self.x, -self.y, -self.z)
    }
}
impl AddAssign for Vec3 {
    #[inline]
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}
impl SubAssign for Vec3 {
    #[inline]
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}
impl MulAssign<f64> for Vec3 {
    #[inline]
    fn mul_assign(&mut self, s: f64) {
        *self = *self * s;
    }
}
impl Index<usize> for Vec3 {
    type Output = f64;
    #[inline]
    fn index(&self, i: usize) -> &f64 {
        match i {
            0 => &self.x,
            1 => &self.y,
            _ => &self.z,
        }
    }
}
impl IndexMut<usize> for Vec3 {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut f64 {
        match i {
            0 => &mut self.x,
            1 => &mut self.y,
            _ => &mut self.z,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub mins: Vec3,
    pub maxs: Vec3,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        mins: v3(1e30, 1e30, 1e30),
        maxs: v3(-1e30, -1e30, -1e30),
    };
    pub fn new(mins: Vec3, maxs: Vec3) -> Self {
        Aabb { mins, maxs }
    }
    pub fn add_point(&mut self, p: Vec3) {
        self.mins = self.mins.min(p);
        self.maxs = self.maxs.max(p);
    }
    pub fn add_box(&mut self, o: &Aabb) {
        self.mins = self.mins.min(o.mins);
        self.maxs = self.maxs.max(o.maxs);
    }
    pub fn is_empty(&self) -> bool {
        self.mins.x > self.maxs.x
    }
    pub fn center(&self) -> Vec3 {
        (self.mins + self.maxs) * 0.5
    }
    pub fn size(&self) -> Vec3 {
        self.maxs - self.mins
    }
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.mins.x <= o.maxs.x
            && self.maxs.x >= o.mins.x
            && self.mins.y <= o.maxs.y
            && self.maxs.y >= o.mins.y
            && self.mins.z <= o.maxs.z
            && self.maxs.z >= o.mins.z
    }
    pub fn expand(&self, d: f64) -> Aabb {
        Aabb {
            mins: self.mins - Vec3::splat(d),
            maxs: self.maxs + Vec3::splat(d),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub dist: f64,
}

impl Plane {
    pub fn new(normal: Vec3, dist: f64) -> Self {
        Plane { normal, dist }
    }
    pub fn from_points(p0: Vec3, p1: Vec3, p2: Vec3) -> Option<Plane> {
        let n = (p0 - p1).cross(p2 - p1);
        let l = n.length();
        if l < 1e-9 {
            return None;
        }
        let n = n / l;
        Some(Plane {
            normal: n,
            dist: p1.dot(n),
        })
    }
    #[inline]
    pub fn distance(&self, p: Vec3) -> f64 {
        self.normal.dot(p) - self.dist
    }
    pub fn flip(&self) -> Plane {
        Plane {
            normal: -self.normal,
            dist: -self.dist,
        }
    }
    pub fn plane_type(&self) -> i32 {
        let n = self.normal;
        if n.x == 1.0 || n.x == -1.0 {
            0
        } else if n.y == 1.0 || n.y == -1.0 {
            1
        } else if n.z == 1.0 || n.z == -1.0 {
            2
        } else {
            let a = n.abs();
            if a.x >= a.y && a.x >= a.z {
                3
            } else if a.y >= a.x && a.y >= a.z {
                4
            } else {
                5
            }
        }
    }
}

pub fn angle_vectors(angles: Vec3) -> (Vec3, Vec3, Vec3) {
    let (sp, cp) = angles.x.to_radians().sin_cos();
    let (sy, cy) = angles.y.to_radians().sin_cos();
    let (sr, cr) = angles.z.to_radians().sin_cos();
    let forward = v3(cp * cy, cp * sy, -sp);
    let right = v3(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp);
    let up = v3(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
    (forward, right, up)
}

pub fn angle_matrix(angles: Vec3) -> [[f64; 3]; 3] {
    let (sp, cp) = angles.x.to_radians().sin_cos();
    let (sy, cy) = angles.y.to_radians().sin_cos();
    let (sr, cr) = angles.z.to_radians().sin_cos();
    [
        [cp * cy, sr * sp * cy - cr * sy, cr * sp * cy + sr * sy],
        [cp * sy, sr * sp * sy + cr * cy, cr * sp * sy - sr * cy],
        [-sp, sr * cp, cr * cp],
    ]
}

pub fn mat_mul_vec(m: &[[f64; 3]; 3], v: Vec3) -> Vec3 {
    v3(
        m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    )
}

pub fn parse_vec3(s: &str) -> Option<Vec3> {
    let f: Vec<f64> = s
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '[' || c == ']')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    if f.len() >= 3 {
        Some(v3(f[0], f[1], f[2]))
    } else {
        None
    }
}
