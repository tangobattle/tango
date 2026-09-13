--!strict
local gba = require("@gba")
local message = require("./message")
return function()
    local source = gba.mapper(buffer.fromstring("\xff\x00\x00\xe6"), nil).region(0x08000000)
    assert(source)
    -- BN5 has e-Reader names. BN6 ignores the unknown opcode and treats the
    -- following bytes as text; stock NaviCust descriptions use this padding.
    assert(message.parse(source, 0, 4, {" "}, nil) == "  ")
    assert(message.parse(source, 0, 4, {" "}, nil, true) == "")
    assert(message.parse(source, 0, 4, {" "}, function(kind: string, index: number): string?
        assert(kind == "ereader_name" and index == 0)
        return "name"
    end, true) == "name")
    assert(message.parse(source, 0, 2, {" "}, nil, true) == nil)
end
