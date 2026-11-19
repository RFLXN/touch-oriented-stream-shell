// 데몬(`tossd`) WebSocket 브리지. UI가 데몬에서 수신하는 상태와 데몬으로 보내는 명령은 모두 이 컴포넌트를 거친다.
// 시각 요소는 없으며, 프로토콜 상세는 `src/app/tossd/src/ui_proto.rs` 상단에 정의되어 있다.
// 연결이 끊기면 2초 간격으로 재연결을 시도하며, 끊긴 상태에서 호출한 전송 메서드는 무시된다
// (이미 수신해 둔 상태는 재연결 후 새 값이 올 때까지 유지된다).
//
// 수신 상태 프로퍼티(변경 시 바인딩된 UI가 자동 갱신됨):
//   connected     WebSocket 연결 상태
//   daemonVersion 데몬 버전(`hello`)
//   catalog       추가 가능한 앱 및 위젯 카탈로그  [{"id":..,"title":..,"about":..,"widgets":[{"kind":..,"title":..,"about":..,
//                 "form":"widget|button","w":..,"h":..,"label":..,"settings":[{"key":..,"title":..,"about":..,"type":"host|command|metrics|text|url|color|light|flag",
//                 "required":..,"filter":..,"own":..,"spotify":..,"max":..,"when_host":..,"initial":..}]}]}]   (`shell/AddPanel`에서 사용)
//   sys           시스템 상태  {"host":..,"ip":..,"links":{"usb":..,"eth":..,"wlan":..},
//                              "net":{"usb":{"name":..,"up":..,"mac":..,"addr":..},"eth":{..},"wlan":{..},"usb_state":..,"eth_speed":..,"bt_present":..}}
//   cols, rows    페이지 그리드 크기(4x6)
//   pages         배치  [{"name":..,"racks":[{"id":..,"label":..,"x":..,"y":..,"w":..,"h":..,"widgets":[..]}],"widgets":[복합 위젯..]}]
//                 위젯: {"id":..,"kind":..,"x":..,"y":..,"w":..,"h":..,"label":..,"color":..,"settings":{..}}
//   device        기기 제어 상태  {"brightness":..,"brightness_min":..,"brightness_max":..,"can_power":..,"ssh":..,"can_ssh":..}
//   wifi          Wi-Fi 상태  {"available":..,"phase":"idle|connecting|connected|failed","ssid":..,"key":..,"ip":..,"scanning":..,"error":..,
//                              "freq":..(MHz),"security":..,"signal":..(dBm),"link_speed":..(Mb/s),
//                              "networks":[{"ssid":..,"key":..,"signal":..(dBm),"kind":"open|psk|other","saved":..}]}
//                 (데몬 정의는 `src/app/tossd/src/wifi.rs`의 `WifiState`)
//   hosts         페어링된 호스트(PC) 목록(오프라인 포함)  [{"id":..,"name":..,"os":..,"caps":[..],"via":"usb|lan|wlan","connected":..,
//                   "media":{"player":..,"status":"playing|paused|stopped","title":..,"artist":..,"album":..,
//                            "length_ms":..,"position_ms":..(수신 시점 기준 위치, 재생 중이면 클라이언트에서 카운트),
//                            "rate":..,"can_next":..,"can_prev":..,"can_seek":..,"shuffle":..(또는 null),"loop_mode":"none|playlist|track"(또는 null),
//                            "art":".."(이미지 ID)} 또는 null,
//                   "art":{"id":..,"mime":..,"data":".."(base64)} 또는 null,
//                   "commands":[{"id":..,"name":..,"kind":"run|switch|stepped|linear","steps":[..],"min":..,"max":..,"step":..}]}]
//                 (`commands`는 해당 PC에 등록된 명령 목록이며, 명령 위젯이 참조한다. 0이나 빈 값은 생략될 수 있음)
//   host          현재 연결된 첫 번째 호스트(없으면 `null`). 대상 PC가 미지정된 위젯이 기본으로 사용한다.
//   pairing       페어링 진행 상태  {"active":..(페어링 모드 활성 여부),
//                                    "candidate":{"name":..,"os":..,"via":..,"code":"123 456","host_confirmed":..,"deck_accepted":..} 또는 null}
//   values        호스트에 등록된 스위치/노브의 현재 값  {"<호스트 id>":{"<명령 id>": 값}}
//                 (스위치 0/1, stepped는 인덱스(0부터), linear는 수치 값. 값을 모르는 항목은 생략됨)
//   meters        측정 가능한 리소스 목록  {"@deck":[{"id":"cpu","name":"CPU","title":"CPU usage","unit":"%","max":100,"decimals":0},..],"<호스트 id>":[..]}
//                 `"@deck"`은 덱 자신, 나머지는 메트릭을 제공하는 호스트(PC)다. 자원 미터 위젯 추가 시 이 목록에서 선택한다.
//   meterValues   리소스 현재 값  {"@deck":{"cpu":12,..},"<호스트 id>":{..}}  — 자원 미터 위젯이 표시 중인 항목만 전달된다.
//   results       위젯 액션의 최근 결과  {"<위젯 id>":{"seq":..,"ok":..,"error":..,"at":..(수신 시각 `Date.now()`)}}
//   vault         덱 금고 상태  {"services":[{"id":"discord","title":"Discord","registered":..,"linked":..,"hosts":[허용한 호스트 id..]}],
//                               "asking":{"host":..,"name":..,"service":..,"title":..} 또는 null(허용 여부를 묻는 중인 요청)}  (키 값은 오지 않는다)
//   icons         사이트 아이콘  {"<주소>":"data:image/png;base64,.."}  — 웹페이지 아이콘 버튼이 표시한다(가져오지 못한 주소는 없음)
//   spotify       덱이 직접 조회하는 Spotify 계정. 미디어 위젯이 호스트처럼 쓸 수 있게 호스트와 같은 모양으로 바꿔 둔다:
//                 {"id":"@spotify","name":"Spotify","connected":..,"linked":..,"caps":[..],"media":..,"art":..,
//                  "offline":"<연결되지 않았을 때 보여 줄 문장. 연결되어 있으면 빈 문자열>",
//                  "extra":{"device":..,"liked":..,"queue":[{"title":..,"artist":..}],"devices":[{"id":..,"name":..,"kind":..,"active":..}],"color":"#rrggbb"}}
//   spotifyView   화면에 띄운 Spotify 창(`"queue"` / `"devices"` / 빈 문자열). 위젯이 `spotify.queue`·`spotify.devices`를 보내면 열린다
//                 (같은 요청으로 덱이 최신 목록을 받아 온다). 닫을 때는 빈 문자열로 되돌린다(`shell/SpotifySheet`)
//   spotifyThemed 그 창을 연 위젯이 Spotify 플레이어(`Spotify theme`)인지 여부. 요청 인자의 `themed`로 받는다(데몬은 이 인자를 쓰지 않는다)
// 송신 메서드:
//   action(name, args, hostId, widget)   호스트에 액션 전송. `name`은 `"media.play_pause"` 등(`src/app/proto`의 `ACTION_*`), `args`는 인자 객체.
//                                        `hostId`가 비어 있으면 연결된 첫 번째 호스트로 전송한다. `widget`(위젯 ID)을 넘기면 결과가 `results`로 돌아온다.
//                                        스위치/노브 설정: `action("command.set", {"id": 명령 id, "value": 값}, hostId, widget)`
//   setBrightness(value, keep)           화면 밝기 변경. `keep`이 true면 기기에 영구 저장(드래그 중에는 false, 손을 뗄 때 true)
//   setSsh(on)                           덱 SSH 활성화/비활성화
//   sleep() / powerOff()                 덱 절전 진입 / 전원 종료
//   wifiScan()                           주변 AP 재스캔
//   wifiConnect(key, psk)                AP 연결. `key`는 `networks[].key`. `psk`가 빈 문자열이면 저장된 설정 또는 개방형 AP로 연결
//   wifiForget(key)                      AP의 저장된 접속 정보 삭제
//   pairStart() / pairStop()             페어링 모드 시작 / 종료
//   pairAccept(code) / pairReject()      페어링 요청 수락(화면에 표시된 `code` 전달) / 거절
//   hostForget(id)                       호스트 페어링 삭제
//   vaultAnswer(allow)                   `vault.asking`에 답하기(true면 그 호스트가 금고를 쓰도록 허용)
//   vaultRevoke(service, host) / vaultForget(service)   호스트의 금고 사용 허용 해제 / 서비스 키와 토큰 삭제
//   배치 편집(편집 모드 전용. 데몬이 수락하면 새 `pages`가 내려옴. `target`은 `"widget"` 또는 `"rack"`, 삭제 시에는 `"button"`도 허용):
//   layoutMove(target, id, page, x, y)   배치 이동(`page`가 페이지 수와 같으면 새 페이지 생성)
//   layoutResize(id, w, h) / layoutRename(id, label)   랙 크기 변경 / 라벨 변경
//   layoutAdd(what, page, settings)      추가(`what`은 위젯 종류 또는 `"rack"`). 첫 빈자리에 배치됨
//   layoutPut(rack, x, y, what, settings) 랙 내부 칸에 버튼 위젯 추가
//   layoutRemove(target, id)             삭제
//   layoutTidy() / layoutReset()         빈 페이지 정리(편집 모드 종료 시) / 기본 배치로 초기화
import QtQuick
import QtWebSockets

Item {
    id: link

    property url url: "ws://127.0.0.1:7878"
    readonly property bool connected: socket.status === WebSocket.Open
    property string daemonVersion: ""
    property var sys: ({})
    property int cols: 4
    property int rows: 6
    property var pages: []
    property var catalog: []
    property var results: ({})
    property var wifi: ({ available: false, phase: "idle", networks: [] })
    property var device: ({ brightness: 0, brightness_min: 0, brightness_max: 0, can_power: false, ssh: false, can_ssh: false })
    property var hosts: []
    property var values: ({})
    property var meters: ({})
    property var meterValues: ({})
    property var pairing: ({ active: false, candidate: null })
    property var vault: ({ services: [], asking: null })
    property var icons: ({})
    property var spotify: null
    property string spotifyView: ""
    property bool spotifyThemed: false
    readonly property var host: {
        for (const h of hosts)
            if (h.connected)
                return h
        return null
    }

    function wifiScan() { send({ type: "wifi_scan" }) }
    function wifiConnect(key, psk) {
        const m = { type: "wifi_connect", key: key }
        if (psk)
            m.psk = psk
        send(m)
    }
    function wifiForget(key) { send({ type: "wifi_forget", key: key }) }
    function setBrightness(value, keep) { send({ type: "brightness", value: value, keep: !!keep }) }
    function powerOff() { send({ type: "power_off" }) }
    function sleep() { send({ type: "sleep" }) }
    function setSsh(on) { send({ type: "ssh", on: !!on }) }
    function pairStart() { send({ type: "pair_start" }) }
    function pairStop() { send({ type: "pair_stop" }) }
    function pairAccept(code) { send({ type: "pair_accept", code: code }) }
    function pairReject() { send({ type: "pair_reject" }) }
    function hostForget(id) { send({ type: "host_forget", id: id }) }
    function vaultAnswer(allow) { send({ type: "vault_answer", allow: !!allow }) }
    function vaultRevoke(service, host) { send({ type: "vault_revoke", service: service, host: host }) }
    function vaultForget(service) { send({ type: "vault_forget", service: service }) }
    function layoutMove(target, id, page, x, y) { send({ type: "layout_move", target: target, id: id, page: page, x: x, y: y }) }
    function layoutResize(id, w, h) { send({ type: "layout_resize", id: id, w: w, h: h }) }
    function layoutRename(id, label) { send({ type: "layout_rename", id: id, label: label }) }
    function layoutAdd(what, page, settings) { send({ type: "layout_add", what: what, page: page, settings: settings || {} }) }
    function layoutPut(rack, x, y, what, settings) { send({ type: "layout_put", rack: rack, x: x, y: y, what: what, settings: settings || {} }) }
    function layoutRemove(target, id) { send({ type: "layout_remove", target: target, id: id }) }
    function layoutTidy() { send({ type: "layout_tidy" }) }
    function layoutReset() { send({ type: "layout_reset" }) }
    function action(name, args, hostId, widget) {
        if (hostId === "@spotify" && (name === "spotify.queue" || name === "spotify.devices")) {
            spotifyThemed = !!(args && args.themed)
            spotifyView = name === "spotify.queue" ? "queue" : "devices"
        }
        const m = { type: "action", host: hostId || "", name: name, args: args || null }
        if (widget)
            m.widget = widget
        send(m)
    }
    function send(msg) {
        if (connected)
            socket.sendTextMessage(JSON.stringify(msg))
    }

    WebSocket {
        id: socket
        url: link.url
        active: false
        Component.onCompleted: link.connectNow()
        onTextMessageReceived: (text) => {
            const m = JSON.parse(text)
            switch (m.type) {
            case "hello": link.daemonVersion = m.version; break
            case "catalog": link.catalog = m.apps; break
            case "layout":
                link.cols = m.cols
                link.rows = m.rows
                link.pages = m.pages
                break
            case "state": link.sys = m; break
            case "wifi": link.wifi = m; break
            case "device": link.device = m; break
            case "hosts":
                link.hosts = m.hosts
                link.pairing = m.pairing
                break
            case "values": link.values = m.hosts; break
            case "meters": link.meters = m.hosts; break
            case "meter_values": link.meterValues = m.hosts; break
            case "vault": link.vault = { services: m.services, asking: m.asking }; break
            case "icons": link.icons = m.icons; break
            case "spotify":
                link.spotify = {
                    id: "@spotify", name: "Spotify", connected: m.live, linked: m.linked,
                    offline: !m.linked ? "Link Spotify in toss-connector console" : m.live ? "" : (m.error || "Waiting for Spotify"),
                    caps: m.live ? ["media.info", "media.control"] : [], media: m.media, art: m.art,
                    extra: { device: m.device, liked: m.liked, queue: m.queue || [], devices: m.devices || [], color: m.color }
                }
                break
            case "result":
                link.results = Object.assign({}, link.results, { [m.widget]: { seq: m.seq, ok: m.ok, error: m.error, at: Date.now() } })
                break
            }
        }
    }
    property real lastAttempt: 0
    function connectNow() {
        lastAttempt = Date.now()
        socket.active = false
        socket.active = true
    }
    Timer {
        interval: 1000
        repeat: true
        running: !link.connected
        onTriggered: {
            if (!link.connected && Date.now() - link.lastAttempt >= 2000)
                link.connectNow()
        }
    }
}
