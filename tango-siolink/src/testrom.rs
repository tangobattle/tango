//! A minimal hand-assembled GBA ROM that exercises SIO MULTI mode, for
//! loopback tests: no real game automates cleanly into link mode, so this
//! stands in for one.
//!
//! Protocol per iteration `c` (both sides run the same image; role read
//! from SIOCNT's slave bit): each side preloads SIOMLT_SEND with `c+1`
//! (slave sets bit 15 so the two payloads are distinguishable), the master
//! burns a settle delay so the slave's preload always lands first, then
//! starts the transfer and spins on the busy bit; the slave spins until
//! SIOMULTI0 shows the master's `c+1`. Both sides then append
//! (SIOMULTI0, SIOMULTI1) to a log at 0x02000000 and loop. The EWRAM logs
//! double as a desync canary: every core in the link must record the
//! identical sequence (1, 1|0x8000, 2, 2|0x8000, ...).

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

fn add_imm(rd: u32, rn: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b0100, 0, rn, rd, 0, imm8)
}

fn subs_imm(rd: u32, rn: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b0010, 1, rn, rd, 0, imm8)
}

fn tst_imm(rn: u32, rot: u32, imm8: u32) -> u32 {
    dp_imm(AL, 0b1000, 1, rn, 0, rot, imm8)
}

fn mov_reg(rd: u32, rm: u32) -> u32 {
    AL << 28 | 0b1101 << 21 | rd << 12 | rm
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
const SIOMULTI1: u32 = 0x02;
const SIOCNT: u32 = 0x08;
const SIOMLT_SEND: u32 = 0x0A;
const RCNT: u32 = 0x14;

/// The EWRAM address both sides log (SIOMULTI0, SIOMULTI1) pairs to.
pub const LOG_ADDR: u32 = 0x0200_0000;

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
    asm.emit(mov_imm(1, 10, 0x02)); // r1 = 0x2000 (MULTI)
    asm.emit(orr_imm(AL, 1, 1, 0, 3)); // baud 115200
    asm.emit(strh(1, 0, SIOCNT));
    asm.emit(mov_imm(4, 0, 0)); // r4 = counter
    asm.emit(mov_imm(5, 4, 0x02)); // r5 = 0x02000000 (log)

    let ready = asm.here();
    asm.emit(ldrh(1, 0, SIOCNT));
    asm.emit(tst_imm(1, 0, 8)); // all units ready?
    asm.branch(EQ, ready);

    let main_loop = asm.here();
    asm.emit(add_imm(8, 4, 1)); // r8 = c + 1 (master payload / expectation)
    asm.emit(mov_reg(7, 8));
    asm.emit(ldrh(1, 0, SIOCNT));
    asm.emit(tst_imm(1, 0, 4)); // slave?
    asm.emit(orr_imm(NE, 7, 7, 12, 0x80)); // slave payload = c+1 | 0x8000
    asm.emit(strh(7, 0, SIOMLT_SEND));
    asm.emit(tst_imm(1, 0, 4));
    let to_slave = asm.branch_fixup(NE);

    // Master: give the slave a settle window, then clock the transfer.
    asm.emit(mov_imm(6, 12, 0x08)); // r6 = 0x800
    let mdelay = asm.here();
    asm.emit(subs_imm(6, 6, 1));
    asm.branch(NE, mdelay);
    asm.emit(ldrh(1, 0, SIOCNT));
    asm.emit(orr_imm(AL, 1, 1, 0, 0x80)); // start
    asm.emit(strh(1, 0, SIOCNT));
    let mbusy = asm.here();
    asm.emit(ldrh(1, 0, SIOCNT));
    asm.emit(tst_imm(1, 0, 0x80));
    asm.branch(NE, mbusy);
    let to_record = asm.branch_fixup(AL);

    // Slave: wait for the master's word for this iteration to land.
    let slave = asm.here();
    asm.emit(ldrh(2, 0, SIOMULTI0));
    asm.emit(cmp_reg(2, 8));
    asm.branch(NE, slave);

    let record = asm.here();
    asm.emit(ldrh(2, 0, SIOMULTI0));
    asm.emit(ldrh(3, 0, SIOMULTI1));
    asm.emit(strh(2, 5, 0));
    asm.emit(add_imm(5, 5, 2));
    asm.emit(strh(3, 5, 0));
    asm.emit(add_imm(5, 5, 2));
    asm.emit(add_imm(4, 4, 1));
    asm.branch(AL, main_loop);

    asm.patch_branch(to_slave, slave);
    asm.patch_branch(to_record, record);

    let mut rom = Vec::with_capacity(1024);
    for w in &asm.words {
        rom.extend_from_slice(&w.to_le_bytes());
    }
    // Pad out to a size no ROM sniffer will balk at.
    rom.resize(1024, 0);
    rom
}
