// 슬라이더: 좌우 드래그로 0~1 범위의 연속 값을 선택하는 컨트롤(예: 화면 밝기).
//
// 토글과 같은 구조다: 버튼과 동일한 홈 안에 슬라이더 노브가 돌출되어 있고, 노브 왼쪽 트랙 바닥에 색상(`color`)이 채워진다.
// 외부에서 `value`(0~1)를 주입받고, 드래그 중에는 손가락을 따라 움직이며 `moved(value)`를, 손을 뗄 때 `released(value)`를 발생시킨다
// (드래그 중에는 외부 `value` 업데이트를 무시해 손가락 아래서 노브가 튀지 않도록 한다).
import QtQuick
import QtQuick.Effects

Item {
    id: root
    property real value: 0
    property color color: Theme.indicatorAmber
    signal moved(real value)
    signal released(real value)

    implicitWidth: 300
    implicitHeight: 46

    readonly property int seat: 3
    readonly property int lift: 4
    readonly property real knobW: 54
    readonly property bool dragging: area.pressed
    property real dragValue: 0
    readonly property real shown: Math.max(0, Math.min(1, dragging ? dragValue : value))
    property real depth: dragging ? 1 : 0
    Behavior on depth { NumberAnimation { duration: root.dragging ? Theme.tPress : Theme.tRelease } }

    Rectangle {
        anchors.fill: parent
        radius: Theme.rButton + root.seat
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.edgeLo }
            GradientStop { position: 1; color: Theme.edgeHi }
        }
    }
    Rectangle {
        id: floor
        anchors { fill: parent; margins: 1 }
        radius: Theme.rButton + root.seat - 1
        color: Theme.well
        clip: true
        Rectangle {
            width: knob.x + knob.width / 2
            height: parent.height
            radius: parent.radius
            color: Theme.mix(Theme.well, root.color, 0.85)
        }
        Rectangle {
            anchors { fill: parent; margins: 2 }
            radius: Theme.rButton
            gradient: Gradient {
                GradientStop { position: 0; color: Theme.displayShade }
                GradientStop { position: 0.3; color: "#00000000" }
                GradientStop { position: 1; color: "#00000000" }
            }
        }
    }
    RectangularShadow {
        x: knob.x
        y: knob.y
        width: knob.width
        height: root.height - root.seat - knob.y
        radius: Theme.rButton
        offset.y: 1 + 2 * (1 - root.depth)
        blur: 3 + 6 * (1 - root.depth)
        color: Theme.shadow
    }
    Rectangle {
        x: knob.x
        y: knob.y
        width: knob.width
        height: root.height - root.seat - knob.y
        radius: Theme.rButton
        color: Qt.darker(knob.face, 1.5)
    }
    Rectangle {
        id: knob
        readonly property color face: Theme.buttonColor("bone")
        x: root.seat + root.shown * (root.width - 2 * root.seat - width)
        y: root.seat + root.depth * root.lift
        width: root.knobW
        height: root.height - 2 * root.seat - root.lift
        radius: Theme.rButton
        border.width: 1
        border.color: Qt.lighter(face, 1.18)
        gradient: Gradient {
            GradientStop { position: 0; color: Qt.lighter(knob.face, 1.05) }
            GradientStop { position: 1; color: Qt.darker(knob.face, 1.05) }
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        preventStealing: true
        function at(x) { return Math.max(0, Math.min(1, (x - root.seat - root.knobW / 2) / (root.width - 2 * root.seat - root.knobW))) }
        onPressed: (m) => {
            root.dragValue = at(m.x)
            root.moved(root.dragValue)
        }
        onPositionChanged: (m) => {
            root.dragValue = at(m.x)
            root.moved(root.dragValue)
        }
        onReleased: root.released(root.dragValue)
        onCanceled: root.released(root.dragValue)
    }
}
