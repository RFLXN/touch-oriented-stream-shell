// 설정의 Wi-Fi 연결 화면: 선택한 AP에 연결하거나(필요 시 비밀번호 입력) 저장된 접속 정보를 삭제한다.
// 사용자가 결정하면 `connect` / `forget` 시그널을 발생시키며 후속 화면 전환(목록 복귀)은 `SettingsPanel`이 처리한다.
//
// 비밀번호는 미저장 AP에만 입력받는다. 저장된 AP의 비밀번호를 바꾸려면 삭제(FORGET) 후 다시 연결해야 한다.
import QtQuick
import "../kit"

Item {
    id: root
    property var network: null
    signal connect(string key, string psk)
    signal forget(string key)

    readonly property bool saved: !!network && !!network.saved
    readonly property bool needsPsk: !!network && network.kind === "psk" && !saved
    readonly property bool ready: !needsPsk || (field.text.length >= 8 && field.text.length <= 63)

    function enter() {
        field.text = ""
        reveal.on = false
        if (needsPsk)
            field.focusInput()
    }
    function leave() {
        field.blur()
        field.text = ""
    }
    function submit() {
        if (!network || !ready)
            return
        root.connect(network.key, needsPsk ? field.text : "")
    }

    Column {
        width: parent.width
        spacing: 22

        SettingsSection {
            width: parent.width
            label: "NETWORK"
            SettingsRow {
                title: root.network ? root.network.ssid : ""
                note: !root.network ? "" : root.saved ? "Saved network"
                    : root.network.kind === "open" ? "Open network. No password"
                    : "Password required"
                SignalBars {
                    anchors.verticalCenter: parent.verticalCenter
                    signal: root.network ? root.network.signal : -100
                }
            }
        }

        SettingsSection {
            width: parent.width
            visible: root.needsPsk
            label: "PASSWORD"
            Item {
                width: parent.width
                height: 84
                TextField {
                    id: field
                    anchors { left: parent.left; right: parent.right; margins: 16; verticalCenter: parent.verticalCenter }
                    placeholder: "8 to 63 characters"
                    secret: !reveal.on
                    plain: true
                    maximumLength: 63
                    onAccepted: root.submit()
                }
                Rectangle {
                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom; leftMargin: 20; rightMargin: 20 }
                    height: 1
                    color: Theme.mix(Theme.plate, Theme.edgeLo, 0.45)
                }
            }
            SettingsRow {
                id: revealRow
                title: "Show password"
                onTapped: reveal.on = !reveal.on
                Toggle {
                    id: reveal
                    interactive: false
                    down: revealRow.pressed
                }
            }
        }

        Row {
            x: 2
            spacing: 14
            Button {
                width: 190
                height: 60
                color: Theme.buttonColor("green")
                enabled: root.ready
                opacity: enabled ? 1 : Theme.dimmed
                text: "CONNECT"
                onTapped: root.submit()
            }
            Button {
                visible: root.saved
                width: 190
                height: 60
                text: "FORGET"
                onTapped: if (root.network) root.forget(root.network.key)
            }
        }
    }
}
