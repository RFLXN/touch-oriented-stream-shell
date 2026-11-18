// 스트림덱 메인 화면. 데몬(`tossd`)이 보내 주는 상태와 배치를 렌더링하고, 위젯의 호스트 액션 요청과 사용자의 배치/설정 변경을 데몬에 전달한다.
// 네이티브 코드 없이 QML만으로 작성되었으며, 데몬과는 WebSocket(JSON)으로 통신한다.
//
// 구조(`docs/03-userland-guide.md` 3.16~3.17절)
//   Main.qml   루트 윈도우. 하위 컴포넌트들을 조립하고 이벤트를 중계한다:
//                편집 모드(`editing`) 전환, 추가 메뉴 및 `Pager`에서 발생한 배치 편집을 데몬에 전송하고,
//                되돌릴 수 없는 작업은 확인 다이얼로그(`ConfirmDialog`)를 거치도록 한다(설정에서 요청한 `confirmAsked` 및
//                위젯이 들어 있는 랙 삭제 — 빈 랙과 단일 위젯은 즉시 삭제).
//                데몬 연결이 끊기면 설정 패널, 확인 다이얼로그, 편집 모드(추가 메뉴, 이름 변경 다이얼로그)를 모두 닫고 재연결 전까지 설정을 열지 않는다
//                (연결이 끊긴 동안에는 입력이 데몬으로 전달되지 않음).
//   shell/     화면 뼈대: 데몬 연결(`DaemonLink`), 상단바, 페이지/그리드(`Pager`), 랙, 위젯 슬롯(`WidgetSlot`), 설정 패널 등
//   kit/       UI 컴포넌트: 디자인 토큰(`Theme` — 용어 및 스타일 규칙 포함), 버튼, 노브, 디스플레이, 인디케이터, 위젯 프레임 등
//   widgets/   앱별 위젯 컴포넌트. 새 앱을 추가할 때 주로 작업하는 영역
import QtQuick
import "kit"
import "shell"

Window {
    id: win
    width: 800
    height: 1280
    visible: true
    visibility: Window.FullScreen
    color: "black"

    DaemonLink { id: daemon }

    property bool editing: false
    function stopEditing() {
        adder.open = false
        rename.close()
        editing = false
        daemon.layoutTidy()
    }
    function removePlaced(target, info) {
        const held = target === "rack" && info.widgets ? info.widgets.length : 0
        if (held === 0) {
            daemon.layoutRemove(target, info.id)
            return
        }
        confirm.ask("REMOVE RACK",
                    (info.label ? "The rack " + info.label : "This rack") + " and the " + held + (held === 1 ? " widget" : " widgets")
                    + " in it will be removed. This cannot be undone.",
                    "REMOVE", () => daemon.layoutRemove(target, info.id), "")
    }
    property var idsBeforeAdd: null
    function ids(pages) {
        const out = {}
        for (const p of pages) {
            for (const r of p.racks)
                out["rack" + r.id] = true
            for (const w of p.widgets)
                out["widget" + w.id] = true
        }
        return out
    }
    Connections {
        target: daemon
        function onPagesChanged() {
            if (!win.idsBeforeAdd)
                return
            const before = win.idsBeforeAdd
            win.idsBeforeAdd = null
            for (let i = 0; i < daemon.pages.length; i++) {
                const p = daemon.pages[i]
                if (p.racks.some((r) => !before["rack" + r.id]) || p.widgets.some((w) => !before["widget" + w.id])) {
                    pager.show(i)
                    return
                }
            }
        }
        function onConnectedChanged() {
            if (daemon.connected)
                return
            settings.open = false
            confirm.close()
            win.stopEditing()
        }
    }

    Item {
        id: screen
        anchors { fill: parent; margins: Theme.screenInset }

        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0; color: Theme.plate }
                GradientStop { position: 1; color: Theme.plateShade }
            }
        }

        TopBar {
            id: bar
            width: parent.width
            visible: !win.editing
            sys: daemon.sys
            connected: daemon.connected
            settingsOpen: settings.open
            onSettings: if (daemon.connected || settings.open) settings.open = !settings.open
            selected: info.which
            onLinkTapped: (name) => info.which = (info.which === name ? "" : name)
        }

        Pager {
            id: pager
            anchors { top: bar.bottom; left: parent.left; right: parent.right; bottom: dots.top }
            link: daemon
            editing: win.editing
            onButtonAddAsked: (rack, x, y) => adder.show("button", { rack: rack, x: x, y: y })
            onMoved: (target, id, page, x, y) => daemon.layoutMove(target, id, page, x, y)
            onResized: (id, w, h) => daemon.layoutResize(id, w, h)
            onRemoved: (target, info) => win.removePlaced(target, info)
            onRenameAsked: (rack) => rename.ask(rack)
        }

        EditBar {
            visible: win.editing
            width: parent.width
            adding: adder.open
            onAdd: adder.open ? adder.open = false : adder.show("widget", null)
            onDone: win.stopEditing()
        }

        PageDots {
            id: dots
            anchors { bottom: parent.bottom; bottomMargin: Theme.gridMargin - 14; horizontalCenter: parent.horizontalCenter }
            count: pager.count
            current: pager.currentIndex
        }

        LinkLost {
            anchors { fill: parent; topMargin: bar.height }
            active: !daemon.connected
        }

        SettingsPanel {
            id: settings
            anchors { top: bar.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            link: daemon
            keyboardCover: keyboard.active ? keyboard.cover : 0
            onEditLayout: {
                settings.open = false
                win.editing = true
            }
            onConfirmAsked: (title, text, yes, then, color) => confirm.ask(title, text, yes, then, color)
        }

        AddPanel {
            id: adder
            anchors { top: bar.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            catalog: daemon.catalog
            hosts: daemon.hosts
            meters: daemon.meters
            deckName: daemon.sys.host || ""
            keyboardCover: keyboard.active ? keyboard.cover : 0
            onAdded: (what, chosen) => {
                if (adder.target) {
                    daemon.layoutPut(adder.target.rack, adder.target.x, adder.target.y, what, chosen)
                } else {
                    win.idsBeforeAdd = win.ids(daemon.pages)
                    daemon.layoutAdd(what, pager.currentIndex, chosen)
                }
                adder.open = false
            }
        }

        RenameDialog {
            id: rename
            anchors.fill: parent
            onRenamed: (id, label) => daemon.layoutRename(id, label)
        }

        ConfirmDialog {
            id: confirm
            anchors.fill: parent
        }

        LinkInfo {
            id: info
            anchors { top: bar.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            sys: daemon.sys
            wifi: daemon.wifi
            connected: daemon.connected
            daemonVersion: daemon.daemonVersion
            daemonUrl: daemon.url
        }

        KeyboardPanel {
            id: keyboard
            width: parent.width
        }
    }

    ScreenCorners {
        anchors.fill: parent
        color: win.color
    }
}
