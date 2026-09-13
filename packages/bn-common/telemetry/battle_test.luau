--!strict
local hand = require("./hand")

local function test(telemetry: Telemetry, units_address: number, battle_state: number, chip_blocks: number)
    local units_offset, battle_offset, chips_offset = units_address - 0x02000000, battle_state - 0x02000000, chip_blocks - 0x02000000
    local ram = buffer.create(0x40000)
    local reads, bytes_read = 0, 0
    local memory: Memory = {read = function(space: string, address: number, length: number): buffer
        assert(space == "main")
        reads += 1
        bytes_read += length
        local result = buffer.create(length)
        buffer.copy(result, 0, ram, address - 0x02000000, length)
        return result
    end}
    local function unit(slot: number, owner: number, hp: number, x: number, y: number)
        local start = units_offset + (slot - 1) * 0xd8
        buffer.writeu8(ram, start + 0x16, owner - 1)
        buffer.writeu16(ram, start + 0x24, hp)
        buffer.writeu8(ram, start + 0x12, x)
        buffer.writeu8(ram, start + 0x13, y)
    end
    local state = hand.initial_state()
    local context: TelemetryContext = {tick = 1, round = 1, player = 1}
    -- The units are uninitialized in menus. No state or edge may advance.
    local before = buffer.tostring(state)
    assert(next(telemetry.poll(state, context, memory).values) == nil)
    assert(buffer.tostring(state) == before)
    unit(1, 2, 950, 5, 2)
    unit(2, 1, 1000, 2, 1) -- owner order differs from slot order
    buffer.writeu16(ram, chips_offset + 2, 100)
    buffer.writeu16(ram, chips_offset + 4, 100)
    buffer.writeu8(ram, battle_offset + 0x14, 4)
    reads, bytes_read = 0, 0
    local frame = telemetry.poll(state, context, memory)
    assert(frame.values.p1_hp == 1000 and frame.values.p1_x == 2 and frame.values.p1_y == 1)
    assert(frame.values.p2_hp == 950 and frame.values.p2_x == 5 and frame.values.p2_y == 2)
    assert(frame.values.custom_open == true and #frame.events == 0)
    assert(reads == 3 and bytes_read == 447)
    buffer.writeu8(ram, battle_offset + 0x14, 0)
    assert(#telemetry.poll(state, context, memory).events == 0)
    local rewind = buffer.tostring(state)
    buffer.writeu16(ram, chips_offset, 1)
    frame = telemetry.poll(state, context, memory)
    assert(#frame.events == 1 and frame.events[1].fields.chip == 100 and frame.events[1].fields.player == 1)
    assert(#telemetry.poll(state, context, memory).events == 0)
    state = buffer.fromstring(rewind)
    assert(#telemetry.poll(state, context, memory).events == 1)
    -- Invalid unit owners must not consume the previous loaded chip.
    before = buffer.tostring(state)
    unit(1, 1, 950, 5, 2)
    assert(next(telemetry.poll(state, context, memory).values) == nil)
    assert(buffer.tostring(state) == before)
    unit(1, 2, 950, 5, 2)
    -- A spent hand emits the last chip once, then stays silent.
    buffer.writeu16(ram, chips_offset, 6)
    assert(#telemetry.poll(state, context, memory).events == 1)
    assert(#telemetry.poll(state, context, memory).events == 0)
    -- Player 2 uses its own custom flag and chip block, even on swapped slots.
    context.player = 2
    state = hand.initial_state()
    buffer.writeu16(ram, chips_offset + 0x52, 200)
    buffer.writeu16(ram, chips_offset + 0x54, 201)
    buffer.writeu8(ram, battle_offset + 0x15, 4)
    frame = telemetry.poll(state, context, memory)
    assert(frame.values.custom_open == true and #frame.events == 0)
    buffer.writeu8(ram, battle_offset + 0x15, 0)
    buffer.writeu16(ram, chips_offset + 0x50, 1)
    frame = telemetry.poll(state, context, memory)
    assert(#frame.events == 1 and frame.events[1].fields.player == 2 and frame.events[1].fields.chip == 200)
    -- The KO teardown must not count the cleared hand as a use.
    unit(1, 2, 0, 5, 2)
    buffer.writeu16(ram, chips_offset + 0x50, 6)
    assert(#telemetry.poll(state, context, memory).events == 0)
    -- New-round state clears history before considering the first reading.
    unit(1, 2, 950, 5, 2)
    buffer.writeu16(ram, chips_offset + 0x50, 0)
    assert(#telemetry.poll(state, context, memory).events == 0)
    context.round = 2
    buffer.writeu16(ram, chips_offset + 0x50, 1)
    assert(#telemetry.poll(state, context, memory).events == 0)

    -- The shared chart and its catalogs must resolve through either game.
    local view, update_view = telemetry.view, telemetry.update_view
    assert(view and update_view)
    local history: TelemetryHistory = {
        value = function(_query): TelemetryValue? return 100 end,
        series = function(_query, _resolution): {TelemetryBucket}
            return {{from_tick = 0, to_tick = 10, first = 100, last = 100, min = 100, max = 100, gap = false}}
        end,
        count_events = function(_query): number return 2 end,
    }
    local view_state = update_view({}, {kind = "change", id = "telemetry-metric", value = "x"})
    assert(view_state.metric == "x")
    local node = view(history, {player = 1, from_tick = 0, to_tick = 10, ticks_per_second = 60, incomplete = false}, view_state)
    assert(node.kind == "column")
    local metric = node.children[1]
    assert(metric.kind == "select" and metric.value == "x")
    assert(metric.choices[1].label ~= "⟦telemetry-hp⟧")
end

return {test = test}
