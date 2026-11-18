// 편집 모드 상단바: 배치 편집 중 일반 상단바 자리에 표시된다(높이 동일: `Theme.barHeight`).
// 왼쪽에 `"EDIT LAYOUT"`, 오른쪽에 추가(`+`) 버튼과 완료(`DONE`) 버튼을 둔다.
//   add()    위젯/랙 추가 메뉴(`shell/AddPanel`) 열기. 메뉴가 열린 동안(`adding`) 버튼이 눌린 상태를 유지한다.
//   done()   편집 모드 종료
import QtQuick
import "../kit"

Item {
    id: root
    property bool adding: false
    signal add()
    signal done()

    height: Theme.barHeight

    Txt {
        anchors { left: parent.left; leftMargin: Theme.gridMargin + 14; verticalCenter: doneKey.verticalCenter }
        text: "EDIT LAYOUT"
        font.pixelSize: Theme.fTitle
        font.bold: true
        font.letterSpacing: 2
    }
    Button {
        id: doneKey
        anchors { right: parent.right; rightMargin: Theme.gridMargin; top: parent.top; topMargin: Theme.gridMarginTop }
        width: 120
        height: Theme.barInner
        color: Theme.buttonColor("orange")
        text: "DONE"
        onTapped: root.done()
    }
    Button {
        id: addKey
        anchors { right: doneKey.left; rightMargin: 12; verticalCenter: doneKey.verticalCenter }
        width: 64
        height: Theme.barInner
        held: root.adding
        onTapped: root.add()
        Glyph { anchors.centerIn: parent; name: "plus"; size: 24; lineWidth: 2.6; color: addKey.inkColor }
    }
}
