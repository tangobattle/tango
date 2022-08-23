pub trait GetMazzoleniNeighborhood<P> {
    fn get_a(&self, x: u32, y: u32) -> P;
    fn get_b(&self, x: u32, y: u32) -> P;
    fn get_c(&self, x: u32, y: u32) -> P;
    fn get_d(&self, x: u32, y: u32) -> P;
    fn get_e(&self, x: u32, y: u32) -> P;
    fn get_f(&self, x: u32, y: u32) -> P;
    fn get_g(&self, x: u32, y: u32) -> P;
    fn get_h(&self, x: u32, y: u32) -> P;
    fn get_i(&self, x: u32, y: u32) -> P;
    fn get_p(&self, x: u32, y: u32) -> P;
    fn get_q(&self, x: u32, y: u32) -> P;
    fn get_r(&self, x: u32, y: u32) -> P;
    fn get_s(&self, x: u32, y: u32) -> P;
}
