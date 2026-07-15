//! A minimal hand-assembled GBA ROM that exercises SIO MULTI mode, for
//! loopback tests: no real game automates cleanly into link mode, so this
//! stands in for one. The same image runs on every unit of a 2-4 player
//! link (role and slot read from SIOCNT's slave and multi-ID bits).
//!
//! Like a real game's link menu, the ROM tolerates the cable being plugged
//! or unplugged at any point: nothing trusts a state that only held at
//! boot. Every iteration re-asserts SIOCNT (refreshing the ready, slave,
//! and multi-ID bits from whatever is on the cable now), no unit owns a
//! counter — the next expected word is always `last observed SIOMULTI0 +
//! 1` — and every wait is bounded, falling back to the re-assert rather
//! than spinning on a partner that may be gone.
//!
//! Protocol per exchange: each unit preloads SIOMLT_SEND with
//! `expected | id << 14` (the multi ID in the top two bits keeps the
//! payloads distinguishable per player), the master burns a settle delay
//! so every slave's preload always lands first, then starts the transfer
//! and polls the busy bit; each slave watches for SIOMULTI0 to change to
//! a fresh non-0xFFFF word. Every unit then appends all four SIOMULTI
//! registers to a log at 0x02000000 and adopts the master's word as its
//! new `last`. From a common reset the logged sequence is exactly
//! `payload(slot, k)` per entry `k`; after a mid-run plug-in the units
//! converge on the master's numbering from the second exchange on.
//! Unattached slots read back 0xFFFF, exactly like a real multi cable
//! with nothing plugged in, and a unit alone on the cable records nothing
//! (an unplugged mgba GBA reads back the slave bit, so it parks in the
//! bounded slave wait). The EWRAM logs double as a desync canary: every
//! core in the link must record the identical sequence.

const ENTRY_WORD: u32 = 0xC0 / 4;

struct Asm {
    words: Vec<u32>,
}

impl Asm {
    fn here(&self) -> usize {
        self.words.len()
    }

    fn emit(&mut self, word: u32) -> usize {
        self.words.push(word);
        self.words.len() - 1
    }

    /// `b`/`bcc` to a known (backward) target.
    fn branch(&mut self, cond: u32, target: usize) {
        let at = self.here();
        self.emit(Self::branch_word(cond, at, target));
    }

    /// Emit a placeholder branch to patch once the target is known.
    fn branch_fixup(&mut self, cond: u32) -> usize {
        self.emit(cond << 28 | 0x0A00_0000)
    }

    fn patch_branch(&mut self, at: usize, target: usize) {
        let cond = self.words[at] >> 28;
        self.words[at] = Self::branch_word(cond, at, target);
    }

    fn branch_word(cond: u32, at: usize, target: usize) -> u32 {
        let offset = (target as i64 - at as i64 - 2) as u32 & 0x00FF_FFFF;
        cond << 28 | 0x0A00_0000 | offset
    }
}

const AL: u32 = 0xE;
const EQ: u32 = 0x0;
const NE: u32 = 0x1;

/// Data-processing immediate: `imm8` rotated right by `2 * rot`.
fn dp_imm(cond: u32, opcode: u32, s: u32, rn: u32, rd: u32, rot: u32, imm8: u32) -> u32 {
    cond << 28 | 1 << 25 | opcode << 21 | s << 20 | rn << 16 | rd << 12 | rot << 8 | imm8
}

fn mov_imm(rd: u32, rot: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b1101, 0, 0, rd, rot, imm8)
}

fn orr_imm(cond: u32, rd: u32, rn: u32, rot: u32, imm8: u32) -> u32 {
    dp_imm(cond, 0b1100, 0, rn, rd, rot, imm8)
}

fn and_imm(rd: u32, rn: u32, rot: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b0000, 0, rn, rd, rot, imm8)
}

fn add_imm(rd: u32, rn: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b0100, 0, rn, rd, 0, imm8)
}

fn subs_imm(rd: u32, rn: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b0010, 1, rn, rd, 0, imm8)
}

fn tst_imm(rn: u32, rot: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b1000, 1, rn, 0, rot, imm8)
}

/// `orr rd, rn, rm, LSL #shift` — register operand with an immediate shift.
fn orr_reg_lsl(rd: u32, rn: u32, rm: u32, shift: u32) -> u32 {
    AL << 28 | 0b1100 << 21 | rn << 16 | rd << 12 | shift << 7 | rm
}

fn cmp_reg(rn: u32, rm: u32) -> u32 {
    AL << 28 | 0b1010 << 21 | 1 << 20 | rn << 16 | rm
}

/// Halfword load/store, immediate offset (P=1 U=1 W=0), offset <= 0xFF.
fn hword(l: u32, rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset <= 0xFF);
    AL << 28 | 0b000_11100 << 20 | l << 20 | rn << 16 | rd << 12 | (offset & 0xF0) << 4 | 0xB0 | (offset & 0xF)
}

fn strh(rd: u32, rn: u32, offset: u32) -> u32 {
    hword(0, rd, rn, offset)
}

fn ldrh(rd: u32, rn: u32, offset: u32) -> u32 {
    hword(1, rd, rn, offset)
}

// r0 = 0x04000120 (SIO register block); halfword offsets from it:
const SIOMULTI0: u32 = 0x00;
const SIOCNT: u32 = 0x08;
const SIOMLT_SEND: u32 = 0x0A;
const RCNT: u32 = 0x14;

/// The EWRAM address every unit logs its per-iteration
/// (SIOMULTI0..SIOMULTI3) quads to.
pub const LOG_ADDR: u32 = 0x0200_0000;

/// Halfwords one log entry spans (all four SIOMULTI registers).
pub const LOG_ENTRY_HALFWORDS: usize = 4;

/// The value an unattached SIOMULTI slot reads back after a transfer.
pub const UNATTACHED: u16 = 0xFFFF;

/// The payload player `id` sends on iteration `c` (0-based): the counter
/// tagged with the multi ID in the top two bits.
pub fn payload(id: usize, c: usize) -> u16 {
    ((c + 1) as u16 & 0x3FFF) | ((id as u16) << 14)
}

pub fn build() -> Vec<u8> {
    let mut asm = Asm { words: Vec::new() };

    // Header: entry branch + zeroed remainder (mgba only warns about the
    // missing logo/checksum when not booting through the real BIOS).
    asm.emit(Asm::branch_word(AL, 0, ENTRY_WORD as usize));
    while asm.here() < ENTRY_WORD as usize {
        asm.emit(0);
    }

    asm.emit(mov_imm(0, 4, 0x04)); // r0 = 0x04000000
    asm.emit(orr_imm(AL, 0, 0, 14, 0x12)); // r0 |= 0x120
    asm.emit(mov_imm(1, 0, 0));
    asm.emit(strh(1, 0, RCNT)); // RCNT = 0
    asm.emit(mov_imm(3, 0, 0xFF));
    asm.emit(orr_imm(AL, 3, 3, 12, 0xFF)); // r3 = 0xFFFF (an undriven line)
    asm.emit(mov_imm(5, 4, 0x02)); // r5 = 0x02000000 (log)
    asm.emit(mov_imm(9, 0, 0)); // r9 = last observed master word

    // Re-assert point: refresh SIOCNT from whatever is on the cable now
    // and preload this unit's send. Every wait below falls back here.
    let outer = asm.here();
    asm.emit(add_imm(8, 9, 1)); // r8 = last + 1 — the next master word
    asm.emit(mov_imm(1, 10, 0x02)); // r1 = 0x2000 (MULTI)
    asm.emit(orr_imm(AL, 1, 1, 0, 3)); // baud 115200
    asm.emit(strh(1, 0, SIOCNT)); // the rewrite refreshes ready/slave/id
    asm.emit(ldrh(1, 0, SIOCNT));
    asm.emit(tst_imm(1, 0, 8)); // all units ready?
    asm.branch(EQ, outer);
    asm.emit(and_imm(2, 1, 0, 0x30)); // r2 = multi ID bits (SIOCNT 4-5)
    asm.emit(orr_reg_lsl(7, 8, 2, 10)); // r7 = expected | id << 14 — this unit's payload
    asm.emit(strh(7, 0, SIOMLT_SEND));
    asm.emit(tst_imm(1, 0, 4)); // slave?
    let to_slave = asm.branch_fixup(NE);

    // Master: give the slaves a settle window, then clock the transfer and
    // poll the busy bit — bounded, since an unplug can strand it set.
    asm.emit(mov_imm(6, 12, 0x08)); // r6 = 0x800
    let mdelay = asm.here();
    asm.emit(subs_imm(6, 6, 1));
    asm.branch(NE, mdelay);
    asm.emit(ldrh(1, 0, SIOCNT));
    asm.emit(orr_imm(AL, 1, 1, 0, 0x80)); // start
    asm.emit(strh(1, 0, SIOCNT));
    asm.emit(mov_imm(6, 12, 0x80)); // r6 = 0x8000
    let mbusy = asm.here();
    asm.emit(ldrh(1, 0, SIOCNT));
    asm.emit(tst_imm(1, 0, 0x80));
    let to_record_master = asm.branch_fixup(EQ);
    asm.emit(subs_imm(6, 6, 1));
    asm.branch(NE, mbusy);
    asm.branch(AL, outer);

    // Slave: watch for a fresh master word — bounded, since with no master
    // on the cable none ever comes.
    let slave = asm.here();
    asm.emit(mov_imm(6, 12, 0x40)); // r6 = 0x4000
    let swait = asm.here();
    asm.emit(ldrh(2, 0, SIOMULTI0));
    asm.emit(cmp_reg(2, 9)); // unchanged since the last exchange?
    let to_snext = asm.branch_fixup(EQ);
    asm.emit(cmp_reg(2, 3)); // 0xFFFF: no master drove the line
    let to_record_slave = asm.branch_fixup(NE);
    let snext = asm.here();
    asm.patch_branch(to_snext, snext);
    asm.emit(subs_imm(6, 6, 1));
    asm.branch(NE, swait);
    asm.branch(AL, outer);

    // Record all four SIOMULTI slots (unattached ones read 0xFFFF), then
    // adopt the master's word as the new `last`.
    let record = asm.here();
    for slot in 0..LOG_ENTRY_HALFWORDS as u32 {
        asm.emit(ldrh(2, 0, SIOMULTI0 + slot * 2));
        asm.emit(strh(2, 5, slot * 2));
    }
    asm.emit(add_imm(5, 5, 2 * LOG_ENTRY_HALFWORDS as u32));
    asm.emit(ldrh(9, 0, SIOMULTI0));
    asm.branch(AL, outer);

    asm.patch_branch(to_slave, slave);
    asm.patch_branch(to_record_master, record);
    asm.patch_branch(to_record_slave, record);

    let mut rom = Vec::with_capacity(1024);
    for w in &asm.words {
        rom.extend_from_slice(&w.to_le_bytes());
    }
    // Pad out to a size no ROM sniffer will balk at.
    rom.resize(1024, 0);
    rom
}
