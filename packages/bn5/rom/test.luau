--!strict
local detect = require("./detect")
return function()
    assert(not detect(buffer.create(0)))
    local header = buffer.create(0xc0)
    for _, code in {"BRBE", "BRKE", "BRBJ", "BRKJ"} do
        buffer.writestring(header, 0xac, code)
        buffer.writeu8(header, 0xbc, 0)
        assert(detect(header))
        buffer.writeu8(header, 0xbc, 1)
        assert(not detect(header))
    end
    buffer.writeu8(header, 0xbc, 0)
    buffer.writestring(header, 0xac, "BR5E")
    assert(not detect(header))
end
