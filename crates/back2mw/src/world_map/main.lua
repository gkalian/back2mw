--[[
	back2mw: a map of the carried-over world.

	The Morrowind world map is one for the whole game and ends before the worlds that
	back2mw places east of Vvardenfell begin. This mod reads the map sheet descriptions that
	the converter puts into `worlds\*.json` and, while the player stands in the cells of such a world,
	shows its sheet in the map window instead of the vanilla world map. Outside those cells the window is left alone.

	The sheet can be zoomed with the mouse wheel and dragged while holding the left button; when the window
	opens, it is positioned so that the player is in the middle.

	The file is shared by all back2mw plugins: the converter writes it identically on every build.
	The names of the map window elements (`MenuMap_world` and the rest) are taken from the strings of Morrowind.exe.
]]

-- The descriptions folder relative to `Data Files\MWSE` — this is where `json.loadfile` expects it.
local WORLDS_DIR = "mods\\back2mw\\map\\worlds"
local LOG_PREFIX = "[back2mw map] "
-- Side of a Morrowind cell in units.
local CELL_UNITS = 8192
-- Side of the player marker in pixels.
local MARKER_SIZE = 9
-- Sheet zoom: 1 means the whole sheet fits the window. Where to start, the wheel step and the limit —
-- how many screen pixels per sheet pixel (beyond that it is just blur).
local ZOOM_START = 2
local ZOOM_STEP = 1.25
local MAX_SCREEN_PER_TEXEL = 4

local ids = {
	menu = tes3ui.registerID("MenuMap"),
	world = tes3ui.registerID("MenuMap_world"),
	overlay = tes3ui.registerID("back2mw_map"),
	canvas = tes3ui.registerID("back2mw_map_canvas"),
	sheet = tes3ui.registerID("back2mw_map_sheet"),
	marker = tes3ui.registerID("back2mw_map_marker"),
}

-- Sheet descriptions, by file name.
local worlds = {}
-- The world whose sheet is shown in the window now; nil means the vanilla map.
local shown = nil
-- Vanilla map elements we hid: make exactly these visible again.
local hidden = {}
-- Sheet view: the zoom and the sheet point (fractions of width and height) in the middle of the window.
local view = { zoom = ZOOM_START, centerX = 0.5, centerY = 0.5 }
-- The last known player position: the mouse handlers redraw the sheet without it.
local last = nil
-- Dragging: where the mouse and the middle of the view were at the moment of the press.
local drag = nil
-- The map window tree is written to the log once per launch: it shows what to do if
-- the player's window is built differently (another MWSE version, a UI mod).
local dumped = false

local function log(text, ...)
	mwse.log(LOG_PREFIX .. text, ...)
end

local function isNumber(value)
	return type(value) == "number"
end

-- A description is valid if it has everything needed for display.
local function valid(world)
	if type(world) ~= "table" or world.format ~= 1 then
		return false
	end
	local cells, sheet = world.cells, world.sheet
	if type(world.texture) ~= "string" or type(cells) ~= "table" or type(sheet) ~= "table" then
		return false
	end
	for _, key in ipairs({ "minX", "minY", "maxX", "maxY" }) do
		if not isNumber(cells[key]) then
			return false
		end
	end
	for _, key in ipairs({ "left", "bottom", "right", "top" }) do
		if not isNumber(sheet[key]) then
			return false
		end
	end
	return isNumber(world.width) and isNumber(world.height) and world.width > 0 and world.height > 0
		and sheet.right > sheet.left and sheet.top > sheet.bottom
end

local function loadWorlds()
	worlds = {}
	local dir = "Data Files\\MWSE\\" .. WORLDS_DIR
	if lfs.attributes(dir, "mode") ~= "directory" then
		log("no world descriptions: %s is missing", dir)
		return
	end
	local names = {}
	for file in lfs.dir(dir) do
		local stem = file:match("^(.+)%.[jJ][sS][oO][nN]$")
		if stem then
			table.insert(names, stem)
		end
	end
	table.sort(names)
	for _, stem in ipairs(names) do
		local ok, world = pcall(json.loadfile, WORLDS_DIR .. "\\" .. stem)
		if ok and valid(world) then
			table.insert(worlds, world)
			log("world %s: cells x %s..%s, y %s..%s, sheet %s", tostring(world.world), tostring(world.cells.minX),
				tostring(world.cells.maxX), tostring(world.cells.minY), tostring(world.cells.maxY), world.texture)
		else
			log("skipped %s.json: not a map description of format 1", stem)
		end
	end
end

-- Where the player is on the world map: outside — the player itself, in an interior — the last exterior point.
local function playerPlace()
	local cell = tes3.getPlayerCell()
	if not cell or not tes3.player then
		return nil
	end
	local position
	if cell.isInterior then
		position = tes3.getLastExteriorPosition()
	else
		position = tes3.player.position
	end
	if not position then
		return nil
	end
	return position.x, position.y
end

local function worldAt(x, y)
	local gridX, gridY = math.floor(x / CELL_UNITS), math.floor(y / CELL_UNITS)
	for _, world in ipairs(worlds) do
		local cells = world.cells
		if gridX >= cells.minX and gridX <= cells.maxX and gridY >= cells.minY and gridY <= cells.maxY then
			return world
		end
	end
	return nil
end

local function dump(element, depth, budget)
	if budget.left <= 0 or depth > 12 then
		return
	end
	budget.left = budget.left - 1
	log("%s%s %sx%s visible=%s path=%s", string.rep("  ", depth), tostring(element.name), tostring(element.width),
		tostring(element.height), tostring(element.visible), tostring(element.contentPath))
	for _, child in ipairs(element.children) do
		dump(child, depth + 1, budget)
	end
end

-- Restores the vanilla world map if we hid it.
local function restore(menu)
	for _, element in ipairs(hidden) do
		element.visible = true
	end
	hidden = {}
	local host = menu:findChild(ids.world)
	local overlay = host and host:findChild(ids.overlay)
	if overlay then
		overlay.visible = false
	end
	if shown then
		shown = nil
		menu:updateLayout()
	end
end

local function clamp01(value)
	return math.max(0, math.min(1, value))
end

-- The sheet's place along one axis: a sheet wider than the window clamps the middle of the view so that there is
-- no emptiness beyond the sheet's edge; a narrower one stands in the middle. Returns the alignment (0 — the left
-- edges of the sheet and the window coincide, 1 — the right ones) and the clamped middle.
local function place(center, canvas, room)
	if canvas <= room then
		return 0.5, 0.5
	end
	local half = room / 2 / canvas
	center = math.max(half, math.min(1 - half, center))
	local left = room / 2 - center * canvas
	return clamp01(left / (room - canvas)), center
end

-- Where the player is on the sheet, in fractions of width and height.
local function along(world, x, y)
	local rect = world.sheet
	return (x - rect.left) / (rect.right - rect.left), (rect.top - y) / (rect.top - rect.bottom)
end

-- Sizes and positions of the sheet and the marker for the current view.
local function layout(menu, host, overlay, world, x, y)
	local roomX, roomY = host.width, host.height
	if roomX < 64 or roomY < 64 then
		roomX, roomY = menu.width - 8, menu.height - 33
	end
	overlay.width = roomX
	overlay.height = roomY

	-- Zoom 1 means the whole sheet fits the window, keeping the proportions.
	local fit = math.min(roomX / world.width, roomY / world.height)
	view.zoom = math.max(1, math.min(view.zoom, math.max(1, MAX_SCREEN_PER_TEXEL / fit)))
	local width = math.max(32, math.floor(world.width * fit * view.zoom))
	local height = math.max(32, math.floor(world.height * fit * view.zoom))

	local canvas = overlay:findChild(ids.canvas)
	if not canvas then
		return
	end
	canvas.width = width
	canvas.height = height
	canvas.absolutePosAlignX, view.centerX = place(view.centerX, width, roomX)
	canvas.absolutePosAlignY, view.centerY = place(view.centerY, height, roomY)
	local sheet = canvas:findChild(ids.sheet)
	if sheet then
		sheet.width = width
		sheet.height = height
	end

	local marker = canvas:findChild(ids.marker)
	if marker then
		local alongX, alongY = along(world, x, y)
		if alongX < 0 or alongX > 1 or alongY < 0 or alongY > 1 then
			marker.visible = false
		else
			marker.visible = true
			-- 0 — the marker is pressed to the left (top) edge, 1 — to the right (bottom): the middle
			-- of the marker must land on the player's point.
			marker.absolutePosAlignX = clamp01((alongX * width - MARKER_SIZE / 2) / (width - MARKER_SIZE))
			marker.absolutePosAlignY = clamp01((alongY * height - MARKER_SIZE / 2) / (height - MARKER_SIZE))
		end
	end
	return width, height
end

-- Redraw the sheet after the wheel or dragging.
local function redraw()
	local menu = tes3ui.findMenu(ids.menu)
	local host = menu and menu:findChild(ids.world)
	local overlay = host and host:findChild(ids.overlay)
	if not overlay or not last then
		return
	end
	local width, height = layout(menu, host, overlay, last.world, last.x, last.y)
	menu:updateLayout()
	return width, height
end

local function guarded(action)
	return function(e)
		local ok, message = pcall(action, e)
		if not ok then
			log("mouse handler failed: %s", tostring(message))
		end
	end
end

local function onWheel(factor)
	return guarded(function()
		view.zoom = view.zoom * factor
		redraw()
	end)
end

local onMouseDown = guarded(function(e)
	drag = { x = e.relativeX, y = e.relativeY, centerX = view.centerX, centerY = view.centerY }
	tes3ui.captureMouseDrag(true)
end)

local onMouseHeld = guarded(function(e)
	local canvas = drag and e.source:findChild(ids.canvas)
	if not canvas or canvas.width <= 0 or canvas.height <= 0 then
		return
	end
	-- The sheet follows the mouse: the middle of the view moves the opposite way.
	view.centerX = drag.centerX - (e.relativeX - drag.x) / canvas.width
	view.centerY = drag.centerY - (e.relativeY - drag.y) / canvas.height
	redraw()
end)

local onMouseUp = guarded(function()
	drag = nil
	tes3ui.captureMouseDrag(false)
end)

-- Set the view so that the player is in the middle (at the sheet's edge — as far as the edge allows).
local function centerOnPlayer()
	if last then
		view.centerX, view.centerY = along(last.world, last.x, last.y)
		view.centerX, view.centerY = clamp01(view.centerX), clamp01(view.centerY)
	end
end

local function show(menu, world, x, y, recenter)
	local host = menu:findChild(ids.world)
	if not host then
		if not dumped then
			dumped = true
			log("MenuMap_world not found, the vanilla map is left as is; menu tree:")
			dump(menu, 0, { left = 120 })
		end
		return
	end
	if not dumped then
		dumped = true
		-- The path from the window to the world map and everything under it: it shows what the sheet is fitted into.
		local chain, element = {}, host
		while element do
			table.insert(chain, 1, string.format("%s %sx%s", tostring(element.name), tostring(element.width),
				tostring(element.height)))
			element = element.parent
		end
		log("path to the world map: %s", table.concat(chain, " > "))
		log("world map subtree before the first sheet:")
		dump(host, 0, { left = 60 })
	end

	local overlay = host:findChild(ids.overlay)
	if overlay and shown ~= world then
		-- Another world — another sheet: simpler to recreate.
		overlay:destroy()
		overlay = nil
	end
	local created = false
	if not overlay then
		created = true
		overlay = host:createBlock({ id = ids.overlay })
		overlay.absolutePosAlignX = 0.5
		overlay.absolutePosAlignY = 0.5
		overlay:register("mouseScrollUp", onWheel(ZOOM_STEP))
		overlay:register("mouseScrollDown", onWheel(1 / ZOOM_STEP))
		overlay:register("mouseDown", onMouseDown)
		overlay:register("mouseStillPressed", onMouseHeld)
		overlay:register("mouseRelease", onMouseUp)
		-- The mouse over the sheet and the marker must reach the block with the handlers.
		local canvas = overlay:createBlock({ id = ids.canvas })
		canvas.consumeMouseEvents = false
		local sheet = canvas:createImage({ id = ids.sheet, path = world.texture })
		sheet.scaleMode = true
		sheet.consumeMouseEvents = false
		local marker = canvas:createRect({ id = ids.marker, color = { 0, 0, 0 } })
		marker.width = MARKER_SIZE
		marker.height = MARKER_SIZE
		marker.paddingAllSides = 2
		marker.consumeMouseEvents = false
		local dot = marker:createRect({ color = { 0.9, 0.1, 0.1 } })
		dot.width = MARKER_SIZE - 4
		dot.height = MARKER_SIZE - 4
		dot.consumeMouseEvents = false
		view.zoom = ZOOM_START
	end
	overlay.visible = true

	-- The vanilla world map is hidden while the sheet is shown.
	for _, child in ipairs(host.children) do
		if child.name ~= "back2mw_map" and child.visible then
			child.visible = false
			table.insert(hidden, child)
		end
	end

	last = { world = world, x = x, y = y }
	if created or recenter or shown ~= world then
		centerOnPlayer()
	end
	layout(menu, host, overlay, world, x, y)

	shown = world
	menu:updateLayout()
end

local function update(recenter)
	local menu = tes3ui.findMenu(ids.menu)
	if not menu then
		return
	end
	local x, y = playerPlace()
	local world = x and worldAt(x, y)
	if world then
		show(menu, world, x, y, recenter)
	else
		last = nil
		restore(menu)
	end
end

local function safeUpdate()
	local ok, message = pcall(update, false)
	if not ok then
		log("update failed: %s", tostring(message))
	end
end

-- The player opened the menu: the sheet is centered on the player (the zoom stays).
local function onMenuEnter()
	drag = nil
	local ok, message = pcall(update, true)
	if not ok then
		log("update failed: %s", tostring(message))
	end
end

-- Once a second: only while the map window is on screen, the events catch the rest.
local function onTick()
	local menu = tes3ui.findMenu(ids.menu)
	if menu and menu.visible then
		safeUpdate()
	end
end

local function onMapActivated()
	-- The window was recreated: the old elements are gone with it.
	shown = nil
	hidden = {}
	drag = nil
	safeUpdate()
end

local function onLoaded()
	shown = nil
	hidden = {}
	safeUpdate()
	if #worlds > 0 then
		-- The map window can be pinned and visible while moving: the marker has to move even without a cell change.
		timer.start({ type = timer.simulate, duration = 1, iterations = -1, callback = onTick })
	end
end

local function onInitialized()
	loadWorlds()
	if #worlds == 0 then
		return
	end
	event.register("uiActivated", onMapActivated, { filter = "MenuMap" })
	event.register("loaded", onLoaded)
	event.register("cellChanged", safeUpdate)
	event.register("menuEnter", onMenuEnter)
	log("ready, worlds: %d", #worlds)
end

event.register("initialized", onInitialized)
