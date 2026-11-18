// 가상 키보드 패널: 텍스트 입력 필드(`kit/TextField`)가 포커스를 받으면 화면 하단에서 올라오고, 입력이 끝나면 내려간다.
//
// 실제 키보드 엔진은 `Qt Virtual Keyboard`이며 앱 프로세스 내에서 동작해 언어별 레이아웃과 조합기(한글, 일본어, 중국어)를 제공한다.
// 입력 필드와 연동하려면 실행 시 `QT_IM_MODULE=qtvirtualkeyboard` 환경 변수가 설정되어 있어야 한다(`app/rootfs/usr/bin/toss-shell`).
// 활성 언어는 `locales`에 지정하며, 2개 이상이면 키보드에 언어 전환 키가 나타난다.
import QtQuick
import QtQuick.VirtualKeyboard
import QtQuick.VirtualKeyboard.Settings
import "../kit"

Item {
    id: root
    property var locales: ["en_US"]
    readonly property bool active: panel.active
    readonly property real cover: panel.height + 2 * pad
    readonly property int pad: 12

    height: cover
    y: parent ? parent.height - shown * cover : 0
    visible: shown > 0
    property real shown: active ? 1 : 0
    Behavior on shown { NumberAnimation { duration: Theme.tPanel; easing.type: Easing.OutCubic } }

    Component.onCompleted: {
        VirtualKeyboardSettings.activeLocales = locales
        VirtualKeyboardSettings.locale = locales[0]
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.plateShade
        Rectangle { width: parent.width; height: 1; color: Theme.edgeLo }
        MouseArea { anchors.fill: parent }
    }
    InputPanel {
        id: panel
        x: root.pad
        y: root.pad
        width: root.width - 2 * root.pad
    }
}
