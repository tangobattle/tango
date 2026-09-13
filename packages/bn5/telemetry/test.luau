--!strict
local memory = require("../gamemode/memory")
return {test = function()
    require("@bn-common/telemetry/battle_test").test(require("./init"), memory.unit, memory.battle_state, memory.chip_blocks)
end}
