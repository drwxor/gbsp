use crate::math::*;

pub const MAX_COORD: f64 = 65536.0;
pub const ON_EPSILON: f64 = 0.1;

pub const SIDE_FRONT: i32 = 0;
pub const SIDE_BACK: i32 = 1;
pub const SIDE_ON: i32 = 2;
pub const SIDE_CROSS: i32 = 3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Winding {
    pub p: Vec<Vec3>,
}

impl Winding {
    pub fn new(p: Vec<Vec3>) -> Self {
        Winding { p }
    }

    pub fn len(&self) -> usize {
        self.p.len()
    }

    pub fn is_empty(&self) -> bool {
        self.p.is_empty()
    }

    pub fn base_for_plane(plane: &Plane) -> Winding {
        let n = plane.normal;
        let axis = n.major_axis();
        let up = if axis == 2 {
            v3(1.0, 0.0, 0.0)
        } else {
            v3(0.0, 0.0, 1.0)
        };
        let v = up.dot(n);
        let up = (up - n * v).normalize();
        let org = n * plane.dist;
        let right = up.cross(n);
        let up = up * MAX_COORD * 4.0;
        let right = right * MAX_COORD * 4.0;
        Winding {
            p: vec![
                org - right + up,
                org + right + up,
                org + right - up,
                org - right - up,
            ],
        }
    }

    pub fn area(&self) -> f64 {
        let mut total = 0.0;
        for i in 2..self.p.len() {
            let d1 = self.p[i - 1] - self.p[0];
            let d2 = self.p[i] - self.p[0];
            total += 0.5 * d1.cross(d2).length();
        }
        total
    }

    pub fn center(&self) -> Vec3 {
        let mut c = Vec3::ZERO;
        for p in &self.p {
            c += *p;
        }
        c / self.p.len().max(1) as f64
    }

    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in &self.p {
            b.add_point(*p);
        }
        b
    }

    pub fn plane(&self) -> Plane {
        let mut best = Vec3::ZERO;
        let n = self.p.len();
        for i in 0..n {
            let a = self.p[i];
            let b = self.p[(i + 1) % n];
            best.x += (a.y - b.y) * (a.z + b.z);
            best.y += (a.z - b.z) * (a.x + b.x);
            best.z += (a.x - b.x) * (a.y + b.y);
        }
        let normal = (-best).normalize();
        Plane {
            normal,
            dist: normal.dot(self.p[0]),
        }
    }

    pub fn reverse(&self) -> Winding {
        let mut p = self.p.clone();
        p.reverse();
        Winding { p }
    }

    fn sides(&self, normal: Vec3, dist: f64, eps: f64) -> (Vec<f64>, Vec<i32>, [usize; 3]) {
        let mut dists = Vec::with_capacity(self.p.len() + 1);
        let mut sides = Vec::with_capacity(self.p.len() + 1);
        let mut counts = [0usize; 3];
        for p in &self.p {
            let d = p.dot(normal) - dist;
            let s = if d > eps {
                SIDE_FRONT
            } else if d < -eps {
                SIDE_BACK
            } else {
                SIDE_ON
            };
            counts[s as usize] += 1;
            dists.push(d);
            sides.push(s);
        }
        if !self.p.is_empty() {
            dists.push(dists[0]);
            sides.push(sides[0]);
        }
        (dists, sides, counts)
    }

    pub fn clip_epsilon(
        &self,
        normal: Vec3,
        dist: f64,
        eps: f64,
    ) -> (Option<Winding>, Option<Winding>) {
        let (dists, sides, counts) = self.sides(normal, dist, eps);
        if counts[0] == 0 {
            return (None, Some(self.clone()));
        }
        if counts[1] == 0 {
            return (Some(self.clone()), None);
        }
        let mut f = Vec::with_capacity(self.p.len() + 4);
        let mut b = Vec::with_capacity(self.p.len() + 4);
        let n = self.p.len();
        for i in 0..n {
            let p1 = self.p[i];
            if sides[i] == SIDE_ON {
                f.push(p1);
                b.push(p1);
                continue;
            }
            if sides[i] == SIDE_FRONT {
                f.push(p1);
            }
            if sides[i] == SIDE_BACK {
                b.push(p1);
            }
            if sides[i + 1] == SIDE_ON || sides[i + 1] == sides[i] {
                continue;
            }
            let p2 = self.p[(i + 1) % n];
            let dot = dists[i] / (dists[i] - dists[i + 1]);
            let mut mid = Vec3::ZERO;
            for j in 0..3 {
                mid[j] = if normal[j] == 1.0 {
                    dist
                } else if normal[j] == -1.0 {
                    -dist
                } else {
                    p1[j] + dot * (p2[j] - p1[j])
                };
            }
            f.push(mid);
            b.push(mid);
        }
        let fw = if f.len() >= 3 {
            Some(Winding { p: f })
        } else {
            None
        };
        let bw = if b.len() >= 3 {
            Some(Winding { p: b })
        } else {
            None
        };
        (fw, bw)
    }

    pub fn chop(&self, normal: Vec3, dist: f64, eps: f64) -> Option<Winding> {
        let (dists, sides, counts) = self.sides(normal, dist, eps);
        if counts[0] == 0 && counts[1] == 0 {
            return Some(self.clone());
        }
        if counts[0] == 0 {
            return None;
        }
        if counts[1] == 0 {
            return Some(self.clone());
        }
        let n = self.p.len();
        let mut f = Vec::with_capacity(n + 4);
        for i in 0..n {
            let p1 = self.p[i];
            if sides[i] == SIDE_ON {
                f.push(p1);
                continue;
            }
            if sides[i] == SIDE_FRONT {
                f.push(p1);
            }
            if sides[i + 1] == SIDE_ON || sides[i + 1] == sides[i] {
                continue;
            }
            let p2 = self.p[(i + 1) % n];
            let dot = dists[i] / (dists[i] - dists[i + 1]);
            let mut mid = Vec3::ZERO;
            for j in 0..3 {
                mid[j] = if normal[j] == 1.0 {
                    dist
                } else if normal[j] == -1.0 {
                    -dist
                } else {
                    p1[j] + dot * (p2[j] - p1[j])
                };
            }
            f.push(mid);
        }
        if f.len() >= 3 {
            Some(Winding { p: f })
        } else {
            None
        }
    }

    pub fn chop_strict(&self, normal: Vec3, dist: f64, eps: f64) -> Option<Winding> {
        let (_, _, counts) = self.sides(normal, dist, eps);
        if counts[0] == 0 {
            return None;
        }
        self.chop(normal, dist, eps)
    }

    pub fn classify(&self, normal: Vec3, dist: f64, eps: f64) -> i32 {
        let (_, _, counts) = self.sides(normal, dist, eps);
        if counts[0] > 0 && counts[1] > 0 {
            SIDE_CROSS
        } else if counts[0] > 0 {
            SIDE_FRONT
        } else if counts[1] > 0 {
            SIDE_BACK
        } else {
            SIDE_ON
        }
    }

    pub fn remove_colinear(&self, eps: f64) -> Winding {
        let mut pts: Vec<Vec3> = Vec::with_capacity(self.p.len());
        for &p in &self.p {
            if pts
                .last()
                .map(|l: &Vec3| (*l - p).length() < eps)
                .unwrap_or(false)
            {
                continue;
            }
            pts.push(p);
        }
        while pts.len() > 1 && (pts[0] - pts[pts.len() - 1]).length() < eps {
            pts.pop();
        }
        let mut changed = true;
        while changed && pts.len() > 3 {
            changed = false;
            let n = pts.len();
            for i in 0..n {
                let a = pts[(i + n - 1) % n];
                let b = pts[i];
                let c = pts[(i + 1) % n];
                let d1 = (b - a).normalize();
                let d2 = (c - b).normalize();
                if d1.dot(d2) > 0.999999 {
                    pts.remove(i);
                    changed = true;
                    break;
                }
            }
        }
        Winding { p: pts }
    }

    pub fn is_tiny(&self) -> bool {
        let mut edges = 0;
        let n = self.p.len();
        for i in 0..n {
            let d = (self.p[(i + 1) % n] - self.p[i]).length();
            if d > 0.2 {
                edges += 1;
                if edges == 3 {
                    return false;
                }
            }
        }
        true
    }

    pub fn is_huge(&self) -> bool {
        self.p
            .iter()
            .any(|p| p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD || p.z.abs() > MAX_COORD)
    }
}
