--!strict
local hand = require("@bn-common/telemetry/hand")
local detect_rom = require("../rom/detect")

-- Shared by revision 0 of BR5E, BR6E, BR5J and BR6J. Unit slots can swap
-- owners between rounds; chip blocks are always in absolute player order.
local UNITS = 0x0203a9b0
local BATTLE_STATE, CHIP_BLOCKS = 0x02034880, 0x020349c0

local function initial_state(): buffer
    local size = tango.input_size("rom")
    assert(size and size >= 0xc0 and detect_rom(tango.read_input("rom", 0, 0xc0)), "unsupported telemetry ROM")
    return hand.initial_state()
end

local function poll(state: buffer, context: TelemetryContext, memory: Memory): TelemetryFrame
    return require("@bn-common/telemetry/battle").poll(state, context, memory, UNITS, BATTLE_STATE, CHIP_BLOCKS)
end

local telemetry: Telemetry = {
    detect_rom = detect_rom, initial_state = initial_state, poll = poll,
    -- Presentation is imported only when requested, never on the per-tick path.
    view = function(history: TelemetryHistory, context: TelemetryViewContext, state: ViewState): Node
        return require("@bn-common/telemetry/view").view(history, context, state)
    end,
    update_view = function(state: ViewState, action: Action): ViewState
        return require("@bn-common/telemetry/view").update_view(state, action)
    end,
}
return telemetry
