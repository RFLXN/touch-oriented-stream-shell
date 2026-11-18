// 확인 다이얼로그: 되돌릴 수 없거나 덱을 쓸 수 없게 되는 작업 실행 전 사용자 확인을 받는다
// (전원 종료, 절전, 기본 배치 초기화, 페어링 삭제, 저장된 AP 삭제, 위젯이 포함된 랙 삭제, 금고 키 삭제).
// 호출 주체는 `Main.qml`이다(설정에서 요청한 작업과 배치 편집 작업).
// 화면 중앙에 표시되며 배경을 흐리게 가려 터치를 차단한다(`shell/Dialog`). 바깥 영역이나 `CANCEL`을 누르면 아무 동작 없이 닫힌다.
//
//   ask(title, text, yes, then, color)  다이얼로그를 연다. `title`은 라벨 형태의 제목, `text`는 안내 문구,
//                                       `yes`는 확인 버튼 텍스트, `then`은 확인 시 실행할 콜백,
//                                       `color`는 확인 버튼 색상 이름(생략 시 빨간색 — 비가역 작업, 가역 작업은 `"orange"`).
import QtQuick
import "../kit"

Dialog {
    id: root
    property string title: ""
    property string text: ""
    property string yes: "OK"
    property var then: null
    property string yesColor: "red"
    property bool open: false

    function ask(title, text, yes, then, color) {
        root.title = title
        root.text = text
        root.yes = yes
        root.then = then
        root.yesColor = color || "red"
        open = true
    }
    function close() {
        open = false
        then = null
    }
    function confirm() {
        const act = then
        close()
        if (act)
            act()
    }

    visible: open
    onDismissed: close()

    Dialog.Card {
        anchors.centerIn: parent
        width: parent.width - 2 * Theme.gridMargin - 80
        height: body.height + 44

        Column {
            id: body
            x: 22
            y: 22
            width: parent.width - 44
            spacing: 14
            Label { text: root.title }
            Txt {
                width: parent.width
                text: root.text
                wrapMode: Text.WordWrap
                font.pixelSize: Theme.fBody
            }
            Item { width: 1; height: 6 }
            Row {
                anchors.right: parent.right
                spacing: 12
                Button {
                    width: 170
                    height: 60
                    text: "CANCEL"
                    onTapped: root.close()
                }
                Button {
                    width: 170
                    height: 60
                    color: Theme.buttonColor(root.yesColor)
                    text: root.yes
                    onTapped: root.confirm()
                }
            }
        }
    }
}
