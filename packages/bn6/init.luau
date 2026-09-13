--!strict
local editor = require("./editor")
return {
    editor = editor,
    test = function()
        require("./rom/detect_test")()
        require("./gamemode/test")()
        require("./gamemode/replay_test")()
        require("./telemetry/test").test()
        assert(editor.test)
        editor.test()
    end,
    t = require("./editor/i18n"),
}
