use crate::address::{Address, Coordinates};

#[derive(Clone, Debug)]
pub struct Regions {
    origin: [i32; 3],
    bounds: [[i32; 2]; 3],
    galaxy: u8,
    radius: i32,
    maximum: i32,
    face: usize,
    cursor: Option<Rectangle>,
    origin_pending: bool,
}

#[derive(Clone, Debug)]
struct Rectangle {
    bounds: [[i32; 2]; 3],
    next: Option<[i32; 3]>,
}

impl Rectangle {
    fn new(bounds: [[i32; 2]; 3]) -> Option<Self> {
        bounds.iter().all(|b| b[0] <= b[1]).then_some(Self {
            bounds,
            next: Some(bounds.map(|b| b[0])),
        })
    }

    fn take(&mut self) -> Option<[i32; 3]> {
        let result = self.next?;
        let mut next = result;
        for axis in (0..3).rev() {
            if next[axis] < self.bounds[axis][1] {
                next[axis] += 1;
                self.next = Some(next);
                return Some(result);
            }
            next[axis] = self.bounds[axis][0];
        }
        self.next = None;
        Some(result)
    }
}

impl Regions {
    pub fn new(origin: Address) -> Self {
        Self::bounded(origin, [[-2047, 2047], [-127, 127], [-2047, 2047]])
    }

    fn bounded(origin: Address, bounds: [[i32; 2]; 3]) -> Self {
        let c = origin.coordinates();
        let origin = [i32::from(c.x), i32::from(c.y), i32::from(c.z)];
        let maximum = (0..3)
            .flat_map(|axis| bounds[axis].map(|edge| (origin[axis] - edge).abs()))
            .max()
            .unwrap_or(0);
        Self {
            origin,
            bounds,
            galaxy: c.galaxy,
            radius: 1,
            maximum,
            face: 0,
            cursor: None,
            origin_pending: true,
        }
    }

    fn face(&self) -> Option<Rectangle> {
        let axis = self.face / 2;
        let fixed = self.origin[axis]
            + if self.face % 2 == 0 {
                -self.radius
            } else {
                self.radius
            };
        if !(self.bounds[axis][0]..=self.bounds[axis][1]).contains(&fixed) {
            return None;
        }
        let mut bounds = self.bounds;
        for (index, bounds) in bounds.iter_mut().enumerate() {
            let inset = i32::from(index < axis);
            bounds[0] = bounds[0].max(self.origin[index] - self.radius + inset);
            bounds[1] = bounds[1].min(self.origin[index] + self.radius - inset);
        }
        bounds[axis] = [fixed, fixed];
        Rectangle::new(bounds)
    }

    fn address(&self, point: [i32; 3]) -> Address {
        Address::new(Coordinates {
            x: point[0] as i16,
            y: point[1] as i16,
            z: point[2] as i16,
            galaxy: self.galaxy,
            system: 0,
            planet: 0,
        })
        .expect("region traversal stays inside validated bounds")
    }
}

impl Iterator for Regions {
    type Item = Address;

    fn next(&mut self) -> Option<Address> {
        if self.origin_pending {
            self.origin_pending = false;
            return Some(self.address(self.origin));
        }
        loop {
            if let Some(point) = self.cursor.as_mut().and_then(Rectangle::take) {
                return Some(self.address(point));
            }
            if self.face == 6 {
                self.radius += 1;
                self.face = 0;
            }
            if self.radius > self.maximum {
                return None;
            }
            self.cursor = self.face();
            self.face += 1;
        }
    }
}

impl std::iter::FusedIterator for Regions {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn clipped_shells_cover_each_region_once_from_every_origin() {
        for x in -3..=3 {
            for y in -1..=1 {
                for z in -3..=3 {
                    let origin = Address::new(Coordinates {
                        x,
                        y,
                        z,
                        galaxy: 249,
                        system: 1058,
                        planet: 4,
                    })
                    .unwrap();
                    let mut regions = Regions::bounded(origin, [[-3, 3], [-1, 1], [-3, 3]]);
                    let rows: Vec<_> = regions.by_ref().collect();
                    assert_eq!(rows.len(), 147);
                    assert_eq!(rows.iter().collect::<HashSet<_>>().len(), rows.len());
                    assert_eq!(rows[0], origin.region());
                    assert!(regions.next().is_none());
                    let mut radius = 0;
                    for row in rows {
                        let c = row.coordinates();
                        assert_eq!(c.galaxy, 249);
                        let distance = (c.x - x).abs().max((c.y - y).abs()).max((c.z - z).abs());
                        assert!(distance >= radius);
                        radius = distance;
                    }
                }
            }
        }
    }

    #[test]
    fn frontier_clone_continues_without_repeating_earlier_regions() {
        let mut regions = Regions::new(Address::from_raw(0).unwrap());
        let first: HashSet<_> = regions.by_ref().take(1000).collect();
        let resumed = regions.clone().take(1000).collect::<Vec<_>>();
        assert!(resumed.iter().all(|address| !first.contains(address)));
        assert_eq!(resumed, regions.take(1000).collect::<Vec<_>>());
        assert!(std::mem::size_of::<Regions>() < 256);
    }
}
