// 상단바: 왼쪽 디스플레이에 시각(12시간제, AM/PM)과 날짜/요일, 오른쪽에 인터페이스별 아이콘 인디케이터와 설정 버튼을 배치한다.
//
// 아이콘 인디케이터(왼쪽부터 USB, LAN, WLAN, BT, CORE — 라벨 없이 아이콘만 표시)
//             USB / LAN / WLAN은 링크가 활성화되어 있으면 점등된다(데몬 보고 기준).
//             BT는 현재 데몬이 제어하지 않으므로 항상 꺼진 상태다.
//             CORE(칩 아이콘)는 데몬(`tossd`) 연결 상태: 연결되면 초록, 끊기면 주황으로 점멸한다.
//             아이콘을 탭하면 `linkTapped(name)`이 발생해 해당 인터페이스 정보 시트(`shell/LinkInfo`)를 연다.
//             `selected`가 일치하는 아이콘 아래에는 선택 표시 밑줄이 나타난다.
// 설정 버튼:  탭하면 `settings()` 시그널을 보낸다. 아이콘이 작으므로 주변 영역까지 터치 영역으로 잡는다.
//             설정 패널이 열려 있는 동안(`settingsOpen`)에는 토글 버튼처럼 절반 눌린 상태를 유지한다.
import QtQuick
import "../kit"

Item {
    id: root
    property var sys: ({})
    property bool connected: false
    property bool settingsOpen: false
    property string selected: ""
    signal linkTapped(string name)
    signal settings()

    readonly property var links: sys.links || ({})
    height: Theme.barHeight

    Display {
        id: clock
        anchors { left: parent.left; leftMargin: Theme.gridMargin; top: parent.top; topMargin: Theme.gridMarginTop }
        width: 308
        height: Theme.barInner

        TextMetrics { id: timeBox; font: time.font; text: "00:00" }
        Txt {
            id: time
            anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
            width: timeBox.width
            horizontalAlignment: Text.AlignRight
            color: Theme.displayInk
            font.family: Theme.mono
            font.pixelSize: 28
            font.bold: true
        }
        Txt {
            id: ampm
            anchors { left: time.right; leftMargin: 6; baseline: time.baseline }
            color: Theme.displayInk
            font.family: Theme.mono
            font.pixelSize: Theme.fSmall
            font.bold: true
        }
        Rectangle {
            anchors { left: ampm.right; leftMargin: 12; verticalCenter: parent.verticalCenter }
            width: 2
            height: 24
            radius: 1
            color: Theme.displayOff
        }
        Txt {
            id: date
            anchors { right: parent.right; rightMargin: 14; baseline: time.baseline }
            color: Theme.displayInk
            font.family: Theme.mono
            font.pixelSize: Theme.fBody
        }
    }
    Timer {
        interval: 1000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            const now = new Date()
            const parts = Qt.formatTime(now, "h:mm AP").split(" ")
            time.text = parts[0]
            ampm.text = parts[1] || ""
            date.text = Qt.formatDate(now, "yyyy.MM.dd ddd").toUpperCase()
        }
    }

    component Link: Item {
        id: cell
        property alias name: icon.name
        property alias on: icon.on
        property alias blink: icon.blink
        property alias color: icon.color
        width: 42
        height: root.height
        IconIndicator {
            id: icon
            anchors { horizontalCenter: parent.horizontalCenter; verticalCenter: parent.top; verticalCenterOffset: clock.y + clock.height / 2 }
        }
        Rectangle {
            anchors { horizontalCenter: icon.horizontalCenter; top: icon.bottom; topMargin: 7 }
            width: 18
            height: 2
            radius: 1
            color: Theme.ink
            visible: root.selected === cell.name
        }
        TapHandler { onTapped: root.linkTapped(cell.name) }
    }
    Row {
        anchors { right: gear.left; rightMargin: 9; top: parent.top }
        Link { name: "usb"; on: !!root.links.usb }
        Link { name: "lan"; on: !!root.links.eth }
        Link { name: "wlan"; on: !!root.links.wlan }
        Link { name: "bt"; on: false; color: Theme.indicatorBlue }
        Link { name: "core"; on: true; blink: !root.connected; color: root.connected ? Theme.indicatorGreen : Theme.indicatorAmber }
    }

    Button {
        id: gear
        anchors { right: parent.right; rightMargin: Theme.gridMargin; verticalCenter: clock.verticalCenter }
        width: height
        height: clock.height
        held: root.settingsOpen
        interactive: false
        down: gearTap.pressed
        Glyph {
            anchors.centerIn: parent
            name: "gear"
            size: 22
            color: gear.inkColor
        }
    }
    Item {
        anchors { top: parent.top; right: parent.right; bottom: parent.bottom; left: gear.left; leftMargin: -10 }
        TapHandler {
            id: gearTap
            onTapped: root.settings()
        }
    }
}
