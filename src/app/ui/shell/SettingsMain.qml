// 설정 메인 화면: 설정 섹션 목록. `SettingsSection` 안에 `SettingsRow` 항목들이 배치된다.
// 기기 상태는 `link`(데몬 연결)에서 읽고, 즉시 반영되는 제어는 `link`로 보낸다. 하위 화면 진입 시 `opened(page)` 시그널을 발생시킨다
// (화면 전환은 `SettingsPanel`이 담당).
//   DISPLAY / Brightness     화면 밝기 슬라이더. 손을 뗀 최종 값은 기기에 저장되어 재부팅 후에도 유지된다. 밝기 조절을 지원하지 않는 기기에서는 숨겨진다.
//   DISPLAY / Dark mode      라이트/다크 테마 전환
//   DISPLAY / Glass effect   디스플레이 유리 반사광 효과 켜기/끄기
//   (위 두 테마 설정은 화면에 바로 적용하고 덱에도 저장해 재부팅 후에도 유지된다: `link.setLook`)
//   NETWORK / Wi-Fi          Wi-Fi 화면으로 이동(`opened("wifi")`). 진입 시 주변 AP 재스캔을 시작한다.
//   NETWORK / SSH            덱 SSH 서버 활성화/비활성화 토글. 끄면 USB와 네트워크 모든 경로에서 접속이 차단되며
//                            기존 연결도 즉시 끊어진다. 설정은 기기에 저장되어 재부팅·재플래싱 후에도 유지된다.
//                            현재 활성화 여부는 데몬이 보고한 `device.ssh`를 그대로 표시한다. SSH 제어를 지원하지 않는 기기(`device.can_ssh`)에서는 숨겨진다.
//   LAYOUT / Edit layout     편집 모드 진입 요청(`editLayout`, 실제 편집은 `Main`과 `Pager`가 처리)
//   LAYOUT / Reset layout    기본 배치로 초기화. 확인 다이얼로그를 거친다(`confirmAsked`, 다이얼로그는 `Main`의 `ConfirmDialog`)
//   DEVICES / Paired devices 페어링된 호스트(PC) 목록 화면으로 이동(`opened("devices")`)
//   INTEGRATIONS / <서비스>  덱 금고에 보관하는 외부 연동 키(현재는 Discord)의 상태. 서비스 상세 화면으로 이동(`opened("vault:<서비스 id>")`)
//   POWER / Sleep            덱 절전 진입. 확인 다이얼로그를 거치며 Wi-Fi에 연결되어 있을 때만 동작한다
//                            (절전 중인 덱은 PC에서 WoWLAN으로 깨울 수 있다: `toss-connector wake`).
//   POWER / Power off        덱 전원 종료. 확인 다이얼로그를 거친다.
//   (POWER 섹션은 전원 제어가 가능한 기기(`device.can_power`)에서만 표시된다.)
import QtQuick
import "../kit"

Column {
    id: root
    property var link: null
    signal opened(string page)
    signal editLayout()
    signal confirmAsked(string title, string text, string yes, var then, string color)

    readonly property var wifi: link ? link.wifi : ({})
    readonly property var device: link ? link.device : ({})
    readonly property var hosts: link ? link.hosts : []
    readonly property var vault: link ? link.vault : ({ services: [] })

    spacing: 22

    SettingsSection {
        width: parent.width
        label: "DISPLAY"
        SettingsRow {
            id: brightRow
            readonly property real lo: root.device.brightness_min || 0
            readonly property real hi: root.device.brightness_max || 0
            readonly property real level: hi > lo ? (root.device.brightness - lo) / (hi - lo) : 0
            function raw(v) { return Math.round(lo + v * (hi - lo)) }
            visible: hi > 0
            title: "Brightness"
            note: Math.round((slider.dragging ? slider.dragValue : level) * 100) + "%"
            Slider {
                id: slider
                width: 340
                value: brightRow.level
                onMoved: (v) => root.link.setBrightness(brightRow.raw(v), false)
                onReleased: (v) => root.link.setBrightness(brightRow.raw(v), true)
            }
        }
        SettingsRow {
            id: darkRow
            title: "Dark mode"
            note: "Use the dark panel"
            onTapped: {
                Theme.dark = !Theme.dark
                root.link.setLook({ dark: Theme.dark })
            }
            Toggle {
                on: Theme.dark
                interactive: false
                down: darkRow.pressed
            }
        }
        SettingsRow {
            id: glassRow
            title: "Glass effect"
            note: "Show reflections on displays"
            onTapped: {
                Theme.glass = !Theme.glass
                root.link.setLook({ glass: Theme.glass })
            }
            Toggle {
                on: Theme.glass
                interactive: false
                down: glassRow.pressed
            }
        }
    }
    SettingsSection {
        width: parent.width
        label: "NETWORK"
        SettingsRow {
            title: "Wi-Fi"
            note: !root.wifi.available ? "Unavailable"
                : root.wifi.phase === "connected" ? "Connected to " + root.wifi.ssid
                : root.wifi.phase === "connecting" ? "Connecting to " + root.wifi.ssid
                : "Not connected"
            chevron: true
            onTapped: {
                root.opened("wifi")
                root.link.wifiScan()
            }
        }
        SettingsRow {
            id: sshRow
            visible: !!root.device.can_ssh
            title: "SSH"
            note: root.device.ssh ? "Open · remote shell over USB and network" : "Closed · no remote shell"
            onTapped: root.link.setSsh(!root.device.ssh)
            Toggle {
                on: !!root.device.ssh
                color: Theme.indicatorAmber
                interactive: false
                down: sshRow.pressed
            }
        }
    }
    SettingsSection {
        width: parent.width
        label: "LAYOUT"
        SettingsRow {
            title: "Edit layout"
            note: "Move, add and remove widgets"
            chevron: true
            onTapped: root.editLayout()
        }
        SettingsRow {
            title: "Reset layout"
            note: "Back to the layout this deck started with"
            onTapped: root.confirmAsked("RESET LAYOUT", "Every widget, rack and page you added or moved will be removed. This cannot be undone.", "RESET",
                                        () => root.link.layoutReset(), "")
        }
    }
    SettingsSection {
        width: parent.width
        label: "DEVICES"
        SettingsRow {
            title: "Paired devices"
            note: {
                const n = root.hosts.length
                const on = root.hosts.filter((h) => h.connected).length
                return n === 0 ? "None" : n + " paired · " + on + " connected"
            }
            chevron: true
            onTapped: root.opened("devices")
        }
    }
    SettingsSection {
        width: parent.width
        label: "INTEGRATIONS"
        Repeater {
            model: root.vault.services || []
            SettingsRow {
                required property var modelData
                title: modelData.title
                note: {
                    const s = modelData
                    if (!s.registered)
                        return "Not set up · add the key from a PC's web console"
                    const n = s.hosts.length
                    return (s.linked ? "Linked" : "Key stored · not linked yet") + " · " + (n === 0 ? "no PC allowed" : n + (n === 1 ? " PC allowed" : " PCs allowed"))
                }
                chevron: true
                onTapped: root.opened("vault:" + modelData.id)
            }
        }
    }
    SettingsSection {
        visible: !!root.device.can_power
        width: parent.width
        label: "POWER"
        SettingsRow {
            id: sleepRow
            readonly property bool wakeable: root.wifi.phase === "connected"
            title: "Sleep"
            note: wakeable ? "Wake it from a paired PC" : "Needs Wi-Fi, or it could not be woken"
            opacity: wakeable ? 1 : Theme.dimmed
            onTapped: {
                if (wakeable)
                    root.confirmAsked("SLEEP", "The deck will sleep. Wake it from a paired PC with: toss-connector wake", "SLEEP", () => root.link.sleep(), "orange")
            }
        }
        SettingsRow {
            title: "Power off"
            note: "Shut the deck down"
            divider: false
            onTapped: root.confirmAsked("POWER OFF", "The deck will shut down.", "POWER OFF", () => root.link.powerOff(), "")
        }
    }
}
