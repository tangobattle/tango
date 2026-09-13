--!strict
export type Card = {id: number, enabled: boolean}
export type View = {
    count: () -> number, card: (number) -> Card?,
    set_count: (number) -> boolean, set_card: (number, Card) -> boolean,
    rebuild_anticheat: () -> (),
}
export type Action = {kind: "add", id: number} | {kind: "remove", slot: number}
    | {kind: "move", from: number, to: number} | {kind: "clear"}

local function integer(n: number, limit: number): boolean
    return n >= 0 and n < limit and n == math.floor(n)
end

local function apply(view: View, capacity: number, action: Action): boolean
    local cards: {Card} = {}
    for slot = 1, view.count() do
        local card = view.card(slot)
        if card then table.insert(cards, card) end
    end
    local previous = #cards
    if action.kind == "add" then
        if #cards >= capacity or not integer(action.id, capacity) then return false end
        for _, card in cards do if card.id == action.id then return false end end
        table.insert(cards, {id = action.id, enabled = true})
    elseif action.kind == "remove" then
        if not integer(action.slot - 1, #cards) then return false end
        table.remove(cards, action.slot)
    elseif action.kind == "move" then
        if action.from == action.to or not integer(action.from - 1, #cards) or not integer(action.to - 1, #cards) then return false end
        local moved = table.remove(cards, action.from)
        assert(moved)
        table.insert(cards, action.to, moved)
    else
        table.clear(cards)
    end
    -- Preserve the same trailing bytes as the native editor. Grow before
    -- writing new entries, then shrink the visible list to its final length.
    assert(view.set_count(math.max(previous, #cards)))
    for index, card in cards do assert(view.set_card(index, card)) end
    assert(view.set_count(#cards))
    view.rebuild_anticheat()
    return true
end

return {apply = apply}
