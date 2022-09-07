use image::{ImageBuffer, Pixel, Rgba, RgbaImage};
use rayon::prelude::*;

use crate::neighborhood::GetMazzoleniNeighborhood;
use crate::utils;
use std::ops::Deref;

fn get_pixel_checked<P: 'static + Pixel, C: Deref<Target = [P::Subpixel]>>(
    image: &ImageBuffer<P, C>,
    i: i32,
    j: i32,
) -> &P {
    let x = utils::clamp(i, 0, image.width() as i32 - 1);
    let y = utils::clamp(j, 0, image.height() as i32 - 1);

    image.get_pixel(x as u32, y as u32)
}

impl GetMazzoleniNeighborhood<Rgba<u8>> for RgbaImage {
    fn get_a(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 - 1, y as i32 - 1)
    }
    fn get_b(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32, y as i32 - 1)
    }
    fn get_c(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 + 1, y as i32 - 1)
    }

    fn get_d(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 - 1, y as i32)
    }
    fn get_e(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32, y as i32)
    }
    fn get_f(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 + 1, y as i32)
    }

    fn get_g(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 - 1, y as i32 + 1)
    }
    fn get_h(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32, y as i32 + 1)
    }
    fn get_i(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 + 1, y as i32 + 1)
    }

    fn get_p(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32, y as i32 - 2)
    }
    fn get_q(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 - 2, y as i32)
    }
    fn get_r(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32 + 2, y as i32)
    }
    fn get_s(&self, x: u32, y: u32) -> Rgba<u8> {
        *get_pixel_checked(self, x as i32, y as i32 + 2)
    }
}

pub fn magnify(image: &RgbaImage) -> RgbaImage {
    let width = image.width();
    let height = image.height();

    let mut output_image: RgbaImage = RgbaImage::new(width * 2, height * 2);

    let out_vec: Vec<((u32, u32), Rgba<u8>)> = (0..height)
        .into_par_iter()
        .fold(
            || Vec::new(),
            |mut vec: Vec<((u32, u32), Rgba<u8>)>, y: u32| {
                let x = 0;

                let mut a = image.get_a(x, y);
                let mut b = image.get_b(x, y);

                let mut d = image.get_d(x, y);
                let mut e = image.get_e(x, y);
                let mut f = image.get_f(x, y);

                let mut g = image.get_g(x, y);
                let mut h = image.get_h(x, y);

                let p = image.get_p(x, y);
                let mut q = image.get_q(x, y);
                let s = image.get_s(x, y);

                for x in 0..width {
                    let c = image.get_c(x, y);
                    let r = image.get_r(x, y);
                    let i = image.get_i(x, y);

                    let b_luma = b.to_luma().0[0];
                    let d_luma = d.to_luma().0[0];
                    let e_luma = e.to_luma().0[0];
                    let f_luma = f.to_luma().0[0];
                    let h_luma = h.to_luma().0[0];

                    let mut j: Rgba<u8>;
                    let mut k: Rgba<u8>;
                    let mut l: Rgba<u8>;
                    let mut m: Rgba<u8>;

                    j = e;
                    k = e;
                    l = e;
                    m = e;

                    // 1:1 slope rules
                    if (d == b && d != h && d != f)
                        && (e_luma >= d_luma || e == a)
                        && utils::any_eq3(e, a, c, g)
                        && ((e_luma < d_luma) || a != d || e != p || e != q)
                    {
                        j = d;
                    }

                    if (b == f && b != d && b != h)
                        && (e_luma >= b_luma || e == c)
                        && utils::any_eq3(e, a, c, i)
                        && ((e_luma < b_luma) || c != b || e != p || e != r)
                    {
                        k = b;
                    }

                    if (h == d && h != f && h != b)
                        && (e_luma >= h_luma || e == g)
                        && utils::any_eq3(e, a, g, i)
                        && ((e_luma < h_luma) || g != h || e != s || e != q)
                    {
                        l = h;
                    }

                    if (f == h && f != b && f != d)
                        && (e_luma >= f_luma || e == i)
                        && utils::any_eq3(e, c, g, i)
                        && ((e_luma < f_luma) || i != h || e != r || e != s)
                    {
                        m = f;
                    }

                    // Intersection rules
                    if (e != f && utils::all_eq4(e, c, i, d, q) && utils::all_eq2(f, b, h))
                        && (f != *get_pixel_checked(image, x as i32 + 3, y as i32))
                    {
                        k = f;
                        m = f;
                    }
                    if (e != d && utils::all_eq4(e, a, g, f, r) && utils::all_eq2(d, b, h))
                        && (d != *get_pixel_checked(image, x as i32 - 3, y as i32))
                    {
                        j = d;
                        l = d;
                    }
                    if (e != h && utils::all_eq4(e, g, i, b, p) && utils::all_eq2(h, d, f))
                        && (h != *get_pixel_checked(image, x as i32, y as i32 + 3))
                    {
                        l = h;
                        m = h;
                    }
                    if (e != b && utils::all_eq4(e, a, c, h, s) && utils::all_eq2(b, d, f))
                        && (b != *get_pixel_checked(image, x as i32, y as i32 - 3))
                    {
                        j = b;
                        k = b;
                    }

                    if b_luma < e_luma && utils::all_eq4(e, g, h, i, s) && utils::none_eq4(e, a, d, c, f) {
                        j = b;
                        k = b;
                    }
                    if h_luma < e_luma && utils::all_eq4(e, a, b, c, p) && utils::none_eq4(e, d, g, i, f) {
                        l = h;
                        m = h;
                    }
                    if f_luma < e_luma && utils::all_eq4(e, a, d, g, q) && utils::none_eq4(e, b, c, i, h) {
                        k = f;
                        m = f;
                    }
                    if d_luma < e_luma && utils::all_eq4(e, c, f, i, r) && utils::none_eq4(e, b, a, g, h) {
                        j = d;
                        l = d;
                    }

                    // 2:1 slope rules
                    if h != b {
                        if h != a && h != e && h != c {
                            if utils::all_eq3(h, g, f, r)
                                && utils::none_eq2(h, d, *get_pixel_checked(image, x as i32 + 2, y as i32 - 1))
                            {
                                l = m;
                            }
                            if utils::all_eq3(h, i, d, q)
                                && utils::none_eq2(h, f, *get_pixel_checked(image, x as i32 - 2, y as i32 - 1))
                            {
                                m = l
                            };
                        }

                        if b != i && b != g && b != e {
                            if utils::all_eq3(b, a, f, r)
                                && utils::none_eq2(b, d, *get_pixel_checked(image, x as i32 + 2, y as i32 + 1))
                            {
                                j = k;
                            }
                            if utils::all_eq3(b, c, d, q)
                                && utils::none_eq2(b, f, *get_pixel_checked(image, x as i32 - 2, y as i32 + 1))
                            {
                                k = j;
                            }
                        }
                    } // H !== B

                    if f != d {
                        if d != i && d != e && d != c {
                            if utils::all_eq3(d, a, h, s)
                                && utils::none_eq2(d, b, *get_pixel_checked(image, x as i32 + 1, y as i32 + 2))
                            {
                                j = l;
                            }
                            if utils::all_eq3(d, g, b, p)
                                && utils::none_eq2(d, h, *get_pixel_checked(image, x as i32 + 1, y as i32 - 2))
                            {
                                l = j;
                            }
                        }

                        if f != e && f != a && f != g {
                            if utils::all_eq3(f, c, h, s)
                                && utils::none_eq2(f, b, *get_pixel_checked(image, x as i32 - 1, y as i32 + 2))
                            {
                                k = m;
                            }
                            if utils::all_eq3(f, i, b, p)
                                && utils::none_eq2(f, h, *get_pixel_checked(image, x as i32 - 1, y as i32 - 2))
                            {
                                m = k;
                            }
                        }
                    } // F !== D

                    vec.append(&mut vec![
                        ((x * 2, y * 2), j),
                        ((x * 2 + 1, y * 2), k),
                        ((x * 2, y * 2 + 1), l),
                        ((x * 2 + 1, y * 2 + 1), m),
                    ]);

                    a = b;
                    b = c;

                    q = d;
                    d = e;
                    e = f;
                    f = r;

                    g = h;
                    h = i;
                }

                vec
            },
        )
        .reduce(
            || Vec::new(),
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        );

    out_vec.iter().for_each(|((x, y), pixel)| {
        output_image.put_pixel(*x, *y, *pixel);
    });

    output_image
}
