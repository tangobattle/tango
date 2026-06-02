//! BN3 compiles MegaMan's abilities from style + placed NaviCust programs +
//! the active EX Code into a 0x40-byte array at save `0x5770` — the array the
//! game actually reads (it's a cache, not re-derived from the inputs on
//! load). Editing the inputs therefore has no effect until this is rebuilt.
//!
//! This is a faithful pure-Rust port of the game's rebuild
//! (`0x803b468→0x8047346→0x803c73c→0x803c370→0x803cce8` in A3XE), validated
//! byte-exact against the game on the persistent slots across every style,
//! every EX Code, and every NaviCust program on/off the command line. The
//! battle-scratch slots ([`SCRATCH`]) are left to the game to recompute at
//! battle start, so they're not written here. See the
//! `project_bn3_ability_compiler` reverse-engineering notes.

const STYLE_OFFSET: usize = 0x1881;
const EXCODE_OFFSET: usize = 0x1270;
const MAXHP_BASE_OFFSET: usize = 0x1a20; // per-level HP index; maxHP slot = *4 + 0x14
const PARTS_OFFSET: usize = 0x1300; // 25 × 8-byte RawNavicustPart, byte0 = id
const MATERIALIZED_OFFSET: usize = 0x1d90; // 5×5 row-major, byte = slot+1 (0 = empty)
const COMMAND_LINE_ROW: usize = 2;

/// Ability-array slots the game recomputes from scratch at battle start
/// (style/element/color-bar scratch). We don't compute these — leaving the
/// save's existing bytes is harmless and avoids needing the color-bar pass.
pub(super) const SCRATCH: [usize; 7] = [0x07, 0x0b, 0x10, 0x11, 0x16, 0x19, 0x1e];

pub(super) const ABILITY_ARRAY_OFFSET: usize = 0x5770;
pub(super) const ABILITY_ARRAY_LEN: usize = 0x40;

/// Compile the full ability array from the save's current inputs (style,
/// placed NaviCust programs + their command-line membership, EX Code).
pub(super) fn compile(buf: &[u8]) -> [u8; ABILITY_ARRAY_LEN] {
    let mut a = [0u8; ABILITY_ARRAY_LEN];

    // --- style base (game fn 0x8047346) ---
    let typ = (buf[STYLE_OFFSET] >> 3) & 7;
    a[0x14] = if typ == 3 { 6 } else { 5 }; // mega folder (Team = 6)
    a[0x15] = 1; // giga folder
    a[0x13] = if typ == 2 { 6 } else { 5 }; // custom gauge (Custom = 6)
    a[0x26] = if typ == 1 { 2 } else { 1 };
    a[0x08] = if typ == 1 { 1 } else { 0 };
    a[0x0d] = if typ == 6 { 0 } else { 1 };
    let maxhp = (buf[MAXHP_BASE_OFFSET] as u16) * 4 + 0x14;
    a[0x2c..0x2e].copy_from_slice(&maxhp.to_le_bytes());
    a[0x1b] = 0xff;

    // Command-line membership: a part is "on the command line" if any of its
    // materialized cells sits in the command-line row.
    let mut on_command_line = [false; 25];
    for col in 0..5 {
        let v = buf[MATERIALIZED_OFFSET + COMMAND_LINE_ROW * 5 + col];
        if v != 0 {
            on_command_line[(v - 1) as usize] = true;
        }
    }

    // --- NaviCust programs (game fns 0x803c73c/0x803c370) ---
    for slot in 0..25 {
        let id = buf[PARTS_OFFSET + slot * 8];
        if id == 0 {
            continue;
        }
        apply_program(&mut a, (id / 4) as usize, on_command_line[slot]);
    }

    // --- EX Code (game fn 0x803cce8) ---
    apply_excode(&mut a, buf[EXCODE_OFFSET]);

    // --- post-process: custom gauge clamped to [2, 10] (0x803c8d8) ---
    a[0x13] = a[0x13].clamp(2, 10);
    a
}

fn add_hp(a: &mut [u8; ABILITY_ARRAY_LEN], v: u16) {
    let n = u16::from_le_bytes([a[0x2c], a[0x2d]]).wrapping_add(v);
    a[0x2c..0x2e].copy_from_slice(&n.to_le_bytes());
}

/// Apply one placed program's effect. Each program has an off-line and an
/// on-line (command-line) effect that can differ — additive (e.g. GigFldr's
/// bug applies anywhere, its folder bonus only on the line) or replacement
/// (e.g. the appearance programs). `cl` = on the command line.
fn apply_program(a: &mut [u8; ABILITY_ARRAY_LEN], prog: usize, cl: bool) {
    let addc4 = |a: &mut [u8; ABILITY_ARRAY_LEN], i: usize| a[i] = (a[i] + 1).min(4);
    // off-line / always part
    match prog {
        3 if !cl => a[0x0c] = 1,
        4..=9 if !cl => a[0x17] = 0x13,
        28 if !cl => a[0x1d] = 1,
        31 if !cl => a[0x18] = 2,
        35 => a[0x0d] += 1,
        36 => add_hp(a, 0x14),
        37 => add_hp(a, 0x28),
        38 => add_hp(a, 0x3c),
        39 => add_hp(a, 0x64),
        40 => a[0x12] += 5,
        41 => {
            let cap = if a[0x26] == 1 { 4 } else { 9 };
            a[0x08] = (a[0x08] + a[0x26]).min(cap);
        }
        42 => addc4(a, 0x09),
        43 => addc4(a, 0x0a),
        48 => a[0x1f] = 1,
        49 => {
            // Grants a bundle (on the command line, below) and halves max HP.
            a[0x2b] = 1;
            let h = u16::from_le_bytes([a[0x2c], a[0x2d]]) / 2;
            a[0x2c..0x2e].copy_from_slice(&h.to_le_bytes());
        }
        50 => a[0x13] = a[0x13].wrapping_sub(1),
        _ => {}
    }
    if !cl {
        return;
    }
    let set = |a: &mut [u8; ABILITY_ARRAY_LEN], i: usize, v: u8| a[i] = v;
    let addc = |a: &mut [u8; ABILITY_ARRAY_LEN], i: usize, v: u8| a[i] = (a[i] + v).min(10);
    match prog {
        1 => set(a, 0x01, 1),
        2 => set(a, 0x06, 1),
        3 => set(a, 0x0e, 1),
        4 => set(a, 0x17, 0x36),
        5 => set(a, 0x17, 0x37),
        6 => set(a, 0x17, 0x38),
        7 => set(a, 0x17, 0x3a),
        8 => set(a, 0x17, 0x35),
        9 => set(a, 0x17, 0x19),
        10 => a[0x13] += 1,
        11 => a[0x13] += 2,
        12 => addc(a, 0x14, 1),
        13 => addc(a, 0x14, 2),
        14 => set(a, 0x0f, 2),
        15 => set(a, 0x0f, 4),
        16 => set(a, 0x0f, 6),
        17 => set(a, 0x02, 1),
        18 => set(a, 0x02, 2),
        19 => set(a, 0x0f, 8),
        20 => set(a, 0x28, 1),
        21 => set(a, 0x24, 1),
        22 => set(a, 0x25, 1),
        23 => set(a, 0x1a, 1),
        24 => set(a, 0x1b, 2),
        25 => set(a, 0x1b, 3),
        26 => set(a, 0x1b, 1),
        27 => set(a, 0x1b, 4),
        28 => set(a, 0x1d, 2),
        29 => set(a, 0x03, 1),
        30 => set(a, 0x04, 1),
        31 => set(a, 0x18, 1),
        32 => set(a, 0x1c, 1),
        33 => set(a, 0x1c, 2),
        34 => set(a, 0x1c, 3),
        35 => set(a, 0x0c, 1),
        44 => set(a, 0x23, 1),
        45 => set(a, 0x22, 1),
        46 => set(a, 0x21, 1),
        47 => {
            a[0x08] = if a[0x26] == 1 { 4 } else { 9 };
            a[0x09] = 4;
            a[0x0a] = 4;
        }
        48 => addc(a, 0x15, 1),
        49 => {
            set(a, 0x01, 1);
            set(a, 0x02, 2);
            set(a, 0x03, 1);
            set(a, 0x04, 1);
            set(a, 0x06, 1);
            set(a, 0x0e, 1);
            set(a, 0x0f, 4);
            a[0x13] += 1;
            addc(a, 0x14, 1);
        }
        50 => set(a, 0x20, 1),
        _ => {}
    }
}

fn apply_excode(a: &mut [u8; ABILITY_ARRAY_LEN], code: u8) {
    let hp: u16 = match code {
        0x1e => 0x14, 0x1f => 0x1e, 0x20 => 0x28, 0x21 => 0x32, 0x22 => 0x3c, 0x23 => 0x46,
        0x24 => 0x50, 0x25 => 0x5a, 0x26 => 0x64, 0x27 => 0x6e, 0x28 => 0x78, 0x29 => 0x82, 0x2a => 0x8c,
        0x3b => 0xa0, 0x3c => 0xb4, 0x3d => 0xc8, _ => 0,
    };
    if hp != 0 {
        add_hp(a, hp);
    }
    match code {
        0x2b => a[0x01] = 1, 0x2c => a[0x06] = 1, 0x2d => a[0x0e] = 1, 0x2e => a[0x02] = 1, 0x2f => a[0x02] = 2,
        0x30 => a[0x03] = 1, 0x31 => a[0x04] = 1, 0x32 => a[0x0f] = 2, 0x33 => a[0x0f] = 4, 0x34 => a[0x0f] = 6,
        0x35 => a[0x0f] = 8, 0x38 => a[0x18] = 1, 0x39 => a[0x1a] = 1, 0x3a => a[0x22] = 1,
        0x36 => a[0x14] = (a[0x14] + 1).min(10), 0x37 => a[0x14] = (a[0x14] + 2).min(10),
        0x3e => a[0x14] = (a[0x14] + 3).min(10), 0x3f => a[0x14] = (a[0x14] + 4).min(10),
        0x40 => a[0x14] = (a[0x14] + 5).min(10), 0x41 => a[0x15] = (a[0x15] + 1).min(10),
        _ => {}
    }
    // inherent bug
    match code {
        0x24 | 0x25 | 0x26 | 0x27 | 0x2d | 0x30 | 0x34 | 0x35 | 0x37 => a[0x13] = a[0x13].wrapping_sub(1),
        0x2c | 0x28 | 0x29 | 0x2a | 0x38 => a[0x13] = a[0x13].wrapping_sub(2),
        0x3b..=0x41 => a[0x1f] = 1,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    /// `compile` must reproduce a real save's existing (game-compiled)
    /// ability array on the non-scratch slots. Point `BN3_TEST_SAVE` at a
    /// real BN3 .sav to run it; skipped otherwise (e.g. CI).
    #[test]
    fn reproduces_real_save() {
        let Ok(path) = std::env::var("BN3_TEST_SAVE") else {
            return;
        };
        let Ok(save) = std::fs::read(&path) else {
            return;
        };
        let abil = super::compile(&save);
        for i in 0..super::ABILITY_ARRAY_LEN {
            if !super::SCRATCH.contains(&i) {
                assert_eq!(
                    abil[i],
                    save[super::ABILITY_ARRAY_OFFSET + i],
                    "ability slot 0x{i:02x}"
                );
            }
        }
    }
}
