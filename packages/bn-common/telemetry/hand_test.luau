--!strict
local hand = require("./hand")

local function test()
    local state = hand.initial_state()
    local function tick(round: number, id: number?, fires: number, custom: boolean?, hp: number?): {TelemetryEvent}
        return hand.tick(state, round, if id then {id = id, fires = fires} else nil, custom == true, hp or 100, 2)
    end
    assert(#tick(1, 10, 0) == 0)
    assert(#tick(1, 10, 0) == 0)
    local events = tick(1, 10, 1) -- duplicate picks are distinct uses
    assert(#events == 1 and events[1].name == "chip_used")
    assert(events[1].fields.player == 2 and events[1].fields.chip == 10)
    events = tick(1, 20, 2)
    assert(#events == 1 and events[1].fields.chip == 10)
    assert(#tick(1, 30, 0) == 0) -- a new hand
    assert(#tick(1, 40, 1, true) == 0) -- rewritten while picking
    assert(#tick(1, 40, 1) == 0)
    events = tick(1, nil, 0)
    assert(#events == 1 and events[1].fields.chip == 40)
    assert(#tick(1, nil, 0) == 0)
    assert(#tick(1, 50, 0) == 0)
    assert(#tick(1, nil, 0, false, 0) == 0) -- KO teardown
    assert(#tick(1, 60, 0) == 0)
    assert(#tick(2, nil, 0) == 0) -- new round clears the previous reading
    assert(#tick(2, 70, 0) == 0)
    local rewind = buffer.tostring(state)
    events = tick(2, 80, 1)
    state = buffer.fromstring(rewind)
    local replayed = tick(2, 80, 1)
    assert(#events == 1 and #replayed == 1 and events[1].fields.chip == replayed[1].fields.chip)
end

return {test = test}
