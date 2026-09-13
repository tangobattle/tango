--!strict
local hand = require("@bn-common/telemetry/hand")
local detect_rom = require("../rom/detect")

local addresses = require("../gamemode/memory")
local UNITS, BATTLE_STATE, CHIP_BLOCKS = addresses.unit, addresses.battle_state, addresses.chip_blocks

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
