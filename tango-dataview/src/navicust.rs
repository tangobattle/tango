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

fn rotate<T>(arr: &ndarray::Array2<T>, num: usize) -> std::borrow::Cow<'_, ndarray::Array2<T>>
where
    T: Clone,
{
    let mut arr = std::borrow::Cow::Borrowed(arr);
    for _ in 0..num {
        arr = std::borrow::Cow::Owned(rotate90(&arr));
    }
    arr
}

fn ncp_bitmap(info: &dyn crate::rom::NavicustPart, compressed: bool, rot: u8) -> crate::rom::NavicustBitmap {
    rotate(
        &info
            .compressed_bitmap()
            .filter(|_| compressed)
            .unwrap_or_else(|| info.uncompressed_bitmap()),
        rot as usize,
    )
    .into_owned()
}

pub type MaterializedNavicust = ndarray::Array2<Option<usize>>;

pub fn materialized_from_wram(buf: &[u8], size: [usize; 2]) -> MaterializedNavicust {
    ndarray::Array2::from_shape_vec(size, buf.iter().map(|v| v.checked_sub(1).map(|v| v as usize)).collect()).unwrap()
}

/// The navicust "color bar": the distinct colors of the installed parts,
/// in the order the parts are placed (slot order). The game stores (and
/// draws) this at a fixed save offset; it isn't derived at runtime, so
/// the editor must rebuild it when parts change. Returned as
/// `Vec<Option<_>>` to mirror the stored slots (a read of an arbitrary
/// save can have `None` gaps).
pub fn materialize_color_bar(
    navicust_view: &dyn crate::save::NavicustView,
    assets: &dyn crate::rom::Assets,
) -> Vec<Option<crate::rom::NavicustPartColor>> {
    let mut colors: Vec<crate::rom::NavicustPartColor> = Vec::new();
    for i in 0..navicust_view.count() {
        let Some(ncp) = navicust_view.navicust_part(i) else {
            continue;
        };
        let Some(info) = assets.navicust_part(ncp.id) else {
            continue;
        };
        let Some(c) = info.color() else { continue };
        if !colors.contains(&c) {
            colors.push(c);
        }
    }
    colors.into_iter().map(Some).collect()
}

/// Inverse of a byte→color decode (each game's `rom::navicust_part_color`):
/// the raw byte that maps to `color`, or 0 if none. Lets the save layer
/// reuse the ROM's color decoding for writing the color bar instead of
/// duplicating the mapping.
pub fn color_to_raw(
    color: &crate::rom::NavicustPartColor,
    from_raw: impl Fn(u8) -> Option<crate::rom::NavicustPartColor>,
) -> u8 {
    (1u8..=0xff).find(|&b| from_raw(b).as_ref() == Some(color)).unwrap_or(0)
}

/// The navi's off-color budget (BN3). If the navi limits off-color parts
/// ([`crate::save::NavicustView::unrestricted_colors`] is `Some`), returns
/// the unrestricted color set together with how many currently-installed
/// parts fall *outside* it — the rule allows at most one. `None` when the
/// navi has no color limit (BN4/5/6). Shared by the editor's commit-time
/// check and its palette greying so both agree on what's allowed.
pub fn off_color_budget(
    navicust_view: &dyn crate::save::NavicustView,
    assets: &dyn crate::rom::Assets,
) -> Option<(Vec<crate::rom::NavicustPartColor>, usize)> {
    let free = navicust_view.unrestricted_colors()?;
    let installed_off = (0..navicust_view.count())
        .filter_map(|i| navicust_view.navicust_part(i))
        .filter_map(|p| assets.navicust_part(p.id).and_then(|info| info.color()))
        .filter(|c| !free.contains(c))
        .count();
    Some((free, installed_off))
}

pub fn materialize(
    navicust_view: &dyn crate::save::NavicustView,
    max_size: [usize; 2],
    assets: &dyn crate::rom::Assets,
) -> MaterializedNavicust {
    let mut materialized = ndarray::Array2::from_elem(max_size, None);
    for i in 0..navicust_view.count() {
        let Some(ncp) = navicust_view.navicust_part(i) else {
            continue;
        };

        let Some(info) = assets.navicust_part(ncp.id) else {
            continue;
        };

        let bitmap = ncp_bitmap(info.as_ref(), ncp.compressed, ncp.rot);
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
            materialized.slice_mut(ndarray::s![dst_y.., dst_x..]).rows_mut(),
        ) {
            for (src, dst) in std::iter::zip(src_row, dst_row) {
                if *src {
                    *dst = Some(i);
                }
            }
        }
    }
    materialized
}
