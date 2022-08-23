pub struct MMPXFilter;
impl super::Filter for MMPXFilter {
    fn output_size(&self, size: (usize, usize)) -> (usize, usize) {
        (size.0 * 2, size.1 * 2)
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], size: (usize, usize)) {
        let src_img =
            image::RgbaImage::from_raw(size.0 as u32, size.1 as u32, src.to_vec()).unwrap();
        dst.copy_from_slice(mmpx::magnify(&src_img).as_raw());
    }
}
