use etagere::{AtlasAllocator, size2};

/// Default size of the glyph atlas in pixels (square). Matches the warp-renderer spec §6.3.
pub const DEFAULT_ATLAS_SIZE: u32 = 2048;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AllocatedRegion {
    pub id: etagere::AllocId,
    /// Pixel rectangle inside the atlas.
    pub px_min: [u32; 2],
    pub px_max: [u32; 2],
    /// Normalized UV (px / atlas_size).
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

pub struct Atlas {
    inner: AtlasAllocator,
    size: u32,
}

impl Atlas {
    pub fn new(size: u32) -> Self {
        Self { inner: AtlasAllocator::new(size2(size as i32, size as i32)), size }
    }

    pub fn size(&self) -> u32 { self.size }

    pub fn allocate(&mut self, w: u32, h: u32) -> Option<AllocatedRegion> {
        let alloc = self.inner.allocate(size2(w as i32, h as i32))?;
        let r = alloc.rectangle;
        let s = self.size as f32;
        Some(AllocatedRegion {
            id: alloc.id,
            px_min: [r.min.x as u32, r.min.y as u32],
            px_max: [r.max.x as u32, r.max.y as u32],
            uv_min: [r.min.x as f32 / s, r.min.y as f32 / s],
            uv_max: [r.max.x as f32 / s, r.max.y as f32 / s],
        })
    }

    pub fn deallocate(&mut self, id: etagere::AllocId) {
        self.inner.deallocate(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_fits_inside_atlas() {
        let mut a = Atlas::new(256);
        let r = a.allocate(32, 32).expect("fits");
        assert!(r.px_max[0] <= 256 && r.px_max[1] <= 256);
        assert!(r.uv_max[0] <= 1.0 && r.uv_max[1] <= 1.0);
    }

    #[test]
    fn allocate_returns_none_when_full() {
        let mut a = Atlas::new(64);
        // 64x64 = 4096 px²; allocating 32x32 four times fills it (in best case).
        // Then one more should fail.
        let mut allocs = Vec::new();
        while let Some(r) = a.allocate(32, 32) {
            allocs.push(r);
            if allocs.len() > 10 { panic!("atlas should have filled by now"); }
        }
        // After loop exits, next must also fail.
        assert!(a.allocate(32, 32).is_none());
    }

    #[test]
    fn deallocate_makes_room() {
        let mut a = Atlas::new(64);
        let first = a.allocate(64, 64).expect("first fits");
        assert!(a.allocate(32, 32).is_none(), "atlas full");
        a.deallocate(first.id);
        assert!(a.allocate(32, 32).is_some(), "room after dealloc");
    }
}
