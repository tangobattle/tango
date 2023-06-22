pub struct MMPXFilter;
impl super::Filter for MMPXFilter {
    fn output_size(&self, [w, h]: [usize; 2]) -> [usize; 2] {
        [w * 2, h * 2]
    }

    fn apply(&self, src: &[u8], dst: &mut [u8], [w, h]: [usize; 2]) {
        let src_img = image::RgbaImage::from_raw(w as u32, h as u32, src.to_vec()).unwrap();
        dst.copy_from_slice(mmpx::magnify(&src_img).as_raw());
    }
}
