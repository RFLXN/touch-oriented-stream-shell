// 진행 표시줄: 디스플레이 내부에서 진행률(0~1)을 표시한다.
import QtQuick

Rectangle {
    id: root
    property real value: 0
    property color fill: Theme.accent

    implicitHeight: 6
    radius: Theme.rDetail
    color: Theme.displayOff
    Rectangle {
        width: Math.max(height, parent.width * Math.max(0, Math.min(1, root.value)))
        height: parent.height
        radius: parent.radius
        color: root.fill
        Behavior on width { NumberAnimation { duration: 300; easing.type: Easing.OutCubic } }
    }
}
