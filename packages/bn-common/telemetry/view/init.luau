--!strict
local t = tango.load_catalog("./locales", require("../../init").t)
local WIDTH, HEIGHT, RESOLUTION = 336, 84, 168
local COLORS: {Color} = {"danger", "primary"}
local METRICS: {[string]: boolean} = {hp = true, x = true, y = true}

local function selected(state: ViewState): string
    local metric = state.metric or "hp"
    return if METRICS[metric] then metric else "hp"
end

local function view(history: TelemetryHistory, context: TelemetryViewContext, state: ViewState): Node
    local metric = selected(state)
    local series: {{TelemetryBucket}} = {}
    local maximum, minimum = 1, 0
    for side = 1, 2 do
        local unit = if side == 1 then context.player else 3 - context.player
        local buckets = history.series({player = context.player, name = "p" .. tostring(unit) .. "_" .. metric,
            from_tick = context.from_tick, to_tick = context.to_tick}, RESOLUTION)
        for _, bucket in buckets do
            if bucket.max then maximum = math.max(maximum, bucket.max) end
            if bucket.min then minimum = math.min(minimum, bucket.min) end
        end
        series[side] = buckets
    end
    local commands: {Draw} = {{kind = "rect", width = WIDTH, height = HEIGHT, fill = "plate", radius = 4}}
    local function y(value: number): number
        return HEIGHT - 4 - (value - minimum) / (maximum - minimum) * (HEIGHT - 8)
    end
    local has_values = false
    for side, buckets in series do
        local previous: number? = nil
        for i, bucket in buckets do
            local left, right = (i - 1) / #buckets * WIDTH, i / #buckets * WIDTH
            local first, last, low, high = bucket.first, bucket.last, bucket.min, bucket.max
            if low and high then
                has_values = true
                -- An envelope retains damage/recovery spikes narrower than a pixel.
                table.insert(commands, {kind = "path", points = {{left, y(low)}, {left, y(high)}}, stroke = COLORS[side], stroke_width = 1.5} :: Draw)
            end
            if first and previous and not bucket.gap then
                table.insert(commands, {kind = "path", points = {{left, y(previous)}, {left, y(first)}}, stroke = COLORS[side], stroke_width = 1.5} :: Draw)
            end
            if first and last and not bucket.gap then
                table.insert(commands, {kind = "path", points = {{left, y(first)}, {right, y(last)}}, stroke = COLORS[side], stroke_width = 1.5} :: Draw)
            end
            previous = if bucket.gap then nil else last
        end
    end
    local pointer_x = tonumber(state.pointer_x)
    local tick = context.to_tick
    if pointer_x then
        pointer_x = math.clamp(pointer_x, 0, WIDTH)
        tick = math.floor(context.from_tick + pointer_x / WIDTH * (context.to_tick - context.from_tick))
        table.insert(commands, {kind = "path", points = {{pointer_x, 0}, {pointer_x, HEIGHT}}, stroke = "muted", stroke_width = 1} :: Draw)
    end
    local captions: {Node} = {}
    for side = 1, 2 do
        local player = if side == 1 then context.player else 3 - context.player
        local count = history.count_events({player = player, name = "chip_used", from_tick = context.from_tick, to_tick = context.to_tick})
        local value = history.value({player = context.player, name = "p" .. tostring(player) .. "_" .. metric, tick = tick})
        local label = if typeof(value) == "number" then tostring(value) else "—"
        table.insert(captions, {kind = "column", width = "fill", spacing = 2, children = {
            {kind = "text", color = COLORS[side], size = "caption", text = t(if side == 1 then "telemetry-you" else "telemetry-opponent") .. " · " .. label},
            {kind = "text", tone = "muted", size = "caption", text = t("telemetry-chips", {count = count})},
        }} :: Node)
    end
    local children: {Node} = {
        {kind = "select", id = "telemetry-metric", value = metric, size = "caption", choices = {
            {value = "hp", label = t("telemetry-hp")}, {value = "x", label = t("telemetry-x")}, {value = "y", label = t("telemetry-y")},
        }},
        if has_values then {kind = "canvas", id = "telemetry-chart", pointer = true, pointer_capture = false, rasterize = {}, canvas_width = WIDTH, canvas_height = HEIGHT, commands = commands, width = "fill", height = HEIGHT}
            else {kind = "text", text = t("telemetry-empty"), tone = "muted"},
        {kind = "text", text = tostring(minimum) .. "–" .. tostring(maximum) .. " · " .. t("telemetry-time", {seconds = math.floor((tick - context.from_tick) / context.ticks_per_second)}), size = "caption", tone = "muted"},
        {kind = "row", spacing = 16, children = captions},
    }
    if context.incomplete then table.insert(children, {kind = "text", text = t("telemetry-incomplete"), tone = "muted", size = "caption"} :: Node) end
    return {kind = "column", spacing = 6, width = "fill", children = children}
end

local function update_view(state: ViewState, action: Action): ViewState
    if action.id == "telemetry-metric" and action.kind == "change" and METRICS[action.value] then state.metric = action.value end
    if action.id == "telemetry-chart" and action.kind == "pointer" then
        if action.phase == "leave" then state.pointer_x = nil
        elseif action.phase == "move" then state.pointer_x = tostring(action.x) end
    end
    return state
end
return {view = view, update_view = update_view}
