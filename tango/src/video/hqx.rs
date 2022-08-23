pub struct HQ2XFilter;
impl super::Filter for HQ2XFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        (size.0 * 2, size.1 * 2)
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], size: (usize, usize)) {
        hqx::hq2x(
            unsafe { src.align_to::<u32>().1 },
            unsafe { dst.align_to_mut::<u32>().1 },
            size.0,
            size.1,
        );
    }
}

pub struct HQ3XFilter;
impl super::Filter for HQ3XFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        (size.0 * 3, size.1 * 3)
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], size: (usize, usize)) {
        hqx::hq3x(
            unsafe { src.align_to::<u32>().1 },
            unsafe { dst.align_to_mut::<u32>().1 },
            size.0,
            size.1,
        );
    }
}

pub struct HQ4XFilter;
impl super::Filter for HQ4XFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        (size.0 * 4, size.1 * 4)
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], size: (usize, usize)) {
        hqx::hq4x(
            unsafe { src.align_to::<u32>().1 },
            unsafe { dst.align_to_mut::<u32>().1 },
            size.0,
            size.1,
        );
    }
}
