// 드롭다운 패널: 상단바 아래로 펼쳐져 메인 영역을 덮는 화면(설정 패널, 위젯 추가 패널)의 공통 프레임.
// 애니메이션과 터치 차단은 `shell/DropSheet`가 처리한다.
// 상단에 제목 줄을 두고 그 아래 본문 영역에 자식 컴포넌트를 배치한다.
//
//   title          제목(AP 이름처럼 대문자 고정 필요가 없는 경우 `titleSpacing: 0` 지정)
//   canGoBack      활성화 시 제목 왼쪽에 뒤로가기 버튼 표시(탭하면 `backTapped` 발생)
//   corner         제목 줄 오른쪽 끝에 둘 컴포넌트(예: ADD 버튼). 표시되는 동안 그 너비만큼 제목 영역이 줄어든다.
//   keyboardCover  가상 키보드가 하단에서 가리는 높이. 해당 높이만큼 본문 영역 높이를 축소한다.
import QtQuick
import "../kit"

DropSheet {
    id: root
    property string title: ""
    property real titleSpacing: 2
    property bool canGoBack: false
    property real keyboardCover: 0
    default property alias content: body.data
    property alias corner: cornerSlot.data
    signal backTapped()

    Item {
        id: head
        x: Theme.gridMargin
        y: Theme.gridMarginV
        width: root.width - 2 * Theme.gridMargin
        height: 44

        Button {
            id: backKey
            visible: root.canGoBack
            anchors.verticalCenter: parent.verticalCenter
            width: 52
            height: 44
            onTapped: root.backTapped()
            Glyph {
                anchors.centerIn: parent
                name: "back"
                size: 22
                lineWidth: 2.4
                color: backKey.inkColor
            }
        }
        Row {
            id: cornerSlot
            anchors { right: parent.right; verticalCenter: parent.verticalCenter }
        }
        Txt {
            anchors {
                left: parent.left; leftMargin: backKey.visible ? backKey.width + 14 : 14
                right: cornerSlot.left; rightMargin: cornerSlot.width > 0 ? 12 : 0
                verticalCenter: parent.verticalCenter
            }
            elide: Text.ElideRight
            text: root.title
            font.pixelSize: Theme.fTitle
            font.bold: true
            font.letterSpacing: root.titleSpacing
        }
    }

    Item {
        id: body
        x: Theme.gridMargin
        y: head.y + head.height + 14
        width: root.width - 2 * Theme.gridMargin
        height: root.height - y - Math.max(Theme.gridMargin, root.keyboardCover)
    }
}
