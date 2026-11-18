// 설정의 Wi-Fi 화면: 현재 연결 상태와 스캔된 AP 목록. AP를 선택하면 `picked` 시그널이 발생한다(`WifiJoin` 화면으로 전환).
// 실제 제어는 데몬이 담당하며, 여기서는 수신한 `wifi` 상태를 렌더링하고 재스캔 요청(`scan`)만 전달한다.
// 목록은 `shell/SettingsList`를 사용하므로 스캔 결과가 갱신되어도 스크롤 위치와 누르고 있던 항목이 유지된다.
import QtQuick
import "../kit"

Item {
    id: root
    property var wifi: ({})
    signal scan()
    signal picked(var network)

    readonly property var networks: wifi.networks || []
    readonly property string phase: wifi.phase || "idle"
    readonly property bool scanning: !!wifi.scanning
    readonly property var gone: ({ ssid: "", key: "", signal: -100, kind: "psk", saved: false })

    function currentNetwork() {
        for (const n of networks)
            if (n.key === wifi.key)
                return n
        return { ssid: wifi.ssid, key: wifi.key, signal: -100, kind: "psk", saved: true }
    }
    function errorText(code) {
        switch (code) {
        case "bad_psk": return "Password must be 8 to 63 characters"
        case "unsupported": return "This security type is not supported"
        default: return "Could not connect. Check the password"
        }
    }

    SettingsSection {
        id: status
        width: parent.width
        label: "STATUS"
        SettingsRow {
            title: !root.wifi.available ? "Wi-Fi unavailable"
                 : root.phase === "connected" || root.phase === "connecting" ? root.wifi.ssid
                 : "Not connected"
            note: !root.wifi.available ? "The Wi-Fi service is not running"
                : root.phase === "connected" ? (root.wifi.ip ? "Connected · " + root.wifi.ip : "Connected · getting an address")
                : root.phase === "connecting" ? "Connecting…"
                : root.phase === "failed" ? root.errorText(root.wifi.error)
                : "Choose a network below"
            chevron: root.phase === "connected"
            onTapped: if (root.phase === "connected") root.picked(root.currentNetwork())
        }
    }

    SettingsList {
        id: list
        anchors { top: status.bottom; topMargin: 22; left: parent.left; right: parent.right; bottom: parent.bottom }
        label: "NETWORKS"
        keyText: root.scanning ? "SCANNING…" : "SCAN"
        keyWidth: 150
        keyEnabled: !!root.wifi.available && !root.scanning
        onKeyTapped: root.scan()
        emptyText: !root.wifi.available ? "" : root.scanning ? "Searching…" : "No networks found"
        rows: root.networks
        delegate: SettingsRow {
            id: row
            required property int index
            readonly property var info: list.rows[index] || root.gone
            readonly property bool current: info.key === root.wifi.key && root.phase === "connected"
            readonly property bool usable: info.kind !== "other" || info.saved
            width: list.width
            height: 72
            divider: index < list.count - 1
            opacity: usable ? 1 : 0.5
            title: info.ssid
            note: current ? "Connected"
                : info.key === root.wifi.key && root.phase === "connecting" ? "Connecting…"
                : info.saved ? "Saved" : info.kind === "other" ? "Not supported"
                : info.kind === "open" ? "Open" : ""
            onTapped: if (usable) root.picked(info)
            Glyph {
                anchors.verticalCenter: parent.verticalCenter
                visible: row.info.kind !== "open"
                name: "lock"
                size: 20
                lineWidth: 1.8
                color: Theme.inkDim
            }
            SignalBars {
                anchors.verticalCenter: parent.verticalCenter
                signal: row.info.signal
            }
            Glyph {
                anchors.verticalCenter: parent.verticalCenter
                name: "chevron"
                size: 20
                color: Theme.inkDim
            }
        }
    }
}
