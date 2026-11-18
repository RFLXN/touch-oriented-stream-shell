// 노브: 돌려서 값을 조절하는 원형 로터리 컴포넌트. 버튼처럼 앞판의 홈에서 돌출되어 있고(측면과 그림자 표시),
// 윗면의 포인터가 현재 값을 가리킨다. 둘레 눈금은 앞판에 인쇄된다.
//
// 실제 값 범위는 알지 못하며 각도와 눈금만 외부에서 주입받고 제스처 이벤트만 내보낸다:
//   angle            포인터 각도(deg). 0도가 위쪽이며 시계 방향이 양수(+)
//   ticks            눈금 배열: `[{"angle": deg, "lit": bool}]`. 활성 눈금은 진하게 표시된다
//   grabbed()        터치 시작(외부에서 현재 값을 기준점으로 저장하는 데 사용)
//   turned(travel)   터치 시작 후 이동 거리(px). 오른쪽/위쪽이 +(증가), 왼쪽/아래쪽이 -(감소)
//   released()       터치 종료(드래그 여부 무관)
//   tapped()         거의 움직이지 않고 탭한 경우
// 작은 노브는 손가락에 가려 원형 드래그가 불편하므로 직선 드래그 방식으로 동작한다.
// 드래그 중에는 페이지 스와이프를 막는다.
import QtQuick
import QtQuick.Effects

Item {
    id: root
    property color color: Theme.buttonColor("bone")
    property real angle: 0
    property var ticks: []
    readonly property bool dragging: area.pressed
    signal grabbed()
    signal turned(real travel)
    signal released()
    signal tapped()

    readonly property int rim: 11
    readonly property int seat: 3
    readonly property int lift: 4
    readonly property real size: Math.min(width, height)

    Behavior on angle { NumberAnimation { duration: 70; easing.type: Easing.OutQuad } }

    Item {
        id: dial
        anchors.centerIn: parent
        width: root.size
        height: root.size

        Repeater {
            model: root.ticks
            Item {
                required property var modelData
                anchors.centerIn: parent
                width: 3
                height: dial.height
                rotation: modelData.angle
                Rectangle {
                    width: parent.width
                    height: 7
                    radius: Theme.rDetail
                    color: modelData.lit ? Theme.ink : Theme.mix(Theme.plate, Theme.inkDim, 0.55)
                    Behavior on color { ColorAnimation { duration: Theme.tState } }
                }
            }
        }

        Rectangle {
            id: hole
            anchors { fill: parent; margins: root.rim }
            radius: width / 2
            gradient: Gradient {
                GradientStop { position: 0; color: Theme.edgeLo }
                GradientStop { position: 1; color: Theme.edgeHi }
            }
        }
        Rectangle {
            anchors { fill: hole; margins: 1 }
            radius: width / 2
            color: Theme.well
        }
        RectangularShadow {
            x: cap.x
            y: cap.y + root.lift
            width: cap.width
            height: cap.height
            radius: width / 2
            offset.y: 3
            blur: 9
            color: Theme.shadow
        }
        Rectangle {
            x: cap.x
            y: cap.y + root.lift
            width: cap.width
            height: cap.height
            radius: width / 2
            color: Qt.darker(root.color, 1.5)
        }
        Rectangle {
            id: cap
            x: hole.x + root.seat
            y: hole.y + root.seat
            width: hole.width - 2 * root.seat
            height: hole.height - 2 * root.seat - root.lift
            radius: width / 2
            border.width: 1
            border.color: Qt.lighter(root.color, 1.18)
            gradient: Gradient {
                GradientStop { position: 0; color: Qt.lighter(root.color, 1.05) }
                GradientStop { position: 1; color: Qt.darker(root.color, 1.05) }
            }
            Item {
                anchors.fill: parent
                rotation: root.angle
                Rectangle {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 6
                    width: 4
                    height: Math.round(parent.height * 0.3)
                    radius: Theme.rDetail
                    color: Theme.inkOn(root.color)
                }
            }
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        preventStealing: true
        property real x0: 0
        property real y0: 0
        property real far: 0
        onPressed: (m) => {
            x0 = m.x
            y0 = m.y
            far = 0
            root.grabbed()
        }
        onPositionChanged: (m) => {
            const travel = (m.x - x0) - (m.y - y0)
            far = Math.max(far, Math.abs(m.x - x0), Math.abs(m.y - y0))
            if (far > 6)
                root.turned(travel)
        }
        onReleased: {
            root.released()
            if (far <= 6)
                root.tapped()
        }
        onCanceled: root.released()
    }
}
