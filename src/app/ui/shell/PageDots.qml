// 페이지 인디케이터: 전체 페이지 수만큼 점(LED)을 나열하고 현재 페이지를 점등한다.
import QtQuick
import "../kit"

Row {
    id: root
    property int count: 0
    property int current: 0

    height: 28
    spacing: 16
    Repeater {
        model: root.count
        Indicator {
            required property int index
            anchors.verticalCenter: parent.verticalCenter
            size: 8
            color: Theme.indicatorAmber
            on: index === root.current
        }
    }
}
