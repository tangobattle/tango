--!strict
local detect = require("./detect")
local offsets = require("../editor/rom/offsets")

return function()
    assert(not detect(buffer.create(0)))
    assert(not detect(buffer.create(0xbf)))
    local rom = buffer.create(0xc0)
    assert(not detect(rom))
    for code in offsets.definitions do
        buffer.writestring(rom, 0xac, code)
        assert(detect(rom))
        buffer.writeu8(rom, 0xbc, 1)
        assert(not detect(rom))
        buffer.writeu8(rom, 0xbc, 0)
    end
end
