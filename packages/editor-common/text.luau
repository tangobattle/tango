--!strict
local function is_space(cp: number): boolean
    return (cp >= 9 and cp <= 13) or cp == 32 or cp == 0x85 or cp == 0xa0 or cp == 0x1680
        or (cp >= 0x2000 and cp <= 0x200a) or cp == 0x2028 or cp == 0x2029
        or cp == 0x202f or cp == 0x205f or cp == 0x3000
end

local function is_blank(value: string): boolean
    for _, cp in utf8.codes(value) do if not is_space(cp) then return false end end
    return true
end

return {is_space = is_space, is_blank = is_blank}
