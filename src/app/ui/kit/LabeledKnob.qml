// 라벨이 분리된 노브: 랙 장비처럼 위쪽에 로터리 노브를 두고 아래쪽에 패널 인쇄 라벨과 현재 값 표시줄을 배치한다.
// 랙 1칸에 들어간다(`kit/LabeledButton`과 한 쌍: 라벨 폰트가 같고 전체 높이도 거의 같아 버튼과 나란히 놓아도 줄이 어긋나지 않는다).
//
// 입력 프로퍼티(`angle`, `ticks`)와 제스처 시그널(`grabbed`, `turned`, `released`, `tapped`)은 `kit/Knob`과 동일하다.
// `value`는 라벨 아래에 작게 표시하는 현재 값 문자열이다(예: `"45"`, `"AUTO"`).
import QtQuick

Item {
    id: root
    property color color: Theme.buttonColor("bone")
    property string label: ""
    property string value: ""
    property alias angle: knob.angle
    property alias ticks: knob.ticks
    readonly property alias dragging: knob.dragging
    signal grabbed()
    signal turned(real travel)
    signal released()
    signal tapped()

    readonly property real knobSize: Math.min(104, width, height - 4 - caption.height)

    Item {
        anchors.centerIn: parent
        width: parent.width
        height: root.knobSize + 4 + caption.height

        Knob {
            id: knob
            anchors { horizontalCenter: parent.horizontalCenter; top: parent.top }
            width: root.knobSize
            height: root.knobSize
            color: root.color
            onGrabbed: root.grabbed()
            onTurned: (travel) => root.turned(travel)
            onReleased: root.released()
            onTapped: root.tapped()
        }
        Column {
            id: caption
            anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom }
            width: parent.width
            height: 38
            Txt {
                width: parent.width - 8
                height: 21
                anchors.horizontalCenter: parent.horizontalCenter
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                elide: Text.ElideRight
                text: root.label
                color: Theme.ink
                font.pixelSize: Theme.fSmall
                font.bold: true
                font.letterSpacing: 1
            }
            Label {
                width: parent.width - 8
                height: 17
                anchors.horizontalCenter: parent.horizontalCenter
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                elide: Text.ElideRight
                text: root.value
            }
        }
    }
}
