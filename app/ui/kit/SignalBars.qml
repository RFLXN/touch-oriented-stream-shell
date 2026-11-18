// Wi-Fi 신호 세기 막대: 계단형 막대 4개. RSSI(dBm)에 따라 왼쪽부터 점등된다.
import QtQuick

Row {
    id: root
    property int signal: -100
    readonly property int level: signal >= -55 ? 4 : signal >= -67 ? 3 : signal >= -78 ? 2 : 1
    spacing: 3
    height: 18

    Repeater {
        model: 4
        Rectangle {
            required property int index
            anchors.bottom: parent.bottom
            width: 4
            height: 6 + index * 4
            radius: Theme.rDetail
            color: index < root.level ? Theme.ink : Theme.indicatorOff
        }
    }
}
