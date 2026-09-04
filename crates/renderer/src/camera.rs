//! Free camera — position, yaw/pitch, perspective.

use glam::{Mat4, Vec3};

/// World coords: Minecraft X east, Y up, Z south.
/// Renderer coords: same as world (no flip), Y up, right-handed.
#[derive(Debug, Clone)]
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32, // degrees, 0 = south? Minecraft yaw 0 = south, 90 = west, -90 east. We'll use -yaw for view.
    pub pitch: f32, // degrees, -90 up, 90 down
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
}

impl Camera {
    pub fn new(position: Vec3, aspect: f32) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 70_f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            aspect,
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();
        let dir = Vec3::new(
            -yaw_rad.sin() * pitch_rad.cos(),
            -pitch_rad.sin(),
            yaw_rad.cos() * pitch_rad.cos(),
        );
        let target = self.position + dir;
        Mat4::look_at_rh(self.position, target, Vec3::Y)
    }

    pub fn proj_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, self.aspect, self.near, self.far)
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj_matrix() * self.view_matrix()
    }

    pub fn move_forward(&mut self, dist: f32) {
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();
        let forward = Vec3::new(
            -yaw_rad.sin() * pitch_rad.cos(),
            -pitch_rad.sin(),
            yaw_rad.cos() * pitch_rad.cos(),
        );
        self.position += forward * dist;
    }

    pub fn move_right(&mut self, dist: f32) {
        let yaw_rad = self.yaw.to_radians();
        let right = Vec3::new(yaw_rad.cos(), 0.0, yaw_rad.sin());
        self.position += right * dist;
    }

    pub fn move_up(&mut self, dist: f32) {
        self.position.y += dist;
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    pub fn new() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
        }
    }
    pub fn update(&mut self, camera: &Camera) {
        self.view_proj = camera.view_proj().to_cols_array_2d();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn view_proj_not_nan() {
        let cam = Camera::new(Vec3::new(0.0, 70.0, 0.0), 16.0 / 9.0);
        let vp = cam.view_proj();
        assert!(!vp.to_cols_array().iter().any(|v| v.is_nan()));
    }
}
