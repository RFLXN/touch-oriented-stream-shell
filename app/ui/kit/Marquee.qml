// 한 줄 마키 텍스트: 너비를 초과하면 좌우로 스크롤한다(끝까지 이동 후 잠시 멈췄다가 처음으로 복귀).
// 스크롤 중에는 지속적으로 리페인트가 발생하므로 `running`으로 필요한 시점에만 활성화한다.
import QtQuick

Item {
    id: root
    property alias text: label.text
    property alias font: label.font
    property alias color: label.color
    property bool running: true
    readonly property real overflow: Math.max(0, label.implicitWidth - width)

    implicitHeight: label.implicitHeight
    clip: true

    Txt { id: label }

    SequentialAnimation {
        id: anim
        running: root.running && root.overflow > 0 && root.visible
        loops: Animation.Infinite
        PauseAnimation { duration: 2500 }
        NumberAnimation { target: label; property: "x"; to: -root.overflow; duration: root.overflow * 22 }
        PauseAnimation { duration: 2500 }
        NumberAnimation { target: label; property: "x"; to: 0; duration: 400; easing.type: Easing.InOutQuad }
        onRunningChanged: if (!running) label.x = 0
    }
    onTextChanged: {
        label.x = 0
        if (anim.running)
            anim.restart()
    }
}
