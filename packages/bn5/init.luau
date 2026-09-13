--!strict
return {
    editor = require("./editor"),
    t = tango.load_catalog("./locales", require("@bn-common").t),
    test = function()
        if tango.input_size("reference.rom") then require("./editor/rom/test").compare(); return end
        require("./editor/rom/test").test()
        require("./rom/test")()
        require("./gamemode/test")()
        require("./gamemode/replay_test")()
        require("./telemetry/test").test()
        require("./editor/save/test")()
        require("./editor/save/model_test")()
    end,
}
