// 인디케이터: 상태를 표시하는 소형 LED. 꺼짐 / 켜짐 / 점멸(`blink`) 및 일회성 깜빡임(`flash()`)을 지원하며, 점멸 박자는 `kit/Blinker`가 맞춘다.
import QtQuick
import QtQuick.Effects

Item {
    id: root
    property bool on: false
    property bool blink: false
    property color color: Theme.indicatorGreen
    property real size: 10
    function flash() { flashing = true; flashTimer.restart() }

    property bool flashing: false
    readonly property bool lit: flashing || (on && (!blink || blinker.phase))
    property real level: lit ? 1 : 0
    Behavior on level { NumberAnimation { duration: 110 } }

    implicitWidth: size
    implicitHeight: size

    Timer { id: flashTimer; interval: 240; onTriggered: root.flashing = false }
    Blinker { id: blinker; active: root.blink && root.on }

    Rectangle {
        anchors.centerIn: parent
        width: root.size + 6
        height: width
        radius: width / 2
        color: "#30000000"
    }
    RectangularShadow {
        anchors.centerIn: parent
        width: root.size
        height: root.size
        radius: root.size / 2
        blur: root.size * 1.8
        spread: root.size * 0.2
        color: root.color
        opacity: root.level * 0.9
        visible: root.level > 0.01
    }
    Rectangle {
        anchors.centerIn: parent
        width: root.size
        height: root.size
        radius: root.size / 2
        color: Theme.mix(Theme.indicatorOff, root.color, root.level)
        border.width: 1
        border.color: "#40000000"
    }
    Rectangle {
        anchors.centerIn: parent
        width: root.size * 0.55
        height: width
        radius: width / 2
        color: Qt.lighter(root.color, 1.5)
        opacity: root.level * 0.9
    }
    Rectangle {
        x: (root.width - root.size) / 2 + root.size * 0.2
        y: (root.height - root.size) / 2 + root.size * 0.16
        width: root.size * 0.32
        height: width
        radius: width / 2
        color: "white"
        opacity: 0.25 + 0.35 * root.level
    }
}
