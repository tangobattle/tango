pub struct HQ2XFilter;
impl super::Filter for HQ2XFilter {
    fn output_size(&self, [w, h]: [usize; 2]) -> [usize; 2] {
        [w * 2, h * 2]
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], [w, h]: [usize; 2]) {
        hqx::hq2x(bytemuck::cast_slice(src), bytemuck::cast_slice_mut(dst), w, h);
    }
}

pub struct HQ3XFilter;
impl super::Filter for HQ3XFilter {
    fn output_size(&self, [w, h]: [usize; 2]) -> [usize; 2] {
        [w * 3, h * 3]
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], [w, h]: [usize; 2]) {
        hqx::hq3x(bytemuck::cast_slice(src), bytemuck::cast_slice_mut(dst), w, h);
    }
}

pub struct HQ4XFilter;
impl super::Filter for HQ4XFilter {
    fn output_size(&self, [w, h]: [usize; 2]) -> [usize; 2] {
        [w * 4, h * 4]
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], [w, h]: [usize; 2]) {
        hqx::hq4x(bytemuck::cast_slice(src), bytemuck::cast_slice_mut(dst), w, h);
    }
}
