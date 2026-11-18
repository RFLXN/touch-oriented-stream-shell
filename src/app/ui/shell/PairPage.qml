// 설정의 페어링 화면: 덱이 페어링 모드에 진입한 동안 표시된다.
// PC에서 실행할 명령을 안내하고(덱 IP 주소마다 명령 1줄 — `shell/KeyValueRows`), PC에서 페어링 요청이 오면 호스트명과 검증 코드를 표시한다.
// 사용자가 PC 화면의 코드와 대조해 일치하면 ACCEPT를 누른다(`accept`: 당시 화면에 떠 있던 코드를 함께 전달해
// 누르는 찰나에 다른 PC가 후보로 바뀐 경우 데몬이 무시하도록 한다). 다르거나 모르는 PC면 REJECT(`reject`)를 누른다.
// 페어링 모드 시작·종료는 상위 컴포넌트(`SettingsPanel`)가 제어한다.
import QtQuick
import "../kit"
import "names.js" as Names

Item {
    id: root
    property var pairing: ({})
    property var net: ({})
    signal accept(string code)
    signal reject()

    readonly property var candidate: pairing.candidate || null

    readonly property var commands: {
        const out = []
        const add = (label, iface) => {
            if (iface && iface.addr)
                out.push([label, "toss-connector pair --deck " + iface.addr.split("/")[0]])
        }
        add("USB", net.usb)
        add("ETHERNET", net.eth)
        add("WI-FI", net.wlan)
        return out
    }

    Column {
        width: parent.width
        spacing: 22

        Column {
            width: parent.width
            spacing: 8
            Label {
                x: 14
                text: "ON THE PC, RUN ONE OF THESE"
            }
            Display {
                width: parent.width
                height: cmds.height + 24
                glass: false
                KeyValueRows {
                    id: cmds
                    x: 18
                    y: 12
                    width: parent.width - 36
                    rows: root.commands
                    keyWidth: 130
                }
            }
        }

        SettingsSection {
            width: parent.width
            label: "REQUEST"
            SettingsRow {
                title: root.candidate ? root.candidate.name : "Waiting for a PC…"
                note: root.candidate ? root.candidate.os + " · " + Names.viaText(root.candidate.via)
                                     : "Pairing mode stays on for 2 minutes"
            }
        }

        Column {
            width: parent.width
            spacing: 14
            visible: !!root.candidate
            Display {
                width: parent.width
                height: 120
                glass: false
                Txt {
                    anchors.centerIn: parent
                    text: root.candidate ? root.candidate.code : ""
                    color: Theme.displayInk
                    font.family: Theme.mono
                    font.pixelSize: 56
                    font.bold: true
                    font.letterSpacing: 4
                }
            }
            Txt {
                x: 14
                width: parent.width - 28
                wrapMode: Text.WordWrap
                text: root.candidate && root.candidate.deck_accepted ? "Accepted here. Waiting for the PC to confirm…"
                    : "The PC must show the same number. If it is different, press REJECT."
                color: Theme.inkDim
                font.pixelSize: Theme.fSmall
            }
            Row {
                x: 2
                spacing: 14
                Button {
                    width: 190
                    height: 60
                    color: Theme.buttonColor("green")
                    enabled: !!root.candidate && !root.candidate.deck_accepted
                    opacity: enabled ? 1 : Theme.dimmed
                    text: "ACCEPT"
                    onTapped: root.accept(root.candidate.code)
                }
                Button {
                    width: 190
                    height: 60
                    text: "REJECT"
                    onTapped: root.reject()
                }
            }
        }
    }
}
