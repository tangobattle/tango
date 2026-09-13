--!strict
local editor = require("@bn5").editor
local test = require("@bn5/editor/test")
return {test = function()
    local size = tango.input_size("fixture")
    assert(size)
    local fixture = buffer.readstring(tango.read_input("fixture", 0, size), 0, size)
    test(editor, fixture)
end}
