const WGS84_A: f64 = 6_378_137.0;
const WGS84_F: f64 = 1.0 / 298.257_223_563;

pub struct GeoContext {
    heading_rad: f64,
    scale: f64,
    origin_ecef: [f64; 3],
    rot_enu_to_ecef: [[f64; 3]; 3],
}

impl GeoContext {
    pub fn new(lat_deg: f64, lon_deg: f64, height: f64, heading_deg: f64, scale: f64) -> Self {
        let lat_rad = lat_deg.to_radians();
        let lon_rad = lon_deg.to_radians();
        let heading_rad = heading_deg.to_radians();
        let origin_ecef = geodetic_to_ecef(lat_rad, lon_rad, height);
        let rot_enu_to_ecef = enu_to_ecef_matrix(lat_rad, lon_rad);
        Self {
            heading_rad,
            scale,
            origin_ecef,
            rot_enu_to_ecef,
        }
    }

    // 模型坐标默认 Y-up，heading 绕 +Y 旋转后再做缩放。
    pub fn transform_local(&self, pos: [f64; 3]) -> [f64; 3] {
        let x = pos[0] * self.scale;
        let y = pos[1] * self.scale;
        let z = pos[2] * self.scale;
        let (sin_h, cos_h) = self.heading_rad.sin_cos();
        let xr = x * cos_h - z * sin_h;
        let zr = x * sin_h + z * cos_h;
        [xr, y, zr]
    }

    pub fn transform_matrix(&self) -> [f64; 16] {
        self.transform_matrix_with_axes(identity_axis_matrix())
    }

    pub fn transform_matrix_with_axes(&self, axis_matrix: [[f64; 3]; 3]) -> [f64; 16] {
        let (sin_h, cos_h) = self.heading_rad.sin_cos();
        let heading = [
            [cos_h, 0.0, -sin_h],
            [sin_h, 0.0, cos_h],
            [0.0, 1.0, 0.0],
        ];

        let mut heading_axes = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                let mut value = 0.0;
                for k in 0..3 {
                    value += heading[i][k] * axis_matrix[k][j];
                }
                heading_axes[i][j] = value;
            }
        }

        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                let mut value = 0.0;
                for k in 0..3 {
                    value += self.rot_enu_to_ecef[i][k] * heading_axes[k][j];
                }
                m[i][j] = value * self.scale;
            }
        }

        [
            m[0][0],
            m[1][0],
            m[2][0],
            0.0,
            m[0][1],
            m[1][1],
            m[2][1],
            0.0,
            m[0][2],
            m[1][2],
            m[2][2],
            0.0,
            self.origin_ecef[0],
            self.origin_ecef[1],
            self.origin_ecef[2],
            1.0,
        ]
    }
}

fn identity_axis_matrix() -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    m[0][0] = 1.0;
    m[1][1] = 1.0;
    m[2][2] = 1.0;
    m
}

fn geodetic_to_ecef(lat_rad: f64, lon_rad: f64, height: f64) -> [f64; 3] {
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let sin_lat = lat_rad.sin();
    let cos_lat = lat_rad.cos();
    let sin_lon = lon_rad.sin();
    let cos_lon = lon_rad.cos();

    let n = WGS84_A / (1.0 - e2 * sin_lat * sin_lat).sqrt();

    let x = (n + height) * cos_lat * cos_lon;
    let y = (n + height) * cos_lat * sin_lon;
    let z = (n * (1.0 - e2) + height) * sin_lat;

    [x, y, z]
}

// 本地坐标约定为 X东Y上Z北（EUN），因此列向量依次为 East/Up/North。
fn enu_to_ecef_matrix(lat_rad: f64, lon_rad: f64) -> [[f64; 3]; 3] {
    let (sin_lat, cos_lat) = lat_rad.sin_cos();
    let (sin_lon, cos_lon) = lon_rad.sin_cos();

    [
        [-sin_lon, cos_lat * cos_lon, -sin_lat * cos_lon],
        [cos_lon, cos_lat * sin_lon, -sin_lat * sin_lon],
        [0.0, sin_lat, cos_lat],
    ]
}
