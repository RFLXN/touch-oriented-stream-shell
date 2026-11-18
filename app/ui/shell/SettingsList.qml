// 설정 리스트 섹션: 상단 라벨(`label`)과 우측 액션 버튼(`keyText` — 탭 시 `keyTapped`), 아래 패널 내부에 스크롤 가능한 행 목록을 배치한다.
// 항목이 없으면 패널 대신 `emptyText`를 표시한다. Wi-Fi AP 목록(`WifiPage`)과 페어링 기기 목록(`DevicesPage`)에서 사용한다.
//
// `rows`에 데이터 배열을 넣고 델리게이트(`delegate`, 보통 `SettingsRow`)는 `index`로 `list.rows[index]`를 참조한다:
//   delegate: SettingsRow {
//       required property int index
//       readonly property var info: list.rows[index] || fallback    (`rows`가 축소되는 순간 방어)
//       width: list.width
//       divider: index < list.count - 1
//   }
// 데몬이 상태 변경 시 배열 전체를 새로 내려 주므로, 배열 자체를 `ListView` 모델로 쓰면 갱신할 때마다 스크롤이 맨 위로 초기화되고
// 터치 중인 행이 풀린다. 따라서 `ListModel`에는 행 개수만 동기화해 기존 델리게이트 인스턴스를 유지하고 스크롤과 터치 상태를 보존한다.
import QtQuick
import "../kit"

Item {
    id: root
    property string label: ""
    property var rows: []
    property Component delegate
    readonly property alias count: list.count
    property string emptyText: ""
    property string keyText: ""
    property real keyWidth: 150
    property bool keyEnabled: true
    signal keyTapped()

    function sync() {
        const n = rows ? rows.length : 0
        while (slots.count < n)
            slots.append({ slot: slots.count })
        if (slots.count > n)
            slots.remove(n, slots.count - n)
    }
    onRowsChanged: sync()
    Component.onCompleted: sync()

    ListModel { id: slots }

    Item {
        id: head
        width: parent.width
        height: 44
        Label {
            anchors { left: parent.left; leftMargin: 14; bottom: parent.bottom; bottomMargin: 8 }
            text: root.label
        }
        Button {
            anchors { right: parent.right; bottom: parent.bottom; bottomMargin: 4 }
            width: root.keyWidth
            height: 40
            enabled: root.keyEnabled
            opacity: enabled ? 1 : Theme.dimmed
            text: root.keyText
            onTapped: root.keyTapped()
        }
    }
    Rectangle {
        anchors { top: head.bottom; topMargin: 4; left: parent.left; right: parent.right }
        height: Math.min(list.contentHeight, root.height - y)
        visible: list.count > 0
        radius: Theme.rFrame
        color: Theme.mix(Theme.plate, Theme.frame, 0.55)
        border.width: 1
        border.color: Theme.mix(Theme.plate, Theme.edgeLo, 0.45)
        ListView {
            id: list
            anchors.fill: parent
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: slots
            delegate: root.delegate
        }
    }
    Txt {
        anchors { top: head.bottom; topMargin: 20; left: parent.left; leftMargin: 14; right: parent.right; rightMargin: 14 }
        visible: list.count === 0
        wrapMode: Text.WordWrap
        text: root.emptyText
        color: Theme.inkDim
        font.pixelSize: Theme.fSmall
    }
}
