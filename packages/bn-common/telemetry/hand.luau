--!strict
-- Detect uses from the hand's next chip and fired counter. State is a buffer
-- owned by the host's rollback timeline, never module-local mutable state.
export type Reading = {id: number, fires: number}
local SIZE = 8

local function initial_state(): buffer
    return buffer.create(SIZE)
end

local function tick(state: buffer, round: number, reading: Reading?, custom_open: boolean, hp: number, player: number): {TelemetryEvent}
    assert(buffer.len(state) == SIZE, "invalid hand tracker state")
    if buffer.readu32(state, 0) ~= round then
        buffer.writeu32(state, 0, round)
        buffer.writeu16(state, 4, 0)
        buffer.writeu16(state, 6, 0)
    end
    local previous_id, previous_fires = buffer.readu16(state, 4), buffer.readu16(state, 6)
    local id, fires = if reading then reading.id else 0, if reading then reading.fires else 0
    if previous_id == id and (id == 0 or previous_fires == fires) then return {} end
    local events: {TelemetryEvent} = {}
    -- A new hand/counter reset is not a use. Clearing the last chip is a use
    -- only while alive; the game's KO teardown also clears that reading.
    if not custom_open and previous_id ~= 0 and ((reading and fires > previous_fires) or (not reading and hp ~= 0)) then
        table.insert(events, {name = "chip_used", fields = {player = player, chip = previous_id}})
    end
    buffer.writeu16(state, 4, id)
    buffer.writeu16(state, 6, fires)
    return events
end

return {initial_state = initial_state, tick = tick}
