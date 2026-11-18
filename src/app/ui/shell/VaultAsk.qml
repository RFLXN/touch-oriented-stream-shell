// 덱 금고 사용 허용 다이얼로그: 허용하지 않은 PC가 덱 금고(외부 연동 키)를 쓰려 할 때 데몬이 묻는 요청(`link.vault.asking`)을 띄운다.
// `ALLOW`를 누르면 그 PC가 이 서비스의 금고를 쓰도록 허용하고(`vaultAnswer(true)`), `DENY`나 바깥 영역을 누르면 거절한다.
// 데몬이 1분 안에 답을 받지 못하거나 그 PC의 연결이 끊기면 요청이 사라지고 다이얼로그도 닫힌다.
// 허용한 PC는 설정의 INTEGRATIONS에서 서비스별로 해제할 수 있다(`shell/VaultPage`).
import QtQuick
import "../kit"

Dialog {
    id: root
    property var link: null
    readonly property var asking: link && link.connected && link.vault ? link.vault.asking : null

    visible: !!asking
    onDismissed: link.vaultAnswer(false)

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
            Label { text: root.asking ? root.asking.title.toUpperCase() + " ACCESS" : "" }
            Txt {
                width: parent.width
                text: root.asking ? root.asking.name + " wants to use the " + root.asking.title + " key stored on this deck."
                                    + " The key stays here; the PC only borrows short-lived tokens made from it." : ""
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
                    text: "DENY"
                    onTapped: root.link.vaultAnswer(false)
                }
                Button {
                    width: 170
                    height: 60
                    color: Theme.buttonColor("green")
                    text: "ALLOW"
                    onTapped: root.link.vaultAnswer(true)
                }
            }
        }
    }
}
