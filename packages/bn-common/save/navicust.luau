--!strict
local types = require("../navicust/types")
local materialize = require("../navicust/materialize")
export type Placement = types.Placement
export type Assets = {read navicust_part: (number) -> materialize.Part?}
export type Layout = {
    count: number, num_parts: number, parts: number,
    grid: number, grid_bytes: number, colors: number,
    board_size: number, bitmap_size: number,
}
export type Model = {
    has_navicust: () -> boolean,
    navicust_part: (number) -> Placement?, set_navicust_part: (number, Placement?) -> boolean,
    materialized: () -> {number}?, color_bar: () -> {number}?,
    clear_materialized: () -> boolean, rebuild_materialized: (Assets) -> boolean,
}
local function integer(value: number, limit: number): boolean
    return value >= 0 and value < limit and value == math.floor(value)
end

local function open(bytes: buffer, layout: Layout, available: (() -> boolean)?): Model
    local enabled = available or function(): boolean return true end
    local function part(slot: number): Placement?
        if not enabled() or not integer(slot - 1, layout.count) then return nil end
        local offset = layout.parts + (slot - 1) * 8
        local id = buffer.readu8(bytes, offset)
        if id == 0 then return nil end
        return {id = id, col = buffer.readu8(bytes, offset + 2), row = buffer.readu8(bytes, offset + 3),
            rot = buffer.readu8(bytes, offset + 4), compressed = buffer.readu8(bytes, offset + 5) ~= 0}
    end
    local function clear(): boolean
        if not enabled() then return false end
        buffer.fill(bytes, layout.grid, 0, layout.grid_bytes)
        buffer.fill(bytes, layout.colors, 0, 6)
        return true
    end
    return {
        has_navicust = enabled,
        navicust_part = part,
        set_navicust_part = function(slot: number, value: Placement?): boolean
            if not enabled() or not integer(slot - 1, layout.count) then return false end
            if value and (not integer(value.id, layout.num_parts) or not integer(value.col, 256)
                or not integer(value.row, 256) or not integer(value.rot, 256)) then return false end
            local offset = layout.parts + (slot - 1) * 8
            buffer.fill(bytes, offset, 0, 8)
            if value then
                buffer.writeu8(bytes, offset, value.id)
                buffer.writeu8(bytes, offset + 2, value.col)
                buffer.writeu8(bytes, offset + 3, value.row)
                buffer.writeu8(bytes, offset + 4, value.rot)
                buffer.writeu8(bytes, offset + 5, if value.compressed then 1 else 0)
            end
            return true
        end,
        materialized = function(): {number}?
            if not enabled() then return nil end
            local grid: {number} = {}
            for slot = 1, layout.board_size * layout.board_size do grid[slot] = buffer.readu8(bytes, layout.grid + slot - 1) end
            return grid
        end,
        color_bar = function(): {number}?
            if not enabled() then return nil end
            local colors: {number} = {}
            for slot = 1, 6 do
                local color = buffer.readu8(bytes, layout.colors + slot - 1)
                colors[slot] = if color <= 6 then color else 0
            end
            return colors
        end,
        clear_materialized = clear,
        rebuild_materialized = function(assets: Assets): boolean
            if not enabled() then return false end
            local grid, colors = materialize(layout.count, part, assets.navicust_part, layout.board_size, layout.bitmap_size)
            clear()
            for slot, value in grid do buffer.writeu8(bytes, layout.grid + slot - 1, value) end
            for slot = 1, math.min(#colors, 6) do buffer.writeu8(bytes, layout.colors + slot - 1, colors[slot]) end
            return true
        end,
    }
end
return {open = open}
