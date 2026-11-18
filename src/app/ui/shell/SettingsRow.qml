// 설정 항목 행: 왼쪽에 제목(`title`)과 보조 설명(`note`), 오른쪽에 컨트롤(토글 등)을 배치한다.
// 행 전체가 터치 영역이며(`tapped`), 누르는 동안 `pressed`가 true가 되므로 하위 컨트롤의 `down`에 연결해 피드백을 준다.
// 마지막 항목이 아니면 하단에 구분선을 그린다(`divider`, 리스트 뷰 내부에서는 직접 지정한다).
// 하위 화면으로 진입하는 행은 `chevron`을 켜서 오른쪽에 `>` 아이콘을 표시한다(다른 컨트롤과 함께 둘 때는 직접 배치한다).
import QtQuick
import "../kit"

Item {
    id: root
    property string title: ""
    property string note: ""
    default property alias control: slot.data
    readonly property bool pressed: tap.pressed
    property bool divider: !Positioner.isLastItem
    property bool chevron: false
    signal tapped()

    width: parent ? parent.width : 0
    height: 84

    Column {
        anchors { left: parent.left; leftMargin: 20; right: slot.left; rightMargin: 16; verticalCenter: parent.verticalCenter }
        spacing: 2
        Txt {
            width: parent.width
            elide: Text.ElideRight
            text: root.title
            font.pixelSize: Theme.fBody
            font.weight: Font.Medium
        }
        Txt {
            visible: root.note !== ""
            width: parent.width
            elide: Text.ElideRight
            text: root.note
            color: Theme.inkDim
            font.pixelSize: Theme.fSmall
        }
    }
    Row {
        id: slot
        anchors { right: parent.right; rightMargin: 20; verticalCenter: parent.verticalCenter }
        spacing: 12
        Glyph {
            visible: root.chevron
            anchors.verticalCenter: parent.verticalCenter
            name: "chevron"
            size: 20
            color: Theme.inkDim
        }
    }
    Rectangle {
        visible: root.divider
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom; leftMargin: 20; rightMargin: 20 }
        height: 1
        color: Theme.mix(Theme.plate, Theme.edgeLo, 0.45)
    }
    TapHandler {
        id: tap
        onTapped: root.tapped()
    }
}
