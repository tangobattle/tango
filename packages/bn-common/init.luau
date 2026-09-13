--!strict
local t = tango.load_catalog("./locales")
-- Shared Battle Network editor wording, preserved from the native editor.
-- Consumers import this package's translator, without knowing its file paths.
return {
    t = t,
    test = function()
        require("./gamemode/legacy_replay_test")()
        require("./auto_battle_data/test")(t)
        require("./rom/message_test")()
        require("./folder/rules_test").test(t)
        require("./folder/edit_test").test()
        require("./folder/state_test").test()
        require("./folder/display_test").test()
        require("./folder/clipboard_test").test()
        require("./patch_cards/test").test(t)
        require("./patch_cards/clipboard_test").test()
        require("./navicust/test").test()
        require("./navicust/clipboard_test").test()
        require("./navi/test").test()
        require("./navi/active_effects_test")()
        require("./telemetry/hand_test").test()
        assert(t("folder-edit-folder") ~= "⟦folder-edit-folder⟧")
        assert(t("navicust-edit-count", {count = 2}) ~= "⟦navicust-edit-count⟧")
    end,
}
