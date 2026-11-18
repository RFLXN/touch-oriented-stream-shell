// 설정의 기기 화면: 페어링된 호스트(PC) 목록. 항목을 누르면 `picked` 시그널(상세 화면 `DevicePage`),
// `PAIR NEW DEVICE`를 누르면 `pair` 시그널(페어링 화면 `PairPage`)을 발생시킨다.
// 목록은 `shell/SettingsList`를 사용하므로 호스트 상태가 갱신되어도 스크롤 위치와 누르고 있던 항목이 유지된다.
import QtQuick
import "../kit"
import "names.js" as Names

Item {
    id: root
    property var hosts: []
    signal picked(var host)
    signal pair()

    readonly property var gone: ({ id: "", name: "", connected: false, via: "" })

    SettingsList {
        id: list
        anchors.fill: parent
        label: "PAIRED DEVICES"
        keyText: "PAIR NEW DEVICE"
        keyWidth: 230
        onKeyTapped: root.pair()
        emptyText: "No paired devices. Press PAIR NEW DEVICE, then run toss-connector pair on the PC."
        rows: root.hosts
        delegate: SettingsRow {
            required property int index
            readonly property var info: list.rows[index] || root.gone
            width: list.width
            divider: index < list.count - 1
            title: info.name
            note: info.connected ? "Connected · " + Names.viaText(info.via) : "Not connected"
            chevron: true
            onTapped: root.picked(info)
        }
    }
}
