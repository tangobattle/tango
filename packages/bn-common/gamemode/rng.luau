--!strict
-- Distinct nonzero game RNG streams from the negotiated seed. Player and
-- stream are 1-based; buffer byte addresses retain their binary meaning.
return function(seed: {number}, player: number, stream: number): number
    assert(#seed == 16 and (player == 1 or player == 2) and (stream == 1 or stream == 2))
    local lane = (player - 1) * 2 + stream
    local offset = (lane - 1) * 4
    local word = seed[offset + 1] + seed[offset + 2] * 256
        + seed[offset + 3] * 65536 + seed[offset + 4] * 16777216
    local value = bit32.bxor(word, (0x9e3779b9 * lane) % 0x100000000)
    return if value == 0 then 1 else value
end
