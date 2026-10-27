//! Support for arbitrary closed polygon masking and country border selection.
//!
//! Allows restricting generation strictly inside a user-drawn freehand polygon
//! or national border, skipping all void/exterior chunks to save disk space and RAM.

use crate::coordinate_system::geographic::LLPoint;

/// Closed non-overlapping polygon mask in geographic coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonMask {
    pub vertices: Vec<LLPoint>,
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lng: f64,
    pub max_lng: f64,
}

impl PolygonMask {
    /// Creates a polygon mask from a list of vertices. Automatically closes the ring if needed.
    pub fn new(mut vertices: Vec<LLPoint>) -> Result<Self, String> {
        if vertices.len() < 3 {
            return Err("A polygon mask must have at least 3 vertices to form a closed shape.".to_string());
        }

        // Close ring if not already closed
        if let (Some(first), Some(last)) = (vertices.first(), vertices.last()) {
            if (first.lat() - last.lat()).abs() > 1e-7 || (first.lng() - last.lng()).abs() > 1e-7 {
                vertices.push(*first);
            }
        }

        let mut min_lat = f64::INFINITY;
        let mut max_lat = f64::NEG_INFINITY;
        let mut min_lng = f64::INFINITY;
        let mut max_lng = f64::NEG_INFINITY;

        for p in &vertices {
            min_lat = min_lat.min(p.lat());
            max_lat = max_lat.max(p.lat());
            min_lng = min_lng.min(p.lng());
            max_lng = max_lng.max(p.lng());
        }

        Ok(Self {
            vertices,
            min_lat,
            max_lat,
            min_lng,
            max_lng,
        })
    }

    /// Parses a polygon from string:
    /// Supports "lat1,lng1;lat2,lng2;lat3,lng3;..."
    /// or GeoJSON coordinate string "[[lng,lat],[lng,lat],...]"
    pub fn parse(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();

        // Check for GeoJSON format
        if trimmed.starts_with('[') {
            return Self::parse_geojson(trimmed);
        }

        // Semicolon or space separated pairs: "46.9,7.4;46.95,7.5;46.9,7.55"
        let mut points = Vec::new();
        for pair_str in trimmed.split([';', ' ']).filter(|s| !s.trim().is_empty()) {
            let coords: Vec<&str> = pair_str.split(',').map(|c| c.trim()).collect();
            if coords.len() != 2 {
                return Err(format!("Invalid coordinate pair: '{pair_str}'. Expected 'lat,lng'."));
            }
            let lat = coords[0]
                .parse::<f64>()
                .map_err(|e| format!("Invalid latitude '{}': {e}", coords[0]))?;
            let lng = coords[1]
                .parse::<f64>()
                .map_err(|e| format!("Invalid longitude '{}': {e}", coords[1]))?;
            points.push(LLPoint::new(lat, lng)?);
        }

        Self::new(points)
    }

    fn parse_geojson(s: &str) -> Result<Self, String> {
        // Strip outer brackets and parse [lng, lat] pairs
        let clean = s.replace(['[', ']'], " ");
        let tokens: Vec<f64> = clean
            .split([',', ' ', '\n', '\t'])
            .filter_map(|t| t.trim().parse::<f64>().ok())
            .collect();

        if tokens.len() < 6 || tokens.len() % 2 != 0 {
            return Err("GeoJSON polygon coordinates must contain at least 3 [lng, lat] pairs.".to_string());
        }

        let mut points = Vec::new();
        for chunk in tokens.chunks(2) {
            let lng = chunk[0];
            let lat = chunk[1];
            points.push(LLPoint::new(lat, lng)?);
        }

        Self::new(points)
    }

    /// Switzerland national boundary perimeter polygon.
    /// Simplified 32-point polygon capturing Switzerland's outer envelope (Geneva, Jura, Basel,
    /// Schaffhausen, Lake Constance, Graubünden, Ticino, Valais).
    pub fn switzerland_border() -> Self {
        let coords = [
            (46.20, 6.14), // Geneva
            (46.40, 6.27), // Lake Geneva North
            (46.75, 6.45), // Jura / Vaud
            (47.10, 6.85), // Neuchatel Jura
            (47.45, 7.05), // Porrentruy
            (47.56, 7.59), // Basel
            (47.60, 8.20), // Aargau / Rhine
            (47.70, 8.55), // Schaffhausen North
            (47.65, 8.90), // Lake Constance West
            (47.50, 9.60), // Lake Constance East / St. Gallen
            (47.30, 9.55), // Appenzell / Rhine Valley
            (47.05, 9.50), // Liechtenstein border
            (46.90, 10.45), // Graubünden East / Samnaun
            (46.75, 10.40), // Inn Valley East
            (46.55, 10.45), // Mustair Valley
            (46.30, 10.05), // Poschiavo South
            (46.35, 9.60), // Bregaglia
            (46.15, 9.05), // Bellinzona
            (45.83, 8.98), // Mendrisio / Chiasso (Southernmost tip)
            (45.98, 8.85), // Lugano West
            (46.15, 8.55), // Locarno / Lake Maggiore
            (46.40, 8.40), // Bedretto / Gotthard
            (46.35, 8.25), // Formazza West
            (46.30, 8.00), // Simplon
            (45.95, 7.75), // Zermatt / Matterhorn
            (45.93, 7.45), // Grand Combin
            (45.92, 7.05), // Great St Bernard
            (46.02, 6.95), // Trient / Mont Blanc ridge
            (46.35, 6.85), // St Gingolph / Lake Geneva South
            (46.20, 6.14), // Return to Geneva
        ];

        let points: Vec<LLPoint> = coords
            .iter()
            .map(|&(lat, lng)| LLPoint::new(lat, lng).unwrap())
            .collect();

        Self::new(points).unwrap()
    }

    /// Tests if a geographic coordinate (lat, lng) is inside the polygon using ray casting.
    pub fn contains_lat_lng(&self, lat: f64, lng: f64) -> bool {
        // Bounding box pre-check (fast rejection)
        if lat < self.min_lat || lat > self.max_lat || lng < self.min_lng || lng > self.max_lng {
            return false;
        }

        let mut inside = false;
        let n = self.vertices.len();
        let mut j = n - 1;

        for i in 0..n {
            let pi = self.vertices[i];
            let pj = self.vertices[j];

            let lat_i = pi.lat();
            let lng_i = pi.lng();
            let lat_j = pj.lat();
            let lng_j = pj.lng();

            let intersect = ((lng_i > lng) != (lng_j > lng))
                && (lat < (lat_j - lat_i) * (lng - lng_i) / (lng_j - lng_i) + lat_i);

            if intersect {
                inside = !inside;
            }
            j = i;
        }

        inside
    }

    /// Converts the geographic polygon to Cartesian block-space vertices.
    pub fn to_cartesian(&self, transformer: &crate::coordinate_system::transformation::CoordTransformer) -> Vec<(f64, f64)> {
        self.vertices
            .iter()
            .map(|p| {
                let xz = transformer.transform_point(*p);
                (xz.x as f64, xz.z as f64)
            })
            .collect()
    }

    /// Tests if a 2D Cartesian point (x, z) is inside a Cartesian polygon using ray casting.
    pub fn cartesian_contains(poly: &[(f64, f64)], x: f64, z: f64) -> bool {
        let mut inside = false;
        let n = poly.len();
        if n < 3 {
            return true;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (xi, zi) = poly[i];
            let (xj, zj) = poly[j];
            let intersect = ((zi > z) != (zj > z)) && (x < (xj - xi) * (z - zi) / (zj - zi) + xi);
            if intersect {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// Fast test if a 512x512 tile intersects or is inside a Cartesian polygon.
    pub fn cartesian_intersects_tile(poly: &[(f64, f64)], min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        let (x0, z0, x1, z1) = (min_x as f64, min_z as f64, max_x as f64, max_z as f64);
        let cx = (x0 + x1) * 0.5;
        let cz = (z0 + z1) * 0.5;

        // 1. Center of tile inside polygon
        if Self::cartesian_contains(poly, cx, cz) {
            return true;
        }

        // 2. Any corner inside polygon
        if Self::cartesian_contains(poly, x0, z0)
            || Self::cartesian_contains(poly, x1, z0)
            || Self::cartesian_contains(poly, x0, z1)
            || Self::cartesian_contains(poly, x1, z1)
        {
            return true;
        }

        // 3. Any polygon vertex inside tile AABB
        for &(px, pz) in poly {
            if px >= x0 && px <= x1 && pz >= z0 && pz <= z1 {
                return true;
            }
        }

        // 4. Polygon segment bounding box crosses tile border
        let n = poly.len();
        for i in 0..n {
            let j = if i == 0 { n - 1 } else { i - 1 };
            let (px0, pz0) = poly[j];
            let (px1, pz1) = poly[i];

            let seg_min_x = px0.min(px1);
            let seg_max_x = px0.max(px1);
            let seg_min_z = pz0.min(pz1);
            let seg_max_z = pz0.max(pz1);

            if seg_max_x >= x0 && seg_min_x <= x1 && seg_max_z >= z0 && seg_min_z <= z1 {
                return true;
            }
        }

        false
    }

    /// Fast test to check if a bounding box intersects or overlaps with the polygon.
    /// Used to quickly keep or discard entire 512x512 block tiles.
    pub fn intersects_bbox(&self, bbox: &crate::coordinate_system::geographic::LLBBox) -> bool {
        let b_min = bbox.min();
        let b_max = bbox.max();

        // 1. AABB overlap pre-check
        if b_max.lat() < self.min_lat
            || b_min.lat() > self.max_lat
            || b_max.lng() < self.min_lng
            || b_min.lng() > self.max_lng
        {
            return false;
        }

        // 2. Check if center of box is inside polygon
        let center_lat = (b_min.lat() + b_max.lat()) / 2.0;
        let center_lng = (b_min.lng() + b_max.lng()) / 2.0;
        if self.contains_lat_lng(center_lat, center_lng) {
            return true;
        }

        // 3. Check if any box corner is inside polygon
        if self.contains_lat_lng(b_min.lat(), b_min.lng())
            || self.contains_lat_lng(b_min.lat(), b_max.lng())
            || self.contains_lat_lng(b_max.lat(), b_min.lng())
            || self.contains_lat_lng(b_max.lat(), b_max.lng())
        {
            return true;
        }

        // 4. Check if any polygon vertex is inside box
        for p in &self.vertices {
            if bbox.contains(&p) {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_semicolon_format() {
        let s = "47.0,8.0; 47.5,8.0; 47.5,8.5; 47.0,8.5";
        let mask = PolygonMask::parse(s).unwrap();
        assert_eq!(mask.vertices.len(), 5); // Automatically closed
        assert!(mask.contains_lat_lng(47.2, 8.2));
        assert!(!mask.contains_lat_lng(47.9, 8.2));
    }

    #[test]
    fn test_switzerland_border_inclusion() {
        let mask = PolygonMask::switzerland_border();

        // Bern, Zurich, Lucerne, Geneva, Lugano should all be INSIDE Switzerland
        assert!(mask.contains_lat_lng(46.948, 7.447), "Bern should be inside");
        assert!(mask.contains_lat_lng(47.376, 8.541), "Zurich should be inside");
        assert!(mask.contains_lat_lng(47.050, 8.309), "Lucerne should be inside");
        assert!(mask.contains_lat_lng(46.204, 6.143), "Geneva should be inside");

        // Milan, Paris, Munich, Frankfurt should all be OUTSIDE Switzerland
        assert!(!mask.contains_lat_lng(45.464, 9.190), "Milan should be outside");
        assert!(!mask.contains_lat_lng(48.135, 11.582), "Munich should be outside");
        assert!(!mask.contains_lat_lng(48.856, 2.352), "Paris should be outside");
    }
}
