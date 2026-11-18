// 드롭다운 시트 베이스: 상단바 아래로 펼쳐져 본문을 덮는 패널들(설정 패널, 위젯 추가 패널, 인터페이스 정보 시트)의 기반 컴포넌트.
// 내용 자체가 위아래로 이동하지 않고 클리핑 높이를 늘려 커튼이 걷히듯 노출되며, 배경은 앞판 그라데이션과 이어진다.
// 시트 영역 위 터치는 아래 화면으로 통과하지 않는다.
//
//   open          열림 여부. 변경 시 `Theme.tPanel` 동안 애니메이션된다
//   shown         현재 노출 비율(0~1). 완전히 닫히면(0) `visible: false`가 된다
//   sheetHeight   시트 전체 높이. 미지정 시 부모 높이 전체를 사용하며, 콘텐츠 크기에 맞추려면 해당 높이를 바인딩한다
//   reveal        현재 노출된 높이(px)
//   edge          켜면 열리고 닫히는 도중 노출 영역 하단에 구분선을 그린다
// 기본 자식은 시트 내부에 배치되어 클리핑되며, 시트 밖 영역에 둘 요소는 `behind`(배경 탭으로 닫기 등)와
// `front`(하단 경계선/그림자 등)에 넣는다(둘 다 전체 영역 크기).
import QtQuick
import "../kit"

Item {
    id: root
    property bool open: false
    property real sheetHeight: height
    property bool edge: false
    default property alias sheetData: sheet.data
    property alias behind: behindSlot.data
    property alias front: frontSlot.data

    property real shown: open ? 1 : 0
    Behavior on shown { NumberAnimation { duration: Theme.tPanel; easing.type: Easing.OutCubic } }
    readonly property real reveal: Math.round(sheetHeight * shown)
    visible: shown > 0

    Item { id: behindSlot; anchors.fill: parent }
    Item {
        width: parent.width
        height: root.reveal
        clip: true

        Rectangle {
            y: -root.y
            width: root.width
            height: root.parent ? root.parent.height : root.height
            gradient: Gradient {
                GradientStop { position: 0; color: Theme.plate }
                GradientStop { position: 1; color: Theme.plateShade }
            }
        }
        MouseArea { width: parent.width; height: root.sheetHeight }
        Item { id: sheet; width: parent.width; height: root.sheetHeight }
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.edgeLo
            visible: root.edge && root.shown < 1
        }
    }
    Item { id: frontSlot; anchors.fill: parent }
}
