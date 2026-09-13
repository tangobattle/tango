--!strict
-- Shared helpers have no knowledge of any game's save format.
local buffers = require("./buffers")
local fields = require("./fields")
local text = require("./text")

return {
    copy = buffers.copy,
    integer = fields.integer,
    test = function()
        assert(text.is_blank("\t \r\n\u{85}\u{a0}\u{3000}"))
        assert(not text.is_blank("\u{3000}ア\u{3000}"))
        assert(not text.is_blank("\u{200b}"))
        local original = buffer.fromstring("abc")
        local copied = buffers.copy(original)
        buffer.writeu8(copied, 0, 42)
        assert(buffer.tostring(original) == "abc")
        assert(buffer.readu8(copied, 0) == 42)
        local input = fields.integer("hp", "HP", 100)
        assert(input.kind == "input" and input.value == "100")
    end,
}
