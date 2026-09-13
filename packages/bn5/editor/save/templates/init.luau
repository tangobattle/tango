--!strict
local save = require("../init")
local rom = require("../../rom")
local t = require("../../i18n")
local function suffix(): string
    local size = tango.input_size("rom")
    if not size then return tango.fail(t, "rom-required") end
    local definition = rom.definition(tango.read_input("rom", 0, math.min(size, 0xc0)))
    return (if definition.variant == "Protoman" then "protoman" else "colonel") ..
        (if definition.region == "JP" then "_jp.raw" else "_us.raw")
end
local function list(): {SaveTemplate}
    suffix()
    return {{name = "dark", label = t("save-dark")}, {name = "light", label = t("save-light")}}
end
local function create(name: string): buffer
    assert(name == "light" or name == "dark", "unknown save template")
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, tango.read_file("editor/save/templates/" .. name .. "_" .. suffix()), 0, save.SIZE)
    return save.encode(bytes)
end
return {list = list, create = create}
