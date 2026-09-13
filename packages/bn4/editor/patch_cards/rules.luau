--!strict
export type View = {read patch_count: () -> number, read patch_card: (number) -> {read id: number, read enabled: boolean}?}
export type Assets = {read patch_card: (number) -> {read slot: number}?}
export type Finding = {slot: number, id: number, enabled: boolean, kind: "not_in_catalog" | "wrong_slot", expected_slot: number?}

local function violations(view: View, assets: Assets): {Finding}
    local result: {Finding} = {}
    for slot = 1, view.patch_count() do
        local card = view.patch_card(slot)
        if not card then continue end
        local info = assets.patch_card(card.id)
        if not info then
            table.insert(result, {slot = slot, id = card.id, enabled = card.enabled, kind = "not_in_catalog" :: "not_in_catalog"})
        elseif info.slot ~= slot then
            table.insert(result, {slot = slot, id = card.id, enabled = card.enabled,
                kind = "wrong_slot" :: "wrong_slot", expected_slot = info.slot})
        end
    end
    return result
end
return {violations = violations}
