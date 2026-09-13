--!strict
local hand = require("./hand")

-- BN5/BN6 unit records and chip hands share this layout. Slot ownership can
-- swap between rounds; hand blocks remain in absolute player order.
local STRIDE = 0xd8

local function poll(state: buffer, context: TelemetryContext, memory: Memory, units_address: number, battle_state: number, chip_blocks: number): TelemetryFrame
    assert(context.player == 1 or context.player == 2, "invalid player")
    local bytes = memory.read("main", units_address, STRIDE * 2)
    local units: {[number]: {hp: number, x: number, y: number}} = {}
    for slot = 1, 2 do
        local start = (slot - 1) * STRIDE
        local owner = buffer.readu8(bytes, start + 0x16) + 1
        if owner > 2 or units[owner] then return {values = {}, events = {}} end
        units[owner] = {hp = buffer.readu16(bytes, start + 0x24), x = buffer.readu8(bytes, start + 0x12), y = buffer.readu8(bytes, start + 0x13)}
    end
    local custom = buffer.readu8(memory.read("main", battle_state + 0x14 + context.player - 1, 1), 0) == 4
    local block = memory.read("main", chip_blocks + (context.player - 1) * 0x50, 14)
    local fired = buffer.readu16(block, 0)
    local id = if fired <= 5 then buffer.readu16(block, 2 + fired * 2) else 0
    local reading: hand.Reading? = if id > 0 and id <= 0x0fff then {id = id, fires = fired} else nil
    return {
        values = {
            p1_hp = units[1].hp, p1_x = units[1].x, p1_y = units[1].y,
            p2_hp = units[2].hp, p2_x = units[2].x, p2_y = units[2].y,
            custom_open = custom,
        },
        events = hand.tick(state, context.round, reading, custom, units[context.player].hp, context.player),
    }
end

return {poll = poll}
