// 키-값 테이블: 디스플레이 내부에 한 줄씩 왼쪽에 라벨, 오른쪽에 고정폭 텍스트 값을 나열한다(인터페이스 정보 시트, 페어링 화면 명령 안내).
// `rows`는 `[[키, 값], ...]` 형태이고, `keyWidth`는 값 칼럼이 시작하는 X 오프셋이다(가장 긴 키 라벨이 들어갈 너비로 지정).
import QtQuick
import "../kit"

Column {
    id: root
    property var rows: []
    property real keyWidth: 150

    Repeater {
        model: root.rows
        Item {
            required property var modelData
            width: root.width
            height: 34
            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: modelData[0]
                color: Theme.displayDim
            }
            Txt {
                anchors { left: parent.left; leftMargin: root.keyWidth; right: parent.right; verticalCenter: parent.verticalCenter }
                elide: Text.ElideRight
                text: modelData[1]
                color: Theme.displayInk
                font.family: Theme.mono
                font.pixelSize: Theme.fSmall + 2
            }
        }
    }
}
