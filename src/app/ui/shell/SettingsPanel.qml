// 설정 패널: 상단바의 설정 아이콘을 누르면 아래로 펼쳐져 메인 영역을 덮고, 다시 누르면 닫힌다
// (`shell/DropPanel` 기반: 제목 줄, 뒤로가기 버튼, 콘텐츠 영역, 열림/닫힘 애니메이션 제공 및 배경 터치 차단).
//
// 하위 화면(`page`)으로 구성된다. 제목 왼쪽 뒤로가기 버튼으로 이전 화면으로 돌아가며, 패널을 닫으면 첫 화면으로 초기화된다.
//   main    설정 목록(`SettingsMain`)
//   wifi    Wi-Fi 상태 및 스캔된 AP 목록(`WifiPage`)
//   join    선택한 AP 연결 및 저장된 정보 삭제(`WifiJoin`). 화면을 벗어날 때 비밀번호 입력 칸을 비운다.
//   devices 페어링된 호스트(PC) 목록(`DevicesPage`)   <- `main`의 DEVICES / Paired devices
//   device  개별 호스트 상세 정보 및 페어링 삭제(`DevicePage`)
//   pair    새 호스트 페어링(`PairPage`). 이 화면에 머무는 동안에만 페어링 모드가 켜진다
//           (진입 시 시작하고 이탈 시 중지하며, 페어링 완료 또는 타임아웃으로 모드가 종료되면 `devices`로 복귀한다).
// 실제 제어는 데몬이 담당한다(`link`로 상태 수신 및 명령 전송).
// 비가역 작업(`SettingsMain`의 일부 항목, `device`의 FORGET, `join`의 FORGET)은 `confirmAsked` 시그널로 확인 다이얼로그(`Main`의 `ConfirmDialog`)를 거친 뒤 실행한다.
import QtQuick
import "../kit"

DropPanel {
    id: root
    property var link: null
    property string page: "main"
    signal editLayout()
    signal confirmAsked(string title, string text, string yes, var then, string color)

    readonly property var wifi: link ? link.wifi : ({})
    readonly property var hosts: link ? link.hosts : []
    readonly property var pairing: link ? link.pairing : ({})

    function show(next) {
        if (page === "join")
            join.leave()
        if (page === "pair" && next !== "pair")
            link.pairStop()
        if (next === "pair" && page !== "pair")
            link.pairStart()
        page = next
    }
    function back() {
        show(page === "join" ? "wifi" : page === "device" || page === "pair" ? "devices" : "main")
    }
    property bool wasPairing: false
    onPairingChanged: {
        const active = !!pairing.active
        if (page === "pair" && wasPairing && !active)
            page = "devices"
        wasPairing = active
    }

    onOpenChanged: {
        if (open)
            return
        if (page === "join")
            join.leave()
        if (page === "pair")
            show("devices")
    }
    onShownChanged: if (shown === 0) page = "main"

    edge: true
    title: page === "wifi" ? "WI-FI"
         : page === "join" && join.network ? join.network.ssid
         : page === "devices" ? "DEVICES"
         : page === "pair" ? "PAIR NEW DEVICE"
         : page === "device" && devicePage.host ? devicePage.host.name
         : "SETTINGS"
    titleSpacing: page === "join" || page === "device" ? 0 : 2
    canGoBack: page !== "main"
    onBackTapped: back()

    SettingsMain {
        visible: root.page === "main"
        width: parent.width
        link: root.link
        onOpened: (page) => root.show(page)
        onEditLayout: root.editLayout()
        onConfirmAsked: (title, text, yes, then, color) => root.confirmAsked(title, text, yes, then, color)
    }

    DevicesPage {
        visible: root.page === "devices"
        anchors.fill: parent
        hosts: root.hosts
        onPair: root.show("pair")
        onPicked: (host) => {
            devicePage.hostId = host.id
            root.show("device")
        }
    }
    DevicePage {
        id: devicePage
        visible: root.page === "device"
        anchors.fill: parent
        property string hostId: ""
        host: {
            for (const h of root.hosts)
                if (h.id === devicePage.hostId)
                    return h
            return null
        }
        onForget: (id) => root.confirmAsked("FORGET DEVICE",
                                            (devicePage.host ? devicePage.host.name : "This device") + " will be unpaired from this deck. To use it again you have to pair it again.",
                                            "FORGET",
                                            () => {
                                                root.link.hostForget(id)
                                                root.show("devices")
                                            }, "")
    }
    PairPage {
        visible: root.page === "pair"
        anchors.fill: parent
        pairing: root.pairing
        net: root.link && root.link.sys.net ? root.link.sys.net : ({})
        onAccept: (code) => root.link.pairAccept(code)
        onReject: root.link.pairReject()
    }

    WifiPage {
        visible: root.page === "wifi"
        anchors.fill: parent
        wifi: root.wifi
        onScan: root.link.wifiScan()
        onPicked: (network) => {
            join.network = network
            root.show("join")
            join.enter()
        }
    }

    WifiJoin {
        id: join
        visible: root.page === "join"
        anchors.fill: parent
        onConnect: (key, psk) => {
            root.link.wifiConnect(key, psk)
            root.show("wifi")
        }
        onForget: (key) => root.confirmAsked("FORGET NETWORK",
                                             "The deck will forget " + (join.network ? join.network.ssid : "this network") + " and stop joining it on its own."
                                             + (join.network && join.network.kind === "open" ? "" : " To join it again you have to enter the password again."),
                                             "FORGET",
                                             () => {
                                                 root.link.wifiForget(key)
                                                 root.show("wifi")
                                             }, "")
    }
}
