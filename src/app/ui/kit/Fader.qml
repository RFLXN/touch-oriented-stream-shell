// 페이더: 위아래 드래그로 0~1 범위의 값을 고르는 세로 컨트롤(음량 믹서). `Slider`를 세로로 세운 것이다.
//
// 앞판에 파인 가는 세로 홈 위로 넓은 손잡이가 돌출되어 있고, 손잡이 아래쪽 홈 바닥에 색상(`color`)이 채워진다.
// 홈 왼쪽에는 10% 간격의 눈금을 인쇄하고(0, 50, 100%는 길게), 손잡이 가운데에는 가로 홈 한 줄을 새긴다. 터치 영역은 컴포넌트 전체다.
// 외부에서 `value`(0~1)를 주입받고, 드래그 중에는 손가락을 따라 움직이며 `moved(value)`를, 손을 뗄 때 `released(value)`를 발생시킨다.
// 드래그 중과 손을 뗀 직후 `settle` 동안은 외부 `value`를 무시한다(손가락 아래서 손잡이가 튀거나, 늦게 도착한 이전 값으로 되돌아가지 않도록).
// 홈을 누르면 손잡이가 그 위치로 바로 온다. `interactive`를 끄면 값만 보여 준다.
import QtQuick
import QtQuick.Effects

Item {
    id: root
    property real value: 0
    property color color: Theme.indicatorAmber
    property bool interactive: true
    property int settle: 700
    signal moved(real value)
    signal released(real value)

    implicitWidth: 56
    implicitHeight: 240

    readonly property int seat: 3
    readonly property int lift: 3
    readonly property real knobH: 34
    readonly property real slotW: 10
    readonly property bool dragging: area.pressed
    property real dragValue: 0
    property bool holding: false
    readonly property real shown: Math.max(0, Math.min(1, dragging || holding ? dragValue : value))
    property real depth: dragging ? 1 : 0
    Behavior on depth { NumberAnimation { duration: root.dragging ? Theme.tPress : Theme.tRelease } }

    Timer {
        id: hold
        interval: root.settle
        onTriggered: root.holding = false
    }

    Rectangle {
        id: slot
        anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; bottom: parent.bottom; topMargin: root.knobH / 2 - 4; bottomMargin: root.knobH / 2 - 4 }
        width: root.slotW
        radius: width / 2
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.edgeLo }
            GradientStop { position: 1; color: Theme.edgeHi }
        }
        Rectangle {
            anchors { fill: parent; margins: 1 }
            radius: width / 2
            color: Theme.well
            clip: true
            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: Math.max(0, parent.height - (knob.y + knob.height / 2 - slot.y - 1))
                radius: width / 2
                color: Theme.mix(Theme.well, root.color, 0.85)
            }
        }
    }
    Repeater {
        model: 11
        Rectangle {
            required property int index
            readonly property real at: root.seat + (index / 10) * (root.height - 2 * root.seat - root.knobH - root.lift) + root.knobH / 2
            x: index % 5 === 0 ? 2 : 6
            y: at
            width: slot.x - x - 5
            height: 1
            color: Theme.mix(Theme.plate, Theme.edgeLo, 0.9)
        }
    }
    RectangularShadow {
        x: knob.x
        y: knob.y
        width: knob.width
        height: knob.height + root.lift * (1 - root.depth)
        radius: Theme.rButton
        offset.y: 1 + 2 * (1 - root.depth)
        blur: 3 + 6 * (1 - root.depth)
        color: Theme.shadow
    }
    Rectangle {
        x: knob.x
        y: knob.y
        width: knob.width
        height: knob.height + root.lift * (1 - root.depth)
        radius: Theme.rButton
        color: Qt.darker(knob.face, 1.5)
    }
    Rectangle {
        id: knob
        readonly property color face: Theme.buttonColor("bone")
        x: root.seat
        y: root.seat + (1 - root.shown) * (root.height - 2 * root.seat - root.knobH - root.lift) + root.depth * root.lift
        width: root.width - 2 * root.seat
        height: root.knobH
        radius: Theme.rButton
        border.width: 1
        border.color: Qt.lighter(face, 1.18)
        gradient: Gradient {
            GradientStop { position: 0; color: Qt.lighter(knob.face, 1.05) }
            GradientStop { position: 1; color: Qt.darker(knob.face, 1.05) }
        }
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 14
            height: 2
            radius: 1
            color: Qt.darker(knob.face, 1.35)
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        enabled: root.interactive
        preventStealing: true
        property real grab: 0
        function at(y) { return Math.max(0, Math.min(1, 1 - (y - grab - root.seat) / (root.height - 2 * root.seat - root.knobH - root.lift))) }
        onPressed: (m) => {
            hold.stop()
            const onKnob = m.y >= knob.y && m.y <= knob.y + knob.height
            grab = onKnob ? m.y - knob.y : root.knobH / 2
            root.dragValue = at(m.y)
            root.moved(root.dragValue)
        }
        onPositionChanged: (m) => {
            root.dragValue = at(m.y)
            root.moved(root.dragValue)
        }
        function finish() {
            root.holding = true
            hold.restart()
            root.released(root.dragValue)
        }
        onReleased: finish()
        onCanceled: finish()
    }
}
