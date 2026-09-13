--!strict
export type Slot = {id: number?}
export type Run = {id: number?, count: number}
export type Section = {name: string, runs: {Run}}
export type Assets = {read num_chips: number, read chip_class: (number) -> string?}
export type View = {read use_count: (number, boolean) -> number}
type Ranked = {id: number, uses: number}

local sections = {
    {name = "secondary_standard", class = "standard", secondary = true, counts = {1, 1, 1}},
    {name = "standard", class = "standard", secondary = false, counts = {4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1}},
    {name = "mega", class = "mega", secondary = false, counts = {1, 1, 1, 1, 1}},
    {name = "giga", class = "giga", secondary = false, counts = {1}},
    {name = "combos", class = "", secondary = false, counts = {1, 1, 1, 1, 1, 1, 1, 1}},
    {name = "program_advance", class = "program_advance", secondary = false, counts = {1}},
}

local function group(view: View, assets: Assets): {Section}
    local result: {Section} = {}
    for _, section in sections do
        local ranked: {Ranked} = {}
        if section.class ~= "" then
            for id = 0, assets.num_chips - 1 do
                local uses = view.use_count(id, section.secondary)
                if uses > 0 and assets.chip_class(id) == section.class then
                    table.insert(ranked, {id = id, uses = uses})
                end
            end
        end
        table.sort(ranked, function(a: Ranked, b: Ranked): boolean
            return if a.uses == b.uses then a.id < b.id else a.uses > b.uses
        end)
        local runs: {Run} = {}
        for index, count in section.counts do
            local chip = ranked[index]
            table.insert(runs, {id = if chip then chip.id else nil, count = count})
        end
        table.insert(result, {name = section.name, runs = runs})
    end
    return result
end

local function materialize(view: View, assets: Assets): {Slot}
    local slots: {Slot} = {}
    for _, section in group(view, assets) do
        for _, run in section.runs do
            for _ = 1, run.count do table.insert(slots, {id = run.id}) end
        end
    end
    return slots
end

return {group = group, materialize = materialize}
