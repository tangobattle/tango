--!strict
local function integer(id: string, label: string, value: number): Node
    return {kind = "input", id = id, text = label, value = tostring(value), width = "fill" :: Size}
end
return {integer = integer}
