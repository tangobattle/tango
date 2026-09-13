--!strict
local function copy(bytes: buffer): buffer
    local output = buffer.create(buffer.len(bytes))
    buffer.copy(output, 0, bytes)
    return output
end
return {copy = copy}
