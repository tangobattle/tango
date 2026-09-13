--!strict
-- BN6 NaviCust program effects, shared by all four color variants.
export type Effect = {kind: string, value: number?}
local programs: {[number]: {Effect}} = {
    [1] = {{kind = "super_armor"}}, -- SuprArmr
    [2] = {{kind = "custom", value = 1}}, -- Custom1
    [3] = {{kind = "custom", value = 2}}, -- Custom2
    [4] = {{kind = "mega_limit", value = 1}}, -- MegFldr1
    [5] = {{kind = "mega_limit", value = 2}}, -- MegFldr2
    [6] = {{kind = "giga_limit", value = 1}}, -- GigFldr1
    [7] = {{kind = "first_barrier"}}, -- FstBarr
    [8] = {{kind = "shield"}}, -- Shield
    [9] = {{kind = "reflect"}}, -- Reflect
    [10] = {{kind = "anti_damage"}}, -- AntiDmg
    [11] = {{kind = "float_shoes"}}, -- FlotShoe
    [12] = {{kind = "air_shoes"}}, -- AirShoes
    [13] = {{kind = "under_shirt"}}, -- UnderSht
    [14] = {{kind = "chip_shuffle"}}, -- ChpShufl
    [15] = {{kind = "number_open"}}, -- NumbrOpn
    [16] = {{kind = "sneak_run"}}, -- SneakRun
    [17] = {{kind = "oil_body"}}, -- OilBody
    [18] = {{kind = "fish"}}, -- Fish
    [19] = {{kind = "battery"}}, -- Battery
    [20] = {{kind = "jungle"}}, -- Jungle
    [21] = {{kind = "collect"}}, -- Collect
    [22] = {{kind = "millions"}}, -- Millions
    [23] = {{kind = "humor"}}, -- Humor
    [24] = {{kind = "poem"}}, -- Poem
    [25] = {{kind = "slip_runner"}}, -- SlipRunr
    [26] = {{kind = "auto_heal"}}, -- AutoHeal
    [27] = {{kind = "attack", value = 3}, {kind = "speed", value = 3}, {kind = "charge", value = 3}}, -- BustPack
    [28] = {{kind = "super_armor"}, {kind = "under_shirt"}, {kind = "float_shoes"}, {kind = "air_shoes"}}, -- BodyPack
    [29] = {{kind = "custom", value = 1}, {kind = "mega_limit", value = 1}}, -- FldrPak1
    [30] = {{kind = "custom", value = 2}, {kind = "mega_limit", value = 2}}, -- FldrPak2
    [31] = {{kind = "bug_stop"}}, -- BugStop
    [32] = {{kind = "rush"}}, -- Rush
    [33] = {{kind = "beat"}}, -- Beat
    [34] = {{kind = "tango"}}, -- Tango
    [35] = {{kind = "attack", value = 1}}, -- Attack+1
    [36] = {{kind = "speed", value = 1}}, -- Speed+1
    [37] = {{kind = "charge", value = 1}}, -- Charge+1
    [38] = {{kind = "attack_max"}}, -- AttckMAX
    [39] = {{kind = "speed_max"}}, -- SpeedMAX
    [40] = {{kind = "charge_max"}}, -- ChargMAX
    [41] = {{kind = "max_hp", value = 50}}, -- HP+50
    [42] = {{kind = "max_hp", value = 100}}, -- HP+100
    [43] = {{kind = "max_hp", value = 200}}, -- HP+200
    [44] = {{kind = "max_hp", value = 300}}, -- HP+300
    [45] = {{kind = "max_hp", value = 400}}, -- HP+400
    [46] = {{kind = "max_hp", value = 500}}, -- HP+500
}
local bugs: {[number]: {string}} = {
    [1] = {"movement"}, [2] = {"emotion_window"}, [3] = {"panel"},
    [4] = {"custom"}, [5] = {"encounter"}, [6] = {"result"},
    [7] = {"buster_blank"}, [8] = {}, [9] = {"battle_hp", "damage_hp"},
}
return {
    for_part = function(id: number): {Effect} return programs[math.floor(id / 4)] or {} end,
    for_group = function(group: number): {string} return bugs[group] or {} end,
}
