// 설정의 기기 상세 화면: 페어링된 호스트(PC) 정보 조회 및 페어링 삭제 버튼.
// `"Allows"` 항목은 해당 호스트가 이 덱에 허용한 권한을 표시하며, 권한 변경은 PC 측에서 수행하고 여기서는 읽기만 한다.
import QtQuick
import "../kit"
import "names.js" as Names

Item {
    id: root
    property var host: null
    signal forget(string id)

    function fingerprint(id) { return id ? id.slice(0, 16).match(/.{1,4}/g).join(" ") : "" }
    function capText(cap) {
        return cap === "media.info" ? "Media info" : cap === "media.control" ? "Media control"
             : cap === "resource.info" ? "Resources" : cap === "command.run" ? "Commands" : cap
    }
    function capsText(caps) { return caps && caps.length ? caps.map(capText).join(", ") : "Nothing" }

    Column {
        width: parent.width
        spacing: 22

        SettingsSection {
            width: parent.width
            label: "DEVICE"
            SettingsRow {
                title: "Status"
                Txt {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.host && root.host.connected ? "Connected · " + Names.viaText(root.host.via) : "Not connected"
                    color: Theme.inkDim
                    font.pixelSize: Theme.fSmall + 2
                }
            }
            SettingsRow {
                visible: !!root.host && root.host.connected
                title: "Allows"
                note: "Chosen on the PC"
                Txt {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.host ? root.capsText(root.host.caps) : ""
                    color: Theme.inkDim
                    font.pixelSize: Theme.fSmall + 2
                }
            }
            SettingsRow {
                title: "System"
                Txt {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.host ? root.host.os : ""
                    color: Theme.inkDim
                    font.pixelSize: Theme.fSmall + 2
                }
            }
            SettingsRow {
                title: "Key"
                note: "Identifies this device on every connection"
                Txt {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.host ? root.fingerprint(root.host.id) : ""
                    color: Theme.inkDim
                    font.family: Theme.mono
                    font.pixelSize: Theme.fSmall + 2
                }
            }
        }
        Button {
            x: 2
            width: 190
            height: 60
            text: "FORGET"
            onTapped: if (root.host) root.forget(root.host.id)
        }
    }
}
