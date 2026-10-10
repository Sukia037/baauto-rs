#Requires AutoHotkey v2.0
#SingleInstance Force
#Warn All, Off

; 在建立 GUI 或讀取螢幕資訊前，強制使用每螢幕 DPI 實體像素座標。
try DllCall("SetProcessDpiAwarenessContext", "ptr", -4, "int")
CoordMode "Mouse", "Screen"
CoordMode "ToolTip", "Screen"

; BAauto：滑鼠拖曳至點位
; 點位資料與功能按鍵資料各自輸出 JSON；舊版 keys / x,y,width,height 仍可匯入。
SetWorkingDir A_ScriptDir
SetTitleMatchMode 2

global GAME_EXE := "BlueArchive.exe"
global mainGui, pointList, actionList, statusText, monitorText, currentPointJsonText
global pointNameEdit, pointHotkeyEdit, pointXEdit, pointYEdit, pointFunctionDrop
global candidateList, presetGui := 0, presetKeyList := 0, presetKeyEditor := 0
global presetStatusText := 0, selectedPresetRow := 0, presetSaveButton := 0
global points := [], bindings := [], candidates := [], nextPointId := 1, nextBindingId := 1
global currentPointJson := ""
global presetDefs := [], presetByRole := Map()
global registeredPreset := Map(), registeredPoint := Map(), runtimeActions := Map()
global mouseButtonOwner := ""
global dragLastCompleted := ""
global recordingMode := "", selectedBindingId := 0
global recordingOverlay := 0, recordingOverlayText := 0
global coordinateMonitor := 0

InitPresets()
coordinateMonitor := DetectTargetMonitor()
BuildMainGui()
LoadDefaultFunctionSettings()
RefreshAll()
OnExit(StopAllMacros)

; -------------------- 主介面 --------------------
BuildMainGui() {
    global mainGui, pointList, actionList, statusText, monitorText, currentPointJsonText
    global pointNameEdit, pointHotkeyEdit, pointXEdit, pointYEdit, pointFunctionDrop
    global candidateList

    mainGui := Gui("+Resize +MinSize930x690", "BAauto")
    mainGui.MarginX := 10
    mainGui.MarginY := 10
    mainGui.SetFont("s9", "Microsoft JhengHei UI")
    mainGui.OnEvent("Close", (*) => ExitApp())
    mainGui.OnEvent("Escape", (*) => ExitApp())

    mainGui.Add("GroupBox", "x10 y10 w340 h88", "座標錄製")
    monitorText := mainGui.Add("Text", "x22 y33 w315", "目前解析度：偵測中…")
    recordButton := mainGui.Add("Button", "x22 y55 w301 h24", "錄製點位")
    recordButton.OnEvent("Click", (*) => StartRecording())

    mainGui.Add("GroupBox", "x10 y108 w340 h449", "候選點位")
    candidateList := mainGui.Add("ListView", "x20 y131 w320 h356 -Multi", ["編號", "名稱", "X", "Y"])
    candidateList.ModifyCol(1, 42), candidateList.ModifyCol(2, 130), candidateList.ModifyCol(3, 65), candidateList.ModifyCol(4, 65)
    deleteCandidateButton := mainGui.Add("Button", "x20 y510 w75 h24", "刪除選取")
    clearCandidateButton := mainGui.Add("Button", "x105 y510 w75 h24", "刪除清單")
    sendCandidate := mainGui.Add("Button", "x190 y510 w150 h24", "帶入點位按鍵設定")
    deleteCandidateButton.OnEvent("Click", (*) => DeleteCandidate())
    clearCandidateButton.OnEvent("Click", (*) => ClearCandidateList())
    sendCandidate.OnEvent("Click", (*) => CandidateToBinding())

    mainGui.Add("GroupBox", "x365 y10 w545 h330", "點位按鍵清單")
    actionList := mainGui.Add("ListView", "x375 y34 w525 h210 -Multi", ["名稱", "熱鍵", "X 座標", "Y 座標", "功能", "狀態"])
    for col, width in [130, 68, 72, 72, 112, 120]
        actionList.ModifyCol(col, width)
    actionList.OnEvent("ItemSelect", BindingSelected)
    mainGui.Add("Text", "x377 y251 w515 cGray", "僅顯示點位按鍵；衝突項目會置頂並標示已停用。")
    deletePoint := mainGui.Add("Button", "x375 y276 w105 h24", "刪除點位按鍵")
    clearPoint := mainGui.Add("Button", "x490 y276 w105 h24", "清空點位按鍵")
    addPoint := mainGui.Add("Button", "x605 y276 w295 h24", "新增空白設定")
    deletePoint.OnEvent("Click", (*) => DeleteBinding())
    clearPoint.OnEvent("Click", (*) => ClearBindings())
    addPoint.OnEvent("Click", (*) => AddBlankBinding())

    mainGui.Add("GroupBox", "x365 y350 w545 h207", "點位按鍵設定")
    mainGui.Add("Text", "x378 y378 w43", "名稱：")
    pointNameEdit := mainGui.Add("Edit", "x424 y374 w180")
    mainGui.Add("Text", "x625 y378 w43", "熱鍵：")
    pointHotkeyEdit := mainGui.Add("Hotkey", "x670 y374 w220")
    mainGui.Add("Text", "x378 y414 w68", "X 座標：")
    pointXEdit := mainGui.Add("Edit", "x450 y410 w145")
    mainGui.Add("Text", "x625 y414 w68", "Y 座標：")
    pointYEdit := mainGui.Add("Edit", "x697 y410 w193")
    mainGui.Add("Text", "x378 y450 w68", "功能：")
    pointFunctionDrop := mainGui.Add("DropDownList", "x450 y446 w440 Choose1", ["普通點位", "滑鼠拖曳至點位"])
    mainGui.Add("Text", "x378 y478 w512 cGray", "按住時在游標原位按下，拖到點位後放開；各階段間隔 20ms。")
    resetForm := mainGui.Add("Button", "x378 y515 w135 h28", "清除目前設定欄位")
    savePoint := mainGui.Add("Button", "x523 y515 w367 h28", "儲存點位按鍵設定")
    resetForm.OnEvent("Click", (*) => NewBindingForm())
    savePoint.OnEvent("Click", (*) => SaveBinding())
    mainGui.Add("GroupBox", "x10 y570 w900 h80", "JSON 檔管理")
    mainGui.Add("Text", "x22 y594 w270", "點位 JSON")
    loadPointJsonButton := mainGui.Add("Button", "x22 y615 w145 h24", "匯入點位 JSON")
    savePointJsonButton := mainGui.Add("Button", "x177 y615 w115 h24", "儲存點位 JSON")
    loadPointJsonButton.OnEvent("Click", (*) => ImportPoints())
    savePointJsonButton.OnEvent("Click", (*) => SavePoints())
    mainGui.Add("Text", "x310 y594 w320", "功能按鍵設定（啟動時自動載入）")
    presetButton := mainGui.Add("Button", "x310 y615 w270 h24", "設定預設按鍵")
    presetButton.OnEvent("Click", ShowPresetDialog)

    currentPointJsonText := mainGui.Add("Text", "x600 y594 w300 cGray", "目前點位 JSON：未載入")
    statusText := mainGui.Add("Text", "x600 y617 w300 cGray", "準備完成")

    mainGui.Show("w920 h690")
}

; -------------------- 預設功能與設定窗 --------------------
InitPresets() {
    global presetDefs, presetByRole
    presetDefs := [
        {role:"global_stop", name:"全域緊急停止", hotkey:"^F9", kind:"stop"},
        {role:"record", name:"錄製", hotkey:"^F10", kind:"record"},

        {role:"skill_1", name:"技能 1", hotkey:"w", kind:"send", send:"1"}, {role:"skill_2", name:"技能 2", hotkey:"e", kind:"send", send:"2"},
        {role:"skill_3", name:"技能 3", hotkey:"r", kind:"send", send:"3"}, {role:"skill_4", name:"技能 4", hotkey:"t", kind:"send", send:"4"},
        {role:"skill_5", name:"技能 5", hotkey:"y", kind:"send", send:"5"},
        {role:"spam_1", name:"技能連點 1", hotkey:"2", kind:"spam", send:"1"}, {role:"spam_2", name:"技能連點 2", hotkey:"3", kind:"spam", send:"2"},
        {role:"spam_3", name:"技能連點 3", hotkey:"4", kind:"spam", send:"3"}, {role:"spam_4", name:"技能連點 4", hotkey:"5", kind:"spam", send:"4"},
        {role:"spam_5", name:"技能連點 5", hotkey:"6", kind:"spam", send:"5"},
        {role:"pause", name:"普通暫停", hotkey:"Space", kind:"send", send:"Esc"},
        {role:"frame_fast", name:"卡偵暫停（16ms）", hotkey:"Down", kind:"double_esc", wait:16},
        {role:"frame_slow", name:"卡偵暫停（50ms）", hotkey:"Up", kind:"double_esc", wait:50},
        {role:"auto", name:"AUTO", hotkey:"XButton1", kind:"send", send:"a"}, {role:"speed", name:"加速", hotkey:"XButton2", kind:"send", send:"s"},
        {role:"click_spam", name:"滑鼠連點", hotkey:"RButton", kind:"click_spam"},
        {role:"card_1", name:"卡偵開牌：第 1 張", hotkey:"a", kind:"card", send:"1"}, {role:"card_2", name:"卡偵開牌：第 2 張", hotkey:"s", kind:"card", send:"2"},
        {role:"card_3", name:"卡偵開牌：第 3 張", hotkey:"d", kind:"card", send:"3"}, {role:"card_4", name:"卡偵開牌：第 4 張", hotkey:"f", kind:"card", send:"4"},
        {role:"card_5", name:"卡偵開牌：第 5 張", hotkey:"g", kind:"card", send:"5"}
    ]
    for def in presetDefs
        presetByRole[def.role] := def
}

GetFunctionConfigPath() {
    return A_ScriptDir "\config\function_keys.json"
}

LoadDefaultFunctionSettings() {
    global presetDefs
    path := GetFunctionConfigPath()
    if !FileExist(path)
        return
    Try data := JsonParse(FileRead(path, "UTF-8"))
    Catch
        return
    if !(data is Map) || !data.Has("hotkeys") || !(data["hotkeys"] is Map)
        return
    hotkeys := data["hotkeys"]
    for def in presetDefs
        if hotkeys.Has(def.role)
            def.hotkey := hotkeys[def.role]
}
ShowPresetDialog(*) {
    global presetGui, mainGui, presetKeyList, presetKeyEditor, presetStatusText, presetSaveButton
    if IsObject(presetGui) {
        RefreshPresetList(), presetGui.Show()
        return
    }
    presetGui := Gui("+Owner" mainGui.Hwnd, "預設按鍵設定")
    presetGui.MarginX := 10, presetGui.MarginY := 10
    presetGui.SetFont("s9", "Microsoft JhengHei UI")
    presetGui.OnEvent("Close", HidePresetDialog)
    presetGui.OnEvent("Escape", HidePresetDialog)
    presetGui.Add("Text", "x10 y10 w520 cGray", "此清單與功能按鍵 JSON 是同一套資料。空白熱鍵代表停用；預設功能彼此衝突時不可輸出。")
    presetKeyList := presetGui.Add("ListView", "x10 y35 w520 h240 -Multi", ["預設功能", "熱鍵", "狀態"])
    presetKeyList.ModifyCol(1, 275), presetKeyList.ModifyCol(2, 105), presetKeyList.ModifyCol(3, 135)
    presetKeyList.OnEvent("ItemSelect", PresetSelected)
    presetGui.Add("GroupBox", "x10 y285 w520 h118", "變更選取項目")
    presetGui.Add("Text", "x22 y316 w45", "熱鍵：")
    presetKeyEditor := presetGui.Add("Hotkey", "x70 y312 w220")
    applyButton := presetGui.Add("Button", "x302 y310 w215 h27", "套用至清單")
    applyButton.OnEvent("Click", ApplyPresetEdit)
    presetGui.Add("Text", "x22 y350 w70", "特殊按鍵：")
    tabButton := presetGui.Add("Button", "x95 y346 w52 h24", "Tab")
    spaceButton := presetGui.Add("Button", "x151 y346 w56 h24", "Space")
    escButton := presetGui.Add("Button", "x211 y346 w52 h24", "Esc")
    mouse1Button := presetGui.Add("Button", "x267 y346 w70 h24", "側鍵 1")
    mouse2Button := presetGui.Add("Button", "x341 y346 w70 h24", "側鍵 2")
    leftButton := presetGui.Add("Button", "x415 y346 w48 h24", "左鍵")
    rightButton := presetGui.Add("Button", "x467 y346 w50 h24", "右鍵")
    tabButton.OnEvent("Click", (*) => SetPresetSpecialKey("Tab"))
    spaceButton.OnEvent("Click", (*) => SetPresetSpecialKey("Space"))
    escButton.OnEvent("Click", (*) => SetPresetSpecialKey("Esc"))
    mouse1Button.OnEvent("Click", (*) => SetPresetSpecialKey("XButton1"))
    mouse2Button.OnEvent("Click", (*) => SetPresetSpecialKey("XButton2"))
    leftButton.OnEvent("Click", (*) => SetPresetSpecialKey("LButton"))
    rightButton.OnEvent("Click", (*) => SetPresetSpecialKey("RButton"))
    resetButton := presetGui.Add("Button", "x10 y416 w205 h28", "恢復預設按鍵")
    presetSaveButton := presetGui.Add("Button", "x225 y416 w305 h28", "儲存至設定檔")
    resetButton.OnEvent("Click", ResetPresets)
    presetSaveButton.OnEvent("Click", (*) => SaveFunctions())
    presetStatusText := presetGui.Add("Text", "x10 y454 w520 cRed", "")
    presetGui.Show("w540 h484")
    RefreshPresetList()
}

HidePresetDialog(guiObj, *) {
    guiObj.Hide()
}

SetPresetSpecialKey(keyName, *) {
    global presetKeyEditor
    if !IsObject(presetKeyEditor)
        return
    presetKeyEditor.Value := ""
    presetKeyEditor.Value := keyName
}
PresetSelected(list, row, isSelected) {
    global selectedPresetRow, presetKeyEditor
    if !isSelected
        return
    selectedPresetRow := row
    selectedHotkey := list.GetText(row, 2)
    ; Hotkey 控制項可能殘留前一次的修飾鍵狀態，先清空再設定。
    presetKeyEditor.Value := ""
    presetKeyEditor.Value := selectedHotkey
}

ApplyPresetEdit(*) {
    global selectedPresetRow, presetKeyList, presetKeyEditor, presetDefs
    if !selectedPresetRow
        return
    role := presetKeyList.GetText(selectedPresetRow, 1)
    for def in presetDefs {
        if def.name = role {
            def.hotkey := Trim(presetKeyEditor.Value)
            break
        }
    }
    RefreshAll()
}

ResetPresets(*) {
    InitPresets()
    RefreshAll()
}

; -------------------- 衝突偵測、清單與熱鍵 --------------------
RefreshAll() {
    global monitorText, statusText, presetGui, presetStatusText, presetSaveButton
    monitor := GetTargetMonitor()
    monitorText.Text := "鎖定解析度：" monitor.width " × " monitor.height "（" monitor.label "）"
    conflicts := AnalyzeConflicts()
    RefreshBindingList(conflicts)
    RefreshPresetList(conflicts)
    RegisterHotkeys(conflicts)
    hasPresetConflict := conflicts.presetConflict
    if IsObject(presetSaveButton)
        presetSaveButton.Enabled := !hasPresetConflict
    if IsObject(presetGui) && WinExist("ahk_id " presetGui.Hwnd) {
        presetStatusText.Text := hasPresetConflict ? "預設按鍵有衝突，修正前不能儲存功能 JSON。" : "設定可儲存；空白熱鍵不會參與衝突。"
    }
    statusText.Text := hasPresetConflict ? "預設按鍵衝突" : "設定已套用"
}

AnalyzeConflicts() {
    global presetDefs, bindings
    pCount := Map(), bCount := Map(), validPreset := Map(), info := Map()
    for def in presetDefs {
        key := NormalizeHotkey(def.hotkey)
        if (key != "")
            pCount[key] := (pCount.Has(key) ? pCount[key] : 0) + 1
    }
    presetConflict := false
    for key, count in pCount {
        if count > 1
            presetConflict := true
        else
            validPreset[key] := true
    }
    for binding in bindings {
        key := NormalizeHotkey(binding.hotkey)
        if key != "" && !validPreset.Has(key)
            bCount[key] := (bCount.Has(key) ? bCount[key] : 0) + 1
    }
    for binding in bindings {
        key := NormalizeHotkey(binding.hotkey)
        status := "啟用"
        if key = ""
            status := "已停用：未綁定"
        else if validPreset.Has(key)
            status := "已停用：與預設衝突"
        else if bCount.Has(key) && bCount[key] > 1
            status := "已停用：點位衝突"
        info[binding.id] := status
    }
    return {presetConflict:presetConflict, presetCount:pCount, validPreset:validPreset, bindingCount:bCount, bindingStatus:info}
}

RefreshBindingList(conflicts := "") {
    global actionList, bindings
    if !IsObject(conflicts)
        conflicts := AnalyzeConflicts()
    actionList.Delete()
    ; 先顯示所有停用／衝突項目，符合置頂規則。
    for pass in [1, 2] {
        for b in bindings {
            status := conflicts.bindingStatus[b.id]
            flagged := status != "啟用"
            if (pass = 1 && !flagged) || (pass = 2 && flagged)
                continue
            pos := RelativeToLockedCoordinates(b.rel_x, b.rel_y)
            actionList.Add("", b.name, b.hotkey, pos.x, pos.y, FunctionDisplay(b.function), status)
        }
    }
}

RefreshPresetList(conflicts := "") {
    global presetKeyList, presetDefs
    if !IsObject(presetKeyList)
        return
    if !IsObject(conflicts)
        conflicts := AnalyzeConflicts()
    presetKeyList.Delete()
    for pass in [1, 2] {
        for def in presetDefs {
            key := NormalizeHotkey(def.hotkey)
            bad := key != "" && conflicts.presetCount.Has(key) && conflicts.presetCount[key] > 1
            if (pass = 1 && !bad) || (pass = 2 && bad)
                continue
            state := key = "" ? "已停用：未綁定" : (bad ? "衝突：不可儲存" : "啟用")
            presetKeyList.Add("", def.name, def.hotkey, state)
        }
    }
}

RegisterHotkeys(conflicts) {
    global registeredPreset, registeredPoint, presetDefs, bindings, GAME_EXE
    ; 全域緊急停止是唯一不受遊戲前景限制的按鍵。
    ; 其他按鍵以 HotIfWinActive 註冊，因此離開遊戲、項目停用或衝突時都不會攔截實體按鍵。
    for key, role in registeredPreset {
        if role = "global_stop" || role = "record" {
            Try Hotkey("$*" key, "Off")
            Try Hotkey("$*" key " Up", "Off")
        } else {
            if IsMouseHotkey(key)
                HotIf(IsGameFocusedAndCursorOverGame)
            else
                HotIfWinActive "ahk_exe " GAME_EXE
            Try {
                Hotkey("$*" key, "Off")
                Hotkey("$*" key " Up", "Off")
            }
            HotIf
        }
    }
    for key, unused in registeredPoint {
        if IsMouseHotkey(key)
            HotIf(IsGameFocusedAndCursorOverGame)
        else
            HotIfWinActive "ahk_exe " GAME_EXE
        Try {
            Hotkey("$*" key, "Off")
            Hotkey("$*" key " Up", "Off")
        }
        HotIf
    }
    registeredPreset := Map(), registeredPoint := Map()
    for def in presetDefs {
        key := NormalizeHotkey(def.hotkey)
        if key = "" || !conflicts.validPreset.Has(key)
            continue
        if def.role = "global_stop" || def.role = "record" {
            Try {
                Hotkey("$*" def.hotkey, PresetDown, "On")
                Hotkey("$*" def.hotkey " Up", PresetUp, "On")
                registeredPreset[key] := def.role
            }
        } else {
            if IsMouseHotkey(def.hotkey)
                HotIf(IsGameFocusedAndCursorOverGame)
            else
                HotIfWinActive "ahk_exe " GAME_EXE
            Try {
                Hotkey("$*" def.hotkey, PresetDown, "On")
                Hotkey("$*" def.hotkey " Up", PresetUp, "On")
                registeredPreset[key] := def.role
            }
            HotIf
        }
    }
    for binding in bindings {
        key := NormalizeHotkey(binding.hotkey)
        if !conflicts.bindingStatus.Has(binding.id) || conflicts.bindingStatus[binding.id] != "啟用"
            continue
        if IsMouseHotkey(binding.hotkey)
            HotIf(IsGameFocusedAndCursorOverGame)
        else
            HotIfWinActive "ahk_exe " GAME_EXE
        Try {
            Hotkey("$*" binding.hotkey, PointDown, "On")
            Hotkey("$*" binding.hotkey " Up", PointUp, "On")
            registeredPoint[key] := binding.id
        }
        HotIf
    }
}

PresetDown(*) {
    global registeredPreset, presetByRole
    key := RuntimeKey(A_ThisHotkey)
    if !registeredPreset.Has(key)
        return
    def := presetByRole[registeredPreset[key]]
    if def.kind = "stop" {
        StopAllMacros()
        ExitApp
    }
    if def.kind = "record" {
        StartRecording()
        return
    }
    if !IsGameFocused()
        return
    if def.kind = "send" {
        Send "{" def.send "}"
    } else if def.kind = "double_esc" {
        Send "{Esc}"
        Sleep def.wait
        Send "{Esc}"
    } else if def.kind = "spam" || def.kind = "click_spam" || def.kind = "card" {
        StartPresetLoop(def)
    }
}

PresetUp(*) {
    global registeredPreset
    key := RuntimeKey(A_ThisHotkey)
    if registeredPreset.Has(key)
        StopRuntime("preset:" registeredPreset[key])
}

StartPresetLoop(def) {
    global runtimeActions
    id := "preset:" def.role
    if runtimeActions.Has(id)
        return
    if def.kind = "card" {
        Send "{Esc}"
        Sleep 10
    }
    runtimeActions[id] := {type:"preset", kind:def.kind, send:(def.HasOwnProp("send") ? def.send : ""), hotkey:def.hotkey, heartbeat:A_TickCount}
    SetTimer(RuntimeTick, 1)
    SetTimer(SafetyTick, 100)
}

PointDown(*) {
    global registeredPoint, bindings, runtimeActions, mouseButtonOwner
    if !IsGameFocused()
        return
    key := RuntimeKey(A_ThisHotkey)
    if !registeredPoint.Has(key)
        return
    id := registeredPoint[key]
    runtimeId := "point:" id
    if runtimeActions.Has(runtimeId)
        return
    binding := FindBinding(id)
    if !IsObject(binding)
        return
    target := RelativeToScreen(binding.rel_x, binding.rel_y)
    if binding.function = "normal" {
        ; 普通點位為持續按住左鍵，不能與拖曳交易共用同一個左鍵。
        if mouseButtonOwner != "" || HasActiveDragPointAction()
            return
        MouseMove target.x, target.y, 0
        Click "Down"
        mouseButtonOwner := runtimeId
        runtimeActions[runtimeId] := {type:"point", kind:"normal", x:target.x, y:target.y, hotkey:binding.hotkey, heartbeat:A_TickCount}
    } else {
        ; 多個拖曳點位可同時排隊，RunOneDragStep 會輪流執行完整交易。
        if HasActiveNormalPointAction()
            return
        MouseGetPos &mx, &my
        runtimeActions[runtimeId] := {type:"point", kind:"drag", x:target.x, y:target.y, ax:mx, ay:my, stage:0, nextTick:A_TickCount, hotkey:binding.hotkey, heartbeat:A_TickCount}
    }
    SetTimer(RuntimeTick, 1)
    SetTimer(SafetyTick, 100)
}

PointUp(*) {
    global registeredPoint
    key := RuntimeKey(A_ThisHotkey)
    if registeredPoint.Has(key)
        StopRuntime("point:" registeredPoint[key])
}

RuntimeTick(*) {
    global runtimeActions, mouseButtonOwner
    if !IsGameFocused() {
        StopAllMacros()
        return
    }
    for id, action in runtimeActions.Clone() {
        action.heartbeat := A_TickCount
        runtimeActions[id] := action
        if action.type = "preset" {
            ; 左鍵被點位交易持有期間，不插入其他 Click 破壞 Down/Up 順序。
            if mouseButtonOwner != ""
                continue
            if action.kind = "spam" || action.kind = "card" {
                Send "{" action.send "}"
                Click
            } else if action.kind = "click_spam" {
                Click
            }
        } else if action.kind = "normal" {
            MouseMove action.x, action.y, 0
        }
    }
    RunOneDragStep()
    if runtimeActions.Count = 0
        SetTimer(RuntimeTick, 0)
}

HasActiveNormalPointAction() {
    global runtimeActions
    for id, action in runtimeActions {
        if action.type = "point" && action.kind = "normal"
            return true
    }
    return false
}

HasActiveDragPointAction() {
    global runtimeActions
    for id, action in runtimeActions {
        if action.type = "point" && action.kind = "drag"
            return true
    }
    return false
}

NextDragActionId() {
    global runtimeActions, dragLastCompleted
    firstId := "", returnNext := false
    for id, action in runtimeActions {
        if action.type != "point" || action.kind != "drag"
            continue
        if firstId = ""
            firstId := id
        if returnNext
            return id
        if id = dragLastCompleted
            returnNext := true
    }
    return firstId
}

RunOneDragStep() {
    global runtimeActions, mouseButtonOwner, dragLastCompleted
    if mouseButtonOwner != "" {
        if !runtimeActions.Has(mouseButtonOwner) {
            Click "Up"
            mouseButtonOwner := ""
            return
        }
        action := runtimeActions[mouseButtonOwner]
        ; 普通點位正在持續按住時，不啟動任何拖曳交易。
        if action.type != "point" || action.kind != "drag"
            return
    } else {
        nextId := NextDragActionId()
        if nextId = ""
            return
        mouseButtonOwner := nextId
        action := runtimeActions[nextId]
    }
    if A_TickCount < action.nextTick
        return
    activeId := mouseButtonOwner
    if action.stage = 0 {
        MouseMove action.ax, action.ay, 0
        Click "Down"
        action.stage := 1
    } else if action.stage = 1 {
        MouseMove action.x, action.y, 0
        action.stage := 2
    } else {
        Click "Up"
        action.stage := 0
        dragLastCompleted := activeId
        mouseButtonOwner := ""
    }
    action.nextTick := A_TickCount + 20
    action.heartbeat := A_TickCount
    runtimeActions[activeId] := action
}
SafetyTick(*) {
    global runtimeActions
    ; 失敗即停止：失焦、實體熱鍵鬆開、心跳逾時或拖曳狀態異常，皆強制結束。
    if !IsGameFocused() {
        StopAllMacros()
        return
    }
    now := A_TickCount
    for id, action in runtimeActions.Clone() {
        if !action.HasOwnProp("hotkey") || action.hotkey = "" || !IsConfiguredHotkeyHeld(action.hotkey) {
            StopRuntime(id)
            continue
        }
        if !action.HasOwnProp("heartbeat") || now - action.heartbeat > 250 {
            StopRuntime(id)
            continue
        }
        if action.type = "point" && action.kind = "drag" {
            if action.stage < 0 || action.stage > 2 || !IsNumber(action.ax) || !IsNumber(action.ay) || !IsNumber(action.x) || !IsNumber(action.y) {
                StopRuntime(id)
                continue
            }
        }
    }
    if runtimeActions.Count = 0
        SetTimer(SafetyTick, 0)
}

StopRuntime(id) {
    global runtimeActions, mouseButtonOwner
    if !runtimeActions.Has(id)
        return
    action := runtimeActions[id]
    ; 僅真正持有左鍵的動作才送出 Up；排隊中的拖曳不能打斷其他交易。
    if action.type = "point" && (action.kind = "normal" || (action.kind = "drag" && mouseButtonOwner = id)) {
        Click "Up"
        if mouseButtonOwner = id
            mouseButtonOwner := ""
    }
    if action.type = "preset" && action.kind = "card" && IsGameFocused()
        Send "{Esc}"
    runtimeActions.Delete(id)
    if runtimeActions.Count = 0 {
        SetTimer(RuntimeTick, 0)
        SetTimer(SafetyTick, 0)
    }
}

StopAllMacros(*) {
    global runtimeActions, mouseButtonOwner
    ; 單一交易鎖確保只需放開一次實際左鍵。
    if mouseButtonOwner != ""
        Click "Up"
    mouseButtonOwner := ""
    for id, action in runtimeActions.Clone() {
        if action.type = "preset" && action.kind = "card" && IsGameFocused()
            Send "{Esc}"
        runtimeActions.Delete(id)
    }
    SetTimer(RuntimeTick, 0)
    SetTimer(SafetyTick, 0)
}

; -------------------- 點位設定與錄製 --------------------
StartRecording(*) {
    global recordingMode, statusText
    recordingMode := true
    statusText.Text := "錄製中：請以滑鼠左鍵點選位置"
    SetTimer(UpdateRecordingTooltip, 30)
    Hotkey("~LButton", CaptureRecordingClick, "On")
    Hotkey("Esc", CancelRecording, "On")
}

EnsureRecordingOverlay() {
    global recordingOverlay, recordingOverlayText
    if IsObject(recordingOverlay)
        return
    recordingOverlay := Gui("+AlwaysOnTop -Caption +ToolWindow -DPIScale +E0x20")
    recordingOverlay.MarginX := 6
    recordingOverlay.MarginY := 4
    recordingOverlay.BackColor := "FFF9D7"
    recordingOverlay.SetFont("s9", "Microsoft JhengHei UI")
    recordingOverlayText := recordingOverlay.Add("Text", "w270 h22", "")
}

UpdateRecordingTooltip(*) {
    global recordingMode, recordingOverlay, recordingOverlayText
    if recordingMode = "" {
        SetTimer(UpdateRecordingTooltip, 0)
        return
    }
    MouseGetPos &screenX, &screenY
    localPos := ScreenToLockedCoordinates(screenX, screenY)
    EnsureRecordingOverlay()
    recordingOverlayText.Text := "錄製中　X: " localPos.x "　Y: " localPos.y
    recordingOverlay.Show("NA x" (screenX + 18) " y" (screenY + 20))
}
CaptureRecordingClick(*) {
    global recordingMode, statusText
    if recordingMode = ""
        return
    Sleep 30
    MouseGetPos &x, &y
    localPos := ScreenToLockedCoordinates(x, y)
    if !IsLockedCoordinate(localPos.x, localPos.y) {
        statusText.Text := "錄製位置不在鎖定螢幕範圍內"
        return
    }
    AddCandidate(x, y, "錄製點位")
    FinishRecording()
}

CancelRecording(*) {
    FinishRecording()
}

FinishRecording() {
    global recordingMode, statusText
    recordingMode := ""
    SetTimer(UpdateRecordingTooltip, 0)
    if IsObject(recordingOverlay)
        recordingOverlay.Hide()
    Try Hotkey("~LButton", "Off")
    Try Hotkey("Esc", "Off")
    statusText.Text := "錄製完成"
}

AddCandidateFromMouse() {
    MouseGetPos &x, &y
    AddCandidate(x, y, "滑鼠點位")
}

AddCandidate(x, y, name) {
    global candidates, candidateList
    pos := ScreenToRelative(x, y)
    localPos := ScreenToLockedCoordinates(x, y)
    id := candidates.Length + 1
    candidates.Push({id:id, name:name, rel_x:pos.rel_x, rel_y:pos.rel_y})
    candidateList.Add("", id, name, localPos.x, localPos.y)
}

DeleteCandidate() {
    global candidateList, candidates
    row := candidateList.GetNext()
    if !row
        return
    candidates.RemoveAt(row)
    candidateList.Delete(row)
}

ClearCandidateList() {
    global candidateList, candidates
    candidates := []
    candidateList.Delete()
}

CandidateToBinding() {
    global candidateList, candidates, pointNameEdit, pointXEdit, pointYEdit
    row := candidateList.GetNext()
    if !row
        return
    c := candidates[row], pos := RelativeToLockedCoordinates(c.rel_x, c.rel_y)
    pointNameEdit.Value := c.name
    pointXEdit.Value := pos.x, pointYEdit.Value := pos.y
}

NewBindingForm() {
    global selectedBindingId, pointNameEdit, pointHotkeyEdit, pointXEdit, pointYEdit, pointFunctionDrop
    selectedBindingId := 0
    pointNameEdit.Value := "", pointHotkeyEdit.Value := "", pointXEdit.Value := "", pointYEdit.Value := ""
    pointFunctionDrop.Choose(1)
}

AddBlankBinding() {
    global points, bindings, nextPointId, nextBindingId, selectedBindingId
    global pointNameEdit, pointHotkeyEdit, pointXEdit, pointYEdit, pointFunctionDrop
    pointId := "p" nextPointId++
    bindingId := "b" nextBindingId++
    points.Push({id:pointId, name:"未命名點位", rel_x:0, rel_y:0})
    bindings.Push({id:bindingId, point_id:pointId, name:"未命名點位", hotkey:"", rel_x:0, rel_y:0, function:"normal"})
    selectedBindingId := bindingId
    RefreshAll()
    pointNameEdit.Value := "未命名點位", pointHotkeyEdit.Value := "", pointXEdit.Value := "0", pointYEdit.Value := "0"
    pointFunctionDrop.Choose(1)
}

SaveBinding() {
    global selectedBindingId, pointNameEdit, pointHotkeyEdit, pointXEdit, pointYEdit, pointFunctionDrop
    global points, bindings, nextPointId, nextBindingId
    name := Trim(pointNameEdit.Value), hotkey := Trim(pointHotkeyEdit.Value)
    if name = "" {
        MsgBox "請填入點位名稱。", "點位設定", 48
        return
    }
    xText := Trim(pointXEdit.Value), yText := Trim(pointYEdit.Value)
    if !RegExMatch(xText, "^\d+$") || !RegExMatch(yText, "^\d+$") {
        MsgBox "請填入有效的 X/Y 整數座標。", "點位設定", 48
        return
    }
    x := Integer(xText), y := Integer(yText)
    if !IsLockedCoordinate(x, y) {
        monitor := GetTargetMonitor()
        MsgBox "座標必須位於鎖定範圍：X 0～" (monitor.width - 1) "，Y 0～" (monitor.height - 1) "。", "點位設定", 48
        return
    }
    rel := LockedCoordinatesToRelative(x, y)
    function := pointFunctionDrop.Value = 2 ? "alternating" : "normal"
    if selectedBindingId {
        b := FindBinding(selectedBindingId)
        b.name := name, b.hotkey := hotkey, b.rel_x := rel.rel_x, b.rel_y := rel.rel_y, b.function := function
        for p in points {
            if p.id = b.point_id {
                p.name := name, p.rel_x := rel.rel_x, p.rel_y := rel.rel_y
                break
            }
        }
    } else {
        pointId := "p" nextPointId++
        points.Push({id:pointId, name:name, rel_x:rel.rel_x, rel_y:rel.rel_y})
        bindings.Push({id:"b" nextBindingId++, point_id:pointId, name:name, hotkey:hotkey, rel_x:rel.rel_x, rel_y:rel.rel_y, function:function})
    }
    RefreshAll(), NewBindingForm()
}

BindingSelected(list, row, selected) {
    global bindings, selectedBindingId, pointNameEdit, pointHotkeyEdit, pointXEdit, pointYEdit, pointFunctionDrop
    if !selected
        return
    name := list.GetText(row, 1), hotkey := list.GetText(row, 2)
    for b in bindings {
        if b.name = name && b.hotkey = hotkey {
            selectedBindingId := b.id, pos := RelativeToLockedCoordinates(b.rel_x, b.rel_y)
            pointNameEdit.Value := b.name, pointHotkeyEdit.Value := b.hotkey, pointXEdit.Value := pos.x, pointYEdit.Value := pos.y
            pointFunctionDrop.Choose(b.function = "alternating" ? 2 : 1)
            return
        }
    }
}

DeleteBinding() {
    global actionList, bindings, selectedBindingId
    if !selectedBindingId {
        row := actionList.GetNext()
        if !row
            return
        name := actionList.GetText(row, 1), hotkey := actionList.GetText(row, 2)
        for b in bindings {
            if b.name = name && b.hotkey = hotkey {
                selectedBindingId := b.id
                break
            }
        }
    }
    if selectedBindingId {
        for i, b in bindings {
            if b.id = selectedBindingId {
                bindings.RemoveAt(i)
                break
            }
        }
    }
    NewBindingForm(), RefreshAll()
}

ClearBindings() {
    global bindings
    if MsgBox("確定要清空所有點位按鍵嗎？", "清空點位按鍵", "YesNo Icon!") != "Yes"
        return
    bindings := []
    NewBindingForm(), RefreshAll()
}

FindBinding(id) {
    global bindings
    for b in bindings
        if b.id = id
            return b
    return 0
}

; -------------------- JSON --------------------
SavePoints() {
    global points, bindings, statusText
    ; 與 2.00 相同：每一次儲存都由使用者重新指定檔名與位置。
    path := FileSelect("S16", "", "儲存點位 JSON", "JSON (*.json)")
    if path = ""
        return
    if StrLower(SubStr(path, -4)) != ".json"
        path .= ".json"
    data := Map("schema", "baauto-points-v1", "points", points, "bindings", bindings)
    WriteJson(path, data, "點位 JSON")
    SetCurrentPointJson(path)
    statusText.Text := "點位 JSON 已儲存"
}

ImportPoints() {
    global points, bindings, nextPointId, nextBindingId, statusText
    path := FileSelect(3, "", "匯入點位 JSON", "JSON (*.json)")
    if path = ""
        return
    Try data := JsonParse(FileRead(path, "UTF-8"))
    Catch as err {
        MsgBox "JSON 格式無法讀取：" err.Message, "匯入失敗", 16
        return
    }
    if data is Map && data.Has("keymaps") {
        importedCount := ImportMuMuCandidates(data)
        if importedCount = 0 {
            MsgBox "此 MuMu JSON 找不到可轉換的遊戲座標。", "匯入失敗", 48
            return
        }
        SetCurrentPointJson(path)
        statusText.Text := "已新增 " importedCount " 個候選點位"
        MsgBox "已新增 " importedCount " 個點位。", "MuMu JSON", 64
        return
    }
    importedPoints := [], importedBindings := []
    if data is Map && data.Has("bindings") {
        importedPoints := data.Has("points") ? data["points"] : []
        for b in data["bindings"] {
            if !(b is Map)
                continue
            id := JsonValue(b, "id", "b" nextBindingId++)
            name := JsonValue(b, "name", "點位 " (importedBindings.Length + 1))
            hotkey := JsonValue(b, "hotkey", JsonValue(b, "key", ""))
            rx := JsonValue(b, "rel_x", ""), ry := JsonValue(b, "rel_y", "")
            if rx = "" || ry = ""
                continue
            fun := JsonValue(b, "function", "normal")
            if fun = "normal_point"
                fun := "normal"
            importedBindings.Push({id:id, point_id:JsonValue(b,"point_id",""), name:name, hotkey:hotkey, rel_x:rx+0, rel_y:ry+0, function:fun})
        }
    } else if data is Map && data.Has("keys") {
        baseW := JsonValue(data, "width", JsonValue(data, "screenW", 0))+0
        baseH := JsonValue(data, "height", JsonValue(data, "screenH", 0))+0
        for item in data["keys"] {
            if !(item is Map)
                continue
            key := JsonValue(item, "key", "")
            rx := JsonValue(item, "rel_x", ""), ry := JsonValue(item, "rel_y", "")
            if rx = "" || ry = "" {
                rx := JsonValue(item, "x", "")
                ry := JsonValue(item, "y", "")
                if baseW <= 0 || baseH <= 0 {
                    monitor := GetTargetMonitor()
                    baseW := monitor.width, baseH := monitor.height
                }
                if rx != "" && ry != ""
                    rx := rx / baseW, ry := ry / baseH
            }
            if rx = "" || ry = ""
                continue
            n := importedBindings.Length + 1, pointId := "p" nextPointId++
            importedPoints.Push({id:pointId, name:"舊版點位 " n, rel_x:rx+0, rel_y:ry+0})
            importedBindings.Push({id:"b" nextBindingId++, point_id:pointId, name:"舊版點位 " n, hotkey:key, rel_x:rx+0, rel_y:ry+0, function:"normal"})
        }
    } else {
        MsgBox "找不到可匯入的 points、bindings 或 keys。", "匯入失敗", 48
        return
    }
    points := importedPoints, bindings := importedBindings
    RefreshAll()
    SetCurrentPointJson(path)
    statusText.Text := "點位 JSON 已匯入"
    MsgBox "已匯入 " bindings.Length " 個點位按鍵。", "點位 JSON", 64
}

SetCurrentPointJson(path) {
    global currentPointJson, currentPointJsonText
    currentPointJson := path
    SplitPath path, &fileName
    currentPointJsonText.Text := "目前點位 JSON：" (fileName = "" ? path : fileName)
}

ImportMuMuCandidates(data) {
    if !(data is Map) || !data.Has("keymaps") || !(data["keymaps"] is Array)
        return 0
    importedCount := 0
    for keymap in data["keymaps"] {
        if !(keymap is Map) || !keymap.Has("key") || !(keymap["key"] is Map)
            continue
        keyInfo := keymap["key"]
        if JsonValue(keyInfo, "device", "") = "mouse"
            continue
        triggerKey := Trim(JsonValue(keyInfo, "text", ""))
        if triggerKey = ""
            continue

        basePos := MuMuBasePosition(keymap)
        realPos := 0
        if JsonValue(keymap, "type", "") = "Macro" {
            realPos := MuMuActionPosition(JsonValue(keymap, "press_actions", []))
            if !IsObject(realPos)
                realPos := MuMuActionPosition(JsonValue(keymap, "release_actions", []))
            if !IsObject(realPos)
                continue
            if IsObject(basePos) && Abs(realPos.x - basePos.x) < 0.0001 && Abs(realPos.y - basePos.y) < 0.0001
                continue
        } else {
            realPos := basePos
        }
        if !IsObject(realPos) || !MuMuValidCoordinate(realPos.x, realPos.y) || MuMuInIgnoredArea(realPos.x, realPos.y)
            continue
        AddCandidateRelative(realPos.x, realPos.y, triggerKey)
        importedCount += 1
    }
    return importedCount
}

MuMuBasePosition(keymap) {
    if keymap.Has("rel_work_position") && keymap["rel_work_position"] is Map {
        pos := keymap["rel_work_position"]
        x := JsonValue(pos, "rel_x", ""), y := JsonValue(pos, "rel_y", "")
        if x != "" && y != ""
            return {x:x + 0, y:y + 0}
    }
    if keymap.Has("icon") && keymap["icon"] is Map {
        icon := keymap["icon"]
        if icon.Has("rel_position") && icon["rel_position"] is Map {
            pos := icon["rel_position"]
            x := JsonValue(pos, "rel_x", ""), y := JsonValue(pos, "rel_y", "")
            if x != "" && y != ""
                return {x:x + 0, y:y + 0}
        }
    }
    return 0
}

MuMuActionPosition(actions) {
    if !(actions is Array)
        return 0
    for action in actions {
        if RegExMatch(action, "click_rel:\(([\d.]+),([\d.]+)\)", &match)
            return {x:match[1] + 0, y:match[2] + 0}
        if RegExMatch(action, "curve_rel:\(([\d.]+),([\d.]+)\)", &match)
            return {x:match[1] + 0, y:match[2] + 0}
    }
    return 0
}

MuMuValidCoordinate(x, y) {
    return x >= 0 && x <= 1 && y >= 0 && y <= 1
}

MuMuInIgnoredArea(x, y) {
    return (x >= 1700 / 2560 && x <= 2330 / 2560 && y >= 1125 / 1440 && y <= 1330 / 1440)
        || (x >= 2324 / 2560 && x <= 2526 / 2560 && y >= 1203 / 1440 && y <= 1395 / 1440)
        || (x >= 2419 / 2560 && x <= 2516 / 2560 && y >= 42 / 1440 && y <= 134 / 1440)
}

AddCandidateRelative(rx, ry, name) {
    global candidates, candidateList
    localPos := RelativeToLockedCoordinates(rx, ry)
    id := candidates.Length + 1
    candidates.Push({id:id, name:name, rel_x:rx + 0, rel_y:ry + 0})
    candidateList.Add("", id, name, localPos.x, localPos.y)
}
SaveFunctions() {
    global presetDefs, statusText
    conflicts := AnalyzeConflicts()
    if conflicts.presetConflict {
        MsgBox "預設按鍵彼此有衝突，修正後才能儲存功能 JSON。", "無法儲存", 48
        return
    }
    path := GetFunctionConfigPath()
    if !DirExist(RegExReplace(path, "\\[^\\]+$")) {
        MsgBox "找不到功能設定資料夾：`n" path, "無法儲存", 16
        return
    }
    hotkeys := Map()
    for def in presetDefs
        hotkeys[def.role] := def.hotkey
    WriteJson(path, Map("schema", "baauto-functions-v1", "hotkeys", hotkeys, "timing_ms", Map("frame_fast", 16, "frame_slow", 50)), "功能按鍵 JSON")
    statusText.Text := "功能按鍵已儲存至設定檔"
}

WriteJson(path, data, kind) {
    if FileExist(path)
        FileDelete path
    FileAppend JsonDump(data, "  "), path, "UTF-8"
    MsgBox kind " 已儲存：`n" path, "JSON", 64
}

JsonValue(obj, key, fallback := "") {
    return obj is Map && obj.Has(key) ? obj[key] : fallback
}

; -------------------- 螢幕、座標、工具 --------------------
DetectTargetMonitor() {
    global GAME_EXE
    hwnd := WinExist("ahk_exe " GAME_EXE)
    bestIndex := 0, bestOverlap := 0
    if hwnd {
        WinGetPos &gx, &gy, &gw, &gh, "ahk_id " hwnd
        Loop MonitorGetCount() {
            MonitorGet A_Index, &ml, &mt, &mr, &mb
            overlap := Max(0, Min(gx + gw, mr) - Max(gx, ml)) * Max(0, Min(gy + gh, mb) - Max(gy, mt))
            if overlap > bestOverlap
                bestOverlap := overlap, bestIndex := A_Index
        }
    }
    if !bestIndex {
        bestPixels := -1
        Loop MonitorGetCount() {
            MonitorGet A_Index, &ml, &mt, &mr, &mb
            pixels := (mr - ml) * (mb - mt)
            if pixels > bestPixels
                bestPixels := pixels, bestIndex := A_Index
        }
    }
    MonitorGet bestIndex, &left, &top, &right, &bottom
    return {index:bestIndex, left:left, top:top, width:right-left, height:bottom-top, label:(hwnd && bestOverlap > 0 ? "遊戲作用螢幕" : "最高解析度螢幕")}
}

GetTargetMonitor() {
    global coordinateMonitor
    if !IsObject(coordinateMonitor)
        coordinateMonitor := DetectTargetMonitor()
    return coordinateMonitor
}
ScreenToLockedCoordinates(screenX, screenY) {
    m := GetTargetMonitor()
    return {x:screenX - m.left, y:screenY - m.top}
}

LockedCoordinatesToRelative(x, y) {
    m := GetTargetMonitor()
    return {rel_x:x / m.width, rel_y:y / m.height}
}

RelativeToLockedCoordinates(rx, ry) {
    m := GetTargetMonitor()
    return {x:Min(m.width - 1, Max(0, Round(rx * m.width))), y:Min(m.height - 1, Max(0, Round(ry * m.height)))}
}

IsLockedCoordinate(x, y) {
    m := GetTargetMonitor()
    return x >= 0 && x < m.width && y >= 0 && y < m.height
}
ScreenToRelative(x, y) {
    m := GetTargetMonitor()
    return {rel_x:(x - m.left) / m.width, rel_y:(y - m.top) / m.height}
}

RelativeToScreen(rx, ry) {
    m := GetTargetMonitor()
    return {x:Round(m.left + rx * m.width), y:Round(m.top + ry * m.height)}
}

IsGameFocused() {
    global GAME_EXE
    return WinActive("ahk_exe " GAME_EXE) != 0
}

IsMouseHotkey(key) {
    base := StrLower(Trim(RegExReplace(key, "[\^!+#<>*$~]")))
    return base ~= "^(lbutton|rbutton|mbutton|xbutton1|xbutton2|wheelup|wheeldown|wheelleft|wheelright)$"
}

IsGameFocusedAndCursorOverGame(*) {
    global GAME_EXE
    if !IsGameFocused()
        return false
    MouseGetPos ,, &cursorHwnd
    if !cursorHwnd
        return false
    Try return StrLower(WinGetProcessName("ahk_id " cursorHwnd)) = StrLower(GAME_EXE)
    Catch
        return false
}
IsConfiguredHotkeyHeld(hotkey) {
    keyText := StrLower(Trim(hotkey))
    if keyText = ""
        return false
    if (InStr(keyText, "^") || InStr(keyText, "ctrl")) && !GetKeyState("Ctrl", "P")
        return false
    if (InStr(keyText, "!") || InStr(keyText, "alt")) && !GetKeyState("Alt", "P")
        return false
    if (InStr(keyText, "#") || InStr(keyText, "win")) && !(GetKeyState("LWin", "P") || GetKeyState("RWin", "P"))
        return false
    if (InStr(keyText, "shift") || RegExMatch(keyText, "^[\^!#<>*$~]*\+")) && !GetKeyState("Shift", "P")
        return false
    baseKey := RegExReplace(hotkey, "i)\b(ctrl|control|shift|alt|lwin|rwin|win)\b")
    baseKey := Trim(RegExReplace(baseKey, "[\^!+#<>*$~\s]"))
    if baseKey = ""
        return false
    Try return GetKeyState(baseKey, "P")
    Catch
        return false
}
NormalizeHotkey(key) {
    return StrLower(RegExReplace(Trim(key), "\s+"))
}

RuntimeKey(key) {
    key := RegExReplace(key, "^[\$\*\~]+")
    key := RegExReplace(key, "\s+Up$")
    return NormalizeHotkey(key)
}

FunctionDisplay(name) {
    return name = "alternating" ? "滑鼠拖曳至點位" : "普通點位"
}

; -------------------- 小型 JSON 讀寫器 --------------------
JsonParse(text) {
    pos := 1
    value := JsonReadValue(text, &pos)
    JsonSkip(text, &pos)
    if pos <= StrLen(text)
        throw Error("JSON 結尾有無法辨識的內容")
    return value
}

JsonSkip(text, &pos) {
    while pos <= StrLen(text) && InStr(" `t`r`n", SubStr(text, pos, 1))
        pos += 1
}

JsonReadValue(text, &pos) {
    JsonSkip(text, &pos)
    ch := SubStr(text, pos, 1)
    if ch = "{"
        return JsonReadObject(text, &pos)
    if ch = "["
        return JsonReadArray(text, &pos)
    if ch = '"'
        return JsonReadString(text, &pos)
    if SubStr(text, pos, 4) = "true" {
        pos += 4
        return true
    }
    if SubStr(text, pos, 5) = "false" {
        pos += 5
        return false
    }
    if SubStr(text, pos, 4) = "null" {
        pos += 4
        return ""
    }
    if RegExMatch(SubStr(text, pos), "^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?", &m) {
        pos += StrLen(m[0])
        return m[0] + 0
    }
    throw Error("JSON 值格式錯誤")
}

JsonReadObject(text, &pos) {
    obj := Map(), pos += 1
    JsonSkip(text, &pos)
    if SubStr(text, pos, 1) = "}" {
        pos += 1
        return obj
    }
    loop {
        JsonSkip(text, &pos)
        if SubStr(text, pos, 1) != '"'
            throw Error("物件鍵名格式錯誤")
        key := JsonReadString(text, &pos)
        JsonSkip(text, &pos)
        if SubStr(text, pos, 1) != ":"
            throw Error("物件缺少冒號")
        pos += 1, obj[key] := JsonReadValue(text, &pos)
        JsonSkip(text, &pos), ch := SubStr(text, pos, 1)
        if ch = "}" {
            pos += 1
            return obj
        }
        if ch != ","
            throw Error("物件缺少逗號")
        pos += 1
    }
}

JsonReadArray(text, &pos) {
    arr := [], pos += 1
    JsonSkip(text, &pos)
    if SubStr(text, pos, 1) = "]" {
        pos += 1
        return arr
    }
    loop {
        arr.Push(JsonReadValue(text, &pos))
        JsonSkip(text, &pos), ch := SubStr(text, pos, 1)
        if ch = "]" {
            pos += 1
            return arr
        }
        if ch != ","
            throw Error("陣列缺少逗號")
        pos += 1
    }
}

JsonReadString(text, &pos) {
    out := "", pos += 1
    while pos <= StrLen(text) {
        ch := SubStr(text, pos, 1), pos += 1
        if ch = '"'
            return out
        if ch != Chr(92) {
            out .= ch
            continue
        }
        esc := SubStr(text, pos, 1), pos += 1
        if esc = '"' || esc = Chr(92) || esc = "/"
            out .= esc
        else if esc = "b"
            out .= Chr(8)
        else if esc = "f"
            out .= Chr(12)
        else if esc = "n"
            out .= "`n"
        else if esc = "r"
            out .= "`r"
        else if esc = "t"
            out .= "`t"
        else if esc = "u" {
            hex := SubStr(text, pos, 4), pos += 4
            out .= Chr("0x" hex)
        } else
            throw Error("字串跳脫格式錯誤")
    }
    throw Error("字串未結束")
}

JsonDump(value, indent := "", level := 0) {
    pad := ""
    Loop level
        pad .= indent
    nextPad := pad indent
    if value is Map {
        if value.Count = 0
            return "{}"
        parts := []
        for key, val in value
            parts.Push(nextPad JsonDumpString(key) ": " JsonDump(val, indent, level + 1))
        return "{`n" JoinParts(parts, ",`n") "`n" pad "}"
    }
    if value is Array {
        if value.Length = 0
            return "[]"
        parts := []
        for val in value
            parts.Push(nextPad JsonDump(val, indent, level + 1))
        return "[`n" JoinParts(parts, ",`n") "`n" pad "]"
    }
    if value is String
        return JsonDumpString(value)
    if value = true
        return "true"
    if value = false
        return "false"
    ; 點位與綁定使用一般物件（非 Map）；逐一輸出其自有屬性。
    if value is Object {
        props := []
        for key, val in value.OwnProps()
            props.Push(nextPad JsonDumpString(key) ": " JsonDump(val, indent, level + 1))
        return props.Length ? "{`n" JoinParts(props, ",`n") "`n" pad "}" : "{}"
    }
    return value
}

JsonDumpString(value) {
    slash := Chr(92)
    value := StrReplace(value, slash, slash slash)
    value := StrReplace(value, '"', slash '"')
    value := StrReplace(value, "`r", slash "r")
    value := StrReplace(value, "`n", slash "n")
    value := StrReplace(value, "`t", slash "t")
    return '"' value '"'
}

JoinParts(parts, separator) {
    out := ""
    for item in parts
        out .= (out = "" ? "" : separator) item
    return out
}
