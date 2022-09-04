pub mod hqx;
pub mod mmpx;

pub trait Filter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize);
    fn apply(&self, input: &[u8], output: &mut [u8], size: (usize, usize));
}

pub struct NullFilter;
impl Filter for NullFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        size
    }
    fn apply(&self, input: &[u8], output: &mut [u8], _size: (usize, usize)) {
        output.copy_from_slice(input)
    }
}

pub fn filter_by_name(name: &str) -> Option<Box<dyn Filter + Sync + Send>> {
    match name {
        "null" | "" => Some(Box::new(NullFilter)),
        "hq2x" => Some(Box::new(hqx::HQ2XFilter)),
        "hq3x" => Some(Box::new(hqx::HQ3XFilter)),
        "hq4x" => Some(Box::new(hqx::HQ4XFilter)),
        "mmpx" => Some(Box::new(mmpx::MMPXFilter)),
        _ => None,
    }
}
