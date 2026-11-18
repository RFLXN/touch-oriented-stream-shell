// 버튼: 앞판의 홈에서 돌출되어 있으며(하단 측면과 그림자 표시), 누르면 홈 안으로 들어간다. 모든 클릭 가능한 요소의 기본 컴포넌트다.
// `held`를 켜면 절반쯤 눌린 상태로 유지된다(토글 상태 유지, 열린 메뉴 버튼 등).
//
// 홈은 `Display` 테두리와 동일한 형태를 사용해 두 컴포넌트가 조화롭게 어우러지도록 했다.
// 윗면은 홈 바닥에서 `buttonTravel`만큼 올라와 있고, 누르면 그만큼 내려가면서 상단에 홈 바닥이 드러나고 위쪽 테두리의 그림자가 진다.
// `text`를 지정하면 윗면 가운데에 작은 대문자 라벨(예: `"CONNECT"`)을 렌더링한다. 내부 자식 요소(아이콘, 인디케이터)는 윗면에 붙어 함께 움직인다.
import QtQuick
import QtQuick.Effects

Item {
    id: root
    property color color: Theme.buttonColor("bone")
    property string text: ""
    property bool held: false
    property bool interactive: true
    property bool down: tap.pressed
    readonly property color inkColor: Theme.inkOn(color)
    default property alias content: slot.data
    signal tapped()

    property real depth: down ? 1 : held ? 0.5 : 0
    Behavior on depth {
        NumberAnimation {
            duration: root.down ? Theme.tPress : Theme.tRelease
            easing.type: root.down ? Easing.OutQuad : Easing.OutBack
            easing.overshoot: 2.2
        }
    }

    readonly property int seat: 3

    Rectangle {
        anchors.fill: parent
        radius: Theme.rButton + root.seat
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.edgeLo }
            GradientStop { position: 1; color: Theme.edgeHi }
        }
    }
    Rectangle {
        anchors { fill: parent; margins: 1 }
        radius: Theme.rButton + root.seat - 1
        color: Theme.well
    }
    RectangularShadow {
        x: root.seat
        y: cap.y
        width: cap.width
        height: parent.height - root.seat - cap.y
        radius: Theme.rButton
        offset.y: 1 + 3 * (1 - Math.max(0, root.depth))
        blur: 3 + 8 * (1 - Math.max(0, root.depth))
        color: Theme.shadow
    }
    Rectangle {
        x: root.seat
        y: cap.y
        width: cap.width
        height: parent.height - root.seat - cap.y
        radius: Theme.rButton
        color: Qt.darker(root.color, 1.5)
    }
    Rectangle {
        id: cap
        x: root.seat
        y: root.seat + Math.max(0, root.depth) * Theme.buttonTravel
        width: parent.width - 2 * root.seat
        height: parent.height - 2 * root.seat - Theme.buttonTravel
        radius: Theme.rButton
        border.width: 1
        border.color: Qt.lighter(root.color, 1.18)
        gradient: Gradient {
            GradientStop { position: 0; color: Qt.lighter(root.color, 1.05) }
            GradientStop { position: 1; color: Qt.darker(root.color, 1.05) }
        }
        Txt {
            visible: root.text !== ""
            anchors.centerIn: parent
            text: root.text
            color: root.inkColor
            font.pixelSize: Theme.fSmall
            font.bold: true
            font.letterSpacing: 1.5
        }
        Item { id: slot; anchors.fill: parent }
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            opacity: Math.max(0, root.depth)
            gradient: Gradient {
                GradientStop { position: 0; color: Theme.displayShade }
                GradientStop { position: Math.min(0.5, 12 / Math.max(1, cap.height)); color: "#0a000000" }
                GradientStop { position: 1; color: "#0a000000" }
            }
        }
    }

    TapHandler {
        id: tap
        enabled: root.interactive
        onTapped: root.tapped()
    }
}
