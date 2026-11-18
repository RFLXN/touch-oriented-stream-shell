// 인터페이스 정보 시트: 상단바 아이콘(USB, LAN, WLAN, BT, CORE)을 누르면 아래로 펼쳐져 해당 인터페이스 상세 정보를 표시한다.
// 같은 아이콘을 다시 누르거나 바깥 영역을 탭하면 닫히고, 다른 아이콘을 누르면 내용만 전환된다.
//
// `which`에 표시할 대상 이름을 넣는다(`""`이면 닫힘). 내용은 데몬 상태(`sys`, `wifi`)로부터 생성해 키-값 테이블(`shell/KeyValueRows`)로 그린다.
// 설정 패널처럼 패널 자체가 이동하지 않고 위에서 아래로 커튼이 걷히듯 드러난다(`shell/DropSheet`). 콘텐츠 높이만큼만 펼쳐지며
// 하단 경계를 구분선과 그림자가 따라간다.
import QtQuick
import "../kit"

DropSheet {
    id: root
    property string which: ""
    property var sys: ({})
    property var wifi: ({})
    property bool connected: false
    property string daemonVersion: ""
    property url daemonUrl

    readonly property var net: sys.net || ({})
    property string shownWhich: ""
    onWhichChanged: if (which !== "") shownWhich = which

    readonly property var titles: ({ usb: "USB", lan: "ETHERNET", wlan: "WI-FI", bt: "BLUETOOTH", core: "DAEMON" })

    function band(freq) { return freq >= 4900 ? "5 GHz" : freq > 0 ? "2.4 GHz" : "" }
    function words(s) { return s ? s.charAt(0).toUpperCase() + s.slice(1) : "" }
    function iface(i, extra) {
        const rows = extra || []
        rows.push(["ADDRESS", i && i.addr ? i.addr : "None"])
        rows.push(["MAC", i && i.mac ? i.mac : "Unknown"])
        rows.push(["INTERFACE", i && i.name ? i.name : ""])
        return rows
    }
    readonly property var rows: {
        if (!connected && shownWhich !== "core")
            return [["STATE", "No data. The daemon is not connected"]]
        switch (shownWhich) {
        case "usb":
            return iface(net.usb, [["STATE", words(net.usb_state) || "Unknown"],
                                   ["LINK", net.usb && net.usb.up ? "Up" : "Down"]])
        case "lan":
            return iface(net.eth, [["LINK", net.eth && net.eth.up ? "Up" + (net.eth_speed ? " · " + net.eth_speed + " Mb/s" : "") : "Down"]])
        case "wlan": {
            const head = []
            if (!wifi.available) {
                head.push(["STATE", "Wi-Fi service is not running"])
            } else if (wifi.phase === "connected") {
                head.push(["NETWORK", wifi.ssid])
                head.push(["SIGNAL", wifi.signal ? wifi.signal + " dBm" + (wifi.link_speed ? " · " + wifi.link_speed + " Mb/s" : "") : "Unknown"])
                head.push(["BAND", band(wifi.freq) + (wifi.freq ? " · " + wifi.freq + " MHz" : "")])
                head.push(["SECURITY", wifi.security || "Unknown"])
            } else {
                head.push(["STATE", wifi.phase === "connecting" ? "Connecting to " + wifi.ssid : "Not connected"])
            }
            return iface(net.wlan, head)
        }
        case "bt":
            return [["STATE", net.bt_present ? "Adapter found. Not set up yet" : "No adapter"]]
        case "core":
            return [["STATE", connected ? "Connected" : "Not connected"],
                    ["VERSION", daemonVersion || "Unknown"],
                    ["SOCKET", daemonUrl.toString()],
                    ["HOST", connected && sys.host ? sys.host : "Unknown"],
                    ["ADDRESS", connected && sys.ip ? sys.ip : "None"]]
        }
        return []
    }

    open: which !== ""
    sheetHeight: card.y + card.height + 16

    behind: MouseArea {
        anchors.fill: parent
        enabled: root.open
        onClicked: root.which = ""
    }
    front: [
        Rectangle {
            y: root.reveal
            width: parent.width
            height: 1
            color: Theme.edgeLo
        },
        Rectangle {
            y: root.reveal + 1
            width: parent.width
            height: 14
            gradient: Gradient {
                GradientStop { position: 0; color: Theme.shadow }
                GradientStop { position: 1; color: "transparent" }
            }
            opacity: 0.6
        }
    ]

    Label {
        id: heading
        x: Theme.gridMargin + 14
        y: Theme.gridMarginV
        text: root.titles[root.shownWhich] || ""
    }
    Display {
        id: card
        x: Theme.gridMargin
        y: heading.y + heading.height + 8
        width: root.width - 2 * Theme.gridMargin
        height: list.height + 24
        glass: false
        KeyValueRows {
            id: list
            x: 18
            y: 12
            width: parent.width - 36
            rows: root.rows
            keyWidth: 150
        }
    }
}
