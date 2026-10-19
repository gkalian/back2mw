--[[
	back2mw: карта перенесённого мира.

	Карта мира Morrowind одна на всю игру и кончается раньше, чем начинаются миры, которые
	back2mw ставит восточнее Вварденфелла. Этот мод читает описания листов карты, которые
	конвертер кладёт в `worlds\*.json`, и, пока игрок стоит в ячейках такого мира, показывает
	в окне карты его лист вместо ванильной карты мира. Вне этих ячеек окно не трогается.

	Лист можно приближать колесом мыши и двигать, зажав левую кнопку; при открытии окна он
	встаёт так, чтобы игрок был посередине.

	Файл общий для всех плагинов back2mw: конвертер пишет его одинаковым при каждой сборке.
	Имена элементов окна карты (`MenuMap_world` и остальные) взяты из строк Morrowind.exe.
]]

-- Папка описаний относительно `Data Files\MWSE` — так её ждёт `json.loadfile`.
local WORLDS_DIR = "mods\\back2mw\\map\\worlds"
local LOG_PREFIX = "[back2mw map] "
-- Сторона ячейки Morrowind в юнитах.
local CELL_UNITS = 8192
-- Сторона метки игрока в пикселях.
local MARKER_SIZE = 9
-- Увеличение листа: 1 — лист вписан в окно целиком. С какого начинать, шаг колеса и предел —
-- сколько пикселей экрана на пиксель листа (дальше одна муть).
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

-- Описания листов, по имени файла.
local worlds = {}
-- Мир, чей лист сейчас показан в окне; nil — ванильная карта.
local shown = nil
-- Элементы ванильной карты, которые мы спрятали: вернуть видимыми ровно их.
local hidden = {}
-- Вид листа: увеличение и точка листа (доли ширины и высоты) в середине окна.
local view = { zoom = ZOOM_START, centerX = 0.5, centerY = 0.5 }
-- Последнее известное место игрока: обработчики мыши перерисовывают лист без него.
local last = nil
-- Перетаскивание: где была мышь и середина вида в момент нажатия.
local drag = nil
-- Дерево окна карты пишется в журнал один раз за запуск: по нему видно, что делать, если
-- у игрока окно устроено иначе (другая версия MWSE, мод интерфейса).
local dumped = false

local function log(text, ...)
	mwse.log(LOG_PREFIX .. text, ...)
end

local function isNumber(value)
	return type(value) == "number"
end

-- Описание годно, если в нём есть всё, что нужно для показа.
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

-- Где игрок на карте мира: снаружи — он сам, в интерьере — последняя наружная точка.
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

-- Возвращает ванильную карту мира, если мы её прятали.
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

-- Место листа вдоль одной оси: лист шире окна — середина вида зажимается так, чтобы за краем
-- листа не было пустоты; уже — лист стоит посередине. Возвращает выравнивание (0 — левые края
-- листа и окна совпадают, 1 — правые) и зажатую середину.
local function place(center, canvas, room)
	if canvas <= room then
		return 0.5, 0.5
	end
	local half = room / 2 / canvas
	center = math.max(half, math.min(1 - half, center))
	local left = room / 2 - center * canvas
	return clamp01(left / (room - canvas)), center
end

-- Где игрок на листе, в долях ширины и высоты.
local function along(world, x, y)
	local rect = world.sheet
	return (x - rect.left) / (rect.right - rect.left), (rect.top - y) / (rect.top - rect.bottom)
end

-- Размеры и положение листа и метки по текущему виду.
local function layout(menu, host, overlay, world, x, y)
	local roomX, roomY = host.width, host.height
	if roomX < 64 or roomY < 64 then
		roomX, roomY = menu.width - 8, menu.height - 33
	end
	overlay.width = roomX
	overlay.height = roomY

	-- Увеличение 1 — лист вписан в окно целиком, с сохранением пропорций.
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
			-- 0 — метка прижата к левому (верхнему) краю, 1 — к правому (нижнему): середина
			-- метки должна попасть в точку игрока.
			marker.absolutePosAlignX = clamp01((alongX * width - MARKER_SIZE / 2) / (width - MARKER_SIZE))
			marker.absolutePosAlignY = clamp01((alongY * height - MARKER_SIZE / 2) / (height - MARKER_SIZE))
		end
	end
	return width, height
end

-- Перерисовать лист после колеса или перетаскивания.
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
	-- Лист едет за мышью: середина вида — в обратную сторону.
	view.centerX = drag.centerX - (e.relativeX - drag.x) / canvas.width
	view.centerY = drag.centerY - (e.relativeY - drag.y) / canvas.height
	redraw()
end)

local onMouseUp = guarded(function()
	drag = nil
	tes3ui.captureMouseDrag(false)
end)

-- Поставить вид так, чтобы игрок был посередине (у края листа — насколько пускает край).
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
		-- Путь от окна до карты мира и всё, что под ней: по ним видно, во что вписан лист.
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
		-- Другой мир — другой лист: проще пересоздать.
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
		-- Мышь над листом и меткой должна доходить до блока с обработчиками.
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

	-- Ванильная карта мира прячется, пока показан лист.
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

-- Игрок открыл меню: лист встаёт игроком в середину (увеличение остаётся).
local function onMenuEnter()
	drag = nil
	local ok, message = pcall(update, true)
	if not ok then
		log("update failed: %s", tostring(message))
	end
end

-- Раз в секунду: только пока окно карты на экране, остальное ловят события.
local function onTick()
	local menu = tes3ui.findMenu(ids.menu)
	if menu and menu.visible then
		safeUpdate()
	end
end

local function onMapActivated()
	-- Окно создано заново: прежние элементы вместе с ним пропали.
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
		-- Окно карты бывает закреплено и видно на ходу: метку надо двигать и без смены ячейки.
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
