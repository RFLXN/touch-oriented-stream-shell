// 설정의 연동 서비스 상세 화면(INTEGRATIONS → 서비스): 덱 금고에 보관한 키의 상태, 금고를 쓰도록 허용한 PC 목록과 해제, 키 삭제.
// 키 값은 화면에 오지 않으며 여기서 입력하지도 않는다(키는 PC의 웹 콘솔이나 `toss-connector discord set`에서 덱으로 보낸다).
//   Status       키가 없음 / 키만 있음(아직 승인 전) / 연결됨(덱이 토큰을 갱신할 수 있음)
//   Allowed PCs  금고를 쓰도록 허용한 PC. `REVOKE`로 해제하면 그 PC는 다음에 쓸 때 다시 묻는다
//   FORGET KEY   키와 토큰을 덱에서 삭제한다(확인 다이얼로그를 거친다. 허용한 PC 목록은 남는다)
import QtQuick
import "../kit"

Item {
    id: root
    property var service: null
    property var hosts: []
    signal revoke(string host)
    signal forget()

    function nameOf(id) {
        for (const h of hosts)
            if (h.id === id)
                return h.name
        return id.slice(0, 12)
    }

    Column {
        width: parent.width
        spacing: 22

        SettingsSection {
            width: parent.width
            label: "KEY"
            SettingsRow {
                title: "Status"
                note: root.service && root.service.registered ? "Kept on this deck, never sent back to a PC" : "Add it from a PC's web console"
                Txt {
                    anchors.verticalCenter: parent.verticalCenter
                    text: !root.service || !root.service.registered ? "Not set up" : root.service.linked ? "Linked" : "Not linked yet"
                    color: Theme.inkDim
                    font.pixelSize: Theme.fSmall + 2
                }
            }
        }
        SettingsSection {
            width: parent.width
            label: "ALLOWED PCS"
            SettingsRow {
                visible: !root.service || root.service.hosts.length === 0
                title: "None"
                note: "A PC asks here the first time it needs the key"
            }
            Repeater {
                model: root.service ? root.service.hosts : []
                SettingsRow {
                    id: allowedRow
                    required property string modelData
                    title: root.nameOf(modelData)
                    note: "Can borrow tokens"
                    Button {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 130
                        height: 50
                        text: "REVOKE"
                        onTapped: root.revoke(allowedRow.modelData)
                    }
                }
            }
        }
        Button {
            x: 2
            visible: !!root.service && root.service.registered
            width: 220
            height: 60
            text: "FORGET KEY"
            onTapped: root.forget()
        }
    }
}
