fn rotate90<T>(arr: &ndarray::Array2<T>) -> ndarray::Array2<T>
where
    T: Clone,
{
    let mut arr = arr.t().as_standard_layout().into_owned();
    for row in arr.rows_mut() {
        row.into_slice().unwrap().reverse();
    }
    arr
}

fn rotate<'a, T>(arr: &'a ndarray::Array2<T>, num: usize) -> std::borrow::Cow<'a, ndarray::Array2<T>>
where
    T: Clone,
{
    let mut arr = std::borrow::Cow::Borrowed(arr);
    for _ in 0..num {
        arr = std::borrow::Cow::Owned(rotate90(&arr));
    }
    arr
}

fn ncp_bitmap<'a>(
    info: &'a Box<dyn crate::rom::NavicustPart + 'a>,
    compressed: bool,
    rot: u8,
) -> crate::rom::NavicustBitmap {
    rotate(
        &if compressed {
            info.compressed_bitmap()
        } else {
            info.uncompressed_bitmap()
        },
        rot as usize,
    )
    .into_owned()
}

pub type ComposedNavicust = ndarray::Array2<Option<usize>>;

pub fn compose<'a>(
    navicust_view: &Box<dyn crate::save::NavicustView<'a> + 'a>,
    assets: &Box<dyn crate::rom::Assets + Send + Sync + 'a>,
) -> ComposedNavicust {
    let mut composed = ndarray::Array2::from_elem((navicust_view.height(), navicust_view.width()), None);
    for i in 0..navicust_view.count() {
        let ncp = if let Some(ncp) = navicust_view.navicust_part(i) {
            ncp
        } else {
            continue;
        };

        let info = if let Some(info) = assets.navicust_part(ncp.id, ncp.variant) {
            info
        } else {
            continue;
        };

        let bitmap = ncp_bitmap(&info, ncp.compressed, ncp.rot);
        let (bitmap_height, bitmap_width) = bitmap.dim();
        let ncp_y = (ncp.row as isize) - bitmap_height as isize / 2;
        let ncp_x = (ncp.col as isize) - bitmap_width as isize / 2;

        let (src_y, dst_y) = if ncp_y < 0 {
            (-ncp_y as usize, 0)
        } else {
            (0, ncp_y as usize)
        };

        let (src_x, dst_x) = if ncp_x < 0 {
            (-ncp_x as usize, 0)
        } else {
            (0, ncp_x as usize)
        };

        for (src_row, dst_row) in std::iter::zip(
            bitmap.slice(ndarray::s![src_y.., src_x..]).rows(),
            composed.slice_mut(ndarray::s![dst_y.., dst_x..]).rows_mut(),
        ) {
            for (src, dst) in std::iter::zip(src_row, dst_row) {
                if *src {
                    *dst = Some(i);
                }
            }
        }
    }
    composed
}
