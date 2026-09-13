--!strict
local save = require("../init")
local rom = require("../../rom")
local t = require("../../i18n")

local function template_file(): string
    local size = tango.input_size("rom")
    if not size then return tango.fail(t, "rom-required") end
    local definition = rom.definition(tango.read_input("rom", 0, math.min(size, 0xc0)))
    return (if definition.variant == "Gregar" then "g" else "f") .. (if definition.region == "JP" then "_jp.raw" else "_us.raw")
end

local function list(): {SaveTemplate}
    template_file()
    return {{name = "default", label = t("new-save-megaman")}}
end

local function create(name: string): buffer
    assert(name == "default", "unknown save template")
    local raw = tango.read_file("editor/save/templates/" .. template_file())
    local bytes = buffer.create(65536)
    buffer.copy(bytes, save.START, raw, 0, save.SIZE)
    return save.encode(bytes)
end

return {list = list, create = create}
