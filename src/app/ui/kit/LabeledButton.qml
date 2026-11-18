// 라벨이 분리된 버튼: 랙 장비처럼 위쪽에 컬러 버튼을 두고 아래쪽에 인디케이터와 패널 인쇄 라벨을 배치한다.
// 랙 내부 버튼 위젯(명령 버튼, 명령 스위치)의 기본 UI이며 복합 위젯 내부에서도 쓸 수 있다.
//
// 버튼 크기는 고정(`Theme.rackButtonW` x `rackButtonH`)이므로 할당된 셀이 더 크면 남는 공간이 여백이 된다.
// 버튼 자체가 작으므로 라벨을 포함한 셀 전체 영역을 터치 영역으로 잡는다.
// `on`이 true면 버튼이 눌린 채 유지되고 인디케이터가 점등된다(스위치 버튼). `flash()`는 인디케이터를 한 번 깜빡인다.
// 상태가 없는 단발 버튼은 인디케이터를 끈다(`indicator: false`): 이때는 라벨만 중앙 정렬된다.
import QtQuick

Item {
    id: root
    property color color: Theme.buttonColor("bone")
    property string label: ""
    property bool on: false
    property bool indicator: true
    property color indicatorColor: Theme.indicatorGreen
    signal tapped()
    function flash() { lamp.flash() }

    Item {
        anchors.centerIn: parent
        width: parent.width
        height: button.height + 7 + caption.height

        Button {
            id: button
            anchors { horizontalCenter: parent.horizontalCenter; top: parent.top }
            width: Theme.rackButtonW
            height: Theme.rackButtonH
            color: root.color
            held: root.on
            interactive: false
            down: tap.pressed
        }
        Row {
            id: caption
            anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom }
            height: 22
            spacing: 8
            Indicator {
                id: lamp
                visible: root.indicator
                anchors.verticalCenter: parent.verticalCenter
                on: root.on
                color: root.indicatorColor
            }
            Txt {
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, root.width - (root.indicator ? 40 : 16))
                elide: Text.ElideRight
                text: root.label
                color: Theme.ink
                font.pixelSize: Theme.fSmall
                font.bold: true
                font.letterSpacing: 1
            }
        }
    }

    TapHandler {
        id: tap
        onTapped: root.tapped()
    }
}
