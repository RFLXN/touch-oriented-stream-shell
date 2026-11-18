// 세그먼트 바: 디스플레이 안에서 0~1 범위의 비율을 눈금 칸 개수로 표시한다. 끝의 두 칸은 경고 색상을 사용한다.
import QtQuick

Row {
    id: root
    property real value: 0
    property int count: 12
    property color color: Theme.displayInk
    property color hot: Theme.indicatorRed
    property real segWidth: 10
    property real segHeight: 14
    readonly property int litCount: Math.round(Math.max(0, Math.min(1, value)) * count)

    spacing: 2
    Repeater {
        model: root.count
        Rectangle {
            required property int index
            width: root.segWidth
            height: root.segHeight
            radius: Theme.rDetail
            color: index < root.litCount ? (index >= root.count - 2 ? root.hot : root.color) : Theme.displayOff
            Behavior on color { ColorAnimation { duration: Theme.tState } }
        }
    }
}
