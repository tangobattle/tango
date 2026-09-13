--!strict
local supported = {BRBE = true, BRKE = true, BRBJ = true, BRKJ = true} :: {[string]: boolean}

return function(rom: buffer): boolean
    return buffer.len(rom) >= 0xc0 and buffer.readu8(rom, 0xbc) == 0
        and supported[buffer.readstring(rom, 0xac, 4)] == true
end
