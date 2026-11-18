// 토글: 좌우 슬라이드 스위치. ON/OFF 두 상태를 전환하는 설정에 사용한다.
//
// 버튼과 같은 홈 안에 손잡이가 돌출되어 있으며, 켜지면 손잡이가 오른쪽으로 이동하면서 드러난 홈 바닥에 색상(`color`)이 표시된다.
// 상태는 외부에서 `on`으로 제어하며, 탭했을 때 자체적으로 상태를 바꾸지 않고 `tapped` 시그널만 발생시킨다.
// `interactive`를 끄면 터치 처리를 부모 컴포넌트(설정 행 전체가 탭 영역인 경우 등)에 위임하며, 이때 `down`을 바인딩해 눌린 피드백을 준다.
import QtQuick
import QtQuick.Effects

Item {
    id: root
    property bool on: false
    property color color: Theme.indicatorGreen
    property bool interactive: true
    property bool down: tap.pressed
    signal tapped()

    implicitWidth: 92
    implicitHeight: 46

    readonly property int seat: 3
    readonly property int lift: 4
    readonly property real knobW: Math.round((width - 2 * seat) * 0.54)

    property real level: on ? 1 : 0
    Behavior on level { NumberAnimation { duration: Theme.tToggle; easing.type: Easing.OutCubic } }
    property real depth: down ? 1 : 0
    Behavior on depth { NumberAnimation { duration: root.down ? Theme.tPress : Theme.tRelease } }

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
        color: Theme.mix(Theme.well, root.color, 0.85 * root.level)
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
        x: root.seat + root.level * (root.width - 2 * root.seat - width)
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

    TapHandler {
        id: tap
        enabled: root.interactive
        onTapped: root.tapped()
    }
}
