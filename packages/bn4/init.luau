--!strict
return {
    test = function()
        if tango.input_size("reference.rom") then require("./editor/rom/compare")(); return end
        require("./editor/rom/test")()
        require("./editor/save/test")()
        require("./editor/save/folders_test")()
        require("./editor/save/model_test")()
        require("./editor/navi/test")()
        require("./editor/patch_cards/test")()
    end,
}
