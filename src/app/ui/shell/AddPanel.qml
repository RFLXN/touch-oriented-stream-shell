// 위젯 추가 패널: 편집 모드에서 + 버튼을 누르면 상단바 아래로 펼쳐진다(`shell/DropPanel`). 항목을 선택하면 `added(what, settings)` 시그널을 발생시키며 호출 측에서 패널을 닫는다.
// 두 가지 컨텍스트(`form`)로 열린다:
//   `"widget"`  페이지에 배치할 복합 위젯 및 랙                   <- 편집 상단바의 + 버튼
//   `"button"`  랙 칸에 배치할 버튼 위젯(버튼, 스위치, 노브, `target`에 랙 ID와 좌표 지정)  <- 랙 빈 칸의 + 버튼
// 세 단계 화면(`page`)으로 진행한다:
//   apps      해당 `form`의 위젯을 제공하는 앱 목록(`"widget"`이면 랙 항목도 포함). 앱을 누르면 위젯 목록으로 이동한다.
//   widgets   선택한 앱의 위젯 종류 목록. 추가 설정이 필요 없는 종류는 누르는 즉시 추가된다.
//   options   해당 위젯 종류의 설정 입력 화면. 각 설정은 `shell/AddOption` 항목 하나로 렌더링된다.
//             선택한 값은 `chosen` 객체(`키 -> 값`)에 누적된다. 초기값으로 색상은 `"bone"`, 메트릭은 빈 배열, 덱 자신을 허용하는 `host`는 `"@deck"`을 넣는다.
//             PC를 변경하면 종속 설정(`command`, `metrics`)을 초기화하고, 명령을 선택하면 비어 있던 `label`을 명령 이름으로 자동 채운다(`chooseFor`).
//             필수 항목(`required`)을 모두 채워야 제목 줄 오른쪽의 ADD 버튼이 활성화된다.
// 추가 가능한 카탈로그 목록과 새 위젯의 자동 배치 위치는 데몬이 결정한다(`catalog`).
// `leave()`는 텍스트 입력 필드의 조합 중 문자를 커밋하고 가상 키보드를 닫는다. ADD 버튼을 누를 때(시그널 방출 직전)와 패널이 닫힐 때 자동으로 호출한다.
import QtQuick
import "../kit"

DropPanel {
    id: root
    property var catalog: []
    property var hosts: []
    property var meters: ({})
    property string deckName: ""
    property string form: "widget"
    property var target: null
    signal added(string what, var settings)

    property string page: "apps"
    property var app: null
    property var kind: null
    property var chosen: ({})

    readonly property var apps: {
        const out = []
        for (const a of catalog) {
            const widgets = a.widgets.filter((w) => w.form === form)
            if (widgets.length)
                out.push({ id: a.id, title: a.title, about: a.about, widgets: widgets })
        }
        return out
    }

    function show(form, target) {
        root.form = form
        root.target = target || null
        page = "apps"
        open = true
    }
    function pick(kind) {
        root.kind = kind
        if (kind.settings.length === 0) {
            root.added(kind.kind, {})
            return
        }
        const first = {}
        for (const s of kind.settings)
            first[s.key] = s.type === "color" ? "bone" : s.type === "url" ? "https://" : s.type === "metrics" ? [] : s.type === "flag" ? !!s.initial : s.type === "host" && s.own ? "@deck" : ""
        chosen = first
        page = "options"
    }
    function choose(key, value) { chosen = Object.assign({}, chosen, { [key]: value }) }
    function chooseFor(setting, value, title) {
        const next = Object.assign({}, chosen, { [setting.key]: value })
        if (setting.type === "command" && next.label === "")
            next.label = title.toUpperCase().slice(0, Theme.labelMax)
        if (setting.type === "host" && next.command !== undefined)
            next.command = ""
        if (setting.type === "host" && next.metrics !== undefined)
            next.metrics = []
        chosen = next
    }
    function toggleMetric(key, id, max) {
        const now = chosen[key] || []
        if (now.includes(id))
            choose(key, now.filter((x) => x !== id))
        else if (now.length < max)
            choose(key, now.concat([id]))
    }
    function back() { page = page === "options" ? "widgets" : "apps" }
    function leave() {
        for (let i = 0; i < optionList.count; i++) {
            const option = optionList.itemAt(i)
            if (option)
                option.leave()
        }
    }
    readonly property bool complete: {
        if (!kind)
            return false
        for (const s of kind.settings) {
            const v = chosen[s.key]
            if (s.required && (Array.isArray(v) ? v.length === 0 : !v))
                return false
            if (s.type === "url" && v && !/^https?:\/\/[^\/\s]\S*$/.test(v))
                return false
        }
        return true
    }

    onShownChanged: if (shown === 0) page = "apps"
    onOpenChanged: if (!open) leave()

    title: page === "widgets" && app ? app.title.toUpperCase()
         : page === "options" && kind ? (app.title + " · " + kind.title).toUpperCase()
         : form === "button" ? "ADD TO RACK" : "ADD"
    canGoBack: page !== "apps"
    onBackTapped: back()
    corner: Button {
        visible: root.page === "options"
        width: 120
        height: 44
        color: Theme.buttonColor("orange")
        enabled: root.complete
        opacity: root.complete ? 1 : Theme.dimmed
        text: "ADD"
        onTapped: {
            root.leave()
            root.added(root.kind.kind, root.chosen)
        }
    }

    Column {
        visible: root.page === "apps"
        width: parent.width
        spacing: 22
        SettingsSection {
            width: parent.width
            label: "APPS"
            Repeater {
                model: root.apps
                SettingsRow {
                    required property var modelData
                    required property int index
                    title: modelData.title
                    note: modelData.about
                    divider: index < root.apps.length - 1
                    chevron: true
                    onTapped: {
                        root.app = modelData
                        root.page = "widgets"
                    }
                }
            }
        }
        SettingsSection {
            visible: root.form === "widget"
            width: parent.width
            label: "LAYOUT"
            SettingsRow {
                title: "Rack"
                note: "An empty group for buttons · 2×1, resizable"
                divider: false
                onTapped: root.added("rack", {})
                Glyph { anchors.verticalCenter: parent.verticalCenter; name: "plus"; size: 20; color: Theme.inkDim }
            }
        }
    }

    SettingsSection {
        visible: root.page === "widgets"
        width: parent.width
        label: "WIDGETS"
        Repeater {
            model: root.app ? root.app.widgets : []
            SettingsRow {
                required property var modelData
                required property int index
                title: modelData.title
                note: modelData.about + (modelData.form === "widget" ? " · " + modelData.w + "×" + modelData.h : "")
                divider: index < root.app.widgets.length - 1
                onTapped: root.pick(modelData)
                Glyph {
                    anchors.verticalCenter: parent.verticalCenter
                    name: modelData.settings.length ? "chevron" : "plus"
                    size: 20
                    color: Theme.inkDim
                }
            }
        }
    }

    Flickable {
        visible: root.page === "options"
        anchors.fill: parent
        clip: true
        contentHeight: options.height + 8
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: options
            width: parent.width
            spacing: 22
            Repeater {
                id: optionList
                model: root.kind ? root.kind.settings : []
                AddOption {
                    width: options.width
                    panel: root
                }
            }
        }
    }
}
