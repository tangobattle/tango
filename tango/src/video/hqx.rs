pub struct HQ2XFilter;
impl super::Filter for HQ2XFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        (size.0 * 2, size.1 * 2)
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], size: (usize, usize)) {
        hqx::hq2x(bytemuck::cast_slice(src), bytemuck::cast_slice_mut(dst), size.0, size.1);
    }
}

pub struct HQ3XFilter;
impl super::Filter for HQ3XFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        (size.0 * 3, size.1 * 3)
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], size: (usize, usize)) {
        hqx::hq3x(bytemuck::cast_slice(src), bytemuck::cast_slice_mut(dst), size.0, size.1);
    }
}

pub struct HQ4XFilter;
impl super::Filter for HQ4XFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        (size.0 * 4, size.1 * 4)
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], size: (usize, usize)) {
        hqx::hq4x(bytemuck::cast_slice(src), bytemuck::cast_slice_mut(dst), size.0, size.1);
    }
}
