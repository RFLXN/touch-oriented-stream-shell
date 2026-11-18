// 데몬 연결 끊김 오버레이: `tossd`와의 WebSocket 연결이 끊겼을 때 화면을 흐리게 덮고 중앙 디스플레이로 상태를 알린다.
// 활성 상태에서는 아래 화면의 터치 입력을 차단한다.
import QtQuick
import "../kit"

Rectangle {
    id: root
    property bool active: false

    color: Qt.alpha(Theme.plate, 0.86)
    opacity: active ? 1 : 0
    visible: opacity > 0
    Behavior on opacity { NumberAnimation { duration: 300 } }

    MouseArea { anchors.fill: parent }
    Display {
        anchors.centerIn: parent
        width: 420
        height: 96
        Txt {
            anchors.centerIn: parent
            color: Theme.displayInk
            font.family: Theme.mono
            font.pixelSize: Theme.fTitle
            text: "NO LINK TO TOSSD"
        }
    }
}
