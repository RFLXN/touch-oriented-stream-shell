// 랙 이름 변경 다이얼로그: 편집 모드에서 랙 라벨을 탭하면 열린다. 새 이름을 입력하고 SAVE를 누르면 `renamed(id, label)`를 발생시킨다.
// 배경을 흐리게 가리고 터치를 막으며(`shell/Dialog`), 가상 키보드가 아래에서 올라오므로 다이얼로그는 상단에 배치한다.
// 패널 인쇄형 라벨 규격에 맞춰 영문은 대문자로 변환해 전달한다.
// SAVE를 누르면 먼저 `blur()`로 조합 중인 문자를 커밋한 뒤 텍스트 값을 읽는다.
import QtQuick
import "../kit"

Dialog {
    id: root
    property var rack: null
    readonly property int maxLength: Theme.labelMax
    signal renamed(int id, string label)

    function ask(rack) {
        root.rack = rack
        field.text = rack.label || ""
        field.focusInput()
    }
    function close() {
        field.blur()
        root.rack = null
    }
    function save() {
        field.blur()
        if (root.rack)
            root.renamed(root.rack.id, field.text.trim().toUpperCase())
        close()
    }

    visible: rack !== null
    onDismissed: close()

    Dialog.Card {
        x: Theme.gridMargin
        y: 150
        width: parent.width - 2 * Theme.gridMargin
        height: 212

        Label {
            x: 22
            y: 20
            text: "RENAME RACK"
        }
        TextField {
            id: field
            anchors { left: parent.left; right: parent.right; margins: 20; top: parent.top; topMargin: 50 }
            placeholder: "Name"
            maximumLength: root.maxLength
            onAccepted: root.save()
        }
        Row {
            anchors { right: parent.right; rightMargin: 20; bottom: parent.bottom; bottomMargin: 20 }
            spacing: 12
            Button {
                width: 150
                height: 56
                text: "CANCEL"
                onTapped: root.close()
            }
            Button {
                width: 150
                height: 56
                color: Theme.buttonColor("orange")
                text: "SAVE"
                onTapped: root.save()
            }
        }
    }
}
