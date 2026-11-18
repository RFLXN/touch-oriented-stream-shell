// 설정 섹션: 상단에 인쇄형 라벨을 두고 그 아래 옅은 패널 영역 안에 `SettingsRow` 항목들을 세로로 배치한다.
import QtQuick
import "../kit"

Column {
    id: root
    property string label: ""
    default property alias rows: list.data
    spacing: 8

    Label {
        x: 14
        text: root.label
    }
    Rectangle {
        width: root.width
        height: list.height
        radius: Theme.rFrame
        color: Theme.mix(Theme.plate, Theme.frame, 0.55)
        border.width: 1
        border.color: Theme.mix(Theme.plate, Theme.edgeLo, 0.45)
        Column {
            id: list
            width: parent.width
        }
    }
}
