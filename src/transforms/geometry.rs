use crate::models::DataValue;

// ── EWKB parser ───────────────────────────────────────────────────────────────

struct EwkbParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> EwkbParser<'a> {
    fn read_u8(&mut self) -> Result<u8, String> {
        if self.pos >= self.data.len() {
            return Err(String::from("EWKB: unexpected end of data"));
        }
        let val = self.data[self.pos];
        self.pos += 1;
        Ok(val)
    }

    fn read_u32(&mut self, le: bool) -> Result<u32, String> {
        if self.pos + 4 > self.data.len() {
            return Err(String::from("EWKB: unexpected end of data reading u32"));
        }
        let b = &self.data[self.pos..self.pos + 4];
        self.pos += 4;
        let bytes = [b[0], b[1], b[2], b[3]];
        Ok(if le {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    fn read_f64(&mut self, le: bool) -> Result<f64, String> {
        if self.pos + 8 > self.data.len() {
            return Err(String::from("EWKB: unexpected end of data reading f64"));
        }
        let b = &self.data[self.pos..self.pos + 8];
        self.pos += 8;
        let bytes = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        Ok(if le {
            f64::from_le_bytes(bytes)
        } else {
            f64::from_be_bytes(bytes)
        })
    }

    /// Returns (base_type, has_z, has_m, has_srid, little_endian) from a geometry header.
    fn read_geometry_header(&mut self) -> Result<(u32, bool, bool, bool, bool), String> {
        let byte_order = self.read_u8()?;
        let le = byte_order == 1;
        let raw_type = self.read_u32(le)?;

        // EWKB flag bits
        let has_srid = (raw_type & 0x20000000) != 0;
        let has_z_flag = (raw_type & 0x80000000) != 0;
        let has_m_flag = (raw_type & 0x40000000) != 0;
        let base_with_iso = raw_type & 0x1FFFFFFF;

        // ISO WKB encodes dimensionality in the type number range
        let (base_type, has_z, has_m) = if base_with_iso >= 3000 {
            (base_with_iso - 3000, true, true)
        } else if base_with_iso >= 2000 {
            (base_with_iso - 2000, false, true)
        } else if base_with_iso >= 1000 {
            (base_with_iso - 1000, true, false)
        } else {
            (base_with_iso, has_z_flag, has_m_flag)
        };

        Ok((base_type, has_z, has_m, has_srid, le))
    }

    fn parse_ring(&mut self, le: bool, has_z: bool, has_m: bool) -> Result<Vec<(f64, f64)>, String> {
        let num_points = self.read_u32(le)?;
        let mut points = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            let x = self.read_f64(le)?;
            let y = self.read_f64(le)?;
            if has_z {
                self.read_f64(le)?;
            }
            if has_m {
                self.read_f64(le)?;
            }
            points.push((x, y));
        }
        Ok(points)
    }

    fn parse_polygon_rings(
        &mut self,
        le: bool,
        has_z: bool,
        has_m: bool,
    ) -> Result<Vec<Vec<(f64, f64)>>, String> {
        let num_rings = self.read_u32(le)?;
        let mut rings = Vec::with_capacity(num_rings as usize);
        for _ in 0..num_rings {
            rings.push(self.parse_ring(le, has_z, has_m)?);
        }
        Ok(rings)
    }

    /// Parses one geometry and returns polygons (Vec<polygon> where polygon = Vec<ring>).
    fn parse_geometry(&mut self) -> Result<Vec<Vec<Vec<(f64, f64)>>>, String> {
        let (base_type, has_z, has_m, has_srid, le) = self.read_geometry_header()?;

        if has_srid {
            self.pos += 4;
        }

        match base_type {
            3 => {
                let rings = self.parse_polygon_rings(le, has_z, has_m)?;
                Ok(vec![rings])
            }
            6 => {
                let num_polygons = self.read_u32(le)?;
                let mut all_polygons = Vec::with_capacity(num_polygons as usize);
                for _ in 0..num_polygons {
                    let (sub_type, sub_z, sub_m, sub_srid, sub_le) =
                        self.read_geometry_header()?;
                    if sub_srid {
                        self.pos += 4;
                    }
                    if sub_type != 3 {
                        return Err(format!(
                            "EWKB: expected Polygon inside MultiPolygon, got type {sub_type}"
                        ));
                    }
                    let rings = self.parse_polygon_rings(sub_le, sub_z, sub_m)?;
                    all_polygons.push(rings);
                }
                Ok(all_polygons)
            }
            other => Err(format!(
                "EWKB: unsupported geometry type {other}; only Polygon (3) and MultiPolygon (6) are supported"
            )),
        }
    }
}

/// Parse raw EWKB bytes into a `DataValue::Geometry`.
pub(crate) fn parse_ewkb(bytes: &[u8]) -> Result<DataValue, String> {
    let mut parser = EwkbParser { data: bytes, pos: 0 };
    let polygons = parser.parse_geometry()?;
    Ok(DataValue::Geometry(polygons))
}

// ── Geometry computations ─────────────────────────────────────────────────────

fn shoelace_area(ring: &[(f64, f64)]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let n = ring.len();
    let mut sum = 0.0f64;
    for i in 0..n {
        let (x1, y1) = ring[i];
        let (x2, y2) = ring[(i + 1) % n];
        sum += x1 * y2 - x2 * y1;
    }
    sum.abs() / 2.0
}

fn ring_length(ring: &[(f64, f64)]) -> f64 {
    if ring.len() < 2 {
        return 0.0;
    }
    let n = ring.len();
    let mut len = 0.0f64;
    for i in 0..n {
        let (x1, y1) = ring[i];
        let (x2, y2) = ring[(i + 1) % n];
        let dx = x2 - x1;
        let dy = y2 - y1;
        len += (dx * dx + dy * dy).sqrt();
    }
    len
}

/// Compute the total area across all polygons (exterior ring minus holes).
pub(crate) fn compute_area(polygons: &[Vec<Vec<(f64, f64)>>]) -> f64 {
    polygons
        .iter()
        .map(|rings| {
            if rings.is_empty() {
                return 0.0;
            }
            let ext = shoelace_area(&rings[0]);
            let holes: f64 = rings[1..].iter().map(|r| shoelace_area(r)).sum();
            ext - holes
        })
        .sum::<f64>()
        .abs()
}

/// Compute the total perimeter (sum of all exterior ring lengths).
pub(crate) fn compute_perimeter(polygons: &[Vec<Vec<(f64, f64)>>]) -> f64 {
    polygons
        .iter()
        .map(|rings| rings.first().map(|r| ring_length(r)).unwrap_or(0.0))
        .sum()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn square_ewkb() -> Vec<u8> {
        // POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))  — LE, no SRID, no Z/M
        let mut bytes = Vec::new();
        bytes.push(1u8); // little endian
        bytes.extend_from_slice(&3u32.to_le_bytes()); // Polygon
        bytes.extend_from_slice(&1u32.to_le_bytes()); // 1 ring
        bytes.extend_from_slice(&5u32.to_le_bytes()); // 5 points
        for (x, y) in [(0.0f64, 0.0f64), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
            bytes.extend_from_slice(&x.to_le_bytes());
            bytes.extend_from_slice(&y.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn parse_ewkb_square_polygon() {
        let bytes = square_ewkb();
        let result = parse_ewkb(&bytes).unwrap();
        match result {
            DataValue::Geometry(polygons) => {
                assert_eq!(polygons.len(), 1);
                assert_eq!(polygons[0].len(), 1); // 1 ring
                assert_eq!(polygons[0][0].len(), 5); // 5 points
            }
            _ => panic!("expected Geometry"),
        }
    }

    #[test]
    fn compute_area_of_unit_square() {
        let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        let polygons = vec![vec![ring]];
        let area = compute_area(&polygons);
        assert!((area - 100.0).abs() < 1e-9, "area was {area}");
    }

    #[test]
    fn compute_perimeter_of_unit_square() {
        let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        let polygons = vec![vec![ring]];
        let perimeter = compute_perimeter(&polygons);
        assert!((perimeter - 40.0).abs() < 1e-9, "perimeter was {perimeter}");
    }

    #[test]
    fn area_subtracts_holes() {
        // 10×10 square with a 2×2 hole
        let exterior = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        let hole = vec![(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0), (4.0, 4.0)];
        let polygons = vec![vec![exterior, hole]];
        let area = compute_area(&polygons);
        assert!((area - 96.0).abs() < 1e-9, "area was {area}");
    }

    #[test]
    fn parse_ewkb_with_srid() {
        let mut bytes = Vec::new();
        bytes.push(1u8); // little endian
        // Polygon | SRID flag
        bytes.extend_from_slice(&(3u32 | 0x20000000).to_le_bytes());
        bytes.extend_from_slice(&4326i32.to_le_bytes()); // SRID
        bytes.extend_from_slice(&1u32.to_le_bytes()); // 1 ring
        bytes.extend_from_slice(&5u32.to_le_bytes()); // 5 points
        for (x, y) in [(0.0f64, 0.0f64), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
            bytes.extend_from_slice(&x.to_le_bytes());
            bytes.extend_from_slice(&y.to_le_bytes());
        }
        let result = parse_ewkb(&bytes).unwrap();
        assert!(matches!(result, DataValue::Geometry(_)));
    }

    #[test]
    fn parse_ewkb_with_z_coordinates() {
        let mut bytes = Vec::new();
        bytes.push(1u8); // little endian
        // Polygon | Z flag
        bytes.extend_from_slice(&(3u32 | 0x80000000).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes()); // 1 ring
        bytes.extend_from_slice(&4u32.to_le_bytes()); // 4 points
        for (x, y, z) in [(0.0f64, 0.0f64, 1.0f64), (5.0, 0.0, 1.0), (5.0, 5.0, 1.0), (0.0, 0.0, 1.0)] {
            bytes.extend_from_slice(&x.to_le_bytes());
            bytes.extend_from_slice(&y.to_le_bytes());
            bytes.extend_from_slice(&z.to_le_bytes());
        }
        let result = parse_ewkb(&bytes).unwrap();
        match result {
            DataValue::Geometry(polygons) => {
                assert_eq!(polygons[0][0].len(), 4);
            }
            _ => panic!("expected Geometry"),
        }
    }
}
