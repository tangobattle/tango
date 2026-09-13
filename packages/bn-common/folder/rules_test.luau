--!strict
local rules = require("./rules")

local function test(t: Translate)
    local catalog: {[number]: rules.ChipInfo} = {
        [1] = {name = "Cannon", codes = {0, 26}, class = "standard", mb = 10},
        [2] = {name = "Mega", codes = {1}, class = "mega", mb = 40},
        [3] = {name = "Giga", codes = {2}, class = "giga", mb = 50},
        [4] = {name = "Dark", codes = {0}, class = "mega", dark = true, mb = 20},
        [5] = {name = "Navi", codes = {0}, class = "navi", mb = 30},
    }
    local assets: rules.Assets = {chip = function(id: number): rules.ChipInfo? return catalog[id] end,
        chip_is_legal = function(id: number): boolean return id >= 1 and id <= 5 end}
    local chips: {[number]: rules.Chip} = {
        [1] = {id = 1, code = 0}, [2] = {id = 1, code = 26}, [3] = {id = 2, code = 0},
        [4] = {id = 3, code = 2}, [5] = {id = 4, code = 0}, [6] = {id = 5, code = 0}, [7] = {id = 999, code = 0},
    }
    local view: rules.View = {size = 8, chip = function(slot: number): rules.Chip? return chips[slot] end}
    local limits: rules.Limits = {mega_limit = 0, giga_limit = 0, dark_limit = 0, navi_limit = 0,
        reg_memory = 30, tag_memory = 50, max_copies = function(_chip: rules.ChipInfo): number return 1 end}
    local usage = rules.scan(view, assets)
    assert(usage.mega == 1 and usage.giga == 1 and usage.dark == 1 and usage.navi == 1)
    assert(usage.copies[1] == 2 and usage.copies[999] == 1)
    local selections: rules.Selections = {regular = 3, tags = {2, 4}}
    local expected = {
        {"too_many_copies_of_chip"}, {"too_many_copies_of_chip", "tag_chips_exceed_memory"},
        {"too_many_mega_chips", "chip_code_unavailable", "regular_chip_exceeds_memory"},
        {"too_many_giga_chips", "tag_chips_exceed_memory"}, {"too_many_dark_chips"},
        {"too_many_navi_chips"}, {"chip_illegal_for_game"}, {},
    }
    for slot = 1, 8 do
        local issues = rules.slot_issues(view, assets, usage, slot, limits, selections)
        assert(#issues == #expected[slot])
        for index, issue in issues do assert(issue.kind == expected[slot][index]) end
    end
    local findings = rules.violations(view, assets, limits, selections)
    assert(findings[1].kind == "folder_not_full")
    local first = findings[1]
    if first.kind == "folder_not_full" then assert(first.used == 7 and first.required == 8) end
    local warnings = rules.warnings(findings, assets, t)
    assert(#warnings == 10)
    assert(warnings[2] == t("build-violation", {subject = "Cannon A, Cannon *",
        reason = t("build-violation-too-many-copies-of-chip", {used = 2, limit = 1})}))
    assert(warnings[3] == t("build-violation", {subject = "Cannon *, Giga C",
        reason = t("build-violation-tag-chips-exceed-memory", {used = 60, limit = 50})}))
    local additional = rules.chip_issues(usage, assets, 1, limits, 1)
    assert(#additional == 1 and (additional[1] :: rules.LimitIssue).used == 3)
    -- Defaults do not impose class or memory caps. A missing record still
    -- participates in copy counts and the legal-ID check, but has no metadata.
    local unrestricted: rules.Limits = {max_copies = function(_chip: rules.ChipInfo): number return math.huge end}
    assert(#rules.chip_issues(usage, assets, 2, unrestricted, 100) == 0)
    assert(#rules.slot_issues(view, assets, usage, 3, unrestricted, selections) == 1)
    -- Exceeding a limit marks every contributing copy, independent of order.
    chips[1], chips[6] = chips[6], chips[1]
    local reordered = rules.scan(view, assets)
    for _, slot in {2, 6} do
        assert(rules.slot_issues(view, assets, reordered, slot, limits, {})[1].kind == "too_many_copies_of_chip")
    end
end

return {test = test}
