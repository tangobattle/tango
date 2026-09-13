--!strict
local common = require("@bn-common/navi/ui")
local emblem = require("@bn-common/navi/emblem")
local model = require("../save/model")
local rom = require("../rom")
local stats = require("./stats")
local t = require("../i18n")
local motion = require("@editor-common/navigation_motion")

local function entry(assets: rom.Assets, id: number): common.Entry
    local navi = assets.navi(id)
    if not navi then return {id = id} end
    local image, accent = emblem.prepare(navi.emblem)
    return {id = id, name = navi.name, emblem = image, accent = accent}
end

local function identity(value: model.Model, assets: rom.Assets, editable: boolean?, actions: Node?): Node
    local derived = stats.derive(value, assets)
    local display: common.Identity = {hp = derived.max_hp,
        buster = {attack = derived.attack, speed = derived.speed, charge = derived.charge},
        navi = if assets.navi(value.navi()) then entry(assets, value.navi()) else nil}
    return common.identity(display, if editable == false then nil else "navi.picker", actions or {kind = "space"}, t)
end

local function picker(value: model.Model, assets: rom.Assets): Node
    local roster: {{common.Entry}} = {}
    for _, ids in assets.definition.navi_order do
        local row: {common.Entry} = {}
        for _, id in ids do table.insert(row, entry(assets, id)) end
        table.insert(roster, row)
    end
    return common.picker(roster, value.navi(), t)
end

local function update(value: model.Model, assets: rom.Assets, state: ViewState, action: Action): boolean
    if action.kind ~= "activate" then return false end
    if action.id == "navi.picker" then
        state["navi.picker"] = if state["navi.picker"] == "open" then nil else "open"
        motion.picker(state)
        state["navicust.hover.col"], state["navicust.hover.row"] = nil, nil
        return true
    end
    local id = tonumber(string.match(action.id, "^navi%.select:(%d+)$") or "")
    if id == nil or state["navi.picker"] ~= "open" then return false end
    for _, row in assets.definition.navi_order do
        if table.find(row, id) then
            assert(value.set_navi(id))
            state["navi.picker"] = nil
            motion.picker(state)
            state["navicust.hover.col"], state["navicust.hover.row"] = nil, nil
            return true
        end
    end
    return false
end

return {identity = identity, picker = picker, update = update}
