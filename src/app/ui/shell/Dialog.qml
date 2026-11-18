// 모달 다이얼로그 베이스: 화면 위에 뜨는 팝업(`ConfirmDialog`, `RenameDialog`)이 공유하는 백드롭 컴포넌트.
// 배경을 흐리게 가리고 터치를 차단하며, 바깥 영역을 탭하면 `dismissed` 시그널을 내보낸다(실제 닫기 처리는 호출 측 담당).
// 본문 카드는 `Dialog.Card`로 구성하며(패널 색상 사각형, 내부 터치 통과 방지) 위치, 크기, 내용은 각 다이얼로그에서 정의한다:
//
//   Dialog {
//       visible: ...
//       onDismissed: close()
//       Dialog.Card { anchors.centerIn: parent; width: ...; height: ...; ... }
//   }
import QtQuick
import "../kit"

Item {
    id: root
    signal dismissed()

    component Card: Rectangle {
        radius: Theme.rFrame
        color: Theme.plate
        border.width: 1
        border.color: Theme.mix(Theme.plate, Theme.edgeLo, 0.6)
        MouseArea { anchors.fill: parent }
    }

    Rectangle {
        anchors.fill: parent
        color: Qt.alpha(Theme.plateShade, 0.82)
        MouseArea { anchors.fill: parent; onClicked: root.dismissed() }
    }
}
