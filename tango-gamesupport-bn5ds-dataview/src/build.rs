//! Headless legality for Double Team DS's Party Customizer loadouts.

use crate::save::{Save, MAX_COPIES_PER_PARTY_PROGRAM, NUM_TEAM_SLOTS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartycustViolationKind {
    GaugeExceeded { used: u32, limit: u32 },
    TooManyCopies { used: usize, limit: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartycustViolation {
    pub slot: usize,
    pub at: usize,
    pub program: usize,
    pub kind: PartycustViolationKind,
}

fn loadout_violations(programs: &[(usize, u8)], capacity: u8) -> Vec<(usize, usize, PartycustViolationKind)> {
    let total_cost: u32 = programs.iter().map(|(_, cost)| u32::from(*cost)).sum();
    let mut total_copies = std::collections::HashMap::new();
    for &(program, _) in programs {
        *total_copies.entry(program).or_insert(0usize) += 1;
    }

    let mut running_cost = 0u32;
    let mut seen_copies = std::collections::HashMap::new();
    let mut violations = vec![];
    for (at, &(program, cost)) in programs.iter().enumerate() {
        running_cost += u32::from(cost);
        let seen = seen_copies.entry(program).or_insert(0usize);
        *seen += 1;

        // Attribute an overfilled gauge only to the ordered programs that do
        // not fit, rather than painting the whole otherwise-valid loadout red.
        if cost != 0 && running_cost > u32::from(capacity) {
            violations.push((
                at,
                program,
                PartycustViolationKind::GaugeExceeded {
                    used: total_cost,
                    limit: u32::from(capacity),
                },
            ));
        }
        if *seen > MAX_COPIES_PER_PARTY_PROGRAM {
            violations.push((
                at,
                program,
                PartycustViolationKind::TooManyCopies {
                    used: total_copies[&program],
                    limit: MAX_COPIES_PER_PARTY_PROGRAM,
                },
            ));
        }
    }
    violations
}

pub fn partycust_violations(save: &Save, assets: &crate::rom::Assets) -> Vec<PartycustViolation> {
    let party = save.view_party();
    let mut violations = vec![];
    for slot in 0..NUM_TEAM_SLOTS {
        let programs = party
            .programs(slot, assets)
            .into_iter()
            .map(|program| {
                let cost = assets.party_program(program).map(|program| program.cost()).unwrap_or(0);
                (program, cost)
            })
            .collect::<Vec<_>>();
        violations.extend(
            loadout_violations(&programs, party.capacity(slot, assets))
                .into_iter()
                .map(|(at, program, kind)| PartycustViolation {
                    slot,
                    at,
                    program,
                    kind,
                }),
        );
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_programs_past_the_gauge_are_attributed() {
        assert_eq!(
            loadout_violations(&[(1, 3), (2, 3), (3, 2)], 6),
            vec![(2, 3, PartycustViolationKind::GaugeExceeded { used: 8, limit: 6 },)]
        );
    }

    #[test]
    fn only_the_copy_past_the_advisory_cap_is_attributed() {
        let programs = vec![(4, 0); MAX_COPIES_PER_PARTY_PROGRAM + 1];
        assert_eq!(
            loadout_violations(&programs, 10),
            vec![(
                MAX_COPIES_PER_PARTY_PROGRAM,
                4,
                PartycustViolationKind::TooManyCopies {
                    used: MAX_COPIES_PER_PARTY_PROGRAM + 1,
                    limit: MAX_COPIES_PER_PARTY_PROGRAM,
                },
            )]
        );
    }
}
