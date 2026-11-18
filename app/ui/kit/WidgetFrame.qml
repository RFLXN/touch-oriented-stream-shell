// 위젯 프레임: 랙과 복합 위젯의 외곽 컨테이너. 높이(입체감)는 주지 않고 네 모서리에 작은 나사로 패널 모듈 형태만 표현한다.
// 왼쪽 상단에 라벨(`label`)을 인쇄하고, 그 아래 영역에 컴포넌트(버튼, 노브, 디스플레이, 인디케이터)를 배치한다.
// 라벨이 있으면 내부 콘텐츠는 라벨 줄(`Theme.labelLine`) 아래에서 시작한다.
// 라벨 줄 오른쪽 끝에는 작은 상태 요소(보통 인디케이터)를 둘 수 있다: `corner: Indicator { ... }`.
import QtQuick

Item {
    id: root
    property string label: ""
    default property alias content: body.data
    property alias corner: cornerSlot.data

    Rectangle {
        anchors.fill: parent
        radius: Theme.rFrame
        color: Theme.mix(Theme.plate, Theme.frame, 0.55)
        border.width: 1
        border.color: Theme.mix(Theme.plate, Theme.edgeLo, 0.45)
    }
    Screw { x: Theme.screwInset; y: Theme.screwInset; angle: 25 }
    Screw { x: parent.width - width - Theme.screwInset; y: Theme.screwInset; angle: 115 }
    Screw { x: Theme.screwInset; y: parent.height - height - Theme.screwInset; angle: 70 }
    Screw { x: parent.width - width - Theme.screwInset; y: parent.height - height - Theme.screwInset; angle: 160 }
    Label {
        x: Theme.screwInset + Theme.screwSize + 8
        y: 8
        text: root.label
    }
    Row {
        id: cornerSlot
        anchors { right: parent.right; rightMargin: Theme.screwInset + Theme.screwSize + 14; verticalCenter: parent.top; verticalCenterOffset: 16 }
        spacing: 8
    }
    Item {
        id: body
        anchors { fill: parent; margins: Theme.framePad; topMargin: root.label ? Theme.labelLine : Theme.framePad }
    }
}
